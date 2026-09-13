//! Leftcar desktop host (Tauri 2) — control server + capture orchestration.
//!
//! Design: docs/plans/2026-08-18-rn-tauri-rebuild-design.md

pub mod aoap;
pub mod aoap_control;
pub mod aoap_proxy;
pub mod audit;
pub mod backend;
pub mod clipboard;
pub mod control;
pub mod fec;
#[cfg(target_os = "macos")]
pub mod ffi;
pub mod file_transfer;
pub mod identity;
pub mod lock;
pub mod media_pacing;
pub mod pairing;
pub mod settings;
pub mod source_grants;
mod state_profile;
#[cfg(target_os = "windows")]
pub mod windows_backend;
pub mod wire;

use backend::SharedBackend;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::WebviewUrl;
use tauri::{Manager, WindowEvent};

/// Preferred control-plane port. If it is already occupied, the Host binds an
/// OS-assigned port and advertises that actual endpoint through mDNS and QR.
const PREFERRED_CONTROL_PORT: u16 = 7777;

#[derive(Clone, Copy)]
struct ControlEndpoint {
    port: u16,
}

/// Report a fatal startup failure without panicking. A message dialog is the
/// only way to reach users when no window exists yet; without it the app
/// exits silently and users report "the app does not launch".
fn fatal_startup_error(message: String) -> ! {
    eprintln!("Leftcar Host startup failed: {message}");
    let _ = rfd::MessageDialog::new()
        .set_title("Leftcar Host")
        .set_level(rfd::MessageLevel::Error)
        .set_description(&message)
        .show();
    std::process::exit(1);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let normal_data = dirs::data_dir();
    let profile = state_profile::StateProfile::from_environment(normal_data.as_deref())
        .unwrap_or_else(|message| fatal_startup_error(message));
    if profile.is_benchmark() {
        eprintln!("Leftcar Host: isolated internal benchmark profile");
    }
    let _profile_owner = source_grants::lock_profile(
        &profile
            .file(".source-grants.lock")
            .unwrap_or_else(|| fatal_startup_error("Host state directory unavailable".into())),
    )
    .unwrap_or_else(|message| fatal_startup_error(message));
    let backend = platform_backend().unwrap_or_else(|message| fatal_startup_error(message));
    let warmup_backend = backend.clone();
    // 호스트 정체 키: QR과 핸드셰이크 서명의 뿌리. 최초 기동에서 생성·영속된다.
    let identity = Arc::new(identity::load_or_create(
        (if profile.is_benchmark() {
            profile.file("host_identity.json")
        } else {
            identity::default_identity_path()
        })
        .as_deref(),
    ));
    let pairing_store_path = if profile.is_benchmark() {
        profile.file("paired_devices.json")
    } else {
        pairing::PairingServer::default_store_path()
    };
    let pairing = Arc::new(pairing::PairingServer::new(
        identity.public_key(),
        pairing_store_path.clone(),
        if profile.is_benchmark() {
            pairing::token_store_with_service(pairing_store_path, profile.credential_service())
        } else {
            pairing::token_store(pairing_store_path)
        },
    ));
    pairing
        .initialize_source_grants(
            profile
                .file("source_grants.json")
                .unwrap_or_else(|| fatal_startup_error("Host state directory unavailable".into())),
        )
        .unwrap_or_else(|message| fatal_startup_error(message));
    let audit = Arc::new(audit::SessionAudit::new(if profile.is_benchmark() {
        profile.file("sessions.jsonl")
    } else {
        audit::SessionAudit::default_path()
    }));
    // 파일 공유 게이트: 기본 꺼짐, 승인 토글처럼 영속된다(0600 settings.json).
    let settings = Arc::new(settings::SharedSettings::load_or_default(
        if profile.is_benchmark() {
            profile.file("settings.json")
        } else {
            settings::default_settings_path()
        },
    ));
    let server = Arc::new(control::ControlServer::new(
        backend.clone(),
        pairing.clone(),
        identity.clone(),
    ));
    server.set_audit(audit.clone());
    // 클립보드 동기화 호스트 게이트(U5): 같은 0600 settings.json에서 읽고,
    // 손상 시 기본 꺼짐으로 되돌아간다. 토글은 즉시 효력을 가진다.
    server.set_clipboard_share(settings.clipboard_share());
    server.set_settings(settings.clone());
    // 세션 종료 후 화면 잠금 실행부(설정 lock_on_disconnect가 켜져 있을 때
    // 마지막 세션 teardown에서 호출된다).
    server.set_lock_screen(std::sync::Arc::new(lock::lock_workstation));
    let (control_listener, control_port) =
        bind_control_listener().unwrap_or_else(|message| fatal_startup_error(message));
    server.set_control_port(control_port);
    start_control_server(server.clone(), control_listener, control_port);
    // AOAP devices re-enumerate after the accessory handshake. Keep discovery
    // independent from the Tauri window so a cable connection is available
    // before the viewer asks for its first USB stream.
    aoap_control::start_usb_watcher(server.clone());
    // Advertise independently from the Tauri window so a viewer can connect
    // while WebView/AppKit initialization is still in progress.
    if let Err(e) = advertise_mdns(control_port) {
        eprintln!("mDNS advertise failed: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_host_platform,
            get_control_port,
            get_lan_ip,
            get_input_permission,
            request_input_permission,
            get_screen_permission,
            open_system_settings,
            set_session_input,
            set_session_quality,
            force_stop_session,
            begin_pairing,
            cancel_pairing,
            list_pending_pairings,
            approve_pending_pairing,
            reject_pending_pairing,
            list_paired_devices,
            list_paired_device_state,
            list_host_sources,
            set_source_grants,
            revoke_paired_device,
            revoke_all_devices,
            get_clipboard_share,
            set_clipboard_share,
            get_file_share,
            set_file_share,
            get_privacy_settings,
            set_lock_on_disconnect,
            set_privacy_curtain,
            add_share_files,
            list_share_queue,
            remove_share_file
        ])
        .setup(move |app| {
            // 클립보드 접근은 플러그인의 Rust API로 한다(U5). pbcopy/pbpaste는
            // 이 주입이 없는 환경의 폴백일 뿐이다.
            server.set_clipboard(Arc::new(clipboard::TauriClipboard::new(
                app.handle().clone(),
            )));
            let curtain_controller = make_curtain_controller(app.handle().clone());
            server.set_curtain_controller(curtain_controller);
            app.manage(server);
            app.manage(pairing);
            app.manage(audit);
            app.manage(settings);
            app.manage(ControlEndpoint { port: control_port });
            warm_display_catalog(warmup_backend);
            create_indicator_window(app);

            let show_item =
                MenuItem::with_id(app, "show", "Leftcar Host 열기", true, None::<&str>)?;
            let pairing_item =
                MenuItem::with_id(app, "pairing", "연결 코드 만들기…", true, None::<&str>)?;
            let quit_item =
                MenuItem::with_id(app, "quit", "Leftcar Host 종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &pairing_item, &quit_item])?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| "Leftcar Host tray icon is not configured".to_string())?;

            TrayIconBuilder::with_id("leftcar-host")
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                .tooltip("Leftcar Host — 백그라운드 실행 중")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "pairing" => show_pairing_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // The dashboard window can lose its first-show race on slow
            // AppKit startups. Show+focus defensively; users reported the
            // app appearing to "not launch" when only the tray existed.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // Closing the dashboard hides it; the control server, mDNS
                    // advertisement, and active capture sessions keep running.
                    api.prevent_close();
                    let _ = window.hide();
                }
            } else if window.label() == "pairing" {
                if let WindowEvent::CloseRequested { .. } = event {
                    // The pairing window is a plain closable window (close is
                    // NOT prevented). The QR secret lives until canceled and
                    // webview teardown cannot run React cleanup, so burn every
                    // live offer here (pairing.rs cancel_active).
                    window
                        .app_handle()
                        .state::<Arc<pairing::PairingServer>>()
                        .cancel_active();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("tauri build")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Err(error) = app
                    .state::<Arc<control::ControlServer>>()
                    .shutdown_source_access()
                {
                    eprintln!("Host grant shutdown uncertain: {error}");
                }
            }
        });
}

