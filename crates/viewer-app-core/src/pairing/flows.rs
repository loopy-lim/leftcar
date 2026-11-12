//! 페어링 흐름(TS pairing.ts의 pairWithHost·pairWithHostByCode·
//! pairWithHostApproval). 접속은 [`crate::control`] 클라이언트로, 저장은
//! 주입 포트로 수행한다. 실패해도 저장된 토큰을 치우지 않는다 — 토큰은
//! 발급한 호스트의 것이고, 만료 토큰은 연결 시 401로 자정된다.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use super::credential::{
    get_device_id, get_stored_pinned_host_key, store_token, validate_issued_token, PairingAttempt,
    PairingError,
};
use super::ports::{CancelToken, CorePorts};
use super::{is_trusted_host, normalize_pairing_code, QrPayload};
use crate::control::{connect, ConnectOptions};

/// 승인 기반 QR 페어링의 기본 대기 상한(150초).
pub const PAIRING_APPROVAL_TIMEOUT_MS: u64 = 150_000;
/// pending 동안의 재폴링 간격.
pub const DEFAULT_POLL_MS: u64 = 2_500;

#[derive(Clone, Default)]
pub struct PairingFlowOptions {
    /// QR 페어링에 싣는 핀 — 없으면 저장된 핀을 읽는다.
    pub pinned_host_key: Option<String>,
    pub poll_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
    /// pending 통지(호스트 승인 화면 대기).
    pub on_pending: Option<Arc<dyn Fn() + Send + Sync>>,
    /// 폴링 루프 취소(화면 종료·스캔 대체).
    pub cancel: Option<Arc<CancelToken>>,
    /// 페어링 수명(rollback 보관).
    pub attempt: Option<Arc<PairingAttempt>>,
    /// pair 요청 데드라인 재정의(테스트·임베딩용).
    pub request_timeout_overrides: BTreeMap<String, u64>,
}

impl PairingFlowOptions {
    pub fn new() -> Self {
        Self::default()
    }

    fn cancel_token(&self) -> Option<&CancelToken> {
        self.attempt
            .as_ref()
            .map(|a| a.signal().as_ref())
            .or(self.cancel.as_deref())
    }

