//! Host-side pairing server: wraps `session::PairingService` with device
//! persistence, token issuance, and the QR payload format consumed by the
//! viewer (design §2).
//!
//! QR payload: `{"v":1,"id":<offer_id>,"s":<base64url 32B secret>,"h":<host_ip>,"p":<port>}`.
//! The 6-digit human verification code is shown separately by the host UI and
//! presented by the viewer on `pair` — it must never be embedded in the QR.

use base64::Engine as _;
use serde_json::json;
use std::collections::{HashMap, HashSet};
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
        self.epoch.get_or_init(Instant::now).elapsed()
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
    /// QR 시크릿을 제시했지만 Mac 사용자의 허용을 기다리는 요청.
    pending: HashMap<String, PendingPairing>,
    /// 승인이 끝나고 뷰어 폴링이 픽업할 토큰. 픽업 요청이 같은 시크릿의
    /// 소지자임을 다시 증명하도록 사본을 함께 보관한다.
    completed: HashMap<String, CompletedPairing>,
    /// Mac 사용자가 명시적으로 거절한 offer. 폴링에 "거절됨"을 알리기 위해
    /// 다음 offer 생성까지 보관한다.
    rejected: HashSet<String>,
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

/// Redacted device metadata safe to expose to the webview. Authentication
/// tokens stay inside the Rust host process and its restricted local store.
#[derive(serde::Serialize, Clone)]
pub struct PairedDeviceView {
    pub device_id: String,
    pub name: String,
    pub paired_at: String,
}

#[derive(serde::Serialize)]
pub struct PairingSessionView {
    pub qr_payload: String,
    pub code: String,
    pub expires_in_secs: u64,
}

/// QR 시크릿을 제시했지만 아직 Mac 사용자의 허용을 기다리는 요청.
#[derive(Clone)]
pub struct PendingPairing {
    pub device_id: String,
    pub device_name: String,
    pub requested_at: String,
    requested_at_instant: Instant,
}

/// 웹뷰 승인 카드에 필요한 최소 정보.
#[derive(serde::Serialize, Clone)]
pub struct PendingPairingView {
    pub offer_id: String,
    pub device_name: String,
    pub requested_at: String,
}