#[cfg(target_os = "macos")]
fn platform_backend() -> Result<SharedBackend, String> {
    let backend = ffi::FfiBackend::new()?;
    println!("{}", ffi::dylib_report());
    Ok(Arc::new(backend))
}

#[cfg(target_os = "windows")]
fn platform_backend() -> Result<SharedBackend, String> {
    Ok(Arc::new(windows_backend::WindowsBackend::new()?))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_backend() -> Result<SharedBackend, String> {
    Err(format!(
        "{} is not a supported Leftcar host platform",
        std::env::consts::OS
    ))
}

/// Create the pairing window on first open; show+focus on later opens.
fn show_pairing_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("pairing") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if let Err(e) = tauri::WebviewWindowBuilder::new(
        app,
        "pairing",
        WebviewUrl::App("index.html#/pairing".into()),
    )
    .title("연결 코드 만들기")
    .inner_size(420.0, 560.0)
    .resizable(false)
    .build()
    {
        eprintln!("failed to open pairing window: {e}");
    }
}

/// Create the "보고 있음" indicator window (U4a). A streaming host must be
/// visible on the captured desktop (TeamViewer-style no-stealth norm) — the
/// tray alone is not enough. The borderless always-on-top badge starts
/// hidden; the `#/indicator` route polls get_status and shows/hides itself.
fn create_indicator_window(app: &tauri::App) {
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        "indicator",
        WebviewUrl::App("index.html#/indicator".into()),
    )
    .title("Leftcar")
    .decorations(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .resizable(false)
    .inner_size(220.0, 30.0)
    .visible(false);

    // 주 디스플레이 우상단 — 물리 픽셀을 논리 좌표로 환산해 여백 12px에 띄운다.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let margin = 12.0;
        let x = monitor.size().width as f64 / scale - 220.0 - margin;
        let y = margin;
        builder = builder.position(x, y);
    }

    if let Err(error) = builder.build() {
        eprintln!("failed to create indicator window: {error}");
        return;
    }
    if let Some(window) = app.get_webview_window("indicator") {
        // 배지가 클릭이나 포커스를 훔치지 않게 한다(가능한 버전에서만).
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.set_visible_on_all_workspaces(true);
    }
}

