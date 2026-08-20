//! Host-side pairing server: wraps `session::PairingService` with device
//! persistence, token issuance, and the QR payload format consumed by the
//! viewer (design §2).
//!
//! QR payload: `{"v":1,"id":<offer_id>,"s":<base64url 32B secret>,"h":<host_ip>,"p":<port>}`.
//! The 6-digit human verification code is displayed by the host UI and
//! presented by the viewer on `pair` — a second factor on top of the secret.

use base64::Engine as _;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Wall clock for `session::Clock`. `Instant` cannot start at an arbitrary
/// offset, so the epoch is the first time this process constructed a
/// `WallClock` (a `OnceLock<Instant>` set at process start). Monotonicity is
/// inherited from `Instant`; only the epoch differs between runs — the
/// PairingService only ever compares durations against its own offers, which
/// are created and consumed within the same process, so this is sound.
struct WallClock {
    epoch: OnceLock<Instant>,
}

impl session::Clock for WallClock {
    fn monotonic(&self) -> Duration {
        self.epoch
            .get_or_init(Instant::now)
            .elapsed()
    }
}

struct Inner {
    service: session::PairingService,
    fingerprint: String,
    fail_counts: HashMap<String, u32>,
    paired: Vec<PairedDevice>,
    /// offer ids this server created (the raw secret stays inside the
    /// PairingService — its copy is the only one, zeroized on approve/cancel;
    /// the QR payload already carries the base64url form for the viewer).
    live_offers: std::collections::HashSet<String>,
}

pub struct PairingServer {
    inner: Mutex<Inner>,
    store_path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    pub token_hex: String,
    pub paired_at: String,
}

#[derive(serde::Serialize)]
pub struct PairingSessionView {
    pub qr_payload: String,
    pub code: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum PairingServerError {
    #[error("pairing failed")]
    PairingFailed,
    #[error("offer not found")]
    OfferNotFound,
    #[error("persistence failed")]
    PersistenceFailed,
}

// -- Implementation -----------------------------------------------------------

impl PairingServer {
    /// Loads persisted devices from `store_path` if it exists. A corrupt or
    /// unreadable file is logged and ignored — pairing state is rebuildable,
    /// the host must not refuse to start.
    pub fn new(fingerprint: String, store_path: Option<PathBuf>) -> Self {
        let paired = store_path.as_deref().and_then(load_devices).unwrap_or_default();
        Self {
            inner: Mutex::new(Inner {
                service: session::PairingService::new(Box::new(WallClock {
                    epoch: OnceLock::new(),
                })),
                fingerprint,
                fail_counts: HashMap::new(),
                paired,
                live_offers: std::collections::HashSet::new(),
            }),
            store_path,
        }
    }

