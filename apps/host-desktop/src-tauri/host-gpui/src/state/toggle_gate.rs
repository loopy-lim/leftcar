//! 낙관적 토글의 응답 반영 판정(src/toggleGate.ts 포팅). React 렌더러 없이
//! 원본 훅의 비동기 흐름을 검증하기 어려워 판정 로직만 순수로 분리한
//! 것이므로 Rust에서는 그대로 `&mut self` 구조체가 된다:
//! - 더 새 토글이 발행된(superseded) 요청의 응답·실패는 현재 값을 덮지 않는다.
//! - 토글이 한 번이라도 발행됐으면 초기 로드 응답은 무시된다.
//!
//! 원본 스펙은 src/Privacy.test.ts의 "toggle gate" describe에 있다(원본
//! 모듈의 전용 스펙 파일이 아니다 — 그렇기 때문에 여기서 같은 케이스를
//! 옮겼다).

#[derive(Debug, Default)]
pub struct ToggleGate {
    latest: u64,
    toggled_since_load: bool,
}

impl ToggleGate {
    pub fn new() -> ToggleGate {
        ToggleGate::default()
    }

    /// 토글 요청을 발행하고 순번을 돌려준다.
    pub fn issue(&mut self) -> u64 {
        self.toggled_since_load = true;
        self.latest += 1;
        self.latest
    }

    /// 이 순번의 요청이 아직 최신인지 — 아니면 응답·실패를 무시한다.
    pub fn is_current(&self, seq: u64) -> bool {
        seq == self.latest
    }

    /// 초기 로드 응답을 반영해도 되는지.
    pub fn allows_initial_load(&self) -> bool {
        !self.toggled_since_load
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 원본(Privacy.test.ts): ignores responses from superseded requests
    #[test]
    fn ignores_responses_from_superseded_requests() {
        let mut gate = ToggleGate::new();
        let first = gate.issue();
        assert!(gate.is_current(first));

        // 더 새 토글이 발행되면 앞 요청의 응답·실패는 무시된다.
        let second = gate.issue();
        assert!(!gate.is_current(first));
        assert!(gate.is_current(second));
    }

    // 원본(Privacy.test.ts): ignores initial-load responses once a toggle
    // was issued
    #[test]
    fn ignores_initial_load_responses_once_a_toggle_was_issued() {
        let mut gate = ToggleGate::new();
        assert!(gate.allows_initial_load());

        gate.issue();
        assert!(!gate.allows_initial_load());
    }

    // 파생(원본 카운터 0 선례): 발행 전 순번 0도 최신이다 — 첫 issue가
    // 1을 돌려주므로 0은 "아직 아무 것도 발행되지 않음"의 상대다.
    #[test]
    fn seq_zero_is_current_before_any_issue() {
        let gate = ToggleGate::new();
        assert!(gate.is_current(0));
    }
}
