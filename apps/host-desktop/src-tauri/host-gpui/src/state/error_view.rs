//! 호스트 오류 분류(src/hooks/useHostStatus.ts의 hostErrorView 포팅).
//! 코어 오류 문자열을 화면 문구 5종(remote-desktop-permission |
//! screen-permission | network | service | generic)으로 분류한다. 원본
//! 훅의 분기 규칙(소문자화 → 부분 문자열 검사, 검사 순서 포함)을 그대로
//! 옮겼고, 문구는 ui-tokens i18n.ts의 host.* 키와 같은 문장이다.

use leftcar_host_desktop::settings::HostLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostErrorKind {
    RemoteDesktopPermission,
    ScreenPermission,
    Network,
    Service,
    Generic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostErrorView {
    pub message: String,
    pub kind: HostErrorKind,
}

/// host.* 오류 문구 4종(ko/en — ui-tokens i18n.ts와 같은 문장).
pub fn host_error_strings(language: HostLanguage) -> HostErrorStrings {
    match language {
        HostLanguage::Ko => HostErrorStrings {
            screen_permission_error: "화면 공유 권한이 필요합니다. 시스템 설정에서 Leftcar를 허용해 주세요.",
            network_not_found_error: "연결할 네트워크를 찾지 못했습니다. Wi-Fi 또는 Tailscale 연결을 확인해 주세요.",
            app_service_init_error: "앱 서비스를 시작할 수 없습니다. Leftcar를 완전히 종료한 뒤 다시 실행해 주세요.",
            connection_check_error: "연결 상태를 확인하지 못했습니다. 잠시 후 새로고침해 주세요.",
        },
        HostLanguage::En => HostErrorStrings {
            screen_permission_error: "Screen recording permission required. Please allow Leftcar in System Settings.",
            network_not_found_error: "No network found. Please check your Wi-Fi or Tailscale connection.",
            app_service_init_error: "Unable to start app services. Please quit Leftcar completely and relaunch.",
            connection_check_error: "Unable to check connection status. Please refresh in a moment.",
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HostErrorStrings {
    pub screen_permission_error: &'static str,
    pub network_not_found_error: &'static str,
    pub app_service_init_error: &'static str,
    pub connection_check_error: &'static str,
}

/// 원본 규칙과 같은 순서다: "remote desktop"이 먼저이고("permission"보다
/// 우선), 그다음 permission/not authorized, no lan interface,
/// invoke/initialization, 나머지는 generic이다.
pub fn host_error_view(cause: &str, language: HostLanguage) -> HostErrorView {
    let message = cause.to_lowercase();
    let t = host_error_strings(language);
    let kind_and_message = if message.contains("remote desktop") {
        (HostErrorKind::RemoteDesktopPermission, t.screen_permission_error)
    } else if message.contains("permission") || message.contains("not authorized") {
        (HostErrorKind::ScreenPermission, t.screen_permission_error)
    } else if message.contains("no lan interface") {
        (HostErrorKind::Network, t.network_not_found_error)
    } else if message.contains("invoke") || message.contains("initialization") {
        (HostErrorKind::Service, t.app_service_init_error)
    } else {
        (HostErrorKind::Generic, t.connection_check_error)
    };
    HostErrorView {
        message: kind_and_message.1.to_string(),
        kind: kind_and_message.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 원본 훅 분기: remote desktop이 permission보다 우선한다.
    #[test]
    fn classifies_remote_desktop_before_generic_permission() {
        let view = host_error_view("Remote Desktop permission denied", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::RemoteDesktopPermission);
        assert_eq!(
            view.message,
            "화면 공유 권한이 필요합니다. 시스템 설정에서 Leftcar를 허용해 주세요."
        );
    }

    // 원본 훅 분기: permission / not authorized → screen-permission.
    #[test]
    fn classifies_permission_failures_as_screen_permission() {
        let view = host_error_view("capture permission required", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::ScreenPermission);
        let view = host_error_view("device not authorized", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::ScreenPermission);
        // 대소문자 무관(원본이 toLowerCase로 검사한다).
        let view = host_error_view("NOT AUTHORIZED", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::ScreenPermission);
    }

    // 원본 훅 분기: no lan interface → network.
    #[test]
    fn classifies_a_missing_lan_interface_as_network() {
        let view = host_error_view("no LAN interface found", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::Network);
        assert_eq!(
            view.message,
            "연결할 네트워크를 찾지 못했습니다. Wi-Fi 또는 Tailscale 연결을 확인해 주세요."
        );
    }

    // 원본 훅 분기: invoke / initialization → service.
    #[test]
    fn classifies_transport_failures_as_service() {
        let invoke = host_error_view("invoke get_status failed", HostLanguage::Ko);
        assert_eq!(invoke.kind, HostErrorKind::Service);
        let init = host_error_view("backend initialization failed", HostLanguage::Ko);
        assert_eq!(init.kind, HostErrorKind::Service);
        assert_eq!(
            init.message,
            "앱 서비스를 시작할 수 없습니다. Leftcar를 완전히 종료한 뒤 다시 실행해 주세요."
        );
    }

    // 원본 훅 분기: 나머지는 generic이다.
    #[test]
    fn falls_back_to_generic() {
        let view = host_error_view("encoder failed to start", HostLanguage::Ko);
        assert_eq!(view.kind, HostErrorKind::Generic);
        assert_eq!(
            view.message,
            "연결 상태를 확인하지 못했습니다. 잠시 후 새로고침해 주세요."
        );
    }

    // 파생: en 문구 표도 같은 분류를 쓴다.
    #[test]
    fn english_strings_follow_the_same_classification() {
        let view = host_error_view("No LAN interface found", HostLanguage::En);
        assert_eq!(view.kind, HostErrorKind::Network);
        assert_eq!(
            view.message,
            "No network found. Please check your Wi-Fi or Tailscale connection."
        );
        let generic = host_error_view("mystery", HostLanguage::En);
        assert_eq!(
            generic.message,
            "Unable to check connection status. Please refresh in a moment."
        );
    }
}
