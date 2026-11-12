//! 제어 평면 응답 뷰 타입(TS control.ts의 인터페이스 대응). 호스트가 필드를
//! 생략하거나 늘릴 수 있으므로 모두 기본값/옵셔널로 파싱하고, 모르는 필드는
//! `extra`에 남겨 RN 계층으로 그대로 흘려보낸다.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// TS DEFAULT_CONTROL_PORT(apps/viewer-expo/src/defaults.ts) — 의존성 없는
/// 잎 모듈이어야 한다는 주석 규칙을 따라 여기서도 상수로 둔다.
pub const DEFAULT_CONTROL_PORT: u16 = 7777;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CaptureBackendInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowSizeView {
    #[serde(rename = "widthPx", default)]
    pub width_px: i64,
    #[serde(rename = "heightPx", default)]
    pub height_px: i64,
}

/// TS CatalogView. 느슨한 필드는 `Value`(unknown 대응), 모르는 필드는
/// `extra`로 흡수한다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CatalogView {
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub capture_backends: Vec<CaptureBackendInfo>,
    #[serde(rename = "mediaHost", default, skip_serializing_if = "Option::is_none")]
    pub media_host: Option<String>,
    #[serde(
        rename = "publicMediaEndpoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub public_media_endpoint: Option<String>,
    /// 호스트의 테일넷 자기 주소(100.64/10). 구버전 호스트는 생략한다.
    #[serde(
        rename = "tailscaleHost",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tailscale_host: Option<String>,
    #[serde(default)]
    pub displays: Vec<DisplayInfo>,
    #[serde(
        rename = "encoderExperiments",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub encoder_experiments: Option<Value>,
    #[serde(
        rename = "udpStabilityCapabilities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub udp_stability_capabilities: Option<Value>,
    /// reconfigureStream에 encoderExperiment를 받는지(능력 플래그).
    #[serde(
        rename = "reconfigureEncoderExperiment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reconfigure_encoder_experiment: Option<bool>,
    /// reconfigureStream에 sourceIndex를 받는지(능력 플래그).
    #[serde(
        rename = "reconfigureSource",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reconfigure_source: Option<bool>,
    /// 이 기기가 마지막으로 보고한 스트림 창 크기(px).
    #[serde(
        rename = "windowSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub window_size: Option<WindowSizeView>,
    /// 상시 원격 입력 허용 여부(기본 허용). 미인증 호출에는 필드가 없다.
    #[serde(
        rename = "inputAllowed",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_allowed: Option<bool>,
    /// 방금 제거 요청된 확장 화면의 sourceId(시스템 반영 최대 ~30s).
    #[serde(
        rename = "virtualDisplayPendingRemoval",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_display_pending_removal: Option<String>,
    #[serde(
        rename = "virtualDisplayControl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_display_control: Option<bool>,
    #[serde(
        rename = "virtualDisplaySourceId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_display_source_id: Option<String>,
    /// 모르는 필드 흡수(호스트 신버전과의 라운드트립 보존).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

// TS SessionView — UI와 T-7 정책 모듈이 쓰는 핵심 텔레메트리만 타입으로
// 고정하고 나머지는 `extra`로 흡수한다(호스트가 늘리는 진단 필드가 많다).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionView {
    #[serde(
        rename = "deviceName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub device_name: Option<String>,
    #[serde(default)]
    pub session: i64,
    #[serde(rename = "sourceIndex", default)]
    pub source_index: i64,
    #[serde(rename = "sourceName", default)]
    pub source_name: String,
    #[serde(rename = "viewerAddr", default)]
    pub viewer_addr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub kbps: f64,
    #[serde(rename = "fpsTarget", default)]
    pub fps_target: f64,
    #[serde(
        rename = "qualityState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub quality_state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_fps: Option<f64>,
    #[serde(
        rename = "renderedFps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendered_fps: Option<f64>,
    #[serde(default)]
    pub dropped: i64,
    #[serde(
        rename = "networkDropped",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub network_dropped: Option<i64>,
    #[serde(default)]
    pub frames: i64,
    #[serde(default)]
    pub bytes: i64,
    #[serde(
        rename = "captureBackend",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub capture_backend: Option<String>,
    #[serde(
        rename = "mediaTransport",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub media_transport: Option<String>,
    #[serde(rename = "firstCaptureMs", default)]
    pub first_capture_ms: f64,
    #[serde(rename = "firstEncodeMs", default)]
    pub first_encode_ms: f64,
    #[serde(rename = "firstSendMs", default)]
    pub first_send_ms: f64,
    #[serde(rename = "currentBitrate", default)]
    pub current_bitrate: f64,
    #[serde(rename = "captureToEncodeUs", default)]
    pub capture_to_encode_us: f64,
    #[serde(rename = "maxCaptureToEncodeUs", default)]
    pub max_capture_to_encode_us: f64,
    #[serde(rename = "sendBlockUs", default)]
    pub send_block_us: f64,
    #[serde(rename = "maxSendBlockUs", default)]
    pub max_send_block_us: f64,
    #[serde(default)]
    pub pending_frame: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(
        rename = "receiverRttMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiver_rtt_ms: Option<f64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusView {
    #[serde(default)]
    pub sessions: Vec<SessionView>,
}

