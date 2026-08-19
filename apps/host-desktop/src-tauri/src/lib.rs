//! Leftcar desktop host (Tauri 2) — control server + capture orchestration.
//!
//! Design: docs/plans/2026-08-18-rn-tauri-rebuild-design.md

pub mod backend;
pub mod control;
pub mod ffi;

use backend::SharedBackend;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Real shim FFI when the dylib is available; FakeBackend otherwise (UI dev).
    let backend: SharedBackend = match ffi::FfiBackend::new() {
        Ok(b) => {
            println!("{}", ffi::dylib_report());
            Arc::new(b)
        }
        Err(e) => {
            eprintln!("FFI backend unavailable ({e}) — falling back to FakeBackend");
            Arc::new(backend::FakeBackend { displays: vec![] })
        }
    };
    let warmup_backend = backend.clone();
    let server = std::sync::Arc::new(control::ControlServer::new(backend.clone()));
    start_control_server(server.clone());
    // Advertise independently from the Tauri window so a viewer can connect
    // while WebView/AppKit initialization is still in progress.
    if let Err(e) = advertise_mdns() {
        eprintln!("mDNS advertise failed: {e}");
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_status])
        .setup(move |app| {
            app.manage(server);
            warm_display_catalog(warmup_backend);

            let show_item = MenuItem::with_id(app, "show", "Leftcar Host 열기", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Leftcar Host 종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
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
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Closing the dashboard hides it; the control server, mDNS
                // advertisement, and active capture sessions keep running.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("tauri run");
}

/// Prime ScreenCaptureKit before the first viewer opens the source catalog.
/// The first macOS enumeration can take several seconds; later calls are
/// served by the capture shim's stale-while-refresh cache.
fn warm_display_catalog(backend: SharedBackend) {
    let _ = std::thread::Builder::new()
        .name("leftcar-catalog-warmup".into())
        .spawn(move || {
            // Tauri setup runs before AppKit has fully entered its event loop.
            // Let that loop become live before asking ScreenCaptureKit to
            // enumerate shareable content; an immediate CoreGraphics catalog
            // remains available to control clients during this short delay.
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
fn start_control_server(server: std::sync::Arc<control::ControlServer>) {
    std::thread::Builder::new()
        .name("leftcar-control".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime");
            runtime.block_on(async move {
                match tokio::net::TcpListener::bind("0.0.0.0:7777").await {
                    Ok(listener) => {
                        println!("control server on {}", listener.local_addr().unwrap());
                        server.run(listener).await;
                    }
                    Err(e) => eprintln!("control bind failed: {e}"),
                }
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

/// Register `_leftcar._tcp.local.` pointing at this host's LAN IP:7777.
/// The ServiceDaemon is leaked on purpose — it must outlive the app setup.
fn advertise_mdns() -> Result<(), String> {
    use std::collections::HashMap;
    let daemon =
        mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e:?}"))?;
    let ip = local_lan_ip().ok_or("no LAN interface found")?;
    let info = mdns_sd::ServiceInfo::new(
        "_leftcar._tcp.local.",
        "leftcar-host",
        "leftcar-host.local.",
        &ip,
        7777,
        // Android's NSD resolver rejects an empty TXT property set on some
        // vendor builds ("Key cannot be empty"). Keep one stable property so
        // multiple hosts can still be resolved and listed independently.
        HashMap::from([(String::from("product"), String::from("leftcar"))]),
    )
    .map_err(|e| format!("mdns service info: {e:?}"))?;
    daemon
        .register(info)
        .map_err(|e| format!("mdns register: {e:?}"))?;
    println!("mDNS: leftcar-host._leftcar._tcp.local. at {ip}:7777");
    std::mem::forget(daemon);
    Ok(())
}

/// Best-effort local interface address (UDP connect trick — no packets sent).
fn local_lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}
