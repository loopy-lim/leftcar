//! GPUI 화면 계층의 순수 로직(계획 T-3/T-4). React 컴포넌트 안에 있던
//! 계산 가운데 UI 프레임워크와 무관한 것들을 모아 함수로 고정한다 —
//! vitest 렌더 단정 대신 "뷰 데이터 + 파생 규칙" 테스트로 증명한다
//! (T-2 encoder_diagnostics의 치환 원칙과 같다).
//!
//! - 테마 순환(시스템→라이트→다크)과 라벨
//! - 종료 배너 dedup(useHostStatus의 seenTerminations + 소실 세션 추적)
//! - 화질 상한 퍼센트(QualityOverride의 qualityOverride ?? qualityHint ?? 0.5)
//! - 페어링 QR 카운트다운·진행률·만료(PairingQrCard + usePairingModel의 1s tick)
//! - 파일 크기·페어링 날짜·종료 시각·카운트다운 포맷
//! - 확장 디스플레이 모드 검증(ExtendedDisplayCard.validDisplayMode)과 프리셋
//! - 실험 숫자 행 검증(ExperimentsSection ExperimentNumberRow.commit)

use crate::state::host_state::format_host_address;
use crate::state::session_row::SessionRow;
use crate::state::termination::{create_termination_notice, is_terminal_session, TerminationNotice, TerminationSession};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ===== 테마 =====

/// React의 ThemeMode("system"|"light"|"dark"). React는 localStorage에
/// 남기지만 GPUI 경로는 세션 안에서만 유지한다(마이그레이션 문서에 명시).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// 헤더 테마 버튼의 순환: system → light → dark → system.
    pub fn cycle(self) -> ThemeMode {
        match self {
            ThemeMode::System => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::System,
        }
    }
}

// ===== 종료 배너 dedup =====

/// 한 번의 get_status 접수에서 만들어지는 종료 배너 결과. `seen`에는
/// 실제로 배너가 된 키만 추가한다(React의 seenTerminations 규칙).
pub struct TerminationScan {
    pub notice: Option<TerminationNotice>,
    pub seen: HashSet<String>,
}

/// 상태 폴러가 관리하는 dedup 상태(seen 키 + 직전 활성 세션). useHostStatus의
/// useRef 3개(priorActiveSessions·seenTerminations·hasStatusSnapshot)에 해당.
#[derive(Debug, Default)]
pub struct TerminationTracker {
    seen: HashSet<String>,
    prior_active: HashMap<u32, TerminationSession>,
    has_snapshot: bool,
}

impl TerminationTracker {
    pub fn new() -> TerminationTracker {
        TerminationTracker::default()
    }

    /// get_status 결과를 접수하고 이번 폴링에서 새로 보여줄 배너 하나를
    /// 돌려준다. 터미널 세션과 "직전엔 활성이었는데 목록에서 사라진 세션"
    /// 두 경로 모두 dedup 키로 걸러진다. 이미 본 키는 seen에 다시 넣지
    /// 않는다(집합 크기 불변).
    pub fn scan(&mut self, sessions: &[SessionRow], now: SystemTime) -> TerminationScan {
        let mut candidate: Option<TerminationNotice> = None;
        for session in sessions.iter().filter(|session| is_terminal_session(&session.state)) {
            let notice = create_termination_notice(&TerminationSession::from(session), now);
            if !self.seen.contains(&notice.key) {
                self.seen.insert(notice.key.clone());
                candidate.get_or_insert(notice);
            }
        }
        if self.has_snapshot {
            let reported: HashSet<u32> = sessions.iter().map(|session| session.session).collect();
            for prior in self.prior_active.values() {
                if reported.contains(&prior.session) {
                    continue;
                }
                let notice = create_termination_notice(
                    &TerminationSession {
                        state: "stopped".into(),
                        error: None,
                        ..prior.clone()
                    },
                    now,
                );
                if !self.seen.contains(&notice.key) {
                    self.seen.insert(notice.key.clone());
                    candidate.get_or_insert(notice);
                }
            }
        }
        self.prior_active = sessions
            .iter()
            .filter(|session| !is_terminal_session(&session.state))
            .map(|session| (session.session, TerminationSession::from(session)))
            .collect();
        self.has_snapshot = true;
        TerminationScan {
            notice: candidate,
            seen: self.seen.clone(),
        }
    }

