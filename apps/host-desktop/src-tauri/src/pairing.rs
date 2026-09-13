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
    grants: crate::source_grants::GrantStore,
    shutting_down: bool,
    view_revision: u64,
    catalogs: HashMap<String, HashMap<u32, String>>,
    service: session::PairingService,
    /// QR `k` 필드와 offer 핑거프린트 메타데이터의 원천.
    host_public_key: [u8; 32],
    token_store: Box<dyn TokenStore>,
    fail_counts: HashMap<String, u32>,
    paired: Vec<PairedDevice>,
    authorization_generation: u64,
    device_generations: HashMap<String, u64>,
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

/// A pairing incarnation, invalidated by any credential or authorization change.
#[derive(Clone)]
pub(crate) struct Authorization {
    device: String,
    generation: u64,
    owner: String,
    pub(crate) access: Option<crate::source_grants::CaptureAccess>,
}

impl Authorization {
    pub(crate) fn device_id(&self) -> &str {
        &self.device
    }
}

type GrantUpdate = (
    Result<crate::source_grants::GrantView, String>,
    Vec<std::sync::Arc<crate::source_grants::SourceLease>>,
);

pub struct PairingServer {
    inner: Mutex<Inner>,
    store_path: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    /// 토큰은 [TokenStore]로만 저장된다(파일에는 절대 기록되지 않는다).
    #[serde(default, skip_serializing)]
    pub token_hex: String,
    pub paired_at: String,
    /// Private credential reference; legacy metadata uses the device ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_id: Option<String>,
}

impl PairedDevice {
    fn owner(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "host-device:{:x}",
            Sha256::digest(format!("{}:{}", self.device_id, self.credential_key()).as_bytes())
        )
    }
    fn credential_key(&self) -> &str {
        self.credential_id.as_deref().unwrap_or(&self.device_id)
    }
}

/// Redacted device metadata safe to expose to the webview. Authentication
/// tokens stay inside the Rust host process and its restricted local store.
#[derive(serde::Serialize, Clone)]
pub struct PairedDeviceView {
    pub device_id: String,
    pub name: String,
    pub paired_at: String,
    pub source_grants: crate::source_grants::GrantView,
}