/// reconfigureStream 응답. encoderExperiment·sourceIndex는 능력이 있는 호스트만
/// 돌려준다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReconfigureStreamOutput {
    #[serde(default)]
    pub session: i64,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub fps: f64,
    #[serde(rename = "qualityState", default)]
    pub quality_state: Value,
    #[serde(
        rename = "encoderExperiment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub encoder_experiment: Option<String>,
    #[serde(
        rename = "sourceIndex",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_index: Option<i64>,
    #[serde(
        rename = "sourceName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_name: Option<String>,
}

/// TS preferredCaptureBackend — 지원 목록에 있으면 현재 선택을 유지하고,
/// 아니면 호스트가 나열한 첫 자동 백엔드, 없으면 macOS 기존 기본값.
pub fn preferred_capture_backend(catalog: Option<&CatalogView>, current: Option<&str>) -> String {
    let available = catalog
        .map(|c| c.capture_backends.as_slice())
        .unwrap_or(&[]);
    // TS `current &&` — 빈 문자열은 선택 없음과 같다.
    if let Some(current) = current.filter(|c| !c.is_empty()) {
        if available.iter().any(|backend| backend.id == current) {
            return current.to_string();
        }
    }
    available
        .first()
        .map(|backend| backend.id.clone())
        .unwrap_or_else(|| "screenCaptureKit".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_catalog_with_unknown_fields_round_tripped() {
        let raw = serde_json::json!({
            "platform": "linux",
            "captureBackends": [
                {"id": "windowsGraphicsCapture", "label": "WGC", "hint": "hardware H.264"}
            ],
            "mediaHost": "10.0.0.5",
            "displays": [{"index": 0, "name": "Screen 1", "width": 2560, "height": 1440}],
            "reconfigureEncoderExperiment": true,
            "brandNewField": {"x": 1}
        });
        let catalog: CatalogView = serde_json::from_value(raw.clone()).expect("catalog");
        assert_eq!(catalog.platform, "linux");
        assert_eq!(catalog.displays[0].name, "Screen 1");
        assert_eq!(catalog.reconfigure_encoder_experiment, Some(true));
        assert!(catalog.extra.contains_key("brandNewField"));
        // 왕복 직렬화에서 모르는 필드를 잃지 않는다.
        let back: Value = serde_json::to_value(&catalog).expect("serialize");
        assert_eq!(back.get("brandNewField"), raw.get("brandNewField"));
    }

    #[test]
    fn prefers_the_supported_backend_selection() {
        let catalog = CatalogView {
            capture_backends: vec![
                CaptureBackendInfo {
                    id: "screenCaptureKit".into(),
                    label: "SCK".into(),
                    hint: "default".into(),
                },
                CaptureBackendInfo {
                    id: "cgDisplayStream".into(),
                    label: "CGDS".into(),
                    hint: "compat".into(),
                },
            ],
            ..Default::default()
        };
        // Windows Graphics Capture instead of the macOS legacy default.
        let wgc = CatalogView {
            capture_backends: vec![CaptureBackendInfo {
                id: "windowsGraphicsCapture".into(),
                label: "Windows Graphics Capture".into(),
                hint: "hardware H.264".into(),
            }],
            ..Default::default()
        };
        assert_eq!(
            preferred_capture_backend(Some(&wgc), Some("screenCaptureKit")),
            "windowsGraphicsCapture"
        );
        assert_eq!(
            preferred_capture_backend(Some(&catalog), Some("cgDisplayStream")),
            "cgDisplayStream"
        );
        assert_eq!(preferred_capture_backend(None, None), "screenCaptureKit");
        // TS `current &&` — 빈 문자열 선택은 호스트 첫 자동 백엔드를 따른다.
        let auto_only = CatalogView {
            capture_backends: vec![CaptureBackendInfo {
                id: "cgDisplayStream".into(),
                label: "Automatic".into(),
                hint: "pickerless".into(),
            }],
            ..Default::default()
        };
        assert_eq!(
            preferred_capture_backend(Some(&auto_only), Some("")),
            "cgDisplayStream"
        );
    }
}
