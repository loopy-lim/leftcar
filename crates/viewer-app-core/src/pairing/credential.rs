//! 자격증명 저장·마이그레이션 코어(TS pairing.ts의 토큰 저장 부분). 저장 IO는
//! [`CorePorts`]로 주입되며, 키 스키마(v2 엔드포인트 / v3 정체 / 구버전 전역)
//! 과 되돌리기(rollback) 규율은 여기서 고정한다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::ports::{CancelToken, CorePorts, StoreError};
use super::{
    identity_token_key, is_valid_issued_token, parse_recent_hosts_strict, token_key, HostEndpoint,
    DEVICE_ID_KEY, LEGACY_TOKEN_KEY,
};
use crate::control::ControlError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("leftcar:trustedHostError")]
    TrustedHost,
    #[error("leftcar:errPairingCodeInvalid")]
    CodeInvalid,
    #[error("leftcar:errPairingResponseInvalid")]
    ResponseInvalid,
    #[error("leftcar:errPairingApprovalTimeout")]
    ApprovalTimeout,
    #[error("pairing approval polling aborted")]
    Cancelled,
    #[error("credential rollback failed")]
    RollbackFailed,
    #[error("Conflicting saved host identities")]
    ConflictingHostIdentities,
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Control(#[from] ControlError),
    #[error("{0}")]
    Message(String),
}

/// 저장-복원 쌍 하나. None은 삭제(키가 없었음)를 뜻한다.
type Snapshot = (String, Option<String>);

/// 저장값을 되돌린다. 일부라도 실패하면 RollbackFailed(TS rollbackValues).
fn rollback_values(ports: &CorePorts, entries: &[Snapshot]) -> Result<(), PairingError> {
    let mut failures = 0;
    for (key, value) in entries {
        let result = match value {
            None => ports.store.delete(key),
            Some(value) => ports.store.set(key, value.clone()),
        };
        if result.is_err() {
            failures += 1;
        }
    }
    if failures > 0 {
        Err(PairingError::RollbackFailed)
    } else {
        Ok(())
    }
}

/// One cancellation lifetime for pair persistence, connect, and navigation.
pub struct PairingAttempt {
    cancelled: Arc<CancelToken>,
    committed: AtomicBool,
    rollbacks: Mutex<Vec<Arc<dyn Fn() -> Result<(), StoreError> + Send + Sync>>>,
}