/// Host-local state order is scoped to this process/profile and includes empty snapshots.
#[derive(serde::Serialize)]
pub struct PairedDeviceState {
    pub revision: u64,
    pub devices: Vec<PairedDeviceView>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedDevice {
    pub device_id: String,
    pub credential_id: String,
}
#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RevokeOutcome {
    pub removed_devices: Vec<RemovedDevice>,
    pub state_revision: u64,
    pub persistence_errors: Vec<String>,
    #[serde(skip)]
    pub(crate) retired_leases: Vec<std::sync::Arc<crate::source_grants::SourceLease>>,
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
    pub fn new(
        host_public_key: [u8; 32],
        store_path: Option<PathBuf>,
        token_store: Box<dyn TokenStore>,
    ) -> Self {
        let mut paired: Vec<PairedDevice> = store_path
            .as_deref()
            .and_then(load_devices)
            .unwrap_or_default();
        // 구버전 파일에는 token_hex가 인라인으로 있었다 — 토큰 저장소로
        // 이관하고 파일에서는 지운다. 이관은 저장소 쓰기+재독기가 확인된
        // 기기만 수행한다: 키체인이 잠겨 있거나 거부하는 동안에는 인라인
        // 토큰을 파일에 그대로 두고 다음 기동에 다시 시도한다(실패해도
        // 자격 증명을 파괴하지 않는다).
        let mut migrated = Vec::new();
        let mut needs_rewrite = false;
        for mut device in paired {
            if !device.token_hex.is_empty() {
                // 쓰기가 Err여도 뒤의 재독기 검증이 어긋나므로 인라인 토큰이
                // 살아 남는다 — 이관 경로에서는 실패를 굳이 끊지 않는다.
                let stored = token_store
                    .set(device.credential_key(), &device.token_hex)
                    .ok()
                    .and_then(|_| token_store.get(device.credential_key()));
                if stored.as_deref() == Some(device.token_hex.as_str()) {
                    // 이관 성공: 메모리 토큰은 그대로 두고(이번 기동의
                    // 인증에 쓴다) 파일에서만 인라인 토큰을 걷어 간다.
                    needs_rewrite = true;
                } else {
                    eprintln!(
                        "leftcar: token store round-trip failed for {}; keeping the inline token",
                        device.device_id
                    );
                }
            } else {
                // 이미 이관된 기기 — 저장소에서만 읽는다. 읽기 실패 시
                // token_hex는 빈 채로 남고 authorize가 그 기기를 건너뛴다
                // (fail-closed).
                device.token_hex = token_store.get(device.credential_key()).unwrap_or_default();
            }
            migrated.push(device);
        }
        paired = migrated;
        if needs_rewrite {
            if let Some(path) = &store_path {
                let _ = persist_devices(path, &paired);
            }
        }
        Self {
            inner: Mutex::new(Inner {
                grants: crate::source_grants::GrantStore::open(None)
                    .expect("in-memory grant store"),
                shutting_down: false,
                view_revision: 0,
                catalogs: HashMap::new(),
                service: session::PairingService::new(Box::new(WallClock {
                    epoch: OnceLock::new(),
                })),
                host_public_key,
                token_store,
                fail_counts: HashMap::new(),
                device_generations: paired.iter().map(|d| (d.device_id.clone(), 0)).collect(),
                paired,
                authorization_generation: 0,
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

        let fingerprint = inner
            .host_public_key
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let offer = inner.service.begin_offer(fingerprint);
        // Borrow the secret only to encode it into the QR payload; the
        // service's own copy (the only other one) is zeroized on approve/
        // cancel. We do not retain it — the QR already carries the base64url
        // form the viewer will present as its proof.
        let secret = inner
            .service
            .take_secret_for_qr(&offer.ephemeral_offer_id)
            .expect("secret exists right after begin_offer");
        // v2: `k` = 호스트 Ed25519 공개키(base64url 32B). 뷰어는 이 키를 핀해
        // 제어 평면 핸드셰이크의 ServerHello 서명을 검증한다(secure-channel).
        let payload = json!({
            "v": 2,
            "id": offer.ephemeral_offer_id,
            "s": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret.0),
            "k": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(inner.host_public_key),
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
            return Self::pair_awaiting_approval(
                &mut inner,
                offer_id,
                secret_b64url,
                device_id,
                name,
            );
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
            Ok(_device) => self.complete_pairing(&mut inner, offer_id, device_id, name),
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
            Ok(_device) => self.complete_pairing(&mut inner, &offer_id, device_id, name),
            Err(_) => {
                Self::record_pair_failure(&mut inner, &offer_id);
                Err(PairingServerError::PairingFailed)
            }
        }
    }

    /// Mint the pairing token, register the device, consume the offer, and
    /// persist. Shared success tail of every pairing flow. Fails with
    /// [PairingServerError::PersistenceFailed] when the token cannot be
    /// stored — publishing a pickup token for a device the host would not
    /// recognize after a restart is worse than failing the pairing.
    fn complete_pairing(
        &self,
        inner: &mut Inner,
        offer_id: &str,
        device_id: &str,
        name: &str,
    ) -> Result<String, PairingServerError> {
        if inner.shutting_down {
            return Err(PairingServerError::PairingFailed);
        }
        inner.fail_counts.remove(offer_id);
        let token = session::OfferSecret::from_random();
        let token_hex: String = token.0.iter().map(|b| format!("{b:02x}")).collect();
        let credential_id = format!("leftcar-pairing-{}", uuid::Uuid::new_v4());
        let mut paired = inner.paired.clone();
        paired.retain(|d| d.device_id != device_id && (name.is_empty() || d.name != name));
        paired.push(PairedDevice {
            device_id: device_id.to_owned(),
            name: name.to_owned(),
            token_hex: token_hex.clone(),
            paired_at: unix_timestamp_utc(),
            credential_id: Some(credential_id.clone()),
        });
        // Never overwrite the committed credential. A crash or metadata failure
        // leaves the previous metadata+credential pair usable after restart.
        inner.token_store.set(&credential_id, &token_hex)?;
        if let Err(error) = self.persist(paired.clone()) {
            inner.token_store.delete(&credential_id);
            return Err(error);
        }
        let retired_owners: Vec<_> = inner
            .paired
            .iter()
            .filter(|old| {
                !paired
                    .iter()
                    .any(|d| d.credential_key() == old.credential_key())
            })
            .map(PairedDevice::owner)
            .collect();
        for owner in retired_owners {
            inner.grants.invalidate(&owner);
        }
        for old in &inner.paired {
            if !paired
                .iter()
                .any(|d| d.credential_key() == old.credential_key())
            {
                inner.token_store.delete(old.credential_key());
            }
        }
        let paired_ids: HashSet<_> = paired.iter().map(|d| d.device_id.clone()).collect();
        inner
            .device_generations
            .retain(|id, _| paired_ids.contains(id));
        inner.paired = paired;
        inner.view_revision += 1;
        inner.authorization_generation += 1;
        let generation = inner.authorization_generation;
        inner
            .device_generations
            .insert(device_id.to_owned(), generation);
        inner.live_offers.remove(offer_id);
        Ok(token_hex)
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
            if record.device_id == device_id && session::constant_time_eq(&record.secret.0, &proof)
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
                let token_hex = self.complete_pairing(
                    &mut inner,
                    offer_id,
                    &request.device_id,
                    &request.device_name,
                )?;
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
    /// devices are paired. A device whose token failed to load from the
    /// store (empty `token_hex`) is skipped — comparing the zeros fallback
    /// would let `"0".repeat(64)` authenticate as that device (fail-open).
    pub fn authorize(&self, token_hex: &str) -> bool {
        self.authorize_device(token_hex).is_some()
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

    /// Denial is effective even if persistence fails; the caller must surface the outcome.
    pub fn revoke(&self, device_id: &str) -> RevokeOutcome {
        self.revoke_matching(Some(device_id))
    }

    fn revoke_matching(&self, device_id: Option<&str>) -> RevokeOutcome {
        let mut inner = self.inner.lock().unwrap();
        let mut outcome = RevokeOutcome {
            state_revision: inner.view_revision,
            ..Default::default()
        };
        if inner.shutting_down {
            outcome
                .persistence_errors
                .push("Host is shutting down".into());
            return outcome;
        }
        let matches = |id: &str| device_id.is_none_or(|wanted| wanted == id);
        inner
            .pending
            .retain(|_, request| !matches(&request.device_id));
        inner
            .completed
            .retain(|_, record| !matches(&record.device_id));
        let removed: Vec<_> = inner
            .paired
            .iter()
            .filter(|d| matches(&d.device_id))
            .cloned()
            .collect();
        for device in &removed {
            let owner = device.owner();
            let (result, leases) = inner.grants.update(&owner, vec![]);
            outcome.retired_leases.extend(leases);
            if let Err(error) = result {
                outcome.persistence_errors.push(error);
            }
            inner.token_store.delete(device.credential_key());
            inner.device_generations.remove(&device.device_id);
            outcome.removed_devices.push(RemovedDevice {
                device_id: device.device_id.clone(),
                credential_id: owner,
            });
        }
        if !removed.is_empty() {
            inner.paired.retain(|d| !matches(&d.device_id));
            inner.view_revision += 1;
            outcome.state_revision = inner.view_revision;
            if let Err(error) = self.persist(inner.paired.clone()) {
                outcome
                    .persistence_errors
                    .push(format!("device removal persistence failed: {error}"));
            }
        }
        outcome
    }

    /// 토큰으로 장치를 찾는다(상수 시간 비교). 세션을 장치에 귀속시켜
    /// revoke 시 라이브 스트림을 즉시 끊기 위해 필요하다. 저장소에서
    /// 토큰을 못 읽은 기기(빈 token_hex)는 건너뛴다 — 제로 폴백과 비교하면
    /// `"0".repeat(64)`가 그 기기로 인증된다(fail-open).
    pub fn authorize_device(&self, token_hex: &str) -> Option<String> {
        self.authenticate(token_hex).map(|auth| auth.device)
    }

    /// Token validation and its exact incarnation are one atomic read. Never
    /// recover an incarnation later from a connection's cached device ID.
    pub(crate) fn authenticate(&self, token_hex: &str) -> Option<Authorization> {
        let inner = self.inner.lock().unwrap();
        if inner.shutting_down || inner.paired.is_empty() {
            return None;
        }
        let Ok(bytes) = hex_decode32(token_hex) else {
            return None;
        };
        inner
            .paired
            .iter()
            .find(|d| {
                hex_decode32(&d.token_hex)
                    .map(|stored| session::constant_time_eq(&stored, &bytes))
                    .unwrap_or(false)
            })
            .map(|d| Authorization {
                device: d.device_id.clone(),
                owner: d.owner(),
                access: None,
                generation: *inner
                    .device_generations
                    .get(&d.device_id)
                    .expect("paired device generation"),
            })
    }

    /// 장치가 지금도 페어링돼 있는지 검사한다. 연결 인증은 소켓 수명당 한
    /// 번이므로, 명령 실행 직전에 이 재검사를 돌려야 revoke 이후 같은
    /// 연결로 계속 명령을 보내는 유리시간이 남지 않는다.
    pub fn is_device_paired(&self, device_id: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .paired
            .iter()
            .any(|d| d.device_id == device_id)
    }

    #[cfg(test)]
    pub(crate) fn authorization(&self, device: &str) -> Option<Authorization> {
        let inner = self.inner.lock().unwrap();
        inner
            .paired
            .iter()
            .any(|d| d.device_id == device && !d.token_hex.is_empty())
            .then(|| Authorization {
                device: device.to_owned(),
                owner: inner
                    .paired
                    .iter()
                    .find(|d| d.device_id == device)
                    .unwrap()
                    .owner(),
                access: None,
                generation: *inner
                    .device_generations
                    .get(device)
                    .expect("paired device generation"),
            })
    }

    /// Lock order: pairing -> endpoint registry -> session state. The closure must only mutate local
    /// state; never call a backend, user callback, or another pairing method.
    pub(crate) fn with_authorization<T>(
        &self,
        authorization: Option<&Authorization>,
        commit: impl FnOnce() -> T,
    ) -> Option<T> {
        let inner = self.inner.lock().unwrap();
        if inner.shutting_down {
            return None;
        }
        if let Some(auth) = authorization {
            if auth.access.as_ref().is_some_and(|a| !a.lease.current()) {
                return None;
            }
            if inner.device_generations.get(&auth.device) != Some(&auth.generation)
                || !inner.paired.iter().any(|d| d.device_id == auth.device)
            {
                return None;
            }
        }
        Some(commit())
    }

    pub fn revoke_all(&self) -> RevokeOutcome {
        self.revoke_matching(None)
    }

    /// Token-bearing records for in-crate tests only; the UI reads
    /// [PairingServer::list_device_views].
    #[cfg(test)]
    pub fn list_devices(&self) -> Vec<PairedDevice> {
        self.inner.lock().unwrap().paired.clone()
    }

    pub fn list_device_views(&self) -> Vec<PairedDeviceView> {
        self.list_device_state().devices
    }
    pub fn list_device_state(&self) -> PairedDeviceState {
        let inner = self.inner.lock().unwrap();
        PairedDeviceState {
            revision: inner.view_revision,
            devices: inner
                .paired
                .iter()
                .map(|device| {
                    let mut grants = inner.grants.view(&device.owner());
                    grants.state_revision = inner.view_revision;
                    PairedDeviceView {
                        device_id: device.device_id.clone(),
                        name: device.name.clone(),
                        paired_at: device.paired_at.clone(),
                        source_grants: grants,
                    }
                })
                .collect(),
        }
    }

    pub fn initialize_source_grants(&self, path: PathBuf) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        inner.grants = crate::source_grants::GrantStore::open(Some(path))?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn initialize_source_grants_with_writer(
        &self,
        path: PathBuf,
        writer: std::sync::Arc<dyn crate::source_grants::JournalWriter>,
    ) {
        self.inner.lock().unwrap().grants =
            crate::source_grants::GrantStore::open_with_writer(Some(path), writer).unwrap();
    }
    pub(crate) fn source_authorization(
        &self,
        auth: &Authorization,
        source: &str,
    ) -> Result<Authorization, String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.shutting_down
            || inner.device_generations.get(&auth.device) != Some(&auth.generation)
            || auth
                .access
                .as_ref()
                .is_some_and(|access| !access.lease.current())
        {
            return Err("unauthorized".into());
        }
        let mut bound = auth.clone();
        let mut access = inner.grants.access(&auth.owner, source)?;
        // A replacement belongs to the same session lifetime. Stopping that
        // session must also fence a replacement still inside native setup.
        if let Some(previous) = &auth.access {
            access.lease = previous.lease.clone();
        }
        bound.access = Some(access);
        Ok(bound)
    }
    pub(crate) fn remember_catalog(
        &self,
        auth: &Authorization,
        displays: &[control_contract::host::DisplayInfo],
    ) {
        let mut inner = self.inner.lock().unwrap();
        if inner.shutting_down
            || inner.device_generations.get(&auth.device) != Some(&auth.generation)
        {
            return;
        }
        inner.catalogs.insert(
            auth.owner.clone(),
            displays
                .iter()
                .filter_map(|d| d.source_id.as_ref().map(|id| (d.index, id.clone())))
                .collect(),
        );
    }
    pub(crate) fn catalog_source(
        &self,
        auth: &Authorization,
        index: u32,
    ) -> Result<String, String> {
        self.inner
            .lock()
            .unwrap()
            .catalogs
            .get(&auth.owner)
            .and_then(|catalog| catalog.get(&index))
            .cloned()
            .ok_or_else(|| {
                "source_refresh_required: Host에서 화면 접근을 허용한 뒤 목록을 새로 고치세요"
                    .into()
            })
    }
    pub(crate) fn source_allowed(&self, auth: &Authorization, source: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        !inner.shutting_down
            && inner.device_generations.get(&auth.device) == Some(&auth.generation)
            && inner.grants.allows(&auth.owner, source)
    }
    #[cfg(test)]
    pub(crate) fn update_source_grants(
        &self,
        device: &str,
        sources: Vec<String>,
    ) -> (
        Result<crate::source_grants::GrantView, String>,
        Vec<std::sync::Arc<crate::source_grants::SourceLease>>,
    ) {
        match self.update_source_grants_for_credential(device, sources, None) {
            Ok(update) => update,
            Err(error) => (Err(error), vec![]),
        }
    }
    /// An outer error means no mutation was admitted; do not retire a newer credential.
    pub(crate) fn update_source_grants_for_credential(
        &self,
        device: &str,
        sources: Vec<String>,
        expected_credential: Option<&str>,
    ) -> Result<GrantUpdate, String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.shutting_down {
            return Err("Host is shutting down".into());
        }
        let owner = inner
            .paired
            .iter()
            .find(|d| d.device_id == device)
            .map(PairedDevice::owner)
            .ok_or("no such paired device")?;
        if expected_credential.is_some_and(|expected| expected != owner) {
            return Err("paired credential changed; review current device before saving".into());
        }
        let (result, leases) = inner.grants.update(&owner, sources);
        inner.view_revision += 1;
        let revision = inner.view_revision;
        Ok((
            result.map(|mut view| {
                view.state_revision = revision;
                view
            }),
            leases,
        ))
    }
    pub(crate) fn fence_source_access(
        &self,
    ) -> Vec<std::sync::Arc<crate::source_grants::SourceLease>> {
        let mut inner = self.inner.lock().unwrap();
        inner.shutting_down = true;
        inner.grants.fence()
    }
    pub(crate) fn finish_source_shutdown(&self) -> Result<(), String> {
        self.inner.lock().unwrap().grants.finish_shutdown()
    }

    /// Best-effort persist; parent dirs created, file written 0600.
    fn persist(&self, devices: Vec<PairedDevice>) -> Result<(), PairingServerError> {
        let Some(path) = self.store_path.as_ref() else {
            return Ok(());
        };
        persist_devices(path, &devices)
    }
}

/// 장치 메타데이터(토큰 제외)를 0600 파일로 기록한다.
fn persist_devices(
    path: &std::path::Path,
    devices: &[PairedDevice],
) -> Result<(), PairingServerError> {
    let body = serde_json::to_string_pretty(devices).map_err(|e| {
        eprintln!("leftcar: serialize paired devices: {e}");
        PairingServerError::PersistenceFailed
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            eprintln!("leftcar: create store dir: {e}");
            PairingServerError::PersistenceFailed
        })?;
    }
    atomic_write_private(path, body.as_bytes())
}

/// Write a sibling file completely before replacing the last good metadata.
fn atomic_write_private(path: &std::path::Path, body: &[u8]) -> Result<(), PairingServerError> {
    use std::io::Write;
    let temporary = path.with_file_name(format!(".leftcar-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(body)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| {
        eprintln!("leftcar: atomic store write failed: {error}");
        PairingServerError::PersistenceFailed
    })
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

// -- 토큰 저장소 ---------------------------------------------------------------

/// 페어링 토큰의 저장소. 운영에서는 OS 자격 증명 보관소(macOS Keychain /
/// Windows Credential Manager)를 쓴다. [TokenStore::set]이 실패하면 Err를
/// 돌려 페어링 자체를 실패시킨다 — 토큰을 저장하지 못한 채 성공을 보고하면
/// 재기동 후 그 기기의 인증이 깨진다. 파일에는 메타데이터만 두고 토큰은
/// 저장소로 분리한다.
pub trait TokenStore: Send + Sync {
    fn set(&self, device_id: &str, token_hex: &str) -> Result<(), PairingServerError>;
    fn get(&self, device_id: &str) -> Option<String>;
    fn delete(&self, device_id: &str);
}

/// 운영 플랫폼에 맞는 저장소를 고른다. macOS·Windows에서는 OS 보관소를
/// 쓰고, 그 밖의 플랫폼에서만 0600 파일 저장소를 쓴다. OS 보관소 쪽에는
/// 파일 폴백이 없다 — 저장 실패는 폴백으로 흡수하지 않고 페어링을
/// 실패시킨다([PairingServerError::PersistenceFailed]).
pub fn token_store(fallback_path: Option<PathBuf>) -> Box<dyn TokenStore> {
    token_store_with_service(fallback_path, "leftcar-host")
}

pub fn token_store_with_service(
    fallback_path: Option<PathBuf>,
    service: &str,
) -> Box<dyn TokenStore> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let _ = fallback_path;
        Box::new(KeychainTokenStore {
            service: service.to_owned(),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = service;
        Box::new(FileTokenStore::new(
            fallback_path.map(|p| p.with_extension("tokens")),
        ))
    }
}

/// OS 자격 증명 보관소 (keyring 크레이트 → macOS Keychain / Windows Credential
/// Manager). 생성·저장 실패는 Err로 올라가 페어링을 실패시킨다 — 실패한
/// 저장을 무시하고 성공을 보고하지 않는다.
pub struct KeychainTokenStore {
    service: String,
}

impl TokenStore for KeychainTokenStore {
    fn set(&self, device_id: &str, token_hex: &str) -> Result<(), PairingServerError> {
        let entry = keyring::Entry::new(&self.service, device_id).map_err(|e| {
            eprintln!("leftcar: keychain entry failed for {device_id}: {e}");
            PairingServerError::PersistenceFailed
        })?;
        entry.set_password(token_hex).map_err(|e| {
            eprintln!("leftcar: keychain set failed for {device_id}: {e}");
            PairingServerError::PersistenceFailed
        })
    }

    fn get(&self, device_id: &str) -> Option<String> {
        keyring::Entry::new(&self.service, device_id)
            .ok()
            .and_then(|entry| match entry.get_password() {
                Ok(token) => Some(token),
                Err(keyring::Error::NoEntry) => None,
                Err(e) => {
                    eprintln!("leftcar: keychain get failed for {device_id}: {e}");
                    None
                }
            })
    }

    fn delete(&self, device_id: &str) {
        if let Ok(entry) = keyring::Entry::new(&self.service, device_id) {
            let _ = entry.delete_credential();
        }
    }
}

/// 파일 폴백 저장소 — 하나의 0600 JSON(장치 → 토큰) 토큰 파일.
pub struct FileTokenStore {
    path: Option<PathBuf>,
}

impl FileTokenStore {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    fn write(
        &self,
        tokens: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), PairingServerError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                eprintln!("leftcar: create token file dir: {e}");
                PairingServerError::PersistenceFailed
            })?;
        }
        let body = serde_json::to_string(tokens).unwrap_or_else(|_| "{}".into());
        atomic_write_private(path, body.as_bytes())
    }

    fn read(&self) -> std::collections::BTreeMap<String, String> {
        let Some(path) = &self.path else {
            return Default::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default()
    }
}

