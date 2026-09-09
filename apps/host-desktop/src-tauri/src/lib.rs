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
pub mod identity;
pub mod pairing;
pub mod settings;
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
    let backend = platform_backend().unwrap_or_else(|message| fatal_startup_error(message));
    let warmup_backend = backend.clone();
    // 호스트 정체 키: QR과 핸드셰이크 서명의 뿌리. 최초 기동에서 생성·영속된다.
    let identity = Arc::new(identity::load_or_create(
        identity::default_identity_path().as_deref(),
    ));
    let pairing_store_path = pairing::PairingServer::default_store_path();
    let pairing = Arc::new(pairing::PairingServer::new(
        identity.public_key(),
        pairing_store_path.clone(),
        pairing::token_store(pairing_store_path),
    ));
    let audit = Arc::new(audit::SessionAudit::new(audit::SessionAudit::default_path()));
    let server = Arc::new(control::ControlServer::new(
        backend.clone(),
        pairing.clone(),
        identity.clone(),
    ));
    server.set_audit(audit.clone());
    // 클립보드 동기화 호스트 게이트(U5): 0600 settings.json에서 읽고,
    // 손상 시 기본 꺼짐으로 되돌아간다. 토글은 즉시 효력을 가진다.
    let host_settings = settings::load(settings::default_settings_path().as_deref());
    server.set_clipboard_share(host_settings.clipboard_share);
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
            revoke_paired_device,
            revoke_all_devices,
            get_clipboard_share,
            set_clipboard_share
        ])
        .setup(move |app| {
            // 클립보드 접근은 플러그인의 Rust API로 한다(U5). pbcopy/pbpaste는
            // 이 주입이 없는 환경의 폴백일 뿐이다.
            server.set_clipboard(Arc::new(clipboard::TauriClipboard::new(
                app.handle().clone(),
            )));
            app.manage(server);
            app.manage(pairing);
            app.manage(ControlEndpoint { port: control_port });
            warm_display_catalog(warmup_backend);
            create_indicator_window(app);

            let show_item =
                MenuItem::with_id(app, "show", "Leftcar Host 열기", true, None::<&str>)?;
            let pairing_item =
                MenuItem::with_id(app, "pairing", "기기 페어링…", true, None::<&str>)?;
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
        .run(|_app, _event| {});
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
    .title("기기 페어링")
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
    state
        .approve_pending(&offer_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reject_pending_pairing(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    offer_id: String,
) -> Result<(), String> {
    state.reject_pending(&offer_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_paired_devices(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
) -> Vec<pairing::PairedDeviceView> {
    state.list_device_views()
}

#[tauri::command]
fn revoke_paired_device(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    server: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
    device_id: String,
) -> bool {
    let removed = state.revoke(&device_id);
    if removed {
        // 철회는 즉시 효력을 가진다 — 라이브 스트림도 함께 끊는다(문서 §18).
        let stopped = server.stop_sessions_for_device(&device_id);
        audit_state.log(
            "device_revoked",
            serde_json::json!({ "device": device_id, "stopped_sessions": stopped }),
        );
    }
    removed
}

#[tauri::command]
fn revoke_all_devices(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    server: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    audit_state: tauri::State<'_, std::sync::Arc<audit::SessionAudit>>,
) -> usize {
    let count = state.revoke_all();
    server.stop_all_sessions();
    audit_state.log(
        "devices_revoked_all",
        serde_json::json!({ "devices": count }),
    );
    count
}

#[tauri::command]
fn get_clipboard_share(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
) -> bool {
    state.clipboard_share_enabled()
}

#[tauri::command]
fn set_clipboard_share(
    state: tauri::State<'_, std::sync::Arc<control::ControlServer>>,
    enabled: bool,
) -> Result<bool, String> {
    // 토글은 즉시 효력을 가지고 다음 기동까지 남는다(0600 settings.json).
    state.set_clipboard_share(enabled);
    if let Some(path) = settings::default_settings_path() {
        settings::persist(
            &path,
            &settings::HostSettings {
                clipboard_share: enabled,
            },
        )?;
    }
    Ok(enabled)
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