/// 승인 완료 후 뷰어 폴링이 픽업하는 레코드. 메모리에만 살고 다음 offer가
/// 만들어지면 지워진다. 승인은 특정 기기의 요청에 대한 것이므로 픽업도 그
/// 기기만 할 수 있다 — 같은 QR을 찍은 다른 기기는 토큰을 받을 수 없다.
struct CompletedPairing {
    device_id: String,
    token: String,
    secret: session::OfferSecret,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PairingServerError {
    #[error("pairing failed")]
    PairingFailed,
    #[error("offer not found")]
    OfferNotFound,
    #[error("persistence failed")]
    PersistenceFailed,
    /// 시크릿은 맞지만 Mac 사용자의 허용이 아직 없다. 오류가 아니라 폴링
    /// 축이 해석하는 상태 신호다.
    #[error("pairing pending approval")]
    Pending,
    /// Mac 사용자가 명시적으로 거절했다.
    #[error("pairing rejected")]
    Rejected,
}

// -- Implementation -----------------------------------------------------------

impl PairingServer {
    /// Loads persisted devices from `store_path` if it exists. A corrupt or
    /// unreadable file is logged and ignored — pairing state is rebuildable,
    /// the host must not refuse to start.
    pub fn new(fingerprint: String, store_path: Option<PathBuf>) -> Self {
        let paired = store_path
            .as_deref()
            .and_then(load_devices)
            .unwrap_or_default();
        Self {
            inner: Mutex::new(Inner {
                service: session::PairingService::new(Box::new(WallClock {
                    epoch: OnceLock::new(),
                })),
                fingerprint,
                fail_counts: HashMap::new(),
                paired,
                live_offers: std::collections::HashSet::new(),
                pending: HashMap::new(),
                completed: HashMap::new(),
                rejected: HashSet::new(),
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
        // Replacing the QR must invalidate every older image immediately. A
        // screenshot of a superseded offer must not remain usable for 2 min.
        let stale_offer_ids: Vec<String> = inner.live_offers.drain().collect();
        for offer_id in stale_offer_ids {
            inner.service.cancel(&offer_id);
        }
        inner.fail_counts.clear();
        // 새 QR은 이전 offer의 승인 대기·완료·거절 기록도 모두 무효화한다.
        inner.pending.clear();
        inner.completed.clear();
        inner.rejected.clear();

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
    ///
    /// `code`가 비어 있으면 승인 기반 경로다: 시크릿 소지만 증명하고 Mac
    /// 사용자의 [허용]을 기다린다([PairingServer::approve_pending]).
    pub fn pair(
        &self,
        offer_id: &str,
        secret_b64url: &str,
        code: &str,
        device_id: &str,
        name: &str,
    ) -> Result<String, PairingServerError> {
        let mut inner = self.inner.lock().unwrap();

        if code.is_empty() {
            return Self::pair_awaiting_approval(&mut inner, offer_id, secret_b64url, device_id, name);
        }

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
            Ok(_device) => Ok(self.complete_pairing(&mut inner, offer_id, device_id, name)),
            Err(_) => {
                Self::record_pair_failure(&mut inner, offer_id);
                Err(PairingServerError::PairingFailed)
            }
        }
    }

    /// Complete pairing through a direct Host connection using only the
    /// six-digit code displayed in the Host window. The raw offer secret is
    /// resolved inside the Host process; it is never sent by the Viewer.
    pub fn pair_by_code(
        &self,
        code: &str,
        device_id: &str,
        name: &str,
    ) -> Result<String, PairingServerError> {
        let mut inner = self.inner.lock().unwrap();
        let Some(offer_id) = inner.service.find_offer_by_code(code) else {
            let live: Vec<String> = inner.live_offers.iter().cloned().collect();
            for offer_id in live {
                Self::record_pair_failure(&mut inner, &offer_id);
            }
            return Err(PairingServerError::PairingFailed);
        };

        if !inner.live_offers.contains(&offer_id) {
            return Err(PairingServerError::OfferNotFound);
        }

        let Some(secret) = inner.service.take_secret_for_qr(&offer_id) else {
            return Err(PairingServerError::PairingFailed);
        };
        let device = domain::ids::DeviceId::from_raw(device_id)
            .map_err(|_| PairingServerError::PairingFailed)?;

        match inner.service.approve(&offer_id, device, &secret.0, code) {
            Ok(_device) => Ok(self.complete_pairing(&mut inner, &offer_id, device_id, name)),
            Err(_) => {
                Self::record_pair_failure(&mut inner, &offer_id);
                Err(PairingServerError::PairingFailed)
            }
        }
    }

    /// Mint the pairing token, register the device, consume the offer, and
    /// persist. Shared success tail of every pairing flow.
    fn complete_pairing(
        &self,
        inner: &mut Inner,
        offer_id: &str,
        device_id: &str,
        name: &str,
    ) -> String {
        inner.fail_counts.remove(offer_id);
        let token = session::OfferSecret::from_random();
        let token_hex: String = token.0.iter().map(|b| format!("{b:02x}")).collect();
        let paired = PairedDevice {
            device_id: device_id.to_owned(),
            name: name.to_owned(),
            token_hex: token_hex.clone(),
            paired_at: unix_timestamp_utc(),
        };
        inner
            .paired
            .retain(|d| d.device_id != device_id && (name.is_empty() || d.name != name));
        inner.paired.push(paired);
        inner.live_offers.remove(offer_id); // single-use: offer consumed
        if self.persist(inner.paired.clone()).is_err() {
            // pairing itself succeeded; persistence is best-effort but
            // surfaced so callers/tests can detect a broken store
            eprintln!("leftcar: paired-device persistence failed");
        }
        token_hex
    }

    /// Count a failed attempt against `offer_id`; three failures burn the
    /// offer (docs §7: 무차별 시도는 오퍼 폐기로 끝난다).
    fn record_pair_failure(inner: &mut Inner, offer_id: &str) {
        let count = inner.fail_counts.entry(offer_id.to_owned()).or_insert(0);
        *count += 1;
        if *count >= 3 {
            inner.service.cancel(offer_id);
            inner.live_offers.remove(offer_id);
            inner.fail_counts.remove(offer_id);
        }
    }

    /// 승인 기반 페어링의 뷰어 측 폴링. 시크릿 소지를 증명하면 대기 요청을
    /// 등록하고 `Pending`을 돌려준다. Mac 사용자가 허용하면 완료 레코드의
    /// 토큰을, 거절하면 `Rejected`를 돌려준다.
    ///
    /// docs §7.3의 "QR 스캔만으로 승인하지 않는다"는 규칙을 지킨다 — 시크릿
    /// 제시는 요청 등록일 뿐이고 승인은 Host 화면의 허용 행동이다.
    fn pair_awaiting_approval(
        inner: &mut Inner,
        offer_id: &str,
        secret_b64url: &str,
        device_id: &str,
        name: &str,
    ) -> Result<String, PairingServerError> {
        if inner.rejected.contains(offer_id) {
            return Err(PairingServerError::Rejected);
        }
        let decoded: Vec<u8> = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(secret_b64url)
            .map_err(|_| PairingServerError::PairingFailed)?;
        let Ok(proof) = <[u8; 32]>::try_from(decoded.as_slice()) else {
            return Err(PairingServerError::PairingFailed);
        };
        if let Some(record) = inner.completed.get(offer_id) {
            if record.device_id == device_id
                && session::constant_time_eq(&record.secret.0, &proof)
            {
                return Ok(record.token.clone());
            }
            return Err(PairingServerError::PairingFailed);
        }
        if !inner.live_offers.contains(offer_id) {
            return Err(PairingServerError::OfferNotFound);
        }
        let expected = inner
            .service
            .take_secret_for_qr(offer_id)
            .ok_or(PairingServerError::PairingFailed)?;
        if !session::constant_time_eq(&expected.0, &proof) {
            Self::record_pair_failure(inner, offer_id);
            return Err(PairingServerError::PairingFailed);
        }
        inner.pending.insert(
            offer_id.to_owned(),
            PendingPairing {
                device_id: device_id.to_owned(),
                device_name: truncate_label(name),
                requested_at: unix_timestamp_utc(),
                requested_at_instant: Instant::now(),
            },
        );
        Err(PairingServerError::Pending)
    }

    /// Mac 사용자가 승인 카드에서 [허용]을 눌렀다. 대기 요청을 실제 페어링으로
    /// 바꾸고 토큰을 완료 레코드에 올려 뷰어 폴링이 픽업하게 한다.
    pub fn approve_pending(&self, offer_id: &str) -> Result<(), PairingServerError> {
        let mut inner = self.inner.lock().unwrap();
        prune_stale_requests(&mut inner);
        let request = inner
            .pending
            .remove(offer_id)
            .ok_or(PairingServerError::OfferNotFound)?;
        let approved_device_id = request.device_id.clone();
        let Some(secret) = inner.service.take_secret_for_qr(offer_id) else {
            return Err(PairingServerError::PairingFailed);
        };
        let Some(code) = inner.service.offer_code(offer_id) else {
            return Err(PairingServerError::PairingFailed);
        };
        let device = domain::ids::DeviceId::from_raw(&request.device_id)
            .map_err(|_| PairingServerError::PairingFailed)?;
        // 승인은 6자리 코드 대신 Host 사용자의 행동이므로, 코드 인자에는
        // offer 자신의 코드를 되돌려 준다(항상 일치).
        match inner.service.approve(offer_id, device, &secret.0, &code) {
            Ok(_device) => {
                let token = session::OfferSecret::from_random();
                let token_hex: String = token.0.iter().map(|b| format!("{b:02x}")).collect();
                let paired = PairedDevice {
                    device_id: request.device_id,
                    name: request.device_name,
                    token_hex: token_hex.clone(),
                    paired_at: unix_timestamp_utc(),
                };
                inner.paired.retain(|d| {
                    d.device_id != paired.device_id
                        && (paired.name.is_empty() || d.name != paired.name)
                });
                inner.paired.push(paired);
                if self.persist(inner.paired.clone()).is_err() {
                    eprintln!("leftcar: paired-device persistence failed");
                }
                inner.completed.insert(
                    offer_id.to_owned(),
                    CompletedPairing {
                        device_id: approved_device_id,
                        token: token_hex,
                        secret,
                    },
                );
                // 승인으로 offer는 소진됐다 — 이후 픽업은 completed 레코드로만
                // 이뤄지고, 소진된 offer에 새 대기 요청이 붙는 일은 없다.
                inner.live_offers.remove(offer_id);
                Ok(())
            }
            Err(_) => Err(PairingServerError::PairingFailed),
        }
    }

    /// Mac 사용자가 [거절]을 눌렀다. offer를 소각하고 폴링에 거절을 알린다.
    pub fn reject_pending(&self, offer_id: &str) -> Result<(), PairingServerError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .pending
            .remove(offer_id)
            .ok_or(PairingServerError::OfferNotFound)?;
        inner.service.cancel(offer_id);
        inner.live_offers.remove(offer_id);
        inner.rejected.insert(offer_id.to_owned());
        Ok(())
    }

    /// 승인 카드 목록(웹뷰 폴링용). 만료된 대기 요청은 잘라낸다.
    pub fn list_pending_views(&self) -> Vec<PendingPairingView> {
        let mut inner = self.inner.lock().unwrap();
        prune_stale_requests(&mut inner);
        inner
            .pending
            .iter()
            .map(|(offer_id, request)| PendingPairingView {
                offer_id: offer_id.clone(),
                device_name: request.device_name.clone(),
                requested_at: request.requested_at.clone(),
            })
            .collect()
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
        inner.paired.iter().any(|d| {
            session::constant_time_eq(&hex_decode32(&d.token_hex).unwrap_or([0u8; 32]), &bytes)
        })
    }

    /// Cancel every live offer (zeroizing each secret via the service) and
    /// reset failure counters. Closing the pairing window or restarting a
    /// session must not leave scannable offers behind.
    pub fn cancel_active(&self) {
        let mut inner = self.inner.lock().unwrap();
        let offer_ids: Vec<String> = inner.live_offers.drain().collect();
        for offer_id in offer_ids {
            inner.service.cancel(&offer_id);
        }
        inner.fail_counts.clear();
        inner.pending.clear();
        inner.completed.clear();
        inner.rejected.clear();
    }

    /// Remove a device and its token; persists the change. False when the
    /// device was not paired.
    pub fn revoke(&self, device_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        // 철회된 기기가 미픽업 승인 토큰으로 되짚어 들어오지 못하게 한다.
        // 철회 대상이 아닌 기기의 진행 중 요청·미픽업 토큰은 그대로 둔다.
        inner
            .pending
            .retain(|_, request| request.device_id != device_id);
        inner
            .completed
            .retain(|_, record| record.device_id != device_id);
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

    /// Remove all paired devices and tokens; persists the change.
    pub fn revoke_all(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let count = inner.paired.len();
        inner.paired.clear();
        inner.completed.clear();
        inner.pending.clear();
        if let Err(e) = self.persist(Vec::new()) {
            eprintln!("leftcar: persist after revoke_all failed: {e}");
        }
        count
    }

    /// Token-bearing records for in-crate tests only; the UI reads
    /// [PairingServer::list_device_views].
    #[cfg(test)]
    pub fn list_devices(&self) -> Vec<PairedDevice> {
        self.inner.lock().unwrap().paired.clone()
    }

    pub fn list_device_views(&self) -> Vec<PairedDeviceView> {
        self.inner
            .lock()
            .unwrap()
            .paired
            .iter()
            .map(|device| PairedDeviceView {
                device_id: device.device_id.clone(),
                name: device.name.clone(),
                paired_at: device.paired_at.clone(),
            })
            .collect()
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
        // Mode applies at creation: fs::write would create the file 0644 and
        // leave it world-readable until a follow-up chmod lands.
        #[cfg(unix)]
        let file = {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
                .and_then(|mut f| f.write_all(body.as_bytes()))
        };
        #[cfg(not(unix))]
        let file = std::fs::write(path, &body);
        file.map_err(|e| {
            eprintln!("leftcar: write store: {e}");
            PairingServerError::PersistenceFailed
        })?;
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
fn unix_timestamp_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

/// 승인 카드에 표시할 기기 이름은 뷰어가 보낸 임의 문자열이므로 길이를
/// 제한한다(문자 단위 절단, UI 폭 붕괴 방지).
fn truncate_label(name: &str) -> String {
    name.chars().take(40).collect()
}

/// offer TTL + 픽업 여유를 넘긴 대기 요청은 승인 카드에서도, 승인 시에도
/// 유효하지 않다.
fn prune_stale_requests(inner: &mut Inner) {
    let ttl = session::PAIRING_TTL + Duration::from_secs(60);
    inner
        .pending
        .retain(|_, request| request.requested_at_instant.elapsed() < ttl);
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
        assert!(
            payload.get("c").is_none(),
            "QR must not contain the human code"
        );
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
        assert!(
            view.code.bytes().all(|b| b.is_ascii_digit()),
            "{}",
            view.code
        );
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
        assert!(token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));

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
    fn replacing_or_canceling_an_offer_invalidates_old_qr_codes() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let first = server.begin_pairing("192.168.0.10", 7777);
        let second = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&first.qr_payload).unwrap();
        let offer_id = payload["id"].as_str().unwrap();
        let secret_b64 = payload["s"].as_str().unwrap();
        assert!(server
            .pair(
                offer_id,
                secret_b64,
                &first.code,
                "viewer-1",
                "Android Viewer"
            )
            .is_err());

        let payload2 = serde_json::from_str::<serde_json::Value>(&second.qr_payload).unwrap();
        server
            .pair(
                payload2["id"].as_str().unwrap(),
                payload2["s"].as_str().unwrap(),
                &second.code,
                "viewer-2",
                "Android Viewer",
            )
            .unwrap();
        assert_eq!(server.list_devices().len(), 1);

        let third = server.begin_pairing("192.168.0.10", 7777);
        let payload3 = serde_json::from_str::<serde_json::Value>(&third.qr_payload).unwrap();
        server.cancel_active();

        assert!(server
            .pair(
                payload3["id"].as_str().unwrap(),
                payload3["s"].as_str().unwrap(),
                &third.code,
                "viewer-3",
                "Android Viewer",
            )
            .is_err());
        assert_eq!(server.list_devices().len(), 1);
        assert_eq!(server.list_devices()[0].device_id, "viewer-2");
    }

    #[test]
    fn device_views_never_expose_authentication_tokens() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        server
            .pair(
                payload["id"].as_str().unwrap(),
                payload["s"].as_str().unwrap(),
                &view.code,
                "viewer-1",
                "Android Viewer",
            )
            .unwrap();

        let serialized = serde_json::to_value(server.list_device_views()).unwrap();
        assert_eq!(serialized[0]["device_id"], "viewer-1");
        assert!(serialized[0].get("token_hex").is_none());
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

    // -- 승인 기반 QR 페어링 (시크릿 제시 → Mac 허용 → 폴링 픽업) --------

    fn begin_offer_parts(server: &PairingServer) -> (String, String, String) {
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        (
            payload["id"].as_str().unwrap().to_owned(),
            payload["s"].as_str().unwrap().to_owned(),
            view.code,
        )
    }

    #[test]
    fn approval_pairing_waits_pending_then_approves_and_pickup_issues_token() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);

        // 첫 제시: 아직 Mac 승인 전 — Pending 상태 신호.
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        // 아직 기기가 생기지 않는다.
        assert!(server.list_devices().is_empty());
        // 승인 카드에 대기 요청이 보인다.
        let pending = server.list_pending_views();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].device_name, "Galaxy XR");
        assert_eq!(pending[0].offer_id, offer_id);
        assert!(pending[0].device_name.len() <= 40);