impl TokenStore for FileTokenStore {
    fn set(&self, device_id: &str, token_hex: &str) -> Result<(), PairingServerError> {
        let mut tokens = self.read();
        tokens.insert(device_id.to_owned(), token_hex.to_owned());
        self.write(&tokens)
    }

    fn get(&self, device_id: &str) -> Option<String> {
        self.read().get(device_id).cloned()
    }

    fn delete(&self, device_id: &str) {
        let mut tokens = self.read();
        tokens.remove(device_id);
        // 삭제 실패는 무시한다 — 메타데이터에서 기기가 지워졌다면 남은
        // 토큰으로는 authorize가 불가능하다(토큰 파일에 접근하지 않는다).
        let _ = self.write(&tokens);
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

    /// 모든 동작이 실패하는 저장소(잠긴 키체인 등) — 마이그레이션·페어링
    /// 실패 경로 검증용.
    struct FailingStore;

    impl TokenStore for FailingStore {
        fn set(&self, _: &str, _: &str) -> Result<(), PairingServerError> {
            Err(PairingServerError::PersistenceFailed)
        }
        fn get(&self, _: &str) -> Option<String> {
            None
        }
        fn delete(&self, _: &str) {}
    }

    fn write_legacy_file(path: &std::path::Path, device_id: &str, token_hex: &str) {
        let body = format!(
            r#"[{{"device_id":"{device_id}","name":"Viewer","token_hex":"{token_hex}","paired_at":"2026-09-09T00:00:00Z"}}]"#
        );
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn legacy_inline_token_survives_a_failing_token_store() {
        let path = temp_store_path("legacy-keep");
        let token = "11".repeat(32);
        write_legacy_file(&path, "dev-1", &token);
        let server = PairingServer::new([9u8; 32], Some(path.clone()), Box::new(FailingStore));
        // 저장소가 전부 실패해도 인라인 토큰이 살아 있어 인증이 동작한다.
        assert!(server.authorize(&token));
        assert_eq!(server.authorize_device(&token).as_deref(), Some("dev-1"));
        // 파일도 그대로 — 마이그레이션이 자격 증명을 파괴하지 않는다.
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains(&token),
            "the inline token must stay until the store accepts it"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn failing_token_store_fails_pairing_and_registers_nothing() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FailingStore));
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        let e = server.pair(
            payload["id"].as_str().unwrap(),
            payload["s"].as_str().unwrap(),
            &view.code,
            "viewer-1",
            "Quest 3",
        );
        // 토큰을 못 저장했으면 성공을 보고하지 않는다.
        assert_eq!(e.unwrap_err(), PairingServerError::PersistenceFailed);
        assert!(
            server.list_devices().is_empty(),
            "a failed token store must not register the device"
        );
    }

