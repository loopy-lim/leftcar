//! SessionRow JSON 계약(src/sessionTypes.ts 포팅). React 앱이 get_status
//! 결과(SessionView + StatsInfo flatten)를 그대로 읽던 형태다 — 필드
//! 이름(camelCase)과 선택성을 그대로 유지해야 dispatch JSON이 역직렬화된다.
//!
//! 숫자 선택 필드는 Option<f64>다. TS number는 실수 하나로 정수·실수를
//! 모두 담고, encoderDiagnostics의 measured 헬퍼가 `Number.isFinite` +
//! trunc로 소비하기 때문이다. JSON에서 정수도 f64로 무손실 읽힌다.
//!
//! 예외 하나: `baseFrameQp`는 TS가 `?: number | null`로 부재(null 아님)와
//! null을 구별한다 — `QP 측정 중`(부재)과 `QP 적용 안 됨`(null)이 다른
//! 문장이므로 Option<Option<f64>>로 그 구별을 보존한다(아래 double_option).

use serde::{Deserialize, Deserializer, Serialize};

/// 부재가 아닌 null을 Some(None)으로 읽는 헬퍼. 필드가 아예 없으면
/// `#[serde(default)]`가 None을 준다.
fn some_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<f64>>, D::Error> {
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    pub session: u32,
    pub source_index: u32,
    pub source_name: String,
    pub viewer_addr: String,
    pub state: String,
    pub fps: f64,
    pub kbps: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps_target: Option<f64>,
    /// AppliedUdpStability — UI는 모양을 결정하지 않고 라벨만 골랐다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_submit_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_output_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_callbacks: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_output_callbacks: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_submit_failures: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_in_flight: Option<f64>,
    pub input_enabled: bool,
    pub input_rate_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_dropped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_queue_dropped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_frames_dropped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_keyframes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_requests_suppressed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_send_failures: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_queue_dropped: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_to_encode_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_capture_to_encode_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_queue_wait_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_capture_queue_wait_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_output_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_encode_output_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packetization_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_packetization_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_block_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_send_block_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_frame: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_frame_bytes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_frame_oldest_age_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_capture_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_encode_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_send_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_bitrate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_hint: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_override: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_adaptation_checks: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_adaptation_changes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_adaptation_rejections: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_adaptation_last_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_experiment_diagnostics_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_experiment_requested: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_experiment_applied: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_experiment_fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_frame_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_frame_drop_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_encode_output_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_submit_call_p50_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_submit_call_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_callback_p50_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_callback_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packetization_in_flight: Option<f64>,
    /// 부재(측정 전)와 null(적용 안 됨)이 다른 문장이다 — 모듈 문서 참고.
    /// 직렬화에서도 부재는 부재로 남는다(읽기 전용 뷰의 JSON 왕복 보존).
    #[serde(
        default,
        deserialize_with = "some_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_frame_qp: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_frame_qp_changes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_mode: Option<String>,
    /// TS 필드 이름이 encoderID다(camelCase 변환과 철자가 다르다).
    #[serde(rename = "encoderID", default, skip_serializing_if = "Option::is_none")]
    pub encoder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_hardware_accelerated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_applied_properties: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_unsupported_properties: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_rejected_properties: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_interval_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_to_encode_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_queue_wait_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_output_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packetization_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_output_interval_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_block_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_pace_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_bytes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_fragments: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_parity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_expected_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_send_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_au_is_keyframe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_au_bytes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_au_fragments: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_parity_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_frame_gaps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_input_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_incomplete_aus: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_stale_frames: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_output_burst_discards: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_rtt_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_wire_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_feedback_age_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_burst_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_pacing_rate_multiplier: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_fec_parity_shards: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_adaptive_pacing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_burst_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_media_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_data_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_parity_datagrams: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_fec_restored_fragments: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_unrecoverable_fec_groups: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_max_missing_data_fragments: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_one_frame_gap_events: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_multi_frame_gap_events: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_paired_idr_episodes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_suppressed_recovery_requests: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_fec_decode_failures: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_preparation_p50_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_preparation_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_pair_admission_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_pair_callback_p50_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_pair_callback_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_pair_timeouts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_pair_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_valid_encode_output_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_valid_encode_output_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_encoder_frame_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_encoder_frame_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_bitrate_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_bitrate_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_bitrate_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_receiver_loss: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_receiver_loss: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_rendered_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_rendered_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_rendered_fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_ready_delta_p95_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_ready_delta_max_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_sync_timeouts: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unmatched_output_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_recovery_requests: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_recovery_keyframes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_test_injected_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_flow_active_leases: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_flow_capacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_pre_encode_admission_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_encoded_queue_depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_encoded_queue_oldest_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_recovery_boundary_discards: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_post_encode_delta_drops: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_wire_pairs_attempted: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_wire_pair_send_failures: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_keyframe_gap_recoveries: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_delta_gap_recoveries: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// encoderDiagnostics.test.ts의 base session과 같은 최소 JSON + 실제
    /// StatsInfo의 다른 필드(width, qualityState 등)가 섞인 모양. 이후
    /// 테스트들이 같은 상수를 쓴다.
    pub(crate) fn sample_json() -> serde_json::Value {
        json!({
            "deviceName": "Galaxy XR",
            "session": 7,
            "sourceIndex": 0,
            "sourceName": "Built-in Display",
            "viewerAddr": "192.168.0.20:5001",
            "width": 1920,
            "height": 1080,
            "qualityState": "native",
            "state": "running",
            "fps": 60,
            "kbps": 4200,
            "inputEnabled": true,
            "inputRateHz": 60,
            "encoderMode": "ave",
            "encoderID": "com.apple.videotoolbox.videoencoder.ave.avc",
            "encoderHardwareAccelerated": true,
            "encoderPreset": "high-speed",
            "encoderProfile": "main",
            "encoderAppliedProperties": ["HighSpeed", "Quality"],
            "encoderUnsupportedProperties": ["SuggestedLookAheadFrameCount"],
            "encoderRejectedProperties": ["Quality=-12900"],
            "encoderFallbackReason": null
        })
    }

    #[test]
    fn decodes_the_react_session_shape_verbatim() {
        let row: SessionRow = serde_json::from_value(sample_json()).unwrap();
        assert_eq!(row.session, 7);
        assert_eq!(row.state, "running");
        assert_eq!(row.fps, 60.0);
        assert_eq!(row.input_rate_hz, 60.0);
        assert!(row.input_enabled);
        assert_eq!(row.device_name.as_deref(), Some("Galaxy XR"));
        assert_eq!(
            row.encoder_id.as_deref(),
            Some("com.apple.videotoolbox.videoencoder.ave.avc")
        );
        assert_eq!(row.encoder_hardware_accelerated, Some(true));
        assert_eq!(row.quality_hint, None);
        // JSON의 null과 부재가 같은 None으로 읽힌다(baseFrameQp 제외).
        assert_eq!(row.encoder_fallback_reason, None);
    }

    #[test]
    fn base_frame_qp_keeps_null_apart_from_absent() {
        let with_qp: SessionRow = serde_json::from_value(json!({
            "session": 1, "sourceIndex": 0, "sourceName": "s", "viewerAddr": "a",
            "state": "running", "fps": 60, "kbps": 1, "inputEnabled": false,
            "inputRateHz": 60,
            "baseFrameQp": 34, "baseFrameQpChanges": 2
        }))
        .unwrap();
        assert_eq!(with_qp.base_frame_qp, Some(Some(34.0)));

        let with_null: SessionRow = serde_json::from_value(json!({
            "session": 1, "sourceIndex": 0, "sourceName": "s", "viewerAddr": "a",
            "state": "running", "fps": 60, "kbps": 1, "inputEnabled": false,
            "inputRateHz": 60, "baseFrameQp": null
        }))
        .unwrap();
        assert_eq!(with_null.base_frame_qp, Some(None), "null은 적용 안 됨이다");

        let absent: SessionRow = serde_json::from_value(json!({
            "session": 1, "sourceIndex": 0, "sourceName": "s", "viewerAddr": "a",
            "state": "running", "fps": 60, "kbps": 1, "inputEnabled": false,
            "inputRateHz": 60
        }))
        .unwrap();
        assert_eq!(absent.base_frame_qp, None, "부재는 측정 전이다");
    }

    #[test]
    fn unknown_fields_are_ignored_like_the_ts_type() {
        // 실제 StatsInfo에는 SessionRow에 없는 필드가 더 있다(예:
        // captureWatchdogRestarts, udpSendRetries). TS가 무시하듯 serde도
        // 모른 필드를 버린다.
        let value = sample_json();
        let row: SessionRow = serde_json::from_value(value).unwrap();
        let back = serde_json::to_value(&row).unwrap();
        let reparsed: SessionRow = serde_json::from_value(back).unwrap();
        assert_eq!(reparsed, row);
    }

    #[test]
    fn split_and_receiver_fields_round_trip() {
        let row: SessionRow = serde_json::from_value(json!({
            "session": 1, "sourceIndex": 0, "sourceName": "s", "viewerAddr": "a",
            "state": "running", "fps": 60, "kbps": 1, "inputEnabled": false,
            "inputRateHz": 60,
            "splitDirection": "vertical",
            "leftValidEncodeOutputFps": 60,
            "rightValidEncodeOutputFps": 59,
            "receiverRttMs": null,
            "udpStability": { "mode": "burst" }
        }))
        .unwrap();
        assert_eq!(row.split_direction.as_deref(), Some("vertical"));
        assert_eq!(row.left_valid_encode_output_fps, Some(60.0));
        assert_eq!(row.right_valid_encode_output_fps, Some(59.0));
        assert_eq!(row.receiver_rtt_ms, None);
        assert!(row.udp_stability.is_some());
    }
}
