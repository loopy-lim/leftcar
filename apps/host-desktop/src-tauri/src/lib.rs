//! Leftcar desktop host (Tauri 2) — control server + capture orchestration.
//!
//! Design: docs/plans/2026-08-18-rn-tauri-rebuild-design.md

pub mod aoap;
pub mod aoap_control;
pub mod aoap_proxy;
pub mod backend;
pub mod clamshell_mode;
pub mod control;
pub mod display_management;
pub mod display_matching;
pub mod fec;
#[cfg(target_os = "macos")]
pub mod ffi;
pub mod pairing;
pub mod power_assertion;
pub mod provider;
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
    let backend = platform_backend().unwrap_or_else(|message| fatal_startup_error(message));
    let warmup_backend = backend.clone();
    let pairing = Arc::new(pairing::PairingServer::new(
        "leftcar-host".into(),
        pairing::PairingServer::default_store_path(),
    ));
    let server = Arc::new(control::ControlServer::new(
        backend.clone(),
        pairing.clone(),
    ));
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
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_host_platform,
            get_control_port,
            get_lan_ip,
            get_input_permission,
            request_input_permission,
            open_system_settings,
            set_session_input,
            set_session_quality,
            force_stop_session,
            begin_pairing,
            cancel_pairing,
            list_paired_devices,
            revoke_device,
            revoke_paired_device,
            revoke_all_devices,
            create_virtual_display,
            remove_virtual_display,
            tablet_display_start,
            tablet_display_stop,
            tablet_display_status,
            list_managed_displays,
            add_managed_display,
            remove_managed_display,
            set_managed_display_position
        ])
        .setup(move |app| {
            app.manage(server);
            app.manage(pairing);
            app.manage(ControlEndpoint { port: control_port });
            app.manage(TabletSessionRegistry::new(None));
            app.manage(display_management::DisplayManager::new(Some(
                display_management::DisplayManager::default_state_path(),
            )));
            warm_display_catalog(warmup_backend);

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
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                for error in app
                    .state::<display_management::DisplayManager>()
                    .cleanup_all()
                {
                    eprintln!("managed display cleanup failed: {error}");
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
    .title("기기 페어링")
    .inner_size(420.0, 560.0)
    .resizable(false)
    .build()
    {
        eprintln!("failed to open pairing window: {e}");
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
fn list_paired_devices(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
) -> Vec<pairing::PairedDeviceView> {
    state.list_device_views()
}

#[tauri::command]
fn revoke_device(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    device_id: String,
) -> bool {
    state.revoke(&device_id)
}

#[tauri::command]
fn revoke_paired_device(
    state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>,
    device_id: String,
) -> bool {
    state.revoke(&device_id)
}

#[tauri::command]
fn revoke_all_devices(state: tauri::State<'_, std::sync::Arc<pairing::PairingServer>>) -> usize {
    state.revoke_all()
}

/// Async so the blocking `betterdisplaycli` spawn runs off the main thread
/// (Tauri 2 executes async commands on a separate thread pool).
#[tauri::command]
async fn create_virtual_display(name: String, width: u32, height: u32) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        virtual_display::create_virtual_display(&name, width, height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (name, width, height);
        Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
    }
}

/// Async so the blocking `betterdisplaycli` spawn runs off the main thread.
#[tauri::command]
async fn remove_virtual_display(name: String) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        virtual_display::remove_virtual_display(&name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
    }
}

#[tauri::command]
fn list_managed_displays(
    state: tauri::State<'_, display_management::DisplayManager>,
) -> Vec<display_management::ManagedDisplayView> {
    state.list()
}

#[tauri::command]
async fn add_managed_display(
    state: tauri::State<'_, display_management::DisplayManager>,
    provider_kind: String,
    name: String,
    width: u32,
    height: u32,
    scale: u8,
    position: String,
) -> Result<display_management::ManagedDisplayView, String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(error) = provider_kind_error(&provider_kind) {
            return Err(error);
        }
        let position = display_management::DisplayPosition::parse(&position)?;
        let manager = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            manager.add(
                provider_for_kind(&provider_kind),
                name,
                width,
                height,
                scale,
                position,
            )
        })
        .await
        .map_err(|error| format!("디스플레이 작업 실행 실패: {error}"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, provider_kind, name, width, height, scale, position);
        Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
    }
}

#[tauri::command]
async fn remove_managed_display(
    state: tauri::State<'_, display_management::DisplayManager>,
    id: String,
) -> Result<(), String> {
    let manager = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.remove(&id))
        .await
        .map_err(|error| format!("디스플레이 작업 실행 실패: {error}"))?
}