    /// 테스트·관찰용: 지금까지 본 dedup 키.
    pub fn seen_keys(&self) -> &HashSet<String> {
        &self.seen
    }
}

// ===== 세션 카드 =====

/// QualityOverride 슬라이더의 시작 퍼센트. qualityOverride ?? qualityHint ?? 0.5
/// 후 25–50 슬라이더 범위로 잘라낸다.
pub fn quality_slider_percent(session: &SessionRow) -> i64 {
    let fraction = session
        .quality_override
        .or(session.quality_hint)
        .unwrap_or(0.5);
    (fraction * 100.0).round().clamp(25.0, 50.0) as i64
}

/// 슬라이더 퍼센트 → set_session_quality 값(0.25–0.50).
pub fn quality_fraction(percent: i64) -> f32 {
    percent as f32 / 100.0
}

/// 세션 상태가 running이 아니면 카드 본문에 "상태 확인 중" 안내를 보인다.
pub fn show_status_checking(session: &SessionRow) -> bool {
    session.state != "running"
}

// ===== 페어링 QR 카드 =====

/// 활성 페어링 제안의 시계 상태. usePairingModel의 session 필드 3개
/// (expiresAt·durationMs)와 1초 tick 파생값 3개(expired·remainingMs·
/// progressPercent)를 하나로 묶었다.
#[derive(Debug, Clone, PartialEq)]
pub struct PairingClock {
    pub expires_at: SystemTime,
    pub duration: Duration,
}

impl PairingClock {
    pub fn new(expires_in_secs: u64, now: SystemTime) -> PairingClock {
        PairingClock {
            expires_at: now + Duration::from_secs(expires_in_secs),
            duration: Duration::from_secs(expires_in_secs),
        }
    }

    /// 만료 판정(usePairingModel의 `now >= session.expiresAt`).
    pub fn expired(&self, now: SystemTime) -> bool {
        now >= self.expires_at
    }

    /// 남은 시간. 만료 후 0이다.
    pub fn remaining(&self, now: SystemTime) -> Duration {
        self.expires_at.duration_since(now).unwrap_or(Duration::ZERO)
    }

    /// 남은 비율 0..=100(QR 카드 진행 바). duration이 0이면 0이다.
    pub fn progress_percent(&self, now: SystemTime) -> f64 {
        if self.duration.is_zero() {
            return 0.0;
        }
        let elapsed = self.duration.saturating_sub(self.remaining(now));
        let ratio = elapsed.as_secs_f64() / self.duration.as_secs_f64();
        (ratio * 100.0).clamp(0.0, 100.0)
    }

    /// QR 카드의 "남은 시간: MM:SS" — PairingQrCard.formatCountdown.
    pub fn countdown(&self, now: SystemTime) -> String {
        format_countdown(self.remaining(now))
    }
}

/// React의 formatCountdown: 올림한 초를 MM:SS로.
pub fn format_countdown(remaining: Duration) -> String {
    let total = remaining.as_secs_f64().ceil() as u64;
    let total = total.max(0);
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// 6자리 코드 표시 "123 456" — PairingQrCard의 replace 규칙.
pub fn format_pairing_code(code: &str) -> String {
    let digits: String = code.chars().collect();
    if digits.chars().all(|c| c.is_ascii_digit()) && digits.chars().count() == 6 {
        format!("{} {}", &digits[..3], &digits[3..])
    } else {
        digits
    }
}

// ===== 포맷 =====

/// FileShareCard.formatShareFileSize.
pub fn format_share_file_size(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{} KB", (size / 1024).max(1))
    } else {
        format!("{size} B")
    }
}

/// PairedDeviceRow.formatPairedAt: `unix:<secs>` 형식만 현지 포맷으로 바꾸고
/// 나머지는 그대로 둔다.
pub fn format_paired_at(paired_at: &str, now: SystemTime) -> String {
    let Some(secs_text) = paired_at.strip_prefix("unix:") else {
        return paired_at.to_string();
    };
    let Ok(unix_secs) = secs_text.parse::<f64>() else {
        return paired_at.to_string();
    };
    if !unix_secs.is_finite() || unix_secs <= 0.0 {
        return paired_at.to_string();
    }
    let elapsed = Duration::from_secs_f64(unix_secs);
    let Some(paired) = now.checked_sub(elapsed) else {
        return paired_at.to_string();
    };
    let since = now
        .duration_since(paired)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // 월·일·시각 표기는 toLocaleString(ko-KR)의 "M. D. HH:mm" 모양을 따른다.
    let days = since / 86_400;
    let clock = format_clock(UNIX_EPOCH + Duration::from_secs(since % 86_400));
    if days > 0 {
        format!("{days}. 일 전 · {clock}")
    } else {
        clock
    }
}