        // Mac 사용자가 허용.
        server.approve_pending(&offer_id).unwrap();
        // 기기는 즉시 등록되지만 토큰 픽업은 뷰어 폴링이 한다.
        let devices = server.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "viewer-1");

        // 같은 시크릿으로 폴링하면 토큰을 픽업한다.
        let token = server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(token, server.list_devices()[0].token_hex);
        // 다른 시크릿으로는 픽업할 수 없다.
        let wrong = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert!(server.pair(&offer_id, &wrong, "", "viewer-1", "x").is_err());
    }

    #[test]
    fn approval_pairing_rejection_tells_the_poller() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);

        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        server.reject_pending(&offer_id).unwrap();
        assert!(server.list_pending_views().is_empty());
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Rejected)
        ));
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn approval_pairing_rejects_without_a_pending_request() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, _secret_b64, _code) = begin_offer_parts(&server);
        assert_eq!(
            server.approve_pending(&offer_id).unwrap_err(),
            PairingServerError::OfferNotFound
        );
        assert_eq!(
            server.reject_pending(&offer_id).unwrap_err(),
            PairingServerError::OfferNotFound
        );
    }

    #[test]
    fn approval_pending_polls_do_not_burn_the_offer() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);

        // 승인 대기 폴링은 실패 횟수를 올리지 않는다 — 5번 넘게 반복해도.
        for _ in 0..5 {
            assert!(matches!(
                server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
                Err(PairingServerError::Pending)
            ));
        }
        server.approve_pending(&offer_id).unwrap();
        assert!(server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .is_ok());
    }

    #[test]
    fn approved_token_is_delivered_only_to_the_approved_device() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        server.approve_pending(&offer_id).unwrap();

        // 같은 QR 시크릿을 가진 다른 기기는 승인의 대상이 아니므로 토큰을
        // 받을 수 없다(촬영·공유된 QR의 두 번째 수신자 차단).
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-2", "Nexus 7"),
            Err(PairingServerError::PairingFailed)
        ));
        assert!(server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .is_ok());
        assert_eq!(server.list_devices().len(), 1);
    }

    #[test]
    fn revoking_one_device_keeps_another_devices_pending_pickup() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        server.approve_pending(&offer_id).unwrap();

        // 다른 기기 철회는 viewer-1의 미픽업 토큰을 건드리지 않는다.
        assert!(!server.revoke("viewer-2"));
        assert!(server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .is_ok());

        // 정작 viewer-1을 철회하면 승인 레코드도 함께 사라진다.
        assert!(server.revoke("viewer-1"));
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::OfferNotFound)
        ));
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn wrong_secret_polls_still_burn_the_offer_after_three_tries() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, _secret_b64, _code) = begin_offer_parts(&server);
        let wrong = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([9u8; 32]);

        for _ in 0..3 {
            assert!(matches!(
                server.pair(&offer_id, &wrong, "", "viewer-1", "Galaxy XR"),
                Err(PairingServerError::PairingFailed)
            ));
        }
        // 정확한 시크릿으로도 소각된 offer에는 승인 대기를 등록할 수 없다.
        let (_id2, secret_b64, code) = begin_offer_parts(&server); // 새 QR로 갱신됨
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::OfferNotFound)
        ));
        let _ = code;
    }

    #[test]
    fn new_begin_pairing_invalidates_outstanding_approvals() {
        let server = PairingServer::new("leftcar-host".into(), None);
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));

        begin_offer_parts(&server); // Mac에서 QR 재생성

        assert!(server.list_pending_views().is_empty());
        assert!(server.approve_pending(&offer_id).is_err());
    }
}
