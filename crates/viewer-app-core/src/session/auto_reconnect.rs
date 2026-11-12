//! 홈 화면 백그라운드 자동 재연결 판정(TS auto-reconnect.ts). 순수 함수 부분
//! ([`should_auto_reconnect`])은 게이트 상태를 인자로 받아 테스트로 검증하고,
//! 게이트는 앱 런타임 상태(사용자가 직접 연결 해제했는지, 조용한 401 재시도
//! 루프에 빠졌는지)를 세션 동안만 기억한다. TS는 모듈 싱글턴이지만 Rust 코어는
//! [`AutoReconnectGate`] 인스턴스다 — 임베딩이 프로세스당 하나를 들고 다닌다.

/// 같은 호스트에 대한 자동 재시도 최소 간격. 포커스마다 반복 시도를 막는다.
pub const AUTO_RECONNECT_MIN_INTERVAL_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReconnectDecision {
    pub has_client: bool,
    pub has_recent_host: bool,
    /// 사용자가 이번 앱 실행 중 "연결 해제"를 눌렀다면 자동 재연결하지 않는다.
    pub user_disconnected: bool,
    /// 자동 연결 중 401(승인 만료)을 만났다면 사용자가 직접 시도할 때까지 멈춘다.
    pub pairing_stale: bool,
    pub last_attempt_at: Option<u64>,
    pub now: u64,
    pub min_interval_ms: Option<u64>,
}