#[tauri::command]
async fn set_managed_display_position(
    state: tauri::State<'_, display_management::DisplayManager>,
    id: String,
    position: String,
) -> Result<display_management::ManagedDisplayView, String> {
    let position = display_management::DisplayPosition::parse(&position)?;
    let anchor = display_management::active_anchor_rect()
        .ok_or_else(|| "활성 주 화면이 없어 배치할 수 없습니다.".to_string())?;
    let manager = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.set_position(&id, position, anchor))
        .await
        .map_err(|error| format!("디스플레이 작업 실행 실패: {error}"))?
}

/// Validates the provider kind sent from the UI. CGVD stays opt-in behind the
/// `cgvirtualdisplay` kind; R-015 forbids promoting it to the default.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn provider_kind_error(kind: &str) -> Option<String> {
    match kind {
        "betterdisplay" | "cgvirtualdisplay" => None,
        other => Some(format!("지원하지 않는 엔진입니다: {other}")),
    }
}

/// Shared per-app session slot: one tablet display session at a time.
type TabletSessionRegistry = std::sync::Mutex<Option<clamshell_mode::TabletDisplaySession>>;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn provider_for_kind(kind: &str) -> std::sync::Arc<dyn provider::VirtualDisplayProvider> {
    match kind {
        "cgvirtualdisplay" => std::sync::Arc::new(provider::CgvdProvider::new()),
        _ => std::sync::Arc::new(provider::BetterDisplayProvider::new()),
    }
}

/// Async so blocking engine spawns run off the main thread (same rationale as
/// create_virtual_display above).
#[tauri::command]
async fn tablet_display_start(
    state: tauri::State<'_, TabletSessionRegistry>,
    provider_kind: String,
    name: String,
    width: u32,
    height: u32,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(error) = provider_kind_error(&provider_kind) {
            return Err(error);
        }
        let mut guard = state.lock().map_err(|error| error.to_string())?;
        if guard.is_some() {
            return Err("태블릿 화면 세션이 이미 실행 중입니다.".into());
        }
        let session = clamshell_mode::TabletDisplaySession::start(
            provider_for_kind(&provider_kind),
            &provider::DisplaySpec {
                name,
                width,
                height,
                scale: 1,
            },
        )
        .map_err(|error| error.message())?;
        *guard = Some(session);
        Ok("태블릿 화면 세션 시작됨".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, provider_kind, name, width, height);
        Err("태블릿 화면 확장은 macOS에서만 지원됩니다.".into())
    }
}

/// Async so the blocking engine teardown spawn runs off the main thread.
#[tauri::command]
async fn tablet_display_stop(
    state: tauri::State<'_, TabletSessionRegistry>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let mut guard = state.lock().map_err(|error| error.to_string())?;
        match guard.take() {
            Some(session) => {
                drop(session); // Drop removes the VD and kills caffeinate.
                Ok("태블릿 화면 세션을 정리했습니다.".into())
            }
            None => Ok("실행 중인 세션이 없습니다.".into()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Err("태블릿 화면 확장은 macOS에서만 지원됩니다.".into())
    }
}

/// Async (matching tablet_display_start/stop): while streaming, the reported
/// string is derived by probing `ioreg` for the lid state, which is a real
/// process spawn that must not run on the main thread. The lid read is UI
/// display only — the stored ModeState is never mutated and no control path
/// branches on it. The reported decision lives in the pure
/// `clamshell_mode::reported_status` (tested without any spawn); an
/// indeterminate lid reading degrades to "streaming", never "clamshell".
/// The battery flag recorded at start rides the same string as a
/// ";battery" suffix (design: 배터리 `-i` 강등 + UI 경고).
#[tauri::command]
async fn tablet_display_status(
    state: tauri::State<'_, TabletSessionRegistry>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        // Block scope (not drop()) so the MutexGuard's borrow provably ends
        // before the await below — the tauri command future must be Send.
        let (session_state, on_battery) = {
            let guard = state.lock().map_err(|error| error.to_string())?;
            match guard.as_ref() {
                Some(session) => (session.state.clone(), session.on_battery),
                None => (clamshell_mode::ModeState::Idle, false),
            }
        };
        // Probe outside the registry lock so a (capped) ioreg hang cannot
        // block start/stop on the same mutex.
        let lid_closed = if session_state == clamshell_mode::ModeState::Streaming {
            clamshell_mode::read_lid_closed().await
        } else {
            None
        };
        Ok(clamshell_mode::reported_status(
            &session_state,
            lid_closed,
            on_battery,
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Err("태블릿 화면 확장은 macOS에서만 지원됩니다.".into())
    }
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

    #[test]
    fn provider_kind_selects_the_registered_engines() {
        assert!(super::provider_kind_error("betterdisplay").is_none());
        assert!(super::provider_kind_error("cgvirtualdisplay").is_none());
        let unknown = super::provider_kind_error("duet").unwrap();
        assert!(unknown.contains("지원하지 않는"));
    }
}
