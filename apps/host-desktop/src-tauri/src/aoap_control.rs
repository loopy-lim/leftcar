//! AOAP control-channel relay and accessory discovery.

use crate::aoap::{find_accessory, handshake_candidate, install_usb_link, start_accessory};
use crate::control::ControlServer;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    token: Option<String>,
}

const USB_NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(3);

fn usb_request_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Dispatch one newline-delimited control request received on mux channel 0.
/// USB is a physical, already-paired transport; authorization is still
/// required and the token remains the same as the Wi-Fi control session.
pub async fn dispatch_control_line(
    server: &Arc<ControlServer>,
    line: String,
    peer: &str,
) -> String {
    let envelope: Envelope = match serde_json::from_str(&line) {
        Ok(envelope) => envelope,
        Err(_) => return json!({"ok": false, "error": "bad request"}).to_string(),
    };
    if envelope.command != "pair"
        && !server.authorize_token(envelope.token.as_deref().unwrap_or(""))
    {
        return json!({"ok": false, "error": "unauthorized"}).to_string();
    }
    serde_json::to_string(
        &server
            .dispatch(&envelope.command, envelope.args, peer)
            .await,
    )
    .unwrap_or_else(|_| json!({"ok": false, "error": "serialization failed"}).to_string())
}

/// Start passive nusb discovery. Normal USB devices are left untouched;
/// AOAP negotiation is initiated by `ensure_usb_accessory` after a stream
/// request. Accessory mode re-enumerates after START, so this watcher opens
/// the re-enumerated accessory independently of the control request.
pub fn start_usb_watcher(server: Arc<ControlServer>) {
    thread::Builder::new()
        .name("leftcar-aoap-watch".into())
        .spawn(move || {
            // nusb's hotplug stream avoids polling while still allowing the
            // short retry window needed for Windows composite interfaces.
            let Ok(watch) = nusb::watch_devices() else {
                poll_for_usb_devices(server);
                return;
            };
            if let Ok(devices) = nusb::list_devices() {
                for device in devices {
                    handle_device(&server, device);
                }
            }
            futures_lite::future::block_on(async move {
                use futures_lite::StreamExt;
                let mut watch = watch;
                while let Some(event) = watch.next().await {
                    if let nusb::hotplug::HotplugEvent::Connected(device) = event {
                        handle_device(&server, device);
                    }
                }
            });
        })
        .expect("AOAP watcher thread");
}

fn poll_for_usb_devices(server: Arc<ControlServer>) {
    loop {
        if let Ok(devices) = nusb::list_devices() {
            for device in devices {
                handle_device(&server, device);
            }
        }
        thread::sleep(Duration::from_millis(750));
    }
}

fn handle_device(server: &Arc<ControlServer>, device: nusb::DeviceInfo) {
    if crate::aoap::has_usb_link() {
        return;
    }
    if find_accessory(&device) {
        match crate::aoap::AccessoryLink::open(&device) {
            Ok(link) => {
                let control_rx = install_usb_link(link);
                spawn_control_relay(server.clone(), control_rx);
            }
            Err(error) => eprintln!("AOAP accessory link unavailable: {error}"),
        }
    }
}

fn should_start_accessory(handshake_requested: bool, has_link: bool, is_accessory: bool) -> bool {
    handshake_requested && !has_link && !is_accessory
}

/// Request AOAP only for an active USB stream request. The watcher above is
/// responsible for opening the accessory after the device re-enumerates.
pub async fn ensure_usb_accessory() -> Result<(), String> {
    if crate::aoap::has_usb_link() {
        return Ok(());
    }

    let started = {
        let _guard = usb_request_lock()
            .lock()
            .map_err(|_| "USB AOAP request lock is poisoned".to_owned())?;
        if crate::aoap::has_usb_link() {
            true
        } else {
            let devices = nusb::list_devices()
                .map_err(|error| format!("USB 장치를 조회하지 못했습니다: {error}"))?;
            let mut started = false;
            for device in devices {
                if !should_start_accessory(true, false, find_accessory(&device))
                    || !handshake_candidate(&device)
                {
                    continue;
                }
                match start_accessory(&device) {
                    Ok(()) => {
                        started = true;
                        break;
                    }
                    Err(error) => eprintln!("AOAP negotiation candidate rejected: {error}"),
                }
            }
            started
        }
    };

    if !started && !crate::aoap::has_usb_link() {
        return Err("AOAP를 시작할 수 있는 USB 장치를 찾지 못했습니다".into());
    }

    let deadline = Instant::now() + USB_NEGOTIATION_TIMEOUT;
    while !crate::aoap::has_usb_link() {
        if Instant::now() >= deadline {
            return Err("AOAP 액세서리 재연결을 기다리는 시간이 초과되었습니다".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

fn spawn_control_relay(server: Arc<ControlServer>, control_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    thread::Builder::new()
        .name("leftcar-aoap-control".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("AOAP control runtime");
            for line in control_rx {
                let response = runtime.block_on(dispatch_control_line(
                    &server,
                    String::from_utf8_lossy(&line).into_owned(),
                    "usb",
                ));
                let mut payload = response.into_bytes();
                payload.push(b'\n');
                let Some(sender) = crate::aoap::usb_control_sender() else {
                    break;
                };
                if sender
                    .send(usb_mux::MuxFrame {
                        channel: usb_mux::CHANNEL_CONTROL,
                        payload,
                    })
                    .is_err()
                {
                    break;
                }
            }
            crate::aoap::clear_usb_link();
        })
        .expect("AOAP control relay thread");
}

#[cfg(test)]
mod tests {
    use super::should_start_accessory;

    #[test]
    fn passive_usb_attach_does_not_start_aoap() {
        assert!(!should_start_accessory(false, false, false));
    }

    #[test]
    fn explicit_usb_request_starts_aoap_for_normal_device() {
        assert!(should_start_accessory(true, false, false));
    }

    #[test]
    fn accessory_or_existing_link_never_restarts_handshake() {
        assert!(!should_start_accessory(true, false, true));
        assert!(!should_start_accessory(true, true, false));
    }
}
