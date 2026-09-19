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
pub mod upnp;
pub mod virtual_display;
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
    // 실험 스위치 주입: 설정값을 프로세스 환경변수로 심는다. shim은 스트림
    // 시작마다 환경변수를 읽으므로(세션 생성이 dylib 로드보다 늦다) 앱 시작
    // 때 한 번 심으면 이후 모든 세션이 같은 값을 본다. 설정 변경 명령도 같은
    // 주입을 반복해 다음 스트림부터 반영된다.
    for (key, value) in settings::experiment_env_vars(&settings.experiment()) {
        std::env::set_var(key, value);
    }
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
    // WAN 포트 매핑(외부 접속 허용) — 설정이 켜져 있을 때 컨트롤 서버 시작과
    // 함께 등록되고, 토글 off·종료 때 즉시 제거된다. 공개 미디어 엔드포인트는
    // 이 콜백을 통해 상태 스냅숏에 반영된다.
    let upnp = Arc::new(upnp::UpnpMappingManager::new());
    {
        let server_for_upnp = server.clone();
        upnp.set_endpoint_notify(Arc::new(move |endpoint| {
            server_for_upnp.set_public_media_endpoint(endpoint);
        }));
    }
    // 세션 종료 후 화면 잠금 실행부(설정 lock_on_disconnect가 켜져 있을 때
    // 마지막 세션 teardown에서 호출된다).
    server.set_lock_screen(std::sync::Arc::new(lock::lock_workstation));
    // 확장(가상) 디스플레이 매니저 — 프로브는 첫 사용 때 shim에서 늦게
    // 확인한다. 심볼 부재 플랫폼/빌드에서는 "지원 안 됨"으로만 존재한다.
    let virtual_display = Arc::new(virtual_display::VirtualDisplayManager::new());
    let (control_listener, control_port) =
        bind_control_listener().unwrap_or_else(|message| fatal_startup_error(message));
    server.set_control_port(control_port);
    start_control_server(
        server.clone(),
        control_listener,
        control_port,
        settings.clone(),
        upnp.clone(),
    );
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
            virtual_display_status,
            virtual_display_create,
            virtual_display_remove,
            revoke_paired_device,
            revoke_all_devices,
            get_clipboard_share,
            set_clipboard_share,
            get_file_share,
            set_file_share,
            get_privacy_settings,
            get_experiments,
            set_experiments,
            set_lock_on_disconnect,
            set_privacy_curtain,
            add_share_files,
            list_share_queue,
            remove_share_file,
            set_language,
            get_streaming_badge,
            set_streaming_badge,
            get_wan_access,
            set_wan_access
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
            app.manage(virtual_display.clone());
            app.manage(upnp);
            app.manage(ControlEndpoint { port: control_port });
            #[cfg(unix)]
            {
                // Updaters send SIGTERM when the ordinary quit request is
                // unavailable. Route it through Tauri's Exit event so capture
                // leases drain and the saved device approvals close cleanly.
                // An unhandled SIGTERM looked like a crash on the next launch.
                let mut terminate = tauri::async_runtime::block_on(async {
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                })?;
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if terminate.recv().await.is_some() {
                        handle.exit(0);
                    }
                });
            }
            warm_display_catalog(warmup_backend);
            create_indicator_window(app);

            // 트레이 메뉴는 웹뷰와 별개로 네이티브에서 그려지므로, settings.json에
            // 영속된 UI 언어(웹뷰의 leftcar_lang과 같은 값)를 기동 시점에 읽어
            // 라벨을 고른다.
            let language = app.state::<Arc<settings::SharedSettings>>().language();
            let (show_label, pairing_label, quit_label, tray_tooltip) = match language {
                settings::HostLanguage::Ko => (
                    "Leftcar Host 열기",
                    "연결 코드 만들기…",
                    "Leftcar Host 종료",
                    "Leftcar Host — 백그라운드 실행 중",
                ),
                settings::HostLanguage::En => (
                    "Open Leftcar Host",
                    "Generate Pairing Code…",
                    "Quit Leftcar Host",
                    "Leftcar Host — running in background",
                ),
            };

            let show_item = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
            let pairing_item =
                MenuItem::with_id(app, "pairing", pairing_label, true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &pairing_item, &quit_item])?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| "Leftcar Host tray icon is not configured".to_string())?;

            TrayIconBuilder::with_id("leftcar-host")
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                .tooltip(tray_tooltip)
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
                // WAN 포트 매핑을 종료 때 즉시 치운다. 응답 없는 게이트웨이 앞에서
                // 종료가 붙잡히지 않게 3초로 잘라내고, 못 치운 매핑은 lease(1시간)가
                // 만료시킨다.
                let upnp = app
                    .state::<Arc<upnp::UpnpMappingManager>>()
                    .inner()
                    .clone();
                tauri::async_runtime::block_on(async {
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(3), upnp.disable())
                            .await;
                });
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
    // 제목은 웹뷰 대시보드의 연결 코드 모달과 같은 문구를 언어 설정에 따라
    // 쓴다(ui-tokens host.pairingModalTitle과 대응).
    let pairing_title = match app.state::<Arc<settings::SharedSettings>>().language() {
        settings::HostLanguage::Ko => "연결 코드 만들기",
        settings::HostLanguage::En => "Generate Pairing Code",
    };
    if let Err(e) = tauri::WebviewWindowBuilder::new(
        app,
        "pairing",
        WebviewUrl::App("index.html#/pairing".into()),
    )
    .title(pairing_title)
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
    settings: std::sync::Arc<settings::SharedSettings>,
    upnp: std::sync::Arc<upnp::UpnpMappingManager>,
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
                if settings.wan_access() {
                    start_upnp_port_mapping(upnp, port);
                } else {
                    println!("UPnP: WAN access is off in settings (LAN direct only)");
                }
                server.run(listener).await;
            });
        })
        .expect("control server thread");
}