/// 종료 배너 시각과 진단 카드의 HH:MM:SS 표기(TerminationBanner의
/// toLocaleTimeString 대용품 — UTC 기준 24시간제).
pub fn format_clock(at: SystemTime) -> String {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let day_secs = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        day_secs / 3600,
        (day_secs % 3600) / 60,
        day_secs % 60
    )
}

/// 푸터·페어링 네트워크 카드가 복사하는 주소. IP가 없으면 비활성 복사 대신
/// "확인 중"을 보인다(format_host_address의 ":port" 모양은 그대로 둔다).
pub fn footer_address(lan_ip: Option<&str>, control_port: u16) -> String {
    format_host_address(lan_ip, control_port)
}

// ===== 확장 디스플레이 =====

/// 프리셋(ExtendedDisplayCard.PRESETS).
pub const DISPLAY_PRESETS: [(u32, u32, u32); 4] = [
    (1280, 800, 2),
    (1440, 900, 2),
    (1600, 1000, 2),
    (2560, 1600, 1),
];

/// 프리셋·자동·커스텀 선택을 나타내는 폼 상태(React의 choice 문자열 대신
/// 열거형).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayModeChoice {
    Auto,
    Preset(usize),
    Custom,
}

/// ExtendedDisplayCard.validDisplayMode와 같은 검증: 너비 640–4096 짝수,
/// 높이 480–4096 짝수.
pub fn valid_display_mode(width: f64, height: f64, scale: u32) -> bool {
    let integer_even = |value: f64, min: f64| {
        value.is_finite()
            && value.trunc() == value
            && value >= min
            && value <= 4096.0
            && value as u64 % 2 == 0
    };
    integer_even(width, 640.0) && integer_even(height, 480.0) && matches!(scale, 1 | 2)
}

/// 모드 선택 → 실제 모드. Auto는 제안값을, Preset은 프리셋을 쓰고 Custom은
/// 검증을 통과할 때만 값을 돌려준다(React의 `valid` 게이트).
pub fn resolve_display_mode(
    choice: DisplayModeChoice,
    suggested: (u32, u32, u32),
    custom: (f64, f64, u32),
) -> Option<(u32, u32, u32)> {
    match choice {
        DisplayModeChoice::Auto => Some(suggested),
        DisplayModeChoice::Preset(index) => DISPLAY_PRESETS.get(index).copied(),
        DisplayModeChoice::Custom => valid_display_mode(custom.0, custom.1, custom.2)
            .then_some((custom.0 as u32, custom.1 as u32, custom.2)),
    }
}

/// 모드 라벨 "1280×800 (2560×1600)" — ExtendedDisplayCard.modeLabel.
pub fn display_mode_label(mode: (u32, u32, u32)) -> String {
    format!(
        "{}×{} ({}×{})",
        mode.0,
        mode.1,
        mode.0 * mode.2,
        mode.1 * mode.2
    )
}

// ===== 실험 섹션 =====