    #[test]
    fn failing_token_store_fails_pair_by_code() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FailingStore));
        let view = server.begin_pairing("192.168.0.10", 7777);
        let e = server.pair_by_code(&view.code, "viewer-1", "Quest 3");
        assert_eq!(e.unwrap_err(), PairingServerError::PersistenceFailed);
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn failing_token_store_fails_approval_and_publishes_no_pickup_token() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FailingStore));
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        // 승인은 토큰 저장에 실패해 끝난다 — 완료 레코드(픽업 토큰)도 없다.
        assert_eq!(
            server.approve_pending(&offer_id).unwrap_err(),
            PairingServerError::PersistenceFailed
        );
        assert!(server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .is_err());
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn restart_after_a_failed_pairing_grants_no_auth() {
        let path = temp_store_path("failed-pair-restart");
        {
            let server = PairingServer::new([7u8; 32], Some(path.clone()), Box::new(FailingStore));
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            assert!(server
                .pair(
                    payload["id"].as_str().unwrap(),
                    payload["s"].as_str().unwrap(),
                    &view.code,
                    "viewer-1",
                    "Quest 3",
                )
                .is_err());
        }
        // 재기동 후 저장소가 정상이어도 실패한 페어링의 기기·토큰은 없다.
        let restarted = PairingServer::new(
            [7u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
        );
        assert!(restarted.list_devices().is_empty());
        assert!(!restarted.authorize(&"ab".repeat(32)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn zeros_token_never_authenticates_an_unloaded_device() {
        let path = temp_store_path("zeros-failclosed");
        // 토큰이 저장소에서 사라진 기기: 파일에는 토큰이 없고 저장소도 실패.
        std::fs::write(
            &path,
            r#"[{"device_id":"dev-2","name":"Viewer","paired_at":"2026-09-09T00:00:00Z"}]"#,
        )
        .unwrap();
        let server = PairingServer::new([9u8; 32], Some(path.clone()), Box::new(FailingStore));
        // 빈 token_hex 기기는 제로 토큰("0"*64)으로 인증되지 않는다 — fail-closed.
        assert!(!server.authorize(&"0".repeat(64)));
        assert!(server.authorize_device(&"0".repeat(64)).is_none());
        // 그 기기로 어떤 추측 토큰도 인증되지 않는다.
        assert!(server.authorize_device(&"ab".repeat(32)).is_none());
        assert!(!server.authorize(&"ab".repeat(32)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migration_completes_when_the_store_round_trips() {
        let path = temp_store_path("legacy-migrate-ok");
        let token = "22".repeat(32);
        write_legacy_file(&path, "dev-3", &token);
        let mut tokens_path = temp_store_path("legacy-migrate-tokens");
        tokens_path.set_extension("tokens");
        let server = PairingServer::new(
            [9u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(tokens_path))),
        );
        assert!(server.authorize(&token));
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            !body.contains(&token),
            "a successful migration strips the inline token from the file"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn begin_pairing_creates_qr_payload_and_code() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
        let view = server.begin_pairing("192.168.0.10", 7777);

        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        assert_eq!(payload["v"], json!(2));
        // QR은 호스트 공개키를 실어야 한다(뷰어 핀 검증용).
        let host_key = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload["k"].as_str().unwrap())
            .unwrap();
        assert_eq!(host_key, [7u8; 32]);
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
    fn legacy_inline_tokens_migrate_into_the_token_store() {
        let path = temp_store_path("migrate");
        let tokens_path = path.with_extension("tokens");
        // 구버전 형식: token_hex가 파일에 인라인.
        let legacy = format!(
            r#"[{{"device_id":"viewer-1","name":"Old","token_hex":"{}","paired_at":"unix:0"}}]"#,
            "a".repeat(64)
        );
        std::fs::write(&path, legacy).unwrap();

        let server = PairingServer::new(
            [7u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(tokens_path.clone()))),
        );
        let token = "a".repeat(64);
        assert!(server.authorize(&token), "migrated token must authorize");
        assert_eq!(server.authorize_device(&token).as_deref(), Some("viewer-1"));
        // 파일에서 토큰이 지워졌는지 확인.
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(!body.contains(&token), "token must leave the metadata file");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tokens_path);
    }

    #[test]
    fn persisted_devices_survive_restart() {
        let path = temp_store_path("restart");
        let token = {
            let server = PairingServer::new(
                [7u8; 32],
                Some(path.clone()),
                Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
            );
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            let offer_id = payload["id"].as_str().unwrap().to_owned();
            let secret_b64 = payload["s"].as_str().unwrap().to_owned();
            server
                .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
                .unwrap()
        };

        let restarted = PairingServer::new(
            [7u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
        );
        assert!(restarted.authorize(&token));
        assert_eq!(restarted.list_devices().len(), 1);
        assert_eq!(restarted.list_devices()[0].device_id, "viewer-1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_store_is_ignored_not_fatal() {
        let path = temp_store_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        let server = PairingServer::new(
            [7u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
        );
        assert!(server.list_devices().is_empty());
        let view = server.begin_pairing("192.168.0.10", 7777);
        assert_eq!(view.code.len(), 6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn revoke_removes_token_and_persists() {
        let path = temp_store_path("revoke");
        let token = {
            let server = PairingServer::new(
                [7u8; 32],
                Some(path.clone()),
                Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
            );
            let view = server.begin_pairing("192.168.0.10", 7777);
            let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
            let offer_id = payload["id"].as_str().unwrap().to_owned();
            let secret_b64 = payload["s"].as_str().unwrap().to_owned();
            server
                .pair(&offer_id, &secret_b64, &view.code, "viewer-1", "Quest 3")
                .unwrap()
        };

        {
            let server = PairingServer::new(
                [7u8; 32],
                Some(path.clone()),
                Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
            );
            assert!(server.authorize(&token));
            assert!(!server.revoke("viewer-1").removed_devices.is_empty());
            assert!(!server.authorize(&token));
            assert!(server.list_devices().is_empty());
        }
        // persisted across restart
        let restarted = PairingServer::new(
            [7u8; 32],
            Some(path.clone()),
            Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
        );
        assert!(!restarted.authorize(&token));
        assert!(restarted.list_devices().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_device_paired_tracks_revoke() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
        let view = server.begin_pairing("192.168.0.10", 7777);
        let payload = serde_json::from_str::<serde_json::Value>(&view.qr_payload).unwrap();
        server
            .pair(
                payload["id"].as_str().unwrap(),
                payload["s"].as_str().unwrap(),
                &view.code,
                "viewer-1",
                "Quest 3",
            )
            .unwrap();
        assert!(server.is_device_paired("viewer-1"));
        assert!(!server.is_device_paired("viewer-2"));
        // 철회 직후 명령 실행 직전 검사가 거짓이 되어야 한다(M1).
        assert!(!server.revoke("viewer-1").removed_devices.is_empty());
        assert!(!server.is_device_paired("viewer-1"));
    }

    #[test]
    fn replacing_or_canceling_an_offer_invalidates_old_qr_codes() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
            let server = PairingServer::new(
                [7u8; 32],
                Some(path.clone()),
                Box::new(FileTokenStore::new(Some(path.with_extension("tokens")))),
            );
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));
        server.approve_pending(&offer_id).unwrap();

        // 다른 기기 철회는 viewer-1의 미픽업 토큰을 건드리지 않는다.
        assert!(server.revoke("viewer-2").removed_devices.is_empty());
        assert!(server
            .pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR")
            .is_ok());

        // 정작 viewer-1을 철회하면 승인 레코드도 함께 사라진다.
        assert!(!server.revoke("viewer-1").removed_devices.is_empty());
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::OfferNotFound)
        ));
        assert!(server.list_devices().is_empty());
    }

    #[test]
    fn wrong_secret_polls_still_burn_the_offer_after_three_tries() {
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
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
        let server = PairingServer::new([7u8; 32], None, Box::new(FileTokenStore::new(None)));
        let (offer_id, secret_b64, _code) = begin_offer_parts(&server);
        assert!(matches!(
            server.pair(&offer_id, &secret_b64, "", "viewer-1", "Galaxy XR"),
            Err(PairingServerError::Pending)
        ));

        begin_offer_parts(&server); // Mac에서 QR 재생성

        assert!(server.list_pending_views().is_empty());
        assert!(server.approve_pending(&offer_id).is_err());
    }
    #[test]
    fn reaudit_metadata_failure_restores_credentials_and_memory_after_restart() {
        for approval in [false, true] {
            for repairing in [false, true] {
                let root = temp_store_path("metadata-transaction");
                std::fs::create_dir_all(&root).unwrap();
                let metadata = root.join("devices.json");
                let tokens = root.join("tokens.json");
                let server = PairingServer::new(
                    [7; 32],
                    Some(metadata.clone()),
                    Box::new(FileTokenStore::new(Some(tokens.clone()))),
                );
                let old = repairing.then(|| {
                    let view = server.begin_pairing("127.0.0.1", 7777);
                    server
                        .pair_by_code(&view.code, "viewer-1", "Original")
                        .unwrap()
                });
                let saved = metadata.exists().then(|| std::fs::read(&metadata).unwrap());
                if metadata.exists() {
                    std::fs::remove_file(&metadata).unwrap();
                }
                std::fs::create_dir(&metadata).unwrap();
                let view = server.begin_pairing("127.0.0.1", 7777);
                let result = if approval {
                    let qr: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
                    let id = qr["id"].as_str().unwrap();
                    let secret = qr["s"].as_str().unwrap();
                    assert!(matches!(
                        server.pair(id, secret, "", "viewer-1", "Replacement"),
                        Err(PairingServerError::Pending)
                    ));
                    server.approve_pending(id)
                } else {
                    server
                        .pair_by_code(&view.code, "viewer-1", "Replacement")
                        .map(|_| ())
                };
                assert_eq!(result, Err(PairingServerError::PersistenceFailed));
                assert_eq!(server.list_devices().len(), usize::from(repairing));
                if let Some(token) = &old {
                    assert!(server.authorize(token));
                    assert_eq!(server.list_devices()[0].name, "Original");
                }
                std::fs::remove_dir(&metadata).unwrap();
                if let Some(saved) = saved {
                    std::fs::write(&metadata, saved).unwrap();
                }
                let restarted = PairingServer::new(
                    [7; 32],
                    Some(metadata),
                    Box::new(FileTokenStore::new(Some(tokens))),
                );
                assert_eq!(restarted.list_devices().len(), usize::from(repairing));
                if let Some(token) = old {
                    assert!(restarted.authorize(&token));
                }
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    struct KeepOrphanCredentials(FileTokenStore);
    impl TokenStore for KeepOrphanCredentials {
        fn set(&self, key: &str, value: &str) -> Result<(), PairingServerError> {
            self.0.set(key, value)
        }
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key)
        }
        fn delete(&self, _: &str) { /* Simulate credential-provider cleanup failure. */
        }
    }

    #[test]
    fn reaudit_repair_survives_orphan_cleanup_failure_after_metadata_commit() {
        let root = temp_store_path("orphan-cleanup");
        std::fs::create_dir_all(&root).unwrap();
        let metadata = root.join("devices.json");
        let tokens = root.join("tokens.json");
        let server = PairingServer::new(
            [7; 32],
            Some(metadata.clone()),
            Box::new(KeepOrphanCredentials(FileTokenStore::new(Some(
                tokens.clone(),
            )))),
        );
        let first = server.begin_pairing("127.0.0.1", 7777);
        let old = server
            .pair_by_code(&first.code, "viewer-1", "Original")
            .unwrap();
        let second = server.begin_pairing("127.0.0.1", 7777);
        let new = server
            .pair_by_code(&second.code, "viewer-1", "Replacement")
            .unwrap();
        assert!(!server.authorize(&old));
        assert!(server.authorize(&new));
        let restarted = PairingServer::new(
            [7; 32],
            Some(metadata),
            Box::new(FileTokenStore::new(Some(tokens.clone()))),
        );
        assert!(!restarted.authorize(&old));
        assert!(restarted.authorize(&new));
        assert_eq!(restarted.list_devices()[0].name, "Replacement");
        assert_eq!(
            FileTokenStore::new(Some(tokens)).read().len(),
            2,
            "orphan remains stored but cannot authorize"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