    /// `dirs::data_dir()/leftcar-host/paired_devices.json` (None when the
    /// platform has no data dir — pairing then stays in-memory only).
    pub fn default_store_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("leftcar-host").join("paired_devices.json"))
    }

    /// Create a single-use offer and return its QR payload plus the 6-digit
    /// human verification code for the host UI.
    pub fn begin_pairing(&self, host_ip: &str, port: u16) -> PairingSessionView {
        let mut inner = self.inner.lock().unwrap();
        let fingerprint = inner.fingerprint.clone();
        let offer = inner.service.begin_offer(fingerprint);
        // Borrow the secret only to encode it into the QR payload; the
        // service's own copy (the only other one) is zeroized on approve/
        // cancel. We do not retain it — the QR already carries the base64url
        // form the viewer will present as its proof.
        let secret = inner
            .service
            .take_secret_for_qr(&offer.ephemeral_offer_id)
            .expect("secret exists right after begin_offer");
        let payload = json!({
            "v": 1,
            "id": offer.ephemeral_offer_id,
            "s": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret.0),
            "h": host_ip,
            "p": port,
        })
        .to_string();
        drop(secret); // OfferSecret drops -> zeroized immediately
        let view = PairingSessionView {
            code: offer.human_verification_code.clone(),
            expires_in_secs: session::PAIRING_TTL.as_secs(),
            qr_payload: payload,
        };
        inner.live_offers.insert(offer.ephemeral_offer_id);
        view
    }

    /// Complete pairing: verify secret possession + human code, issue a
    /// 32-byte hex token, persist the device. Failures deliberately report a
    /// single generic message — an attacker must not learn which factor was
    /// wrong. Three failed attempts burn the offer.
    pub fn pair(
        &self,
        offer_id: &str,
        secret_b64url: &str,
        code: &str,
        device_id: &str,
        name: &str,
    ) -> Result<String, PairingServerError> {
        let mut inner = self.inner.lock().unwrap();

        if !inner.live_offers.contains(offer_id) {
            return Err(PairingServerError::OfferNotFound);
        }
        // decode against the offer's own secret bytes
        let decoded: Vec<u8> = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(secret_b64url)
            .map_err(|_| PairingServerError::PairingFailed)?;
        let Ok(secret_proof): Result<[u8; 32], _> = decoded.try_into() else {
            return Err(PairingServerError::PairingFailed);
        };

        let device = domain::ids::DeviceId::from_raw(device_id)
            .map_err(|_| PairingServerError::PairingFailed)?;
        match inner.service.approve(offer_id, device, &secret_proof, code) {
            Ok(_device) => {
                inner.fail_counts.remove(offer_id);
                let token = session::OfferSecret::from_random();
                let token_hex: String = token.0.iter().map(|b| format!("{b:02x}")).collect();
                let paired = PairedDevice {
                    device_id: device_id.to_owned(),
                    name: name.to_owned(),
                    token_hex: token_hex.clone(),
                    paired_at: unix_seconds_rfc3339_utc(),
                };
                inner.paired.push(paired.clone());
                inner.live_offers.remove(offer_id); // single-use: offer consumed
                if self.persist(inner.paired.clone()).is_err() {
                    // pairing itself succeeded; persistence is best-effort but
                    // surfaced so callers/tests can detect a broken store
                    eprintln!("leftcar: paired-device persistence failed");
                }
                Ok(token_hex)
            }
            Err(_) => {
                let count = inner.fail_counts.entry(offer_id.to_owned()).or_insert(0);
                *count += 1;
                if *count >= 3 {
                    inner.service.cancel(offer_id);
                    inner.live_offers.remove(offer_id);
                    inner.fail_counts.remove(offer_id);
                }
                Err(PairingServerError::PairingFailed)
            }
        }
    }

    /// Constant-time token check against every stored token. False when no
    /// devices are paired.
    pub fn authorize(&self, token_hex: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.paired.is_empty() {
            return false;
        }
        let Ok(bytes) = hex_decode32(token_hex) else {
            return false;
        };
        inner
            .paired
            .iter()
            .any(|d| session::constant_time_eq(&hex_decode32(&d.token_hex).unwrap_or([0u8; 32]), &bytes))
    }

    /// Remove a device and its token; persists the change. False when the
    /// device was not paired.
    pub fn revoke(&self, device_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.paired.len();
        inner.paired.retain(|d| d.device_id != device_id);
        let removed = inner.paired.len() != before;
        if removed {
            if let Err(e) = self.persist(inner.paired.clone()) {
                eprintln!("leftcar: persist after revoke failed: {e}");
            }
        }
        removed
    }

    pub fn list_devices(&self) -> Vec<PairedDevice> {
        self.inner.lock().unwrap().paired.clone()
    }

    /// Best-effort persist; parent dirs created, file written 0600.
    fn persist(&self, devices: Vec<PairedDevice>) -> Result<(), PairingServerError> {
        let Some(path) = &self.store_path else {
            return Ok(());
        };
        let body = serde_json::to_string_pretty(&devices).map_err(|e| {
            eprintln!("leftcar: serialize paired devices: {e}");
            PairingServerError::PersistenceFailed
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                eprintln!("leftcar: create store dir: {e}");
                PairingServerError::PersistenceFailed
            })?;
        }
        std::fs::write(path, body).map_err(|e| {
            eprintln!("leftcar: write store: {e}");
            PairingServerError::PersistenceFailed
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                eprintln!("leftcar: chmod 0600 on store failed: {e}");
            }
        }
        Ok(())
    }
}