impl PairingAttempt {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: CancelToken::new(),
            committed: AtomicBool::new(false),
            rollbacks: Mutex::new(Vec::new()),
        })
    }

    pub fn signal(&self) -> &Arc<CancelToken> {
        &self.cancelled
    }

    pub fn throw_if_cancelled(&self) -> Result<(), PairingError> {
        if self.cancelled.is_cancelled() {
            Err(PairingError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Predecessor rollback must finish before this run can mutate
    /// Host/storage — 커밋 이후에는 받지 않는다.
    pub fn retain_rollback(&self, rollback: Arc<dyn Fn() -> Result<(), StoreError> + Send + Sync>) {
        if self.committed.load(Ordering::SeqCst) {
            return;
        }
        self.rollbacks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(rollback);
    }

    pub fn commit(&self) -> Result<(), PairingError> {
        self.throw_if_cancelled()?;
        self.committed.store(true, Ordering::SeqCst);
        self.rollbacks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        Ok(())
    }

    /// 실패·취소 시 이 시도가 쓴 자격증명을 되돌린다(커밋 이후에는 무동작).
    pub fn cancel(&self) -> Result<(), PairingError> {
        self.cancelled.cancel();
        if self.committed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut rollbacks = self.rollbacks.lock().unwrap_or_else(|p| p.into_inner());
        let pending = std::mem::take(&mut *rollbacks);
        drop(rollbacks);
        for rollback in pending.into_iter().rev() {
            rollback().map_err(PairingError::Store)?;
        }
        Ok(())
    }
}

/// 이 대상 호스트에 발급된 토큰을 읽는다(없으면 None).
pub fn get_stored_token(
    ports: &CorePorts,
    target: &HostEndpoint,
) -> Result<Option<String>, StoreError> {
    ports.store.get(&token_key(&target.host, target.port))
}

/// Stable per-install device label shown in the host's paired-device list.
/// TS getDeviceId — `Date.now().toString(36)-<random base36>` 형태를 유지한다.
pub fn get_device_id(ports: &CorePorts) -> Result<String, PairingError> {
    if let Some(existing) = ports.store.get(DEVICE_ID_KEY)? {
        return Ok(existing);
    }
    let millis = ports.clock.now_ms();
    // i64 → base36
    let mut stamp = String::new();
    let mut value = millis;
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    loop {
        stamp.push(DIGITS[(value % 36) as usize] as char);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    let stamp: String = stamp.chars().rev().collect();
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut suffix = [0u8; 8];
    ports.random.fill(&mut suffix);
    let suffix: String = suffix
        .iter()
        .map(|b| alphabet[(*b % 36) as usize] as char)
        .collect();
    let id = format!("{stamp}-{suffix}");
    ports.store.set(DEVICE_ID_KEY, id.clone())?;
    Ok(id)
}

/// Resolve the immutable credential incarnation for a newly verified socket.
/// The stable identity path is opt-in: callers may pass a host key only after
/// `client_finish` has verified it. USB has no such proof and remains
/// endpoint-bound.
pub fn get_stored_credential(
    ports: &CorePorts,
    target: &HostEndpoint,
    verified_host_key: Option<&str>,
    cancel: Option<&CancelToken>,
    allow_endpoint_credential: bool,
) -> Result<Option<StoredCredentialView>, PairingError> {
    let _guard = ports
        .credential_lock
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(PairingError::Cancelled);
        }
    }
    if let Some(verified_host_key) = verified_host_key {
        let identity_token = ports.store.get(&identity_token_key(verified_host_key))?;
        if let Some(cancel) = cancel {
            if cancel.is_cancelled() {
                return Err(PairingError::Cancelled);
            }
        }
        if let Some(identity_token) = identity_token {
            migrate_verified_credential(ports, target, verified_host_key, &identity_token, cancel)?;
            return Ok(Some(StoredCredentialView {
                token: identity_token,
                target: target.clone(),
                host_key: Some(verified_host_key.to_string()),
            }));
        }
    }
    if !allow_endpoint_credential {
        return Ok(None);
    }
    let endpoint_token = get_stored_token(ports, target)?;
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(PairingError::Cancelled);
        }
    }
    let Some(endpoint_token) = endpoint_token else {
        return Ok(None);
    };
    if let Some(verified_host_key) = verified_host_key {
        migrate_verified_credential(ports, target, verified_host_key, &endpoint_token, cancel)?;
    }
    Ok(Some(StoredCredentialView {
        token: endpoint_token,
        target: target.clone(),
        host_key: verified_host_key.map(str::to_string),
    }))
}

/// 이 대상 호스트의 토큰만 치운다 — 다른 호스트의 토큰은 그대로 둔다.
pub fn clear_token(ports: &CorePorts, target: &HostEndpoint) -> Result<(), PairingError> {
    let _guard = ports
        .credential_lock
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    ports.store.delete(&token_key(&target.host, target.port))?;
    Ok(())
}

/// Clear only keys that still contain the failed socket's token incarnation.
pub fn clear_stored_credential(
    ports: &CorePorts,
    credential: &StoredCredentialView,
) -> Result<(), PairingError> {
    let _guard = ports
        .credential_lock
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let endpoint_key = token_key(&credential.target.host, credential.target.port);
    let mut keys = vec![endpoint_key];
    if let Some(host_key) = &credential.host_key {
        keys.push(identity_token_key(host_key));
    }
    let mut before = Vec::with_capacity(keys.len());
    for key in &keys {
        before.push((key.clone(), ports.store.get(key)?));
    }
    let mut delete_error = None;
    for (key, value) in &before {
        if value.as_deref() == Some(credential.token.as_str()) {
            if let Err(e) = ports.store.delete(key) {
                delete_error = Some(e);
                break;
            }
        }
    }
    if let Some(error) = delete_error {
        rollback_values(ports, &before)?;
        return Err(error.into());
    }
    Ok(())
}

/// 저장-마이그레이션 성공 후의 되돌리기 절차(취소된 페어링 수명이 저장을 되돌린다).
pub type TokenRollback = Arc<dyn Fn() -> Result<(), StoreError> + Send + Sync>;