/// 설정(wanAccess)이 켜져 있을 때만 호출된다. 게이트웨이 발견·매핑 등록·갱신은
/// [upnp::UpnpMappingManager]가 담당하고, 토글 off·종료 때 disable()이 치운다.
fn start_upnp_port_mapping(upnp: std::sync::Arc<upnp::UpnpMappingManager>, control_port: u16) {
    tokio::spawn(async move {
        let Some(local_ip) = local_lan_ip() else {
            eprintln!("UPnP: local LAN IP not found");
            return;
        };
        upnp.enable(control_port, local_ip).await;
    });
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
    server: tauri::State<'_, Arc<control::ControlServer>>,
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    offer_id: String,
) -> Result<(), String> {
    let approved_device_id = state.approve_pending(&offer_id).map_err(|e| e.to_string())?;
    // 기본적으로 모두 허용: 사용자가 호스트에서 기기 연결을 승인하면 그 기기
    // 한 대에 모든 화면 접근 권한을 기본으로 부여한다. 다른 기기는 이 명령이
    // 절대 승격하지 않는다 — 기기별 승인은 그 기기의 승인 플로우에서만 결정되고,
    // 검토 후 비워 둔 권한(거부)은 그대로 남는다.
    let device = state
        .list_device_views()
        .into_iter()
        .find(|device| device.device_id == approved_device_id);
    let Some(device) = device else {
        return Ok(());
    };
    let fresh =
        device.source_grants.review_required || device.source_grants.source_ids.is_empty();
    if !fresh {
        // 재승인이라도 사용자가 좁혀 둔 권한을 넓히지 않는다.
        return Ok(());
    }
    let displays = server.backend().list_displays().unwrap_or_default();
    let display_ids: Vec<String> = displays.into_iter().filter_map(|d| d.source_id).collect();
    if !display_ids.is_empty() {
        let _ = server.set_source_grants_for_credential(
            &device.device_id,
            display_ids,
            Some(&device.source_grants.credential_id),
        );
    }
    Ok(())
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

/// 확장 디스플레이 상태. UI 폴링용 — 프로브 결과, 라이브 정보, 최근 뷰어
/// 메트릭에서 도출한 기본 모드 제안을 함께 돌려준다.
#[tauri::command]
fn virtual_display_status(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    manager: tauri::State<'_, Arc<virtual_display::VirtualDisplayManager>>,
) -> virtual_display::VirtualDisplayStatusPublic {
    build_virtual_display_status(&server, &manager)
}

fn build_virtual_display_status(
    server: &Arc<control::ControlServer>,
    manager: &Arc<virtual_display::VirtualDisplayManager>,
) -> virtual_display::VirtualDisplayStatusPublic {
    let (supported, reason, live) = manager.status();
    let suggested = match server.latest_viewer_metrics().map(|metrics| {
        virtual_display::ViewerDisplayMetrics {
            physical_width: metrics.physical_width,
            physical_height: metrics.physical_height,
            density_dpi: metrics.density_dpi,
        }
    }) {
        Some(metrics) => virtual_display::match_display_size(&metrics)
            .map(|matched| virtual_display::SuggestedModePublic {
                width: matched.logical_width,
                height: matched.logical_height,
                scale: matched.scale,
                source: "viewerMetrics",
            })
            .unwrap_or_else(|| fallback_suggestion()),
        None => fallback_suggestion(),
    };
    // 제거 요청 뒤 시스템 반영(~30s 비동기)이 카탈로그에 아직 보이는지.
    let removal_pending = manager
        .last_removed_pending()
        .is_some_and(|source_id| {
            server
                .backend()
                .list_displays()
                .map(|displays| {
                    displays
                        .iter()
                        .any(|display| display.source_id.as_deref() == Some(source_id.as_str()))
                })
                .unwrap_or(false)
        });
    virtual_display::VirtualDisplayStatusPublic {
        supported,
        reason,
        live,
        suggested: Some(suggested),
        removal_pending,
    }
}

fn fallback_suggestion() -> virtual_display::SuggestedModePublic {
    virtual_display::SuggestedModePublic {
        width: virtual_display::FALLBACK_MODE.logical_width,
        height: virtual_display::FALLBACK_MODE.logical_height,
        scale: virtual_display::FALLBACK_MODE.scale,
        source: "fallback",
    }
}

/// 확장 디스플레이 생성(호스트 UI 전용). 생성되면 연결 중이 아닌, 이미 화면
/// 승인을 받은 페어드 기기 권한에 이 가상 디스플레이만 추가한다 — 미검토·거부
/// 기기에 자동으로 아무 것도 승인하지 않는다. 승인 갱신은 라이브 세션을 끊으므로
/// 스트리밍 중인 기기는 다음 기회(재연결 뒤 카탈로그)에 포함된다.
#[tauri::command]
async fn virtual_display_create(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    pairing: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    audit: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    manager: tauri::State<'_, Arc<virtual_display::VirtualDisplayManager>>,
    width: u32,
    height: u32,
    scale: u32,
) -> Result<virtual_display::VirtualDisplayStatusPublic, String> {
    let manager = manager.inner().clone();
    // 생성은 활성화+모드 폴링로 수 초 걸린다 — 워커 스레드에서 돌린다.
    let worker = manager.clone();
    let live = tauri::async_runtime::spawn_blocking(move || {
        worker.create(width, height, scale)
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(source_id) = live.source_id.clone() else {
        return Err("virtual display has no stable source id".into());
    };
    audit.log(
        "virtual_display_created",
        serde_json::json!({
            "sourceId": source_id,
            "displayId": live.display_id,
            "logicalWidth": live.logical_width,
            "logicalHeight": live.logical_height,
            "scale": live.scale,
            "modeVerified": live.mode_verified,
        }),
    );
    // 연결 중이 아닌, 이미 화면 승인된 기기에만 새 소스를 추가한다.
    let connected: std::collections::HashSet<String> =
        server.connected_device_ids().into_iter().collect();
    for device in pairing.list_device_views() {
        if connected.contains(&device.device_id) {
            continue;
        }
        let Some(ids) = grants_with_virtual_source(
            &device.source_grants.source_ids,
            device.source_grants.review_required,
            &source_id,
        ) else {
            continue;
        };
        let _ = server.set_source_grants_for_credential(
            &device.device_id,
            ids,
            Some(&device.source_grants.credential_id),
        );
    }
    Ok(build_virtual_display_status(&server, &manager))
}

/// 기존 화면 권한에 새 가상 디스플레이를 얹은 목록. 미검토이거나 권한이 비어
/// 있으면(명시적 거부) None — 이 기기에 자동 승인은 일어나지 않는다.
fn grants_with_virtual_source(
    current: &[String],
    review_required: bool,
    source_id: &str,
) -> Option<Vec<String>> {
    if review_required || current.is_empty() {
        return None;
    }
    let mut ids = current.to_vec();
    if !ids.iter().any(|id| id == source_id) {
        ids.push(source_id.to_owned());
    }
    ids.sort();
    ids.dedup();
    Some(ids)
}

#[cfg(test)]
mod virtual_grant_tests {
    use super::grants_with_virtual_source;

    #[test]
    fn appends_new_source_to_reviewed_devices() {
        let ids = vec!["macos:display:b".to_string(), "macos:display:a".to_string()];
        let next = grants_with_virtual_source(&ids, false, "macos:virtual:v1").unwrap();
        assert_eq!(next, vec!["macos:display:a", "macos:display:b", "macos:virtual:v1"]);
        // 이미 있으면 그대로다.
        assert_eq!(
            grants_with_virtual_source(&next, false, "macos:virtual:v1").unwrap(),
            next
        );
    }

    #[test]
    fn never_grants_to_unreviewed_or_denied_devices() {
        let ids = ["macos:display:a".to_string()];
        assert!(grants_with_virtual_source(&[], true, "v").is_none(), "미검토");
        assert!(grants_with_virtual_source(&ids, true, "v").is_none(), "미검토");
        // 검토 후 비워 둔 권한은 거부다 — 채우지 않는다.
        assert!(grants_with_virtual_source(&[], false, "v").is_none());
    }
}

/// 확장 디스플레이 제거. 시스템 반영은 비동기(~30s)라 상태가 제거 중으로
/// 잠깐 표시될 수 있다.
#[tauri::command]
async fn virtual_display_remove(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    audit: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    manager: tauri::State<'_, Arc<virtual_display::VirtualDisplayManager>>,
) -> Result<virtual_display::VirtualDisplayStatusPublic, String> {
    let manager = manager.inner().clone();
    let worker = manager.clone();
    let _vanished = tauri::async_runtime::spawn_blocking(move || worker.remove())
        .await
        .map_err(|e| e.to_string())??;
    audit.log("virtual_display_removed", serde_json::json!({}));
    Ok(build_virtual_display_status(&server, &manager))
}

#[tauri::command]
fn list_paired_devices(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
) -> Vec<pairing::PairedDeviceView> {
    state.list_device_views()
}

#[tauri::command]
fn list_paired_device_state(
    server: tauri::State<'_, Arc<control::ControlServer>>,
    state: tauri::State<'_, Arc<pairing::PairingServer>>,
) -> pairing::PairedDeviceState {
    // 순수 읽기다 — 이 폴링 명령이 승인을 기록하지 않는다. 화면 승인은 해당
    // 기기의 승인 플로우(approve_pending_pairing)에서만 결정된다.
    let mut state = state.list_device_state();
    // 뷰어 배지와 같은 사실을 말하게 한다: 인증된 제어 연결이 살아 있는
    // 기기만 "연결됨"으로 표시한다.
    let connected: std::collections::HashSet<String> =
        server.connected_device_ids().into_iter().collect();
    for device in state.devices.iter_mut() {
        device.connected = connected.contains(&device.device_id);
    }
    state
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
fn get_streaming_badge(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
) -> bool {
    settings.streaming_badge()
}

#[tauri::command]
fn get_wan_access(settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>) -> bool {
    settings.wan_access()
}

#[tauri::command]
fn set_wan_access(
    endpoint: tauri::State<'_, ControlEndpoint>,
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    upnp: tauri::State<'_, std::sync::Arc<upnp::UpnpMappingManager>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    enabled: bool,
) -> Result<bool, String> {
    // 외부 접속 토글 — 0600 settings.json에 영속되고 매핑 변화는 즉시 효력을
    // 가진다. 매핑 작업은 수 초 걸릴 수 있으니 백그라운드에서 돌린다.
    settings.set_wan_access(enabled)?;
    audit_state.log(
        "wan_access_changed",
        serde_json::json!({ "enabled": enabled }),
    );
    if enabled {
        let upnp = upnp.inner().clone();
        let port = endpoint.port;
        tauri::async_runtime::spawn(async move {
            let Some(local_ip) = local_lan_ip() else {
                eprintln!("UPnP: local LAN IP not found");
                return;
            };
            upnp.enable(port, local_ip).await;
        });
    } else {
        let upnp = upnp.inner().clone();
        tauri::async_runtime::spawn(async move {
            upnp.disable().await;
        });
    }
    Ok(enabled)
}

#[tauri::command]
fn set_streaming_badge(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    enabled: bool,
) -> Result<bool, String> {
    // 배지 표시 토글 — 개인 기기 조합 기본 꺼짐, 0600 settings.json에 영속.
    // Indicator 라우트가 2초 폴링으로 이 값을 따라 show/hide한다.
    settings.set_streaming_badge(enabled)?;
    Ok(enabled)
}

#[tauri::command]
fn get_experiments(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
) -> settings::ExperimentSettings {
    settings.experiment()
}

#[tauri::command]
fn set_experiments(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    experiment: settings::ExperimentSettings,
) -> Result<settings::ExperimentSettings, String> {
    // 실험 스위치 저장 + 프로세스 환경변수 재주입. shim이 스트림 시작마다
    // 환경변수를 읽으므로 앱 재시작 없이 다음 스트림부터 적용된다.
    settings.set_experiment(experiment)?;
    for (key, value) in settings::experiment_env_vars(&settings.experiment()) {
        std::env::set_var(key, value);
    }
    Ok(settings.experiment())
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
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
) -> Result<Vec<file_transfer::ShareQueueEntry>, String> {
    let server = server.inner().clone();
    let dialog_title = match settings.language() {
        settings::HostLanguage::Ko => "Leftcar — 파일 공유",
        settings::HostLanguage::En => "Leftcar — File Sharing",
    };
    let picked = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_title(dialog_title)
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

/// 웹뷰의 언어 토글(leftcar_lang)을 네이티브 표면(트레이 메뉴·파일 대화상자)
/// 와 공유하기 위해 settings.json에 남긴다. 다음 기동 때 트레이가 이 값을 읽는다.
#[tauri::command]
fn set_language(
    settings: tauri::State<'_, std::sync::Arc<settings::SharedSettings>>,
    language: String,
) -> Result<(), String> {
    let parsed = match language.as_str() {
        "ko" => settings::HostLanguage::Ko,
        "en" => settings::HostLanguage::En,
        other => return Err(format!("unsupported language: {other}")),
    };
    settings.set_language(parsed)
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