/// 프라이버시 커튼: 모니터마다 검은 풀스크린 오버레이를 띄운다(커튼 창은
/// macOS shim이 캡처에서 제외한다 — 제목 "leftcar-curtain"으로 식별).
/// 적용 성공 여부를 돌려준다 — 실패한 토글은 상태로 커밋되지 않아 다음
/// refresh 트리거에서 재시도된다(control.rs M3). WGC 모니터 캡처는 창
/// 제외를 지원하지 않아 Windows v1은 no-op이다.
#[cfg(target_os = "macos")]
fn make_curtain_controller(
    app_handle: tauri::AppHandle,
) -> std::sync::Arc<dyn Fn(bool) -> bool + Send + Sync> {
    std::sync::Arc::new(move |show| refresh_curtain_windows(&app_handle, show))
}

#[cfg(not(target_os = "macos"))]
fn make_curtain_controller(
    _app_handle: tauri::AppHandle,
) -> std::sync::Arc<dyn Fn(bool) -> bool + Send + Sync> {
    std::sync::Arc::new(|_show| {
        eprintln!("leftcar: privacy curtain is macOS-only in v1");
        true
    })
}

#[cfg(target_os = "macos")]
fn refresh_curtain_windows(app_handle: &tauri::AppHandle, show: bool) -> bool {
    let monitors = app_handle.available_monitors().unwrap_or_default();
    if monitors.is_empty() {
        // 모니터를 못 읽으면 창을 놓을 자리가 없다 — show는 실패로 보고
        // 다음 refresh에서 재시도하게 한다.
        return !show;
    }
    let mut applied = true;
    for (index, monitor) in monitors.iter().enumerate() {
        let label = format!("curtain-{index}");
        if !show {
            if let Some(window) = app_handle.get_webview_window(&label) {
                if window.close().is_err() {
                    applied = false;
                }
            }
            continue;
        }
        if app_handle.get_webview_window(&label).is_some() {
            continue;
        }
        // 물리 좌표를 논리 좌표로 환산해 모니터 원점에 정확히 맞춘다.
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        let built = tauri::WebviewWindowBuilder::new(
            app_handle,
            &label,
            WebviewUrl::App("index.html#/curtain".into()),
        )
        .title("leftcar-curtain")
        .decorations(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .resizable(false)
        .maximizable(false)
        .position(position.x as f64 / scale, position.y as f64 / scale)
        .inner_size(size.width as f64 / scale, size.height as f64 / scale)
        .build();
        if let Err(error) = built {
            eprintln!("failed to create curtain window {index}: {error}");
            applied = false;
        }
    }
    applied
}

/// Prime the display catalog before the first viewer opens it. On macOS this
/// is a synchronous CoreGraphics metadata read; ScreenCaptureKit consent is
/// requested only when the viewer starts a stream.
fn warm_display_catalog(backend: SharedBackend) {
    let _ = std::thread::Builder::new()
        .name("leftcar-catalog-warmup".into())
        .spawn(move || {
            // Let the AppKit event loop become live before the first catalog
            // probe so a subsequent system picker can be presented cleanly.
            std::thread::sleep(std::time::Duration::from_millis(500));
            match backend.list_displays() {
                Ok(displays) => println!("display catalog warm: {} display(s)", displays.len()),
                Err(error) => eprintln!("display catalog warmup deferred: {error}"),
            }
        });
}

/// Start the control plane before the Tauri window lifecycle. This keeps the
/// host connectable while WebView/AppKit initialization is slow or blocked by
/// a desktop permission prompt.
fn bind_control_listener() -> Result<(std::net::TcpListener, u16), String> {
    bind_control_listener_at(PREFERRED_CONTROL_PORT)
}

fn bind_control_listener_at(preferred_port: u16) -> Result<(std::net::TcpListener, u16), String> {
    let listener = match std::net::TcpListener::bind(("0.0.0.0", preferred_port)) {
        Ok(listener) => listener,
        Err(preferred_error) => {
            eprintln!(
                "control port {preferred_port} unavailable ({preferred_error}); selecting a free \
                 port (another Leftcar Host instance may be running)"
            );
            std::net::TcpListener::bind(("0.0.0.0", 0)).map_err(|fallback_error| {
                format!(
                    "port {preferred_port}: {preferred_error}; fallback: {fallback_error}; \
                     another Leftcar Host instance may be running"
                )
            })?
        }
    };
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("control listener nonblocking mode: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("control listener address: {error}"))?
        .port();
    Ok((listener, port))
}

fn start_control_server(
    server: std::sync::Arc<control::ControlServer>,
    listener: std::net::TcpListener,
    port: u16,
) {
    std::thread::Builder::new()
        .name("leftcar-control".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("convert control listener to tokio");
                println!("control server on 0.0.0.0:{port}");
                server.run(listener).await;
            });
        })
        .expect("control server thread");
}