/// 실험 숫자 행의 커밋 판정(ExperimentsSection.commit): 빈 문자열은 해제(null),
/// 그 외는 정수이고 [min, max] 안일 때만 통과한다.
pub fn valid_experiment_input(draft: &str, min: i64, max: i64) -> Result<Option<i64>, ()> {
    let trimmed = draft.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: i64 = trimmed.parse().map_err(|_| ())?;
    if parsed < min || parsed > max {
        return Err(());
    }
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(extra: serde_json::Value) -> SessionRow {
        let mut base = json!({
            "session": 1, "sourceIndex": 0, "sourceName": "s",
            "viewerAddr": "a", "state": "running", "fps": 60, "kbps": 1,
            "inputEnabled": false, "inputRateHz": 60,
        });
        if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        serde_json::from_value(base).unwrap()
    }

    // 테마 순환: system → light → dark → system.
    #[test]
    fn theme_cycles_through_three_modes() {
        assert_eq!(ThemeMode::System.cycle(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.cycle(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.cycle(), ThemeMode::System);
        assert_eq!(ThemeMode::default(), ThemeMode::System);
    }

    // useHostStatus 선례: 터미널 세션이 처음 보일 때만 배너가 나온다.
    #[test]
    fn terminal_sessions_announce_once() {
        let mut tracker = TerminationTracker::new();
        let stopped = vec![row(json!({ "state": "stopped", "error": "viewer closed stream" }))];
        let now = SystemTime::now();
        let first = tracker.scan(&stopped, now);
        assert!(first.notice.is_some());
        let second = tracker.scan(&stopped, now);
        assert!(second.notice.is_none(), "같은 키는 두 번 알리지 않는다");
        assert_eq!(first.seen.len(), 1);
        assert_eq!(second.seen.len(), 1);
    }

    // useHostStatus 선례: 활성 세션이 목록에서 사라지면 stopped 배너를 만든다.
    #[test]
    fn vanished_active_session_becomes_a_stopped_notice() {
        let mut tracker = TerminationTracker::new();
        let running = vec![row(json!({}))];
        tracker.scan(&running, SystemTime::UNIX_EPOCH);
        let scan = tracker.scan(&[], SystemTime::UNIX_EPOCH);
        let notice = scan.notice.expect("소실 세션 배너");
        assert_eq!(notice.session, 1);
        assert_eq!(notice.title, "화면 공유가 종료되었습니다");
        assert!(tracker.seen_keys().contains(&notice.key));
        // 다음 폴링에서는 다시 알리지 않는다.
        assert!(tracker.scan(&[], SystemTime::UNIX_EPOCH).notice.is_none());
    }

    // useHostStatus 선례: 첫 스냅샷 전에는 소실 추적이 동작하지 않는다.
    #[test]
    fn first_snapshot_does_not_invent_stopped_notices() {
        let mut tracker = TerminationTracker::new();
        assert!(tracker.scan(&[], SystemTime::UNIX_EPOCH).notice.is_none());
    }

    // QualityOverride 선례: override > hint > 0.5, 슬라이더 범위 25–50.
    #[test]
    fn quality_percent_prefers_override_then_hint_then_default() {
        assert_eq!(quality_slider_percent(&row(json!({ "qualityOverride": 0.3, "qualityHint": 0.6 }))), 30);
        assert_eq!(quality_slider_percent(&row(json!({ "qualityHint": 0.6 }))), 50);
        assert_eq!(quality_slider_percent(&row(json!({}))), 50);
        assert_eq!(quality_slider_percent(&row(json!({ "qualityOverride": 0.9 }))), 50);
        assert_eq!(quality_slider_percent(&row(json!({ "qualityOverride": 0.1 }))), 25);
        assert_eq!(quality_fraction(40), 0.4);
    }

    // PairingQrCard 선례: 카운트다운은 올림한 MM:SS.
    #[test]
    fn countdown_formats_ceil_to_minutes_seconds() {
        assert_eq!(format_countdown(Duration::from_millis(115_400)), "01:56");
        assert_eq!(format_countdown(Duration::from_millis(0)), "00:00");
        assert_eq!(format_countdown(Duration::from_secs(120)), "02:00");
    }

    // usePairingModel 선례: 1초 tick에서 만료·잔여·진행률이 같이 움직인다.
    #[test]
    fn pairing_clock_tracks_expiry_and_progress() {
        let start = SystemTime::UNIX_EPOCH;
        let clock = PairingClock::new(120, start);
        assert!(!clock.expired(start + Duration::from_secs(60)));
        assert_eq!(clock.countdown(start + Duration::from_secs(61)), "00:59");
        assert!((clock.progress_percent(start + Duration::from_secs(60)) - 50.0).abs() < 1e-9);
        assert!(clock.expired(start + Duration::from_secs(120)));
        assert_eq!(clock.remaining(start + Duration::from_secs(121)), Duration::ZERO);
        assert!((clock.progress_percent(start + Duration::from_secs(999)) - 100.0).abs() < 1e-9);
    }

    // PairingQrCard 선례: 6자리 코드는 3+3으로 띄어 보인다.
    #[test]
    fn pairing_code_splits_into_two_groups_of_three() {
        assert_eq!(format_pairing_code("123456"), "123 456");
        assert_eq!(format_pairing_code("12ab56"), "12ab56", "숫자가 아니면 그대로");
        assert_eq!(format_pairing_code("12345"), "12345");
    }

    // FileShareCard 선례: 1MB 이상은 소수 1자리, 1KB 이상은 반올림 KB.
    #[test]
    fn file_sizes_format_like_the_react_card() {
        assert_eq!(format_share_file_size(512), "512 B");
        assert_eq!(format_share_file_size(1024), "1 KB");
        assert_eq!(format_share_file_size(1500), "2 KB");
        assert_eq!(format_share_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_share_file_size(1_500_000), "1.4 MB");
    }

    // PairedDeviceRow 선례: unix: 접두 날짜만 포맷하고 나머지는 그대로.
    #[test]
    fn paired_at_formats_unix_stamps_and_keeps_the_rest() {
        assert_eq!(format_paired_at("2026-11-06T00:00:00Z", SystemTime::now()), "2026-11-06T00:00:00Z");
        assert_eq!(format_paired_at("unix:0", SystemTime::now()), "unix:0");
        let at = UNIX_EPOCH + Duration::from_secs(3600); // 01:00:00 UTC
        assert_eq!(format_paired_at("unix:1800", at), "00:30:00");
    }

    // 종료 배너 시각 표기.
    #[test]
    fn clock_formats_as_hh_mm_ss() {
        assert_eq!(format_clock(UNIX_EPOCH), "00:00:00");
        assert_eq!(
            format_clock(UNIX_EPOCH + Duration::from_secs(36_061)),
            "10:01:01"
        );
    }

    // ExtendedDisplayCard 선례: 짝수·범위 검증과 프리셋 목록.
    #[test]
    fn display_mode_validation_and_presets_follow_the_react_card() {
        assert!(valid_display_mode(1280.0, 800.0, 2));
        assert!(!valid_display_mode(1281.0, 800.0, 2), "너비 홀수");
        assert!(!valid_display_mode(1280.0, 479.0, 2), "높이 하한");
        assert!(!valid_display_mode(639.0, 800.0, 2), "너비 하한");
        assert!(!valid_display_mode(4098.0, 800.0, 2), "너비 상한");
        assert!(!valid_display_mode(1280.0, 800.0, 3), "배율은 1·2만");
        assert!(!valid_display_mode(1280.5, 800.0, 2), "정수만");
        assert_eq!(DISPLAY_PRESETS.len(), 4);
        assert_eq!(display_mode_label(DISPLAY_PRESETS[0]), "1280×800 (2560×1600)");
    }

    // 모드 선택 해석: Auto는 제안값, Preset은 목록 밖이면 없음, Custom은 검증.
    #[test]
    fn display_mode_choice_resolves_like_the_form() {
        let suggested = (1920, 1080, 1);
        assert_eq!(
            resolve_display_mode(DisplayModeChoice::Auto, suggested, (640.0, 480.0, 1)),
            Some(suggested)
        );
        assert_eq!(
            resolve_display_mode(DisplayModeChoice::Preset(3), suggested, (0.0, 0.0, 1)),
            Some((2560, 1600, 1))
        );
        assert_eq!(resolve_display_mode(DisplayModeChoice::Preset(9), suggested, (0.0, 0.0, 1)), None);
        assert_eq!(
            resolve_display_mode(DisplayModeChoice::Custom, suggested, (1024.0, 768.0, 2)),
            Some((1024, 768, 2))
        );
        assert_eq!(
            resolve_display_mode(DisplayModeChoice::Custom, suggested, (1023.0, 768.0, 2)),
            None,
            "검증을 통과하지 못한 커스텀 값은 적용 버튼이 막는다"
        );
    }

    // ExperimentsSection 선례: 빈 값은 해제, 정수·범위 밖은 거절.
    #[test]
    fn experiment_inputs_validate_integer_range_or_blank() {
        assert_eq!(valid_experiment_input("", 1, 5), Ok(None));
        assert_eq!(valid_experiment_input(" 3 ", 1, 5), Ok(Some(3)));
        assert_eq!(valid_experiment_input("0", 1, 5), Err(()));
        assert_eq!(valid_experiment_input("6", 1, 5), Err(()));
        assert_eq!(valid_experiment_input("2.5", 1, 5), Err(()));
        assert_eq!(valid_experiment_input("abc", 1, 5), Err(()));
        assert_eq!(valid_experiment_input("0", 0, 120000), Ok(Some(0)));
    }
}
