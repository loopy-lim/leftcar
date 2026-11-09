//! 오류 문구 포맷(TS formatErrorMessage). TS는 언어 저장소에서 번역을 가져오지만
//! Rust 코어는 문구 표를 주입받는다 — T-7의 언어 상태가 이 표를 채운다.
//! 매칭 순서와 판정 규칙은 control.ts의 formatErrorMessage와 동일하다.

/// 사용자 안내 문구 표. 매핑되지 않은 영어 원문은 개발자 진단용으로만 남기고
/// UI는 친절한 안내문 하나만 본다(원본 주석 참고).
#[derive(Debug, Clone)]
pub struct ErrorMessages {
    pub err_generic: &'static str,
    pub err_unauthorized: &'static str,
    pub err_host_key_mismatch: &'static str,
    pub err_pairing_rejected: &'static str,
    pub err_pairing_declined: &'static str,
    pub err_code_expired: &'static str,
    pub err_host_screen_permission: &'static str,
    pub err_host_timeout: &'static str,
    pub err_connect_failed: &'static str,
}

impl ErrorMessages {
    /// packages/ui-tokens/src/i18n.ts의 ko.viewer 문구.
    pub const KOREAN: ErrorMessages = ErrorMessages {
        err_generic: "문제가 발생했습니다. 잠시 후 다시 시도해 주세요.",
        err_unauthorized: "컴퓨터의 연결 승인이 필요합니다.",
        err_host_key_mismatch: "컴퓨터의 보안 키가 저장된 것과 다릅니다. QR을 다시 스캔해 다시 연결해 주세요.",
        err_pairing_rejected: "연결 코드가 맞지 않거나 만료되었습니다. 컴퓨터에서 새 연결 코드를 만들어 주세요.",
        err_pairing_declined: "컴퓨터에서 연결 요청을 거절했습니다. 다시 시도하려면 QR을 다시 스캔해 주세요.",
        err_code_expired: "연결 코드가 만료되었습니다. 컴퓨터에서 새 연결 코드를 만들어 주세요.",
        err_host_screen_permission: "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요.",
        err_host_timeout: "컴퓨터가 응답하지 않습니다. 같은 네트워크인지 확인한 뒤 다시 시도해 주세요.",
        err_connect_failed: "컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.",
    };

    /// packages/ui-tokens/src/i18n.ts의 en.viewer 문구.
    pub const ENGLISH: ErrorMessages = ErrorMessages {
        err_generic: "Something went wrong. Please try again in a moment.",
        err_unauthorized: "Pairing approval from the computer is required.",
        err_host_key_mismatch: "The computer's security key differs from the saved one. Scan the QR code again to re-pair.",
        err_pairing_rejected: "The pairing code is wrong or expired. Generate a new code on the computer.",
        err_pairing_declined: "The computer declined the pairing request. Scan the QR code again to retry.",
        err_code_expired: "The pairing code expired. Generate a new code on the computer.",
        err_host_screen_permission: "Screen sharing is disabled on the computer. Allow Leftcar in macOS System Settings.",
        err_host_timeout: "The computer is not responding. Check that both devices are on the same network and try again.",
        err_connect_failed: "Cannot connect to the computer. Check that Leftcar is running.",
    };
}

/// TS `[가-힣]` 판정과 동일한 한글 음절 범위(U+AC00–U+D7A3).
fn has_hangul(message: &str) -> bool {
    message.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

/// 임의 오류 원문을 사용자 안내 문구로 매핑한다. TS formatErrorMessage의
/// 판정 순서를 그대로 따른다(원문은 소문자로 내려 부분 일치를 본다).
pub fn format_error_message(message: &str, t: &ErrorMessages) -> String {
    let normalized = message.to_lowercase();
    if normalized.contains("unauthorized") {
        return t.err_unauthorized.to_string();
    }
    if normalized.contains("pinned key") {
        return t.err_host_key_mismatch.to_string();
    }
    if normalized.contains("pairing failed") {
        return t.err_pairing_rejected.to_string();
    }
    if normalized.contains("pairing rejected") {
        return t.err_pairing_declined.to_string();
    }
    if normalized.contains("offer not found") {
        return t.err_code_expired.to_string();
    }
    if normalized.contains("screen-recording permission")
        || normalized.contains("screen recording permission")
    {
        return t.err_host_screen_permission.to_string();
    }
    if normalized.contains("timeout") {
        return t.err_host_timeout.to_string();
    }
    if normalized.contains("connection closed")
        || normalized.contains("connection error")
        || normalized.contains("econn")
    {
        return t.err_connect_failed.to_string();
    }
    // 이 코드베이스의 안내 문구는 한국어로 작성되므로 한글이 섞인 메시지는
    // 이미 큐레이션된 것이다.
    if has_hangul(message) {
        return message.to_string();
    }
    t.err_generic.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_host_failures_like_the_ts_classifier() {
        let t = ErrorMessages::KOREAN;
        assert_eq!(
            format_error_message("custom error", &t),
            "문제가 발생했습니다. 잠시 후 다시 시도해 주세요."
        );
        assert_eq!(
            format_error_message("ECONNREFUSED", &t),
            "컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요."
        );
        assert_eq!(
            format_error_message("unauthorized", &t),
            "컴퓨터의 연결 승인이 필요합니다."
        );
        assert_eq!(
            format_error_message(
                "startStream failed: screen-recording permission is not granted to Leftcar Host",
                &t
            ),
            "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요."
        );
        assert_eq!(
            format_error_message("Screen Recording permission required", &t),
            "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요."
        );
        assert_eq!(
            format_error_message("pairing rejected", &t),
            "컴퓨터에서 연결 요청을 거절했습니다. 다시 시도하려면 QR을 다시 스캔해 주세요."
        );
        assert_eq!(
            format_error_message("host key does not match pinned key", &t),
            "컴퓨터의 보안 키가 저장된 것과 다릅니다. QR을 다시 스캔해 다시 연결해 주세요."
        );
        // 큐레이션된 한국어 문구는 그대로 나온다.
        assert_eq!(
            format_error_message("신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다", &t),
            "신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다"
        );
        assert_eq!(
            format_error_message("", &t),
            "문제가 발생했습니다. 잠시 후 다시 시도해 주세요."
        );
    }
}