#[tauri::command]
fn get_status(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> control::StatusViewPublic {
    state.snapshot()
}

#[tauri::command]
fn get_host_platform(state: tauri::State<'_, std::sync::Arc<control::ControlServer>>) -> String {
    state.platform().into()
}

#[tauri::command]
fn get_control_port(state: tauri::State<'_, ControlEndpoint>) -> u16 {
    state.port
}

#[tauri::command]
fn get_lan_ip() -> Option<String> {
    local_lan_ip()
}

#[tauri::command]
fn get_input_permission(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> Result<bool, String> {
    state.input_permission()
}

#[tauri::command]
fn request_input_permission(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> Result<bool, String> {
    state.request_input_permission()
}

#[tauri::command]
fn get_screen_permission(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> Result<bool, String> {
    state.screen_permission()
}

#[tauri::command]
fn open_system_settings(pane: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane.as_deref() {
            Some("screen_capture") | Some("screencapture") => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Some("remote_desktop") | Some("remotedesktop") => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_RemoteDesktop"
            }
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("failed to open macOS settings: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        Ok(())
    }
}

#[tauri::command]
fn set_session_input(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    session: u32,
    enabled: bool,
) -> Result<(), String> {
    state.set_session_input(session, enabled)
}

#[tauri::command]
fn set_session_quality(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    session: u32,
    quality: Option<f32>,
) -> Result<(), String> {
    state.set_session_quality(session, quality)
}

#[tauri::command]
fn force_stop_session(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    session: u32,
) -> Result<(), String> {
    state.force_stop_session(session)
}

#[tauri::command]
fn begin_pairing(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    endpoint: tauri::State<'_, ControlEndpoint>,
) -> Result<pairing::PairingSessionView, String> {
    let ip = local_lan_ip().ok_or("no LAN interface found")?;
    Ok(state.begin_pairing(&ip, endpoint.port))
}

#[tauri::command]
fn cancel_pairing(state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>) {
    state.cancel_active();
}

#[tauri::command]
fn list_pending_pairings(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
) -> Vec<pairing::PendingPairingView> {
    state.list_pending_views()
}

#[tauri::command]
fn approve_pending_pairing(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    offer_id: String,
) -> Result<(), String> {
    state.approve_pending(&offer_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn reject_pending_pairing(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    offer_id: String,
) -> Result<(), String> {
    state.reject_pending(&offer_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_host_sources(
    server: tauri::State<'_, Arc<control::ControlServer>>,
) -> Result<Vec<control_contract::host::DisplayInfo>, String> {
    server.host_sources()
}
#[tauri::command]
fn set_source_grants(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    device_id: String,
    source_ids: Vec<String>,
    credential_id: String,
) -> Result<source_grants::GrantView, String> {
    server.set_source_grants_for_credential(&device_id, source_ids, Some(&credential_id))
}

#[tauri::command]
fn list_paired_devices(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
) -> Vec<pairing::PairedDeviceView> {
    state.list_device_views()
}

#[tauri::command]
fn list_paired_device_state(
    state: tauri::State<'_, Arc<pairing::PairingServer>>,
) -> pairing::PairedDeviceState {
    state.list_device_state()
}

#[tauri::command]
fn revoke_paired_device(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    device_id: String,
) -> pairing::RevokeOutcome {
    server.revoke_device(&device_id)
}
#[tauri::command]
fn revoke_all_devices(
    server: tauri::State<'_, Arc<control::ControlServer>>,
) -> pairing::RevokeOutcome {
    server.revoke_all_devices()
}

#[tauri::command]
fn get_clipboard_share(state: tauri::State<'_, std::sync::Arc<control::ControlServer>>) -> bool {
    state.clipboard_share_enabled()
}

#[tauri::command]
fn set_clipboard_share(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    enabled: bool,
) -> Result<bool, String> {
    // 토글은 즉시 효력을 가지고 file_share와 함께 0600 settings.json에 남는다.
    settings.set_clipboard_share(enabled)?;
    state.set_clipboard_share(enabled);
    Ok(enabled)
}

#[tauri::command]
fn get_file_share(settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>) -> bool {
    settings.file_share()
}

#[tauri::command]
fn get_privacy_settings(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
) -> (bool, bool) {
    (settings.lock_on_disconnect(), settings.privacy_curtain())
}

#[tauri::command]
fn set_lock_on_disconnect(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    enabled: bool,
) -> Result<(), String> {
    settings.set_lock_on_disconnect(enabled)?;
    audit_state.log(
        "lock_on_disconnect_changed",
        serde_json::json!({ "enabled": enabled }),
    );
    Ok(())
}

#[tauri::command]
async fn set_privacy_curtain(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    enabled: bool,
) -> Result<(), String> {
    settings.set_privacy_curtain(enabled)?;
    // 즉시 반영: 켜면 살아 있는 세션이 있을 때 오버레이를 띄우고, 끄면
    // 치운다.
    state.refresh_curtain();
    if enabled {
        // 이미 스트리밍 중인 세션의 SCK 필터는 시작 시점의 창 스냅샷으로
        // 제외 목록을 만들었다 — 방금 띄운 커튼이 캡처에서 빠지게 같은
        // 형태로 재시작해 필터를 다시 만든다(H1).
        state.restart_sessions_for_curtain().await;
    }
    audit_state.log(
        "privacy_curtain_changed",
        serde_json::json!({ "enabled": enabled }),
    );
    Ok(())
}

#[tauri::command]
fn set_file_share(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    enabled: bool,
) -> Result<(), String> {
    settings.set_file_share(enabled)?;
    audit_state.log(
        "file_share_changed",
        serde_json::json!({ "enabled": enabled }),
    );
    Ok(())
}

/// 파일 공유 대기열에 파일을 올린다. 다이얼로그 취소는 no-op(빈 목록)이다.
/// rfd의 동기 패널은 메인 스레드에서 호출하면 막히므로 블로킹 스레드에서
/// 띄운다(Tauri 명령은 기본적으로 메인 스레드에서 실행된다).
#[tauri::command]
async fn add_share_files(
    server: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> Result<Vec<file_transfer::ShareQueueEntry>, String> {
    let server = server.inner().clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title("Leftcar — 파일 공유")
            .pick_files()
            .unwrap_or_default()
    })
    .await
    .map_err(|error| format!("file dialog failed: {error:?}"))?;
    let transfers = server.file_transfer_state();
    let mut entries = Vec::new();
    for path in picked {
        match transfers.add_share_file(path) {
            Ok(entry) => entries.push(entry),
            Err(error) => eprintln!("leftcar: shared file rejected: {error}"),
        }
    }
    Ok(entries)
}

#[tauri::command]
fn list_share_queue(
    server: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> Vec<file_transfer::ShareQueueEntry> {
    server.file_transfer_state().queue_entries()
}

#[tauri::command]
fn remove_share_file(
    server: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    queue_id: String,
) -> bool {
    server.file_transfer_state().remove_share_file(&queue_id)
}

/// Register `_leftcar._tcp.local.` with the listener's actual control port.
/// The ServiceDaemon is leaked on purpose — it must outlive the app setup.
fn advertise_mdns(port: u16) -> Result<(), String> {
    use std::collections::HashMap;
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e:?}"))?;
    let ip = local_lan_ip().ok_or("no LAN interface found")?;
    let info = mdns_sd::ServiceInfo::new(
        "_leftcar._tcp.local.",
        "leftcar-host",
        "leftcar-host.local.",
        &ip,
        port,
        // Android's NSD resolver rejects an empty TXT property set on some
        // vendor builds ("Key cannot be empty"). Keep one stable property so
        // multiple hosts can still be resolved and listed independently.
        HashMap::from([(String::from("product"), String::from("leftcar"))]),
    )
    .map_err(|e| format!("mdns service info: {e:?}"))?;
    daemon
        .register(info)
        .map_err(|e| format!("mdns register: {e:?}"))?;
    println!("mDNS: leftcar-host._leftcar._tcp.local. at {ip}:{port}");
    std::mem::forget(daemon);
    Ok(())
}

/// Best-effort local interface address (UDP connect trick — no packets sent).
fn local_lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn occupied_control_port_falls_back_to_an_available_port() {
        let occupied = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let occupied_port = occupied.local_addr().unwrap().port();

        let (_fallback, actual_port) = super::bind_control_listener_at(occupied_port).unwrap();

        assert_ne!(actual_port, occupied_port);
        assert_ne!(actual_port, 0);
    }
}
