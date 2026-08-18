//! Leftcar desktop host (Tauri 2) — control server + capture orchestration.
//!
//! Design: docs/plans/2026-08-18-rn-tauri-rebuild-design.md

pub mod backend;
pub mod control;
pub mod ffi;

use backend::SharedBackend;
use std::sync::Arc;
use tauri::Manager;

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
    let server = std::sync::Arc::new(control::ControlServer::new(backend.clone()));

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_status])
        .setup(move |app| {
            let server_for_task = server.clone();
            tauri::async_runtime::spawn(async move {
                match tokio::net::TcpListener::bind("0.0.0.0:7777").await {
                    Ok(listener) => {
                        println!("control server on {}", listener.local_addr().unwrap());
                        server_for_task.run(listener).await;
                    }
                    Err(e) => eprintln!("control bind failed: {e}"),
                }
            });
            // advertise _leftcar._tcp so NSD viewers can find us (design §발견)
            if let Err(e) = advertise_mdns() {
                eprintln!("mDNS advertise failed: {e}");
            }
            app.manage(server);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run");
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
        None::<HashMap<String, String>>,
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
