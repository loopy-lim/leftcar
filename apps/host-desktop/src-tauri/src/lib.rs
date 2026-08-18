//! Leftcar desktop host (Tauri 2) — control server + capture orchestration.
//!
//! Design: docs/plans/2026-08-18-rn-tauri-rebuild-design.md

pub mod backend;
pub mod control;

use backend::SharedBackend;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let backend: SharedBackend = Arc::new(backend::FakeBackend { displays: vec![] }); // FFI backend lands in Task 5
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