pub fn should_auto_reconnect(decision: &AutoReconnectDecision) -> bool {
    if decision.has_client || !decision.has_recent_host {
        return false;
    }
    if decision.user_disconnected || decision.pairing_stale {
        return false;
    }
    let min_interval = decision
        .min_interval_ms
        .unwrap_or(AUTO_RECONNECT_MIN_INTERVAL_MS);
    if let Some(last_attempt_at) = decision.last_attempt_at {
        if decision.now.saturating_sub(last_attempt_at) < min_interval {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectRouteTarget {
    pub host: String,
    pub port: u16,
}

/// 같은 호스트를 향한 자동 재연결은 beginHostSelection+connectHost 대신 남아
/// 있는 요청 컨텍스트로 직접 되살린다. 선택 세대를 올리는 경로는 호스트가
/// 내려 있는 동안 실패하면 세대만 남아 옛 컨텍스트를 영구 무효화한다
/// (2026-09-17 실측). 대상이 다르면 기존 선택 경로를 쓴다.
pub fn should_reconnect_retained_context(
    retained: Option<&ReconnectRouteTarget>,
    target: Option<&ReconnectRouteTarget>,
) -> bool {
    let (Some(retained), Some(target)) = (retained, target) else {
        return false;
    };
    retained.host == target.host && retained.port == target.port
}

/// 게이트 상태의 읽기 전용 스냅숏.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoReconnectGateSnapshot {
    pub user_disconnected: bool,
    pub pairing_stale: bool,
}

#[derive(Debug, Default, Clone)]
pub struct AutoReconnectGate {
    user_disconnected: bool,
    pairing_stale: bool,
    last_attempt_at: Option<u64>,
}

impl AutoReconnectGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_user_disconnected(&mut self) {
        self.user_disconnected = true;
    }

    pub fn mark_pairing_stale(&mut self) {
        self.pairing_stale = true;
    }

    /// 어떤 경로로든 연결에 성공하면 게이트를 초기화해 자동 재연결 조건을 되살린다.
    pub fn mark_connected(&mut self) {
        self.user_disconnected = false;
        self.pairing_stale = false;
    }

    pub fn note_auto_reconnect_attempt(&mut self, now: u64) {
        self.last_attempt_at = Some(now);
    }

    /// 홈 배지·자동 재연결 UI용 게이트 상태 스냅숏.
    pub fn snapshot(&self) -> AutoReconnectGateSnapshot {
        AutoReconnectGateSnapshot {
            user_disconnected: self.user_disconnected,
            pairing_stale: self.pairing_stale,
        }
    }

    /// 뷰어 화면용 게이트 스냅숏 판정.
    pub fn should_auto_reconnect_from_gate(
        &self,
        has_client: bool,
        has_recent_host: bool,
        now: u64,
    ) -> bool {
        should_auto_reconnect(&AutoReconnectDecision {
            has_client,
            has_recent_host,
            user_disconnected: self.user_disconnected,
            pairing_stale: self.pairing_stale,
            last_attempt_at: self.last_attempt_at,
            now,
            min_interval_ms: None,
        })
    }

    /// 프로세스당 단일 자동 진입 플래그 — 자동 재연결이 한 번 시작되면
    /// (마지막 시도 기록으로) 같은 순간의 포커스 이벤트가 되풀이하지 않게 한다.
    pub fn try_auto_entry(&mut self, has_client: bool, has_recent_host: bool, now: u64) -> bool {
        if !self.should_auto_reconnect_from_gate(has_client, has_recent_host, now) {
            return false;
        }
        self.note_auto_reconnect_attempt(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(overrides: impl FnOnce(&mut AutoReconnectDecision)) -> AutoReconnectDecision {
        let mut d = AutoReconnectDecision {
            has_client: false,
            has_recent_host: true,
            user_disconnected: false,
            pairing_stale: false,
            last_attempt_at: None,
            now: 100_000,
            min_interval_ms: None,
        };
        overrides(&mut d);
        d
    }

    #[test]
    fn attempts_when_disconnected_with_a_recent_host() {
        assert!(should_auto_reconnect(&decision(|_| {})));
    }

    #[test]
    fn never_attempts_while_a_client_exists_or_no_recent_host_is_known() {
        assert!(!should_auto_reconnect(&decision(|d| d.has_client = true)));
        assert!(!should_auto_reconnect(&decision(
            |d| d.has_recent_host = false
        )));
    }

    #[test]
    fn respects_explicit_disconnect_and_stale_pairing() {
        assert!(!should_auto_reconnect(&decision(
            |d| d.user_disconnected = true
        )));
        assert!(!should_auto_reconnect(
            &decision(|d| d.pairing_stale = true)
        ));
    }

    #[test]
    fn throttles_repeated_attempts_inside_the_minimum_interval() {
        assert!(!should_auto_reconnect(&decision(|d| {
            d.last_attempt_at = Some(95_000);
        })));
        assert!(should_auto_reconnect(&decision(|d| {
            d.last_attempt_at = Some(89_999);
        })));
        assert!(should_auto_reconnect(&decision(|d| {
            d.last_attempt_at = Some(90_000);
            d.min_interval_ms = Some(1_000);
        })));
    }

    #[test]
    fn gate_blocks_after_disconnect_and_reopens_after_connect() {
        let mut gate = AutoReconnectGate::new();
        gate.mark_user_disconnected();
        assert!(!gate.should_auto_reconnect_from_gate(false, true, 200_000));
        gate.mark_connected();
        assert!(gate.should_auto_reconnect_from_gate(false, true, 200_000));
    }

    #[test]
    fn gate_blocks_after_silent_401_until_connect_resets() {
        let mut gate = AutoReconnectGate::new();
        gate.mark_pairing_stale();
        assert!(!gate.should_auto_reconnect_from_gate(false, true, 200_000));
        gate.mark_connected();
        assert!(gate.should_auto_reconnect_from_gate(false, true, 200_000));
    }

    #[test]
    fn gate_applies_the_attempt_throttle() {
        let mut gate = AutoReconnectGate::new();
        gate.mark_connected();
        gate.note_auto_reconnect_attempt(300_000);
        assert!(!gate.should_auto_reconnect_from_gate(false, true, 305_000));
        assert!(gate.should_auto_reconnect_from_gate(false, true, 311_000));
    }

    #[test]
    fn single_auto_entry_consumes_one_gate_window() {
        let mut gate = AutoReconnectGate::new();
        assert!(gate.try_auto_entry(false, true, 100_000));
        // 기록된 시도는 스로틀을 걸어 같은 포커스의 되풀이를 막는다.
        assert!(!gate.try_auto_entry(false, true, 100_001));
        assert!(gate.try_auto_entry(false, true, 110_001));
    }

    #[test]
    fn retained_context_is_reused_only_for_the_same_host_and_port() {
        let context = ReconnectRouteTarget {
            host: "100.80.133.120".into(),
            port: 7777,
        };
        assert!(should_reconnect_retained_context(
            Some(&context),
            Some(&ReconnectRouteTarget {
                host: "100.80.133.120".into(),
                port: 7777
            })
        ));
        assert!(!should_reconnect_retained_context(
            Some(&context),
            Some(&ReconnectRouteTarget {
                host: "100.80.133.121".into(),
                port: 7777
            })
        ));
        assert!(!should_reconnect_retained_context(
            Some(&context),
            Some(&ReconnectRouteTarget {
                host: "100.80.133.120".into(),
                port: 7778
            })
        ));
        assert!(!should_reconnect_retained_context(
            None,
            Some(&ReconnectRouteTarget {
                host: "10.0.0.1".into(),
                port: 7777
            })
        ));
        assert!(!should_reconnect_retained_context(Some(&context), None));
        assert!(!should_reconnect_retained_context(None, None));
    }
}
