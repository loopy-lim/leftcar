//! 스트림 종료 안내 문구 파생(src/streamTermination.ts 포팅). 터미널
//! 세션 판정과 오류 문자열→한글 제목·본문·톤 매핑, 배너 dedup 키를
//! 소유한다. 원본 vitest 스펙(streamTermination.test.ts)을 아래 테스트로
//! 그대로 옮겼다.
//!
//! 원본과의 차이: `observedAt: Date`는 [`std::time::SystemTime`]으로,
//! 세션 번호 타입은 패널 JSON 계약(u32)과 맞췄다. 필드 JSON 이름은
//! SessionRow와 같은 camelCase다.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// 종료 배너를 만들 세션 스냅샷. SessionRow에서 필요한 필드만 모은
/// 좁은 뷰다(원본 TS의 Pick<TerminationSession, ...> 사용례와 같은 역할).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminationSession {
    pub session: u32,
    pub source_name: String,
    pub viewer_addr: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminationTone {
    Neutral,
    Warning,
    Danger,
}

/// 종료 배너 한 건. `key`는 같은 종료 사실을 두 번 알리지 않는 dedup 키다
/// (`{session}:{state}:{error}` — useHostStatus의 seenTerminations 규칙).
#[derive(Debug, Clone, PartialEq)]
pub struct TerminationNotice {
    pub key: String,
    pub session: u32,
    pub source_name: String,
    pub viewer_addr: String,
    pub title: String,
    pub detail: String,
    pub tone: TerminationTone,
    pub observed_at: SystemTime,
}

/// 안내문을 우선 표시하고 영어 원문은 괄호 뒤에 붙여 진단 가능성을 유지한다.
fn with_raw_detail(raw: &str, fallback: &str) -> String {
    if raw.is_empty() {
        fallback.to_string()
    } else {
        format!("{fallback} ({raw})")
    }
}

/// running 세션은 종료 경로에 들어오지 않는다. `unknown`은 "확인 불가"로
/// 처리된 종료 상태다(원본 TERMINAL_STATES와 같다).
pub fn is_terminal_session(state: &str) -> bool {
    matches!(state, "error" | "stopped" | "unknown")
}

pub fn create_termination_notice(
    session: &TerminationSession,
    observed_at: SystemTime,
) -> TerminationNotice {
    let error = session.error.as_deref().map(str::trim).unwrap_or("");
    let normalized = error.to_lowercase();
    let mut title = "화면 공유가 종료되었습니다".to_string();
    let mut detail = "연결된 기기 또는 컴퓨터의 요청으로 화면 공유를 마쳤습니다.".to_string();
    let mut tone = TerminationTone::Neutral;

    if normalized == "viewer closed stream" {
        title = "연결된 기기에서 화면 공유를 종료했습니다".into();
        detail = "연결된 기기에서 뒤로 가기 또는 닫기를 선택해 화면 공유를 마쳤습니다.".into();
    } else if normalized.contains("operator stopped") {
        title = "컴퓨터에서 화면 공유를 종료했습니다".into();
        detail = "영상 전송과 원격 조작을 중지하고 연결된 기기에 종료 사실을 알렸습니다.".into();
    } else if normalized.contains("feedback timeout") || normalized.contains("connection lost") {
        title = "연결된 기기의 응답이 없어 자동 종료했습니다".into();
        detail = "연결된 기기가 약 6초 동안 응답하지 않아 화면 공유를 안전하게 정리했습니다.".into();
        tone = TerminationTone::Warning;
    } else if normalized.contains("backend stats unavailable") {
        title = "화면 상태를 확인할 수 없어 종료했습니다".into();
        detail = "화면 공유 상태를 확인할 수 없어 연결을 안전하게 정리했습니다.".into();
        tone = TerminationTone::Danger;
    } else if normalized.contains("screen recording") || normalized.contains("screen-recording") {
        title = "화면 공유 권한 문제로 종료했습니다".into();
        detail =
            "컴퓨터의 화면 녹화 권한이 꺼져 있습니다. 시스템 설정에서 Leftcar Host를 허용해 주세요.".into();
        tone = TerminationTone::Danger;
    } else if session.state == "error" {
        title = "문제가 생겨 화면 공유를 종료했습니다".into();
        detail = with_raw_detail(error, "화면을 가져오거나 보내는 중 문제가 발생했습니다.");
        tone = TerminationTone::Danger;
    } else if session.state == "unknown" {
        title = "화면 공유 상태를 확인할 수 없습니다".into();
        detail = with_raw_detail(error, "연결 상태를 확인할 수 없어 종료된 것으로 처리했습니다.");
        tone = TerminationTone::Warning;
    } else if !error.is_empty() {
        detail = with_raw_detail(error, "화면 공유가 문제로 종료되었습니다.");
        tone = TerminationTone::Warning;
    }

    TerminationNotice {
        key: format!("{}:{}:{}", session.session, session.state, error),
        session: session.session,
        source_name: session.source_name.clone(),
        viewer_addr: session.viewer_addr.clone(),
        title,
        detail,
        tone,
        observed_at,
    }
}

