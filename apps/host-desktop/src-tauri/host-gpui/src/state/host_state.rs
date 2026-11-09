//! Host 데스크톱 셸 UI 모델(src/hostState.ts 포팅). 트레이 상태 문구와
//! 주소 포맷을 소유한다. 원본 vitest 스펙(hostState.test.ts)을 옮겼다.

use serde::{Deserialize, Serialize};
use leftcar_host_desktop::settings::HostLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Unpaired,
    Advertising,
    AwaitingHostApproval,
    PairedOffline,
    Connecting,
    Connected,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshotView {
    pub platform: HostPlatform,
    pub pairing_state: PairingState,
    pub active_stream_count: u32,
}

/// 메뉴바 상태 문구(docs/07 §10: capture visibility). 스트리밍 중에는
/// 화면 전송 상태가 제일 앞에 온다.
pub fn tray_status(snapshot: &HostSnapshotView) -> String {
    if snapshot.active_stream_count > 0 {
        return format!("Leftcar — 화면 전송 중 ({})", snapshot.active_stream_count);
    }
    if snapshot.pairing_state == PairingState::Connected {
        return "Leftcar — 연결됨, 대기 중".into();
    }
    "Leftcar".into()
}

/// 푸터·페어링 UI가 보여주는 호스트 주소. IP가 아직 없으면 포트만 보인다.
pub fn format_host_address(lan_ip: Option<&str>, control_port: u16) -> String {
    match lan_ip {
        Some(lan_ip) => format!("{lan_ip}:{control_port}"),
        None => format!(":{control_port}"),
    }
}

/// 트레이·네이티브 표면의 언어만 갈아 끼우는 라벨 표(T-5 트레이가 쓴다).
/// Tauri 앱의 트레이 메뉴 라벨(lib.rs run())과 같은 문장이다.
pub fn tray_menu_labels(language: HostLanguage) -> (&'static str, &'static str, &'static str) {
    match language {
        HostLanguage::Ko => (
            "Leftcar Host 열기",
            "연결 코드 만들기…",
            "Leftcar Host 종료",
        ),
        HostLanguage::En => (
            "Open Leftcar Host",
            "Generate Pairing Code…",
            "Quit Leftcar Host",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HostSnapshotView {
        HostSnapshotView {
            platform: HostPlatform::Macos,
            pairing_state: PairingState::Unpaired,
            active_stream_count: 0,
        }
    }

    // 원본: shows capture state prominently
    #[test]
    fn shows_capture_state_prominently() {
        let status = tray_status(&HostSnapshotView {
            active_stream_count: 3,
            ..base()
        });
        assert!(status.contains("화면 전송 중"), "status: {status}");
        assert!(status.contains("3"), "status: {status}");
    }

    // 파생(원본 trayStatus 분기): 연결됨 + 스트림 없음.
    #[test]
    fn shows_the_waiting_line_when_connected_without_streams() {
        let status = tray_status(&HostSnapshotView {
            pairing_state: PairingState::Connected,
            ..base()
        });
        assert_eq!(status, "Leftcar — 연결됨, 대기 중");
    }

    // 파생(원본 trayStatus 분기): 그 외엔 짧은 이름만.
    #[test]
    fn keeps_the_plain_name_otherwise() {
        assert_eq!(tray_status(&base()), "Leftcar");
        let revoked = HostSnapshotView {
            pairing_state: PairingState::Revoked,
            ..base()
        };
        assert_eq!(tray_status(&revoked), "Leftcar");
    }

    // 파생(원본 formatHostAddress): IP 유무로 두 형태.
    #[test]
    fn formats_the_host_address_with_and_without_an_ip() {
        assert_eq!(format_host_address(Some("192.168.0.5"), 7777), "192.168.0.5:7777");
        assert_eq!(format_host_address(None, 7777), ":7777");
    }

    // 파생(lib.rs run()의 트레이 라벨): ko/en 라벨 표.
    #[test]
    fn tray_menu_labels_follow_the_language() {
        assert_eq!(
            tray_menu_labels(HostLanguage::Ko),
            ("Leftcar Host 열기", "연결 코드 만들기…", "Leftcar Host 종료")
        );
        assert_eq!(
            tray_menu_labels(HostLanguage::En),
            (
                "Open Leftcar Host",
                "Generate Pairing Code…",
                "Quit Leftcar Host"
            )
        );
    }

    // JSON 계약: 스냅샷은 camelCase로 왕복한다.
    #[test]
    fn snapshot_round_trips_camel_case() {
        let value = serde_json::json!({
            "platform": "linux",
            "pairingState": "awaiting_host_approval",
            "activeStreamCount": 2
        });
        let snapshot: HostSnapshotView = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(snapshot.platform, HostPlatform::Linux);
        assert_eq!(snapshot.pairing_state, PairingState::AwaitingHostApproval);
        assert_eq!(snapshot.active_stream_count, 2);
        let back: HostSnapshotView = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
        assert_eq!(back, snapshot);
    }
}