    fn throw_if_cancelled(&self) -> Result<(), PairingError> {
        match self.cancel_token() {
            Some(token) if token.is_cancelled() => Err(PairingError::Cancelled),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingApprovalResult {
    Approved { token: String },
    Rejected,
}

/// 페어링 요청 인자. offerId/secret은 QR 경로에만 실린다.
#[derive(Serialize)]
struct PairArgs<'a> {
    #[serde(rename = "offerId", skip_serializing_if = "Option::is_none")]
    offer_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<&'a str>,
    code: &'a str,
    #[serde(rename = "deviceId")]
    device_id: &'a str,
    #[serde(rename = "deviceName")]
    device_name: &'a str,
}

/// Complete pairing against the QR offer host. On success the issued token is
/// persisted; on failure any previously stored token is left untouched.
pub fn pair_with_host(
    ports: &CorePorts,
    payload: &QrPayload,
    code: &str,
    options: &PairingFlowOptions,
) -> Result<String, PairingError> {
    pair_and_store(
        ports,
        &payload.host,
        payload.port,
        code,
        Some((&payload.id, &payload.secret)),
        Some(payload.host_key.clone()),
        options,
    )
}

/// Complete pairing directly against the selected Host endpoint. 수동 입력
/// 경로는 핀이 없다 — TOFU로 첫 핸드셰이크의 키를 핀한다.
pub fn pair_with_host_by_code(
    ports: &CorePorts,
    host: &str,
    port: u16,
    code: &str,
    options: &PairingFlowOptions,
) -> Result<String, PairingError> {
    pair_and_store(ports, host, port, code, None, None, options)
}

/// Shared pairing tail: connect, request, validate, persist, release.
fn pair_and_store(
    ports: &CorePorts,
    host: &str,
    port: u16,
    code: &str,
    offer: Option<(&str, &str)>,
    pinned_override: Option<String>,
    options: &PairingFlowOptions,
) -> Result<String, PairingError> {
    if !is_trusted_host(host) {
        return Err(PairingError::TrustedHost);
    }
    let pairing_code = normalize_pairing_code(code);
    if pairing_code.len() != 6 || !pairing_code.bytes().all(|c| c.is_ascii_digit()) {
        return Err(PairingError::CodeInvalid);
    }
    options.throw_if_cancelled()?;
    let pinned_host_key = match &pinned_override {
        Some(key) => Some(key.clone()),
        None => get_stored_pinned_host_key(ports, host, port)?,
    };
    let client = connect(
        ports.transport.as_ref(),
        host,
        port,
        5000,
        ConnectOptions::new()
            .pinned_host_key(pinned_host_key)
            .messages(ports.messages.clone())
            .timeout_overrides(options.request_timeout_overrides.clone()),
    )?;
    let result = (|| -> Result<String, PairingError> {
        options.throw_if_cancelled()?;
        let device_id = get_device_id(ports)?;
        let (offer_id, secret) = offer
            .map(|(id, secret)| (Some(id), Some(secret)))
            .unwrap_or((None, None));
        let args = PairArgs {
            offer_id,
            secret,
            code: &pairing_code,
            device_id: &device_id,
            device_name: &ports.device_name,
        };
        let args_value = serde_json::to_value(&args)
            .map_err(|_| PairingError::Message("pair request encoding failed".into()))?;
        let response = client.request_simple("pair", Some(&args_value))?;
        options.throw_if_cancelled()?;
        let token = validate_issued_token(&response)?;
        let rollback = store_token(
            ports,
            host,
            port,
            &token,
            client.host_key().as_deref(),
            options.cancel_token(),
        )?;
        if let Some(attempt) = &options.attempt {
            attempt.retain_rollback(rollback);
        }
        Ok(token)
    })();
    // The pairing connection is single-purpose; the token travels via secure
    // storage into the main control session, so always release the socket.
    client.close();
    result
}

/// QR 스캔 한 번으로 페어링을 완결하는 승인 폴링. pending 동안 `poll_ms` 간격으로
/// 같은 요청을 재전송한다(멱등 — 호스트는 매번 시크릿을 상수 시간 비교로 검증하고,
/// 대기 폴링은 offer를 소각하지 않는다).
pub fn pair_with_host_approval(
    ports: &CorePorts,
    payload: &QrPayload,
    options: &PairingFlowOptions,
) -> Result<PairingApprovalResult, PairingError> {
    if !is_trusted_host(&payload.host) {
        return Err(PairingError::TrustedHost);
    }
    options.throw_if_cancelled()?;
    let poll_ms = options.poll_ms.unwrap_or(DEFAULT_POLL_MS);
    let timeout_ms = options.timeout_ms.unwrap_or(PAIRING_APPROVAL_TIMEOUT_MS);
    let deadline = ports.clock.now_ms().saturating_add(timeout_ms);
    let device_id = get_device_id(ports)?;
    let args = PairArgs {
        offer_id: Some(&payload.id),
        secret: Some(&payload.secret),
        code: "",
        device_id: &device_id,
        device_name: &ports.device_name,
    };
    let args_value = serde_json::to_value(&args)
        .map_err(|_| PairingError::Message("pair request encoding failed".into()))?;

    while ports.clock.now_ms() < deadline {
        options.throw_if_cancelled()?;
        let client = connect(
            ports.transport.as_ref(),
            &payload.host,
            payload.port,
            5000,
            ConnectOptions::new()
                .pinned_host_key(Some(payload.host_key.clone()))
                .messages(ports.messages.clone())
                .timeout_overrides(options.request_timeout_overrides.clone()),
        )?;
        enum Step {
            Approved(PairingApprovalResult),
            Pending,
        }
        let step = (|| -> Result<Step, PairingError> {
            options.throw_if_cancelled()?;
            let response = client.request_simple("pair", Some(&args_value))?;
            // 요청 대기 중 취소됐다면 토큰을 검증·저장하지 않는다 — 취소 후 저장은
            // 사용자가 거부한 페어링을 뒤늦게 되살린다(F18).
            options.throw_if_cancelled()?;
            if response.get("token").and_then(|v| v.as_str()).is_some() {
                let token = validate_issued_token(&response)?;
                let rollback = store_token(
                    ports,
                    &payload.host,
                    payload.port,
                    &token,
                    client.host_key().as_deref(),
                    options.cancel_token(),
                )?;
                if let Some(attempt) = &options.attempt {
                    attempt.retain_rollback(rollback);
                }
                return Ok(Step::Approved(PairingApprovalResult::Approved { token }));
            }
            if response.get("status").and_then(|v| v.as_str()) == Some("pending") {
                return Ok(Step::Pending);
            }
            Err(PairingError::ResponseInvalid)
        })();
        // 폴링 연결은 한 번의 요청만 하고 바로 닫는다.
        client.close();
        match step {
            Ok(Step::Approved(result)) => return Ok(result),
            Ok(Step::Pending) => {
                if let Some(callback) = &options.on_pending {
                    callback();
                }
                if wait_cancellable(options.cancel_token(), poll_ms) {
                    return Err(PairingError::Cancelled);
                }
                continue;
            }
            Err(error) => {
                if let PairingError::Control(control) = &error {
                    // 개별 요청 타임아웃은 흐름을 죽이지 않는다 — 호스트가 한 순간
                    // 바빠도 다음 폴링이 상태를 회수한다.
                    if control.kind == crate::control::ControlErrorKind::Timeout {
                        if wait_cancellable(options.cancel_token(), poll_ms) {
                            return Err(PairingError::Cancelled);
                        }
                        continue;
                    }
                }
                if is_rejected_message(&error) {
                    return Ok(PairingApprovalResult::Rejected);
                }
                return Err(error);
            }
        }
    }
    Err(PairingError::ApprovalTimeout)
}

fn is_rejected_message(error: &PairingError) -> bool {
    super::is_pairing_rejected_error(&error.to_string())
}

fn wait_cancellable(cancel: Option<&CancelToken>, ms: u64) -> bool {
    match cancel {
        Some(token) => token.wait(ms),
        None => {
            std::thread::sleep(std::time::Duration::from_millis(ms.max(1)));
            false
        }
    }
}