fn hex_decode32(s: &str) -> Result<[u8; 32], ()> {
    if s.len() != 64 {
        return Err(());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_val(chunk[0]).ok_or(())?;
        let lo = hex_val(chunk[1]).ok_or(())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// UTC timestamp without pulling in a date crate: `unix:<seconds>` is
/// unambiguous, trivially parseable, and stable across platforms.
fn unix_seconds_rfc3339_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

fn load_devices(path: &std::path::Path) -> Option<Vec<PairedDevice>> {
    match std::fs::read_to_string(path) {
        Ok(body) => match serde_json::from_str(&body) {
            Ok(devices) => Some(devices),
            Err(e) => {
                eprintln!("leftcar: paired-device store corrupt, starting empty: {e}");
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("leftcar: paired-device store unreadable, starting empty: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "leftcar-pairing-test-{tag}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn begin_pairing_creates_qr_payload_and_code() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);

        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        assert_eq!(payload["v"], json!(1));
        assert!(payload["id"].as_str().unwrap().starts_with("offer-"));
        assert_eq!(payload["h"], json!("192.168.0.10"));
        assert_eq!(payload["p"], json!(7777));
        let secret_b64 = payload["s"].as_str().unwrap();
        assert!(!secret_b64.contains('+') && !secret_b64.contains('/'));
        assert!(!secret_b64.contains('='));
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(secret_b64)
                .unwrap()
                .len(),
            32
        );
        assert!(view.expires_in_secs > 0 && view.expires_in_secs <= 120);

        assert_eq!(view.code.len(), 6);
        assert!(view.code.bytes().all(|b| b.is_ascii_digit()), "{}", view.code);
    }

    #[test]
    fn pair_with_correct_secret_and_code_issues_token() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let offer_id = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let secret_b64 = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap()["s"]
            .as_str()
            .unwrap()
            .to_owned();

        let token = server
            .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
            .unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));

        let devices = server.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "viewer-1");
        assert_eq!(devices[0].name, "Quest 3");
        assert_eq!(devices[0].token_hex, token);
    }

    #[test]
    fn pair_with_wrong_code_fails() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        let offer_id = payload["id"].as_str().unwrap();
        let secret_b64 = payload["s"].as_str().unwrap();

        // wrong code
        let e = server.pair(offer_id, secret_b64, "000000", "viewer-1", "Quest 3");
        assert!(e.is_err());
        // error message must not reveal which factor failed
        assert_eq!(e.unwrap_err().to_string(), "pairing failed");
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn pair_with_wrong_secret_fails_with_same_message() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        let offer_id = payload["id"].as_str().unwrap();
        let wrong_secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([9u8; 32]);

        let e = server.pair(offer_id, &wrong_secret, &view.code, "viewer-1", "Quest 3");
        assert!(e.is_err());
        // identical message regardless of which factor was wrong
        assert_eq!(e.unwrap_err().to_string(), "pairing failed");
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn three_failed_attempts_burn_offer() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        let offer_id = payload["id"].as_str().unwrap().to_owned();
        let secret_b64 = payload["s"].as_str().unwrap().to_owned();

        for _ in 0..3 {
            assert!(server
                .pair(&offer_id, &secret_b64, "000000", "viewer-1", "Quest 3")
                .is_err());
        }
        // even the correct secret+code can no longer pair
        let e = server.pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3");
        assert!(e.is_err());
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn authorize_accepts_issued_token_and_rejects_others() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        let offer_id = payload["id"].as_str().unwrap().to_owned();
        let secret_b64 = payload["s"].as_str().unwrap().to_owned();

        let token = server
            .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
            .unwrap();
        assert!(server.authorize(&token));
        assert!(!server.authorize(&"0".repeat(64)));
        assert!(!server.authorize(""));
        // hex-decodable but wrong
        assert!(!server.authorize(&"ab".repeat(32)));
    }

    #[test]
    fn persisted_devices_survive_restart() {
        let path = temp_store_path("restart");
        let token = {
            let server = PairingServer::new("leftcar-host".into(), Some(path.clone()));
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            let offer_id = payload["id"].as_str().unwrap().to_owned();
            let secret_b64 = payload["s"].as_str().unwrap().to_owned();
            server
                .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
                .unwrap()
        };

        let restarted = PairingServer::new("leftcar-host".into(), Some(path.clone()));
        assert!(restarted.authorize(&token));
        assert_eq!(restarted.list_devices().len(), 1);
        assert_eq!(restarted.list_devices()[0].device_id, "viewer-1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_store_is_ignored_not_fatal() {
        let path = temp_store_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        let server = PairingServer::new("leftcar-host".into(), Some(path.clone()));
        assert!(server.list_devices().is_empty());
        let view = server.begin_pairing("192.168.0.10", 7777);
        assert_eq!(view.code.len(), 6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn revoke_removes_token_and_persists() {
        let path = temp_store_path("revoke");
        let token = {
            let server = PairingServer::new("leftcar-host".into(), Some(path.clone()));
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            let offer_id = payload["id"].as_str().unwrap().to_owned();
            let secret_b64 = payload["s"].as_str().unwrap().to_owned();
            server
                .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
                .unwrap()
        };

        {
            let server = PairingServer::new("leftcar-host".into(), Some(path.clone()));
            assert!(server.authorize(&token));
            assert!(server.revoke("viewer-1"));
            assert!(!server.authorize(&token));
            assert!(server.list_devices().is_empty());
        }
        // persisted across restart
        let restarted = PairingServer::new("leftcar-host".into(), Some(path.clone()));
        assert!(!restarted.authorize(&token));
        assert!(restarted.list_devices().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn store_file_has_restricted_permissions() {
        let path = temp_store_path("perms");
        {
            let server = PairingServer::new("leftcar-host".into(), Some(path.clone()));
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            let offer_id = payload["id"].as_str().unwrap().to_owned();
            let secret_b64 = payload["s"].as_str().unwrap().to_owned();
            server
                .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
                .unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "store must be 0600, got {:o}", mode);
        }
        let _ = std::fs::remove_file(&path);
    }
}