/// 발급 토큰을 페어링 대상 엔드포인트 아래에 저장하고 구버전 전역 키를 치운다.
/// 성공 시 이 시도의 되돌리기 절차를 돌려준다(TS storeToken).
pub fn store_token(
    ports: &CorePorts,
    host: &str,
    port: u16,
    token: &str,
    host_key: Option<&str>,
    cancel: Option<&CancelToken>,
) -> Result<TokenRollback, PairingError> {
    let _guard = ports
        .credential_lock
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(PairingError::Cancelled);
        }
    }
    let endpoint_key = token_key(host, port);
    let identity_key = host_key.map(identity_token_key);
    let mut keys = vec![endpoint_key.clone()];
    if let Some(identity_key) = &identity_key {
        keys.push(identity_key.clone());
    }
    keys.push(LEGACY_TOKEN_KEY.to_string());
    let mut before = Vec::with_capacity(keys.len());
    for key in &keys {
        before.push((key.clone(), ports.store.get(key)?));
    }
    let recent_before = if host_key.is_some() {
        ports.store.get(super::RECENT_HOSTS_KEY)?
    } else {
        None
    };
    let pin_before = host_key.map(|_| ports.pins.get(host, port));
    let throw_if_cancelled = |cancel: Option<&CancelToken>| -> Result<(), PairingError> {
        if let Some(cancel) = cancel {
            if cancel.is_cancelled() {
                return Err(PairingError::Cancelled);
            }
        }
        Ok(())
    };

    let result = (|| -> Result<Option<String>, PairingError> {
        throw_if_cancelled(cancel)?;
        ports.store.set(&endpoint_key, token.to_string())?;
        throw_if_cancelled(cancel)?;
        if let Some(identity_key) = &identity_key {
            ports.store.set(identity_key, token.to_string())?;
        }
        throw_if_cancelled(cancel)?;
        ports.store.delete(LEGACY_TOKEN_KEY)?;
        throw_if_cancelled(cancel)?;
        let mut recent_after = None;
        if let Some(host_key) = host_key {
            remember_pinned_host_key_locked(ports, host, port, host_key, cancel)?;
            recent_after = Some(ports.store.get(super::RECENT_HOSTS_KEY)?);
        }
        throw_if_cancelled(cancel)?;
        Ok(recent_after.flatten())
    })();
    let recent_after = match result {
        Ok(value) => value,
        Err(error) => {
            if host_key.is_some() {
                ports
                    .pins
                    .restore(host, port, pin_before.flatten().as_deref());
            }
            let mut snapshots = before.clone();
            if host_key.is_some() {
                snapshots.push((super::RECENT_HOSTS_KEY.to_string(), recent_before));
            }
            rollback_values(ports, &snapshots)?;
            return Err(error);
        }
    };

    let recent_key = super::RECENT_HOSTS_KEY.to_string();
    let host_owned = host.to_string();
    let host_key_owned = host_key.map(str::to_string);
    let token_owned = token.to_string();
    let legacy_key = LEGACY_TOKEN_KEY.to_string();
    let recent_before_owned = recent_before.clone();
    let pin_before_owned = pin_before.clone();
    let pins = ports.pins.clone();
    let store = ports.store.clone();
    Ok(Arc::new(move || {
        // TS와 같이 — 새로운 값이 이 토큰의 흔적일 때만 되돌린다. 더 새로운
        // 교체(재페어링)가 이미 덮었으면 그 값을 지우지 않는다.
        let mut rollback: Vec<Snapshot> = Vec::new();
        for (key, previous) in &before {
            let current = store.get(key)?;
            let expected = if *key == legacy_key {
                None
            } else {
                Some(token_owned.as_str())
            };
            if current.as_deref() == expected {
                rollback.push((key.clone(), previous.clone()));
            }
        }
        if host_key_owned.is_some() && store.get(&recent_key)? == recent_after {
            rollback.push((recent_key.clone(), recent_before_owned.clone()));
            pins.restore(
                &host_owned,
                port,
                pin_before_owned.clone().flatten().as_deref(),
            );
        }
        for (key, value) in &rollback {
            match value {
                None => store.delete(key)?,
                Some(value) => store.set(key, value.clone())?,
            };
        }
        Ok(())
    }))
}