impl From<&crate::state::session_row::SessionRow> for TerminationSession {
    fn from(row: &crate::state::session_row::SessionRow) -> TerminationSession {
        TerminationSession {
            session: row.session,
            source_name: row.source_name.clone(),
            viewer_addr: row.viewer_addr.clone(),
            state: row.state.clone(),
            error: row.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_session(state: &str, error: Option<&str>) -> TerminationSession {
        TerminationSession {
            session: 7,
            source_name: "Built-in Display".into(),
            viewer_addr: "192.168.0.20:5001".into(),
            state: state.into(),
            error: error.map(Into::into),
        }
    }

    fn notice(session: &TerminationSession) -> TerminationNotice {
        create_termination_notice(session, SystemTime::UNIX_EPOCH)
    }

    // 원본: keeps running sessions out of the terminal path
    #[test]
    fn keeps_running_sessions_out_of_the_terminal_path() {
        assert!(!is_terminal_session("running"));
        assert!(is_terminal_session("stopped"));
        assert!(is_terminal_session("error"));
        assert!(is_terminal_session("unknown"));
    }

    // 원본: explains an operator stop in reader-facing Korean
    #[test]
    fn explains_an_operator_stop_in_reader_facing_korean() {
        let notice = notice(&base_session("stopped", Some("host operator stopped the stream")));
        assert_eq!(notice.title, "컴퓨터에서 화면 공유를 종료했습니다");
        assert!(notice.detail.contains("연결된 기기에 종료 사실"));
        assert_eq!(notice.tone, TerminationTone::Neutral);
    }

    // 원본: presents an exact viewer close as a normal device-initiated stop
    #[test]
    fn presents_an_exact_viewer_close_as_a_normal_device_initiated_stop() {
        let notice = notice(&base_session("stopped", Some("viewer closed stream")));
        assert_eq!(notice.title, "연결된 기기에서 화면 공유를 종료했습니다");
        assert!(notice.detail.contains("뒤로 가기"));
        assert_eq!(notice.tone, TerminationTone::Neutral);
    }

    // 원본: explains the automatic feedback timeout and its threshold
    #[test]
    fn explains_the_automatic_feedback_timeout_and_its_threshold() {
        let notice = notice(&base_session("error", Some("viewer connection lost (feedback timeout)")));
        assert_eq!(notice.title, "연결된 기기의 응답이 없어 자동 종료했습니다");
        assert!(notice.detail.contains("약 6초"));
        assert_eq!(notice.tone, TerminationTone::Warning);
    }

    // 원본: leads with a friendly message but keeps an unexpected backend
    // error visible
    #[test]
    fn keeps_an_unexpected_backend_error_visible() {
        let notice = notice(&base_session("error", Some("encoder failed to start")));
        assert_eq!(notice.title, "문제가 생겨 화면 공유를 종료했습니다");
        assert_eq!(
            notice.detail,
            "화면을 가져오거나 보내는 중 문제가 발생했습니다. (encoder failed to start)"
        );
        assert_eq!(notice.tone, TerminationTone::Danger);
    }

    // 원본: maps screen-recording permission failures to a settings guide
    #[test]
    fn maps_screen_recording_permission_failures_to_a_settings_guide() {
        let notice = notice(&base_session(
            "error",
            Some("startCapture failed: screen-recording permission required"),
        ));
        assert_eq!(notice.title, "화면 공유 권한 문제로 종료했습니다");
        assert!(notice.detail.contains("화면 녹화 권한"));
        assert_eq!(notice.tone, TerminationTone::Danger);
    }

    // 파생(useHostStatus.ts의 unknown 처리와 매핑표): 확인 불가 상태는 경고로.
    #[test]
    fn treats_an_unknown_state_as_a_warning() {
        let notice = notice(&base_session("unknown", None));
        assert_eq!(notice.title, "화면 공유 상태를 확인할 수 없습니다");
        assert_eq!(
            notice.detail,
            "연결 상태를 확인할 수 없어 종료된 것으로 처리했습니다."
        );
        assert_eq!(notice.tone, TerminationTone::Warning);
    }

    // 파생(매핑표 마지막 분기): 터미널이 아닌 세션의 알 수 없는 오류는
    // 기본 제목 + 경고 본문으로 끝난다.
    #[test]
    fn keeps_the_default_title_for_a_non_terminal_error() {
        let notice = notice(&base_session("running", Some("glitch")));
        assert_eq!(notice.title, "화면 공유가 종료되었습니다");
        assert_eq!(notice.detail, "화면 공유가 문제로 종료되었습니다. (glitch)");
        assert_eq!(notice.tone, TerminationTone::Warning);
    }

    // 파생(useHostStatus의 seenTerminations 규칙): dedup 키는
    // `{session}:{state}:{error}`다.
    #[test]
    fn dedup_key_is_session_state_error() {
        assert_eq!(
            notice(&base_session("stopped", Some("viewer closed stream"))).key,
            "7:stopped:viewer closed stream"
        );
        assert_eq!(notice(&base_session("stopped", None)).key, "7:stopped:");
    }

    // 파생(TS의 `session.error?.trim() ?? ""`): 공백만 있는 오류는 없는
    // 것과 같다.
    #[test]
    fn whitespace_only_error_is_treated_as_absent() {
        let notice = notice(&base_session("stopped", Some("   ")));
        assert_eq!(notice.key, "7:stopped:");
        assert_eq!(notice.detail, "연결된 기기 또는 컴퓨터의 요청으로 화면 공유를 마쳤습니다.");
    }

    // JSON 계약: SessionRow에서 좁혀 담는다(camelCase).
    #[test]
    fn narrows_from_a_session_row() {
        let row: crate::state::session_row::SessionRow = serde_json::from_value(serde_json::json!({
            "session": 3,
            "sourceIndex": 0,
            "sourceName": "Built-in Display",
            "viewerAddr": "192.168.0.20:5001",
            "state": "stopped",
            "fps": 60,
            "kbps": 4200,
            "inputEnabled": true,
            "inputRateHz": 60,
            "error": "viewer closed stream"
        }))
        .unwrap();
        let session = TerminationSession::from(&row);
        assert_eq!(session.session, 3);
        assert_eq!(session.source_name, "Built-in Display");
        assert_eq!(session.error.as_deref(), Some("viewer closed stream"));
        // camelCase JSON 왕복.
        let back: TerminationSession =
            serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert_eq!(back, session);
    }
}
