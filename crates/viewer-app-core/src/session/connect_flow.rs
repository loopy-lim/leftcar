//! 페어링 워크플로 상태머신(TS connect-flow.ts의 PairingWorkflow +
//! finishPairingAttempt). 화면의 카메라 콜백이 같은 승인 오퍼로 되풀이 되어도
//! 살아 있는 실행을 재사용하고, 다른 QR·새 PIN은 이전 실행의 다음 부작용 경계
//! 전에 중단시킨다. UI(Alert·router)는 콜백 경계로 남긴다 — 이 모듈은 상태만
//! 소유한다.

use std::sync::Arc;

use crate::pairing::credential::{PairingAttempt, PairingError};
use crate::pairing::QrPayload;

/// 화면이 소유한 페어링 실행 하나.
pub struct PairingWorkflowRun {
    pub key: String,
    pub attempt: Arc<PairingAttempt>,
}

#[derive(Default)]
pub struct PairingWorkflow {
    current: Option<(String, Arc<PairingAttempt>)>,
    next_pin_run: u64,
}

impl PairingWorkflow {
    pub fn new() -> Self {
        Self::default()
    }

    /// 같은 오퍼(승인 대기 중)의 되풀이 콜백은 살아 있는 실행을 재사용한다 —
    /// None이면 새로 만들 필요가 없다. 이전 실행의 rollback이 실패하면 오류를
    /// 돌려준다(TS는 run.ready await에서 같은 오류를 흘려보낸다).
    pub fn begin_qr(&mut self, payload: &QrPayload) -> Result<Option<PairingWorkflowRun>, PairingError> {
        let key = format!("qr:{}:{}:{}", payload.id, payload.secret, payload.host_key);
        if let Some((current_key, _)) = &self.current {
            if *current_key == key {
                return Ok(None);
            }
        }
        Ok(Some(self.replace(key)?))
    }

    /// PIN 입력은 항상 새 실행이다(같은 코드를 다시 입력해도).
    pub fn begin_pin(&mut self) -> Result<PairingWorkflowRun, PairingError> {
        self.next_pin_run += 1;
        self.replace(format!("pin:{}", self.next_pin_run))
    }

    /// 이 실행이 아직 화면의 현재 실행일 때만 종료시킨다(늦게 끝난 옛 실행의
    /// finish가 새 실행을 지우지 않는다).
    pub fn finish(&mut self, run: &PairingWorkflowRun) -> bool {
        let is_current = self
            .current
            .as_ref()
            .map(|(key, attempt)| key == &run.key && Arc::ptr_eq(attempt, &run.attempt))
            .unwrap_or(false);
        if is_current {
            self.current = None;
        }
        is_current
    }

    /// 이전 실행의 rollback이 끝난 뒤에 새 실행이 저장소를 만지도록, 취소는
    /// 동기로 완결한다(TS는 Promise로 같은 순서를 보장했다).
    pub fn cancel(&mut self) -> Result<(), PairingError> {
        let current = self.current.take();
        if let Some((_, attempt)) = current {
            attempt.cancel()?;
        }
        Ok(())
    }

    fn replace(&mut self, key: String) -> Result<PairingWorkflowRun, PairingError> {
        let previous = self.current.take();
        if let Some((_, attempt)) = previous {
            // 이전 실행의 rollback이 끝난 뒤에야 새 실행이 저장소를 만진다.
            attempt.cancel()?;
        }
        let attempt = PairingAttempt::new();
        let run = PairingWorkflowRun {
            key,
            attempt: attempt.clone(),
        };
        self.current = Some((run.key.clone(), attempt));
        Ok(run)
    }
}

/// 실행 비교는 키와 시도 수명으로 한다(테스트 단정용).
impl PartialEq for PairingWorkflowRun {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && Arc::ptr_eq(&self.attempt, &other.attempt)
    }
}

/// Keep pairing persistence, authenticated connection, and navigation inside
/// one cancelable lifetime. Any failure or cancellation rolls back credentials
/// written by this attempt unless a newer incarnation has already replaced
/// them. UI 경계(navigate)는 콜백으로 받는다.
pub fn finish_pairing_attempt(
    attempt: &PairingAttempt,
    pair: impl FnOnce() -> Result<(), PairingError>,
    connect: impl FnOnce() -> Result<(), PairingError>,
    navigate: impl FnOnce(),
) -> Result<(), PairingError> {
    let run = || -> Result<(), PairingError> {
        attempt.throw_if_cancelled()?;
        pair()?;
        attempt.throw_if_cancelled()?;
        connect()?;
        attempt.throw_if_cancelled()?;
        navigate();
        attempt.commit()
    }();
    if let Err(error) = run {
        attempt.cancel()?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_the_same_offer_and_supersedes_different_ones() {
        let mut workflow = PairingWorkflow::new();
        let payload_a = QrPayload {
            id: "offer-123e4567-e89b-42d3-a456-426614174000".into(),
            secret: "A".repeat(43),
            host_key: "B".repeat(43),
            host: "192.168.1.5".into(),
            port: 7777,
            ts: None,
        };
        let run_a = workflow.begin_qr(&payload_a).expect("begin").expect("first run");
        // 같은 오퍼의 되풀이 콜백은 None — 살아 있는 실행을 재사용한다.
        assert!(workflow.begin_qr(&payload_a).expect("begin").is_none());
        assert!(!run_a.attempt.signal().is_cancelled());

        // 다른 오퍼가 오면 이전 실행을 중단시키고 새 실행이 이긴다.
        let mut payload_b = payload_a.clone();
        payload_b.id = "offer-123e4567-e89b-42d3-a456-426614174001".into();
        let run_b = workflow.begin_qr(&payload_b).expect("begin").expect("second run");
        assert!(run_a.attempt.signal().is_cancelled());
        assert!(!run_b.attempt.signal().is_cancelled());
        // 옛 실행의 finish는 거짓이고 새 실행의 finish만 진짜다.
        assert!(!workflow.finish(&run_a));
        assert!(workflow.finish(&run_b));
        assert!(!workflow.finish(&run_b));
    }

    #[test]
    fn pin_runs_are_always_fresh_and_supersede_qr() {
        let mut workflow = PairingWorkflow::new();
        let payload = QrPayload {
            id: "offer-123e4567-e89b-42d3-a456-426614174000".into(),
            secret: "A".repeat(43),
            host_key: "B".repeat(43),
            host: "192.168.1.5".into(),
            port: 7777,
            ts: None,
        };
        let qr_run = workflow.begin_qr(&payload).expect("begin").expect("qr run");
        let pin_run = workflow.begin_pin().expect("begin");
        assert!(qr_run.attempt.signal().is_cancelled());
        assert!(!pin_run.attempt.signal().is_cancelled());
        assert!(workflow.finish(&pin_run));
    }

    #[test]
    fn cancelled_attempt_rolls_back_and_commit_stops_rollback() {
        // cancel이 rollback을 돌린다(credential 스펙에서 저장 수준으로도 검증).
        let mut workflow = PairingWorkflow::new();
        let run = workflow.begin_pin().expect("begin");
        assert!(workflow.cancel().is_ok());
        assert!(run.attempt.signal().is_cancelled());
        assert!(run.attempt.commit().is_err());
    }
}