/// Once this endpoint has cryptographically proved `hostKey`, its legacy alias
/// must follow the identity credential too(루프백 경로의 오래된 토큰 재사용 방지).
pub fn migrate_verified_credential(
    ports: &CorePorts,
    target: &HostEndpoint,
    host_key: &str,
    token: &str,
    cancel: Option<&CancelToken>,
) -> Result<(), PairingError> {
    let identity_key = identity_token_key(host_key);
    let endpoint_key = token_key(&target.host, target.port);
    let recent_before = ports.store.get(super::RECENT_HOSTS_KEY)?;
    let identity_before = ports.store.get(&identity_key)?;
    let endpoint_before = ports.store.get(&endpoint_key)?;
    let pin_before = ports.pins.get(&target.host, target.port);
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(PairingError::Cancelled);
        }
    }
    let aliases_already_current = identity_before.as_deref() == Some(token)
        && endpoint_before.as_deref() == Some(token)
        && pin_before.as_deref() == Some(host_key);
    if aliases_already_current {
        return Ok(());
    }
    let persist = (|| -> Result<(), PairingError> {
        if identity_before.as_deref() != Some(token) {
            ports.store.set(&identity_key, token.to_string())?;
        }
        // Once this endpoint has cryptographically proved `hostKey`, its legacy
        // alias must follow the identity credential too. Leaving an older token
        // here would make a later USB connection reuse the endpoint's stale
        // incarnation because loopback has no identity handshake of its own.
        if endpoint_before.as_deref() != Some(token) {
            ports.store.set(&endpoint_key, token.to_string())?;
        }
        remember_pinned_host_key_locked(ports, &target.host, target.port, host_key, cancel)
    })();
    if let Err(error) = persist {
        ports
            .pins
            .restore(&target.host, target.port, pin_before.as_deref());
        rollback_values(
            ports,
            &[
                (identity_key, identity_before),
                (endpoint_key, endpoint_before),
                (super::RECENT_HOSTS_KEY.to_string(), recent_before),
            ],
        )?;
        return Err(error);
    }
    Ok(())
}

/// 핀을 메모리에 등록하고 recent hosts 엔트리에도 반영한다. 저장 실패 시 메모리
/// 등록을 되돌린다(TS rememberPinnedHostKey). 자격증명 직렬화 경계 안에서만
/// 호출한다(호출부가 이미 잠금을 보유).
fn remember_pinned_host_key_locked(
    ports: &CorePorts,
    host: &str,
    port: u16,
    host_key: &str,
    _cancel: Option<&CancelToken>,
) -> Result<(), PairingError> {
    let previous_pin = ports.pins.get(host, port);
    ports.pins.register(host, port, host_key);
    match ports.recent.persist_host_key(host, port, host_key) {
        Ok(()) => Ok(()),
        Err(error) => {
            if previous_pin.is_none() {
                ports.pins.restore(host, port, None);
            } else {
                ports.pins.restore(host, port, previous_pin.as_deref());
            }
            Err(error.into())
        }
    }
}

/// 자격증명 경계 밖에서 쓰는 공개 래퍼(세션의 TOFU 첫 핀 저장).
pub fn remember_pinned_host_key(
    ports: &CorePorts,
    host: &str,
    port: u16,
    host_key: &str,
    cancel: Option<&CancelToken>,
) -> Result<(), PairingError> {
    let _guard = ports
        .credential_lock
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(cancel) = cancel {
        if cancel.is_cancelled() {
            return Err(PairingError::Cancelled);
        }
    }
    remember_pinned_host_key_locked(ports, host, port, host_key, cancel)
}

/// Read persistent trust before opening a network socket, including saved
/// aliases. 같은 엔트리의 별칭 경로도 정규화해 비교하고, 핀이 둘 이상이면
/// 오류로 실패 닫힘한다(TS getStoredPinnedHostKey).
pub fn get_stored_pinned_host_key(
    ports: &CorePorts,
    host: &str,
    port: u16,
) -> Result<Option<String>, StoreError> {
    let normalized = host.trim().to_lowercase();
    let entries = parse_recent_hosts_strict(ports.store.get(super::RECENT_HOSTS_KEY)?.as_deref())?;
    let mut keys: Vec<String> = Vec::new();
    for entry in entries {
        if entry.port != port {
            continue;
        }
        let mut routes = vec![entry.host.clone()];
        routes.extend(entry.aliases.clone());
        if routes
            .iter()
            .any(|route| route.trim().to_lowercase() == normalized)
        {
            if let Some(key) = entry.host_key {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
    }
    if keys.len() > 1 {
        return Err(StoreError::new("Conflicting saved host identities"));
    }
    Ok(keys.into_iter().next())
}

/// 응답에서 발급 토큰을 검증해 꺼낸다. 페어링 두 흐름이 공유한다.
pub(crate) fn validate_issued_token(response: &serde_json::Value) -> Result<String, PairingError> {
    let token = response
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or(PairingError::ResponseInvalid)?;
    if !is_valid_issued_token(token) {
        return Err(PairingError::ResponseInvalid);
    }
    Ok(token.to_string())
}

/// getStoredCredential이 돌려주는 뷰 — 저장소에서 검증된 교체 불가 교체값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredentialView {
    pub token: String,
    pub target: HostEndpoint,
    pub host_key: Option<String>,
}

impl StoredCredentialView {
    pub fn into_credential(self) -> super::StoredCredential {
        super::StoredCredential {
            token: self.token,
            target: self.target,
            host_key: self.host_key,
        }
    }
}
