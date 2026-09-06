//! Host control package (docs/04 §5).

use crate::common::*;
use crate::udp_stability::{AppliedUdpStability, UdpStabilityCapabilities, UdpStabilityRequest};
use rustra::prelude::*;
use serde::{Deserialize, Serialize};

// -- permission and sources (5.1) ------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetCapturePermissionStateInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CapturePermissionState {
    NotDetermined,
    Granted,
    Denied,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestSourceSelectionInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListApprovedSourcesInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceCatalogSnapshotView {
    pub revision: u64,
    pub sources: Vec<SourceDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeSourceInput {
    pub request_id: String,
    pub source_id: SourceId,
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    pub request_id: String,
    pub applied: bool,
    pub new_revision: Option<u64>,
}

// -- pairing (5.2) -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BeginPairingInput {
    pub request_id: String,
}

/// QR rendering view: public/ephemeral data only — never private keys or raw
/// long-term tokens (docs/04 §5.2 rules).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingOfferView {
    pub pairing_version: u32,
    pub host_public_fingerprint: String,
    pub ephemeral_offer_id: String,
    pub expiry_unix: u64,
    pub address_hints: Vec<String>,
    pub human_verification_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelPairingInput {
    pub request_id: String,
    pub offer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovePairingInput {
    pub request_id: String,
    pub offer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceView {
    pub device_id: DeviceId,
    pub display_name: String,
    pub fingerprint_short: String,
    pub paired_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RejectPairingInput {
    pub request_id: String,
    pub offer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListPairedDevicesInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeDeviceInput {
    pub request_id: String,
    pub device_id: DeviceId,
}

// -- stream and diagnostics (5.3) -------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetHostSnapshotInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub host_id: HostId,
    pub platform: HostPlatform,
    pub pairing_state: PairingState,
    pub paired_devices: Vec<PairedDeviceView>,
    pub catalog: SourceCatalogSnapshotView,
    pub active_stream_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSourcePolicyInput {
    pub request_id: String,
    pub source_id: SourceId,
    pub expected_revision: Option<u64>,
    pub profile: QualityProfileKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourcePolicyView {
    pub source_id: SourceId,
    pub profile: QualityProfileKind,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopSourceInput {
    pub request_id: String,
    pub source_id: SourceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllStreamsInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportDiagnosticsInput {
    pub request_id: String,
    pub include_metrics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportDiagnosticsOutput {
    pub artifact_path: String,
    pub redacted: bool,
}

/// The canonical H02 proof command: invoked through the real Rustra package
/// invocation path, 20 + 22 must equal 42 (docs/08 H02 수용 기준).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddNumbersInput {
    pub a: i64,
    pub b: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddNumbersOutput {
    pub value: i64,
}

#[command]
fn add_numbers(input: AddNumbersInput) -> rustra::Result<AddNumbersOutput> {
    Ok(AddNumbersOutput {
        value: input.a + input.b,
    })
}

// -- v1 stream control (docs/plans/2026-08-18-rn-tauri-rebuild-design.md) -----
// Stateful commands dispatched by the Tauri control server (not the pure
// rustra Package) — types live here so the contract stays in one place.

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    /// Host operating system. Viewers use this for platform-specific guidance,
    /// never for choosing a transport implementation.
    pub platform: String,
    /// Capture paths the connected host can actually start. The first entry is
    /// the host default; keeping this in the catalog removes viewer-side OS
    /// guesses and lets newer hosts remain compatible with older viewers.
    pub capture_backends: Vec<CaptureBackendInfo>,
    /// Best-effort RFC1918 address for the low-latency media path. Control may
    /// arrive through a tailnet address while media can still travel directly
    /// over the physical LAN. Older hosts omit this field and viewers fall
    /// back to the control address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_host: Option<String>,
    pub displays: Vec<DisplayInfo>,
    #[serde(default)]
    pub encoder_experiments: Vec<EncoderExperimentInfo>,
    /// Host-supported UDP pacing/FEC choices. Older hosts omit this field, so
    /// viewers must preserve the legacy 8-datagram/2-parity behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability_capabilities: Option<UdpStabilityCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureBackendInfo {
    pub id: String,
    pub label: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsInfo {
    pub frames: i64,
    pub bytes: i64,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
    pub fps_target: u32,
    #[serde(default)]
    pub encoder_experiment_diagnostics_available: bool,
    #[serde(default)]
    pub encoder_experiment_requested: String,
    #[serde(default)]
    pub encoder_experiment_applied: String,
    #[serde(default)]
    pub encoder_experiment_fallback_reason: Option<String>,
    #[serde(default)]
    pub encoder_frame_drops: i64,
    #[serde(default)]
    pub encoder_frame_drop_fps: u32,
    #[serde(default)]
    pub valid_encode_output_fps: u32,
    #[serde(default)]
    pub encode_submit_call_p50_us: u64,
    #[serde(default)]
    pub encode_submit_call_p95_us: u64,
    #[serde(default)]
    pub encoder_callback_p50_us: u64,
    #[serde(default)]
    pub encoder_callback_p95_us: u64,
    #[serde(default)]
    pub packetization_in_flight: u32,
    #[serde(default)]
    pub base_frame_qp: Option<i32>,
    #[serde(default)]
    pub base_frame_qp_changes: i64,
    /// Stage-separated rates. `fps` remains the legacy encode-submit rate.
    #[serde(default)]
    pub capture_fps: u32,
    #[serde(default)]
    pub encode_submit_fps: u32,
    #[serde(default)]
    pub encode_output_fps: u32,
    #[serde(default)]
    pub rendered_fps: Option<u32>,
    #[serde(default)]
    pub capture_callbacks: i64,
    #[serde(default)]
    pub encode_output_callbacks: i64,
    #[serde(default)]
    pub encode_submit_failures: i64,
    #[serde(default)]
    pub encode_in_flight: u32,
    pub dropped: i64,
    pub network_dropped: i64,
    #[serde(default)]
    pub network_queue_dropped: i64,
    #[serde(default)]
    pub recovery_frames_dropped: i64,
    #[serde(default)]
    pub udp_send_failures: i64,
    #[serde(default)]
    pub udp_send_retries: i64,
    #[serde(default)]
    pub recovery_keyframes: i64,
    #[serde(default)]
    pub recovery_requests_suppressed: i64,
    pub capture_queue_dropped: i64,
    pub capture_to_encode_us: u64,
    pub max_capture_to_encode_us: u64,
    pub capture_queue_wait_us: u64,
    pub max_capture_queue_wait_us: u64,
    pub encode_output_us: u64,
    pub max_encode_output_us: u64,
    #[serde(default)]
    pub packetization_us: u64,
    #[serde(default)]
    pub max_packetization_us: u64,
    pub send_block_us: u64,
    pub max_send_block_us: u64,
    #[serde(default)]
    pub send_pace_us: u64,
    #[serde(default)]
    pub max_send_pace_us: u64,
    pub pending_frame: u32,
    /// Userspace network queue depth in encoded bytes and age of its oldest
    /// frame. A zero frame count does not prove the kernel socket is empty.
    #[serde(default)]
    pub pending_frame_bytes: u64,
    #[serde(default)]
    pub pending_frame_oldest_age_us: u64,
    pub capture_backend: String,
    pub media_transport: String,
    pub first_capture_ms: u64,
    pub first_encode_ms: u64,
    pub first_send_ms: u64,
    pub current_bitrate: u32,
    #[serde(default)]
    pub bitrate_floor_collapse_count: i64,
    #[serde(default)]
    pub bitrate_floor_collapse_last_reason: String,
    #[serde(default)]
    pub encoder_mode: String,
    #[serde(default, rename = "encoderID")]
    pub encoder_id: String,
    #[serde(default)]
    pub encoder_hardware_accelerated: Option<bool>,
    #[serde(default)]
    pub encoder_preset: String,
    #[serde(default)]
    pub encoder_profile: String,
    #[serde(default)]
    pub encoder_applied_properties: Vec<String>,
    #[serde(default)]
    pub encoder_unsupported_properties: Vec<String>,
    #[serde(default)]
    pub encoder_rejected_properties: Vec<String>,
    #[serde(default)]
    pub encoder_fallback_reason: Option<String>,
    /// Current VideoToolbox quality hint. `None` means the selected encoder
    /// does not expose the adaptive quality controller.
    #[serde(default)]
    pub quality_hint: Option<f32>,
    /// A non-None value means the operator has paused the automatic quality
    /// controller and selected a session-local quality cap from the Host UI.
    #[serde(default)]
    pub quality_override: Option<f32>,
    #[serde(default)]
    pub quality_adaptation_checks: i64,
    #[serde(default)]
    pub quality_adaptation_changes: i64,
    #[serde(default)]
    pub quality_adaptation_rejections: i64,
    #[serde(default)]
    pub quality_adaptation_last_status: String,
    pub capture_interval_p95_us: u64,
    pub capture_to_encode_p95_us: u64,
    pub capture_queue_wait_p95_us: u64,
    pub encode_output_p95_us: u64,
    #[serde(default)]
    pub packetization_p95_us: u64,
    #[serde(default)]
    pub encode_output_interval_p95_us: u64,
    pub send_block_p95_us: u64,
    #[serde(default)]
    pub send_pace_p95_us: u64,
    /// Latest encoded access-unit shape. These values make a motion-induced
    /// burst visible without requiring a packet capture.
    #[serde(default)]
    pub last_au_bytes: u64,
    #[serde(default)]
    pub last_au_fragments: u32,
    #[serde(default)]
    pub last_au_parity: u32,
    #[serde(default)]
    pub last_au_datagrams: u32,
    #[serde(default)]
    pub last_au_expected_datagrams: u32,
    #[serde(default)]
    pub last_au_send_us: u64,
    #[serde(default)]
    pub last_au_is_keyframe: bool,
    #[serde(default)]
    pub max_au_bytes: u64,
    #[serde(default)]
    pub max_au_fragments: u32,
    #[serde(default)]
    pub sent_datagrams: i64,
    #[serde(default)]
    pub sent_parity_datagrams: i64,
    #[serde(default)]
    pub error: Option<String>,
    /// Cumulative receiver-side loss and decode pressure reported by the
    /// authenticated feedback channel. These are optional in wire JSON so an
    /// older shim can still be read by a newer Host.
    #[serde(default)]
    pub receiver_frame_gaps: i64,
    #[serde(default)]
    pub receiver_input_drops: i64,
    #[serde(default)]
    pub receiver_incomplete_aus: i64,
    #[serde(default)]
    pub receiver_stale_frames: i64,
    #[serde(default)]
    pub receiver_stale_input_drops: Option<i64>,
    #[serde(default)]
    pub receiver_output_burst_discards: i64,
    #[serde(default)]
    pub receiver_rtt_ms: Option<u32>,
    #[serde(default)]
    pub receiver_wire_ms: Option<u32>,
    #[serde(default)]
    pub receiver_feedback_age_ms: Option<u64>,
    #[serde(default)]
    pub udp_stability_profile: String,
    #[serde(default)]
    pub udp_burst_datagrams: u32,
    #[serde(default)]
    pub udp_pacing_rate_multiplier: u32,
    #[serde(default)]
    pub udp_fec_parity_shards: u32,
    #[serde(default)]
    pub udp_adaptive_pacing: bool,
    #[serde(default)]
    pub udp_burst_reason: String,
    #[serde(default)]
    pub receiver_media_datagrams: u64,
    #[serde(default)]
    pub receiver_data_datagrams: u64,
    #[serde(default)]
    pub receiver_parity_datagrams: u64,
    #[serde(default)]
    pub receiver_fec_restored_fragments: u64,
    #[serde(default)]
    pub receiver_unrecoverable_fec_groups: u32,
    #[serde(default)]
    pub receiver_max_missing_data_fragments: u32,
    #[serde(default)]
    pub receiver_one_frame_gap_events: u32,
    #[serde(default)]
    pub receiver_multi_frame_gap_events: u32,
    #[serde(default)]
    pub receiver_paired_idr_episodes: u32,
    #[serde(default)]
    pub receiver_suppressed_recovery_requests: u32,
    #[serde(default)]
    pub receiver_fec_decode_failures: u32,
    #[serde(default)]
    pub split_direction: Option<String>,
    #[serde(default)]
    pub split_preparation_p50_us: u64,
    #[serde(default)]
    pub split_preparation_p95_us: u64,
    #[serde(default)]
    pub split_pair_admission_drops: i64,
    #[serde(default)]
    pub encoded_pair_callback_p50_us: u64,
    #[serde(default)]
    pub encoded_pair_callback_p95_us: u64,
    #[serde(default)]
    pub encoded_pair_timeouts: i64,
    #[serde(default)]
    pub encoded_pair_drops: i64,
    #[serde(default)]
    pub left_valid_encode_output_fps: u32,
    #[serde(default)]
    pub right_valid_encode_output_fps: u32,
    #[serde(default)]
    pub left_encoder_frame_drops: i64,
    #[serde(default)]
    pub right_encoder_frame_drops: i64,
    #[serde(default)]
    pub left_bitrate_bps: u64,
    #[serde(default)]
    pub right_bitrate_bps: u64,
    #[serde(default)]
    pub aggregate_bitrate_bps: u64,
    #[serde(default)]
    pub left_receiver_loss: u64,
    #[serde(default)]
    pub right_receiver_loss: u64,
    #[serde(default)]
    pub left_rendered_fps: u32,
    #[serde(default)]
    pub right_rendered_fps: u32,
    #[serde(default)]
    pub joined_rendered_fps: u32,
    #[serde(default)]
    pub pair_ready_delta_p95_us: u64,
    #[serde(default)]
    pub pair_ready_delta_max_us: u64,
    #[serde(default)]
    pub pair_sync_timeouts: i64,
    #[serde(default)]
    pub unmatched_output_drops: i64,
    #[serde(default)]
    pub paired_recovery_requests: i64,
    #[serde(default)]
    pub paired_recovery_keyframes: i64,
    #[serde(default)]
    pub split_test_injected_drops: i64,
    #[serde(default)]
    pub split_flow_active_leases: u32,
    #[serde(default)]
    pub split_flow_capacity: u32,
    #[serde(default)]
    pub split_pre_encode_admission_drops: i64,
    #[serde(default)]
    pub split_encoded_queue_depth: u32,
    #[serde(default)]
    pub split_encoded_queue_oldest_us: u64,
    #[serde(default)]
    pub split_recovery_boundary_discards: i64,
    #[serde(default)]
    pub split_post_encode_delta_drops: i64,
    #[serde(default)]
    pub split_wire_pairs_attempted: i64,
    #[serde(default)]
    pub split_wire_pair_send_failures: i64,
    #[serde(default)]
    pub split_keyframe_gap_recoveries: i64,
    #[serde(default)]
    pub split_delta_gap_recoveries: i64,
}

#[cfg(test)]
mod receiver_stats_contract_tests {
    use super::StatsInfo;

    #[test]
    fn stats_expose_receiver_pipeline_metrics() {
        let read = |stats: &StatsInfo| {
            (
                stats.receiver_frame_gaps,
                stats.receiver_input_drops,
                stats.receiver_incomplete_aus,
                stats.receiver_stale_frames,
                stats.receiver_stale_input_drops,
                stats.receiver_output_burst_discards,
                stats.receiver_rtt_ms,
                stats.receiver_wire_ms,
                stats.receiver_feedback_age_ms,
            )
        };
        let _ = read;
    }
}

#[cfg(test)]
mod split_flow_contract_tests {
    use super::{SessionView, StatsInfo};

    #[test]
    fn split_flow_fields_default_for_old_stats_payloads() {
        let stats: StatsInfo = serde_json::from_value(serde_json::json!({
            "frames": 0,
            "bytes": 0,
            "state": "running",
            "fps": 0,
            "kbps": 0,
            "fpsTarget": 60,
            "dropped": 0,
            "networkDropped": 0,
            "captureQueueDropped": 0,
            "captureToEncodeUs": 0,
            "maxCaptureToEncodeUs": 0,
            "captureQueueWaitUs": 0,
            "maxCaptureQueueWaitUs": 0,
            "encodeOutputUs": 0,
            "maxEncodeOutputUs": 0,
            "sendBlockUs": 0,
            "maxSendBlockUs": 0,
            "pendingFrame": 0,
            "captureBackend": "screenCaptureKit",
            "mediaTransport": "udp",
            "firstCaptureMs": 0,
            "firstEncodeMs": 0,
            "firstSendMs": 0,
            "currentBitrate": 0,
            "captureIntervalP95Us": 0,
            "captureToEncodeP95Us": 0,
            "captureQueueWaitP95Us": 0,
            "encodeOutputP95Us": 0,
            "sendBlockP95Us": 0
        }))
        .unwrap();

        assert_eq!(stats.split_flow_active_leases, 0);
        assert_eq!(stats.split_flow_capacity, 0);
        assert_eq!(stats.split_post_encode_delta_drops, 0);
        assert_eq!(stats.split_wire_pair_send_failures, 0);
        assert_eq!(stats.split_keyframe_gap_recoveries, 0);
        assert_eq!(stats.split_delta_gap_recoveries, 0);
    }

    #[test]
    fn split_flow_fields_survive_stats_and_status_contracts() {
        let mut payload = serde_json::to_value(StatsInfo::default()).unwrap();
        let payload = payload.as_object_mut().unwrap();
        payload.insert("splitFlowActiveLeases".into(), 3.into());
        payload.insert("splitFlowCapacity".into(), 5.into());
        payload.insert("splitPreEncodeAdmissionDrops".into(), 8.into());
        payload.insert("splitEncodedQueueDepth".into(), 1.into());
        payload.insert("splitEncodedQueueOldestUs".into(), 12_300.into());
        payload.insert("splitRecoveryBoundaryDiscards".into(), 4.into());
        payload.insert("splitPostEncodeDeltaDrops".into(), 0.into());
        payload.insert("splitWirePairsAttempted".into(), 600.into());
        payload.insert("splitWirePairSendFailures".into(), 2.into());
        payload.insert("splitKeyframeGapRecoveries".into(), 1.into());
        payload.insert("splitDeltaGapRecoveries".into(), 2.into());
        let stats: StatsInfo =
            serde_json::from_value(serde_json::Value::Object(payload.clone())).unwrap();
        assert_eq!(stats.split_flow_active_leases, 3);
        assert_eq!(stats.split_encoded_queue_oldest_us, 12_300);
        assert_eq!(stats.split_wire_pairs_attempted, 600);

        let status = SessionView {
            split_flow_active_leases: stats.split_flow_active_leases,
            split_flow_capacity: stats.split_flow_capacity,
            split_pre_encode_admission_drops: stats.split_pre_encode_admission_drops,
            split_encoded_queue_depth: stats.split_encoded_queue_depth,
            split_encoded_queue_oldest_us: stats.split_encoded_queue_oldest_us,
            split_recovery_boundary_discards: stats.split_recovery_boundary_discards,
            split_post_encode_delta_drops: stats.split_post_encode_delta_drops,
            split_wire_pairs_attempted: stats.split_wire_pairs_attempted,
            split_wire_pair_send_failures: stats.split_wire_pair_send_failures,
            split_keyframe_gap_recoveries: stats.split_keyframe_gap_recoveries,
            split_delta_gap_recoveries: stats.split_delta_gap_recoveries,
            ..SessionView::default()
        };
        assert_eq!(status.split_flow_capacity, 5);
        assert_eq!(status.split_recovery_boundary_discards, 4);
        assert_eq!(status.split_wire_pair_send_failures, 2);
        assert_eq!(status.split_delta_gap_recoveries, 2);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EncoderExperiment {
    #[default]
    Auto,
    RateControl,
    AdaptiveQp,
    EncoderPool,
    SplitHorizontal,
    SplitVertical,
}

impl EncoderExperiment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::RateControl => "rateControl",
            Self::AdaptiveQp => "adaptiveQp",
            Self::EncoderPool => "encoderPool",
            Self::SplitHorizontal => "splitHorizontal",
            Self::SplitVertical => "splitVertical",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncoderExperimentInfo {
    pub id: EncoderExperiment,
    pub label: String,
    pub hint: String,
    pub requires_reconnect: bool,
}

pub fn phase_a_encoder_experiments() -> Vec<EncoderExperimentInfo> {
    [
        (
            EncoderExperiment::Auto,
            "Automatic",
            "Host-selected encoder policy",
        ),
        (
            EncoderExperiment::RateControl,
            "Rate control",
            "Fixed rate-control encoder",
        ),
        (
            EncoderExperiment::AdaptiveQp,
            "Adaptive QP",
            "Adaptive base-frame quantizer",
        ),
        (
            EncoderExperiment::EncoderPool,
            "Encoder pool",
            "Pooled encoder sessions",
        ),
    ]
    .into_iter()
    .map(|(id, label, hint)| EncoderExperimentInfo {
        id,
        label: label.into(),
        hint: hint.into(),
        requires_reconnect: true,
    })
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamInput {
    pub source_index: u32,
    pub viewer_port: u16,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(default)]
    pub encoder_experiment: EncoderExperiment,
    /// Capture implementation selected for an A/B run. ScreenCaptureKit is
    /// the supported default; cgDisplayStream is an explicit display-only
    /// compatibility path on systems where the legacy symbols remain.
    #[serde(default = "default_capture_backend")]
    pub capture_backend: String,
    /// Concrete media transport for this stream. `tcp` is reliable Wi-Fi/LAN,
    /// `udp` is the low-latency Wi-Fi path, `usb` is AOAP, and `adbTcp` is the
    /// legacy ADB-over-USB path. `auto` tries USB, UDP, then TCP without
    /// duplicating one encoded frame over multiple links.
    #[serde(default = "default_media_transport")]
    pub media_transport: String,
    /// Content-aware encoder policy. `video` keeps 1080p where possible and
    /// gives high-change frames a larger bitrate budget; older viewers omit
    /// the field and retain the interactive policy.
    #[serde(default = "default_content_mode")]
    pub content_mode: String,
    /// Physical viewer interface candidates. The host only considers private
    /// addresses on the control peer's LAN and the production media backend
    /// proves UDP reachability with an unpredictable nonce before capture.
    #[serde(default)]
    pub viewer_ips: Vec<String>,
    /// Optional UDP pacing/FEC selection negotiated by newer peers. Omission
    /// is intentionally the legacy wire policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability: Option<UdpStabilityRequest>,
}

fn default_capture_backend() -> String {
    "screenCaptureKit".into()
}

fn default_media_transport() -> String {
    "udp".into()
}

fn default_content_mode() -> String {
    "interactive".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamOutput {
    pub session: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: u32,
    #[serde(default = "default_quality_state")]
    pub quality_state: String,
    /// Concrete UDP policy accepted by both peers. Non-UDP sessions and older
    /// hosts omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability: Option<AppliedUdpStability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReconfigureStreamInput {
    pub session: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(default = "default_quality_state")]
    pub quality_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReconfigureStreamOutput {
    pub session: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality_state: String,
}

fn default_quality_state() -> String {
    "native".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopStreamInput {
    pub session: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub sessions: Vec<SessionView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: u32,
    pub source_index: u32,
    pub source_name: String,
    pub viewer_addr: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
    pub fps_target: u32,
    #[serde(default = "default_quality_state")]
    pub quality_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp_stability: Option<AppliedUdpStability>,
    #[serde(default)]
    pub encoder_experiment_diagnostics_available: bool,
    #[serde(default)]
    pub encoder_experiment_requested: String,
    #[serde(default)]
    pub encoder_experiment_applied: String,
    #[serde(default)]
    pub encoder_experiment_fallback_reason: Option<String>,
    #[serde(default)]
    pub encoder_frame_drops: i64,
    #[serde(default)]
    pub encoder_frame_drop_fps: u32,
    #[serde(default)]
    pub valid_encode_output_fps: u32,
    #[serde(default)]
    pub encode_submit_call_p50_us: u64,
    #[serde(default)]
    pub encode_submit_call_p95_us: u64,
    #[serde(default)]
    pub encoder_callback_p50_us: u64,
    #[serde(default)]
    pub encoder_callback_p95_us: u64,
    #[serde(default)]
    pub packetization_in_flight: u32,
    #[serde(default)]
    pub base_frame_qp: Option<i32>,
    #[serde(default)]
    pub base_frame_qp_changes: i64,
    #[serde(default)]
    pub capture_fps: u32,
    #[serde(default)]
    pub encode_submit_fps: u32,
    #[serde(default)]
    pub encode_output_fps: u32,
    #[serde(default)]
    pub rendered_fps: Option<u32>,
    #[serde(default)]
    pub capture_callbacks: i64,
    #[serde(default)]
    pub encode_output_callbacks: i64,
    #[serde(default)]
    pub encode_submit_failures: i64,
    #[serde(default)]
    pub encode_in_flight: u32,
    /// Remote input is always host-approved per live stream and starts off.
    pub input_enabled: bool,
    /// Pointer sampling target. Discrete key/button events are immediate.
    pub input_rate_hz: u32,
    pub dropped: i64,
    pub network_dropped: i64,
    #[serde(default)]
    pub network_queue_dropped: i64,
    #[serde(default)]
    pub recovery_frames_dropped: i64,
    #[serde(default)]
    pub udp_send_failures: i64,
    #[serde(default)]
    pub udp_send_retries: i64,
    #[serde(default)]
    pub recovery_keyframes: i64,
    #[serde(default)]
    pub recovery_requests_suppressed: i64,
    pub capture_queue_dropped: i64,
    pub capture_to_encode_us: u64,
    pub max_capture_to_encode_us: u64,
    pub capture_queue_wait_us: u64,
    pub max_capture_queue_wait_us: u64,
    pub encode_output_us: u64,
    pub max_encode_output_us: u64,
    #[serde(default)]
    pub packetization_us: u64,
    #[serde(default)]
    pub max_packetization_us: u64,
    pub send_block_us: u64,
    pub max_send_block_us: u64,
    #[serde(default)]
    pub send_pace_us: u64,
    #[serde(default)]
    pub max_send_pace_us: u64,
    pub pending_frame: u32,
    #[serde(default)]
    pub pending_frame_bytes: u64,
    #[serde(default)]
    pub pending_frame_oldest_age_us: u64,
    pub frames: i64,
    pub bytes: i64,
    pub capture_backend: String,
    pub media_transport: String,
    pub first_capture_ms: u64,
    pub first_encode_ms: u64,
    pub first_send_ms: u64,
    pub current_bitrate: u32,
    #[serde(default)]
    pub bitrate_floor_collapse_count: i64,
    #[serde(default)]
    pub bitrate_floor_collapse_last_reason: String,
    #[serde(default)]
    pub encoder_mode: String,
    #[serde(default, rename = "encoderID")]
    pub encoder_id: String,
    #[serde(default)]
    pub encoder_hardware_accelerated: Option<bool>,
    #[serde(default)]
    pub encoder_preset: String,
    #[serde(default)]
    pub encoder_profile: String,
    #[serde(default)]
    pub encoder_applied_properties: Vec<String>,
    #[serde(default)]
    pub encoder_unsupported_properties: Vec<String>,
    #[serde(default)]
    pub encoder_rejected_properties: Vec<String>,
    #[serde(default)]
    pub encoder_fallback_reason: Option<String>,
    #[serde(default)]
    pub quality_hint: Option<f32>,
    #[serde(default)]
    pub quality_override: Option<f32>,
    #[serde(default)]
    pub quality_adaptation_checks: i64,
    #[serde(default)]
    pub quality_adaptation_changes: i64,
    #[serde(default)]
    pub quality_adaptation_rejections: i64,
    #[serde(default)]
    pub quality_adaptation_last_status: String,
    pub capture_interval_p95_us: u64,
    pub capture_to_encode_p95_us: u64,
    pub capture_queue_wait_p95_us: u64,
    pub encode_output_p95_us: u64,
    #[serde(default)]
    pub packetization_p95_us: u64,
    #[serde(default)]
    pub encode_output_interval_p95_us: u64,
    pub send_block_p95_us: u64,
    #[serde(default)]
    pub send_pace_p95_us: u64,
    #[serde(default)]
    pub last_au_bytes: u64,
    #[serde(default)]
    pub last_au_fragments: u32,
    #[serde(default)]
    pub last_au_parity: u32,
    #[serde(default)]
    pub last_au_datagrams: u32,
    #[serde(default)]
    pub last_au_expected_datagrams: u32,
    #[serde(default)]
    pub last_au_send_us: u64,
    #[serde(default)]
    pub last_au_is_keyframe: bool,
    #[serde(default)]
    pub max_au_bytes: u64,
    #[serde(default)]
    pub max_au_fragments: u32,
    #[serde(default)]
    pub sent_datagrams: i64,
    #[serde(default)]
    pub sent_parity_datagrams: i64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub receiver_frame_gaps: i64,
    #[serde(default)]
    pub receiver_input_drops: i64,
    #[serde(default)]
    pub receiver_incomplete_aus: i64,
    #[serde(default)]
    pub receiver_stale_frames: i64,
    #[serde(default)]
    pub receiver_stale_input_drops: Option<i64>,
    #[serde(default)]
    pub receiver_output_burst_discards: i64,
    #[serde(default)]
    pub receiver_rtt_ms: Option<u32>,
    #[serde(default)]
    pub receiver_wire_ms: Option<u32>,
    #[serde(default)]
    pub receiver_feedback_age_ms: Option<u64>,
    #[serde(default)]
    pub udp_stability_profile: String,
    #[serde(default)]
    pub udp_burst_datagrams: u32,
    #[serde(default)]
    pub udp_pacing_rate_multiplier: u32,
    #[serde(default)]
    pub udp_fec_parity_shards: u32,
    #[serde(default)]
    pub udp_adaptive_pacing: bool,
    #[serde(default)]
    pub udp_burst_reason: String,
    #[serde(default)]
    pub receiver_media_datagrams: u64,
    #[serde(default)]
    pub receiver_data_datagrams: u64,
    #[serde(default)]
    pub receiver_parity_datagrams: u64,
    #[serde(default)]
    pub receiver_fec_restored_fragments: u64,
    #[serde(default)]
    pub receiver_unrecoverable_fec_groups: u32,
    #[serde(default)]
    pub receiver_max_missing_data_fragments: u32,
    #[serde(default)]
    pub receiver_one_frame_gap_events: u32,
    #[serde(default)]
    pub receiver_multi_frame_gap_events: u32,
    #[serde(default)]
    pub receiver_paired_idr_episodes: u32,
    #[serde(default)]
    pub receiver_suppressed_recovery_requests: u32,
    #[serde(default)]
    pub receiver_fec_decode_failures: u32,
    #[serde(default)]
    pub split_direction: Option<String>,
    #[serde(default)]
    pub split_preparation_p50_us: u64,
    #[serde(default)]
    pub split_preparation_p95_us: u64,
    #[serde(default)]
    pub split_pair_admission_drops: i64,
    #[serde(default)]
    pub encoded_pair_callback_p50_us: u64,
    #[serde(default)]
    pub encoded_pair_callback_p95_us: u64,
    #[serde(default)]
    pub encoded_pair_timeouts: i64,
    #[serde(default)]
    pub encoded_pair_drops: i64,
    #[serde(default)]
    pub left_valid_encode_output_fps: u32,
    #[serde(default)]
    pub right_valid_encode_output_fps: u32,
    #[serde(default)]
    pub left_encoder_frame_drops: i64,
    #[serde(default)]
    pub right_encoder_frame_drops: i64,
    #[serde(default)]
    pub left_bitrate_bps: u64,
    #[serde(default)]
    pub right_bitrate_bps: u64,
    #[serde(default)]
    pub aggregate_bitrate_bps: u64,
    #[serde(default)]
    pub left_receiver_loss: u64,
    #[serde(default)]
    pub right_receiver_loss: u64,
    #[serde(default)]
    pub left_rendered_fps: u32,
    #[serde(default)]
    pub right_rendered_fps: u32,
    #[serde(default)]
    pub joined_rendered_fps: u32,
    #[serde(default)]
    pub pair_ready_delta_p95_us: u64,
    #[serde(default)]
    pub pair_ready_delta_max_us: u64,
    #[serde(default)]
    pub pair_sync_timeouts: i64,
    #[serde(default)]
    pub unmatched_output_drops: i64,
    #[serde(default)]
    pub paired_recovery_requests: i64,
    #[serde(default)]
    pub paired_recovery_keyframes: i64,
    #[serde(default)]
    pub split_test_injected_drops: i64,
    #[serde(default)]
    pub split_flow_active_leases: u32,
    #[serde(default)]
    pub split_flow_capacity: u32,
    #[serde(default)]
    pub split_pre_encode_admission_drops: i64,
    #[serde(default)]
    pub split_encoded_queue_depth: u32,
    #[serde(default)]
    pub split_encoded_queue_oldest_us: u64,
    #[serde(default)]
    pub split_recovery_boundary_discards: i64,
    #[serde(default)]
    pub split_post_encode_delta_drops: i64,
    #[serde(default)]
    pub split_wire_pairs_attempted: i64,
    #[serde(default)]
    pub split_wire_pair_send_failures: i64,
    #[serde(default)]
    pub split_keyframe_gap_recoveries: i64,
    #[serde(default)]
    pub split_delta_gap_recoveries: i64,
}

/// Events (docs/04 §7) — low-frequency only, never per-frame.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind", content = "payload")]
pub enum HostEvent {
    PermissionChanged { state: CapturePermissionState },
    SourceCatalogChanged { revision: u64 },
    PairingRequestCreated { offer_id: String },
    PairingStateChanged { state: PairingState },
    StreamSummaryChanged { active_streams: u32 },
}

/// Build the host control package.
pub fn host_package() -> Package {
    Package::builder("leftcar.host.control")
        .command_fn(add_numbers)
        .build()
}

#[cfg(test)]
mod stream_control_tests {
    use super::*;

    #[test]
    fn start_stream_input_roundtrips_camel_case() {
        let json = r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#;
        let v: StartStreamInput = serde_json::from_str(json).unwrap();
        assert_eq!(v.source_index, 0);
        assert_eq!(v.viewer_port, 5001);
        assert_eq!(v.fps, 90);
        assert_eq!(v.capture_backend, "screenCaptureKit");
        assert_eq!(v.content_mode, "interactive");
        let back = serde_json::to_string(&v).unwrap();
        assert!(back.contains("\"sourceIndex\""));
    }

    #[test]
    fn start_stream_defaults_to_auto_encoder_experiment() {
        let input: StartStreamInput = serde_json::from_str(
            r#"{"sourceIndex":0,"viewerPort":5001,"width":3840,"height":2160,"fps":60}"#,
        )
        .unwrap();
        assert_eq!(input.encoder_experiment, EncoderExperiment::Auto);
    }

    #[test]
    fn start_stream_roundtrips_adaptive_qp() {
        let input: StartStreamInput = serde_json::from_str(
            r#"{"sourceIndex":0,"viewerPort":5001,"width":3840,"height":2160,"fps":60,"encoderExperiment":"adaptiveQp"}"#,
        )
        .unwrap();
        assert_eq!(input.encoder_experiment, EncoderExperiment::AdaptiveQp);
        assert!(serde_json::to_string(&input)
            .unwrap()
            .contains("\"encoderExperiment\":\"adaptiveQp\""));
    }

    #[test]
    fn start_stream_input_roundtrips_video_content_mode() {
        let json = r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":30,"contentMode":"video"}"#;
        let v: StartStreamInput = serde_json::from_str(json).unwrap();
        assert_eq!(v.content_mode, "video");
        let back = serde_json::to_string(&v).unwrap();
        assert!(back.contains("\"contentMode\":\"video\""));
    }

    #[test]
    fn catalog_advertises_platform_capture_capabilities() {
        let catalog = CatalogView {
            platform: "windows".into(),
            capture_backends: vec![CaptureBackendInfo {
                id: "windowsGraphicsCapture".into(),
                label: "Windows Graphics Capture".into(),
                hint: "hardware H.264".into(),
            }],
            media_host: Some("192.168.0.134".into()),
            displays: Vec::new(),
            encoder_experiments: Vec::new(),
            udp_stability_capabilities: None,
        };
        let json = serde_json::to_string(&catalog).unwrap();
        assert!(json.contains("\"platform\":\"windows\""));
        assert!(json.contains("\"captureBackends\""));
        assert!(json.contains("\"mediaHost\":\"192.168.0.134\""));
        assert!(json.contains("\"windowsGraphicsCapture\""));
    }

    #[test]
    fn split_horizontal_deserializes_but_is_not_phase_a_capability() {
        let experiment: EncoderExperiment = serde_json::from_str("\"splitHorizontal\"").unwrap();
        assert_eq!(experiment, EncoderExperiment::SplitHorizontal);

        let catalog = CatalogView {
            platform: "macos".into(),
            capture_backends: Vec::new(),
            media_host: None,
            displays: Vec::new(),
            encoder_experiments: phase_a_encoder_experiments(),
            udp_stability_capabilities: None,
        };
        assert!(catalog
            .encoder_experiments
            .iter()
            .all(|info| info.id != EncoderExperiment::SplitHorizontal));
    }

    #[test]
    fn status_view_serializes() {
        let v = StatusView {
            sessions: vec![SessionView {
                session: 1,
                source_index: 0,
                source_name: "Main Display".into(),
                viewer_addr: "192.168.0.18:5001".into(),
                state: "running".into(),
                fps: 90,
                kbps: 12000,
                fps_target: 60,
                encoder_experiment_diagnostics_available: true,
                encoder_experiment_requested: "auto".into(),
                encoder_experiment_applied: "rateControl".into(),
                encoder_experiment_fallback_reason: None,
                encoder_frame_drops: 0,
                encoder_frame_drop_fps: 0,
                valid_encode_output_fps: 90,
                encode_submit_call_p50_us: 0,
                encode_submit_call_p95_us: 0,
                encoder_callback_p50_us: 0,
                encoder_callback_p95_us: 0,
                packetization_in_flight: 0,
                base_frame_qp: None,
                base_frame_qp_changes: 0,
                capture_fps: 90,
                encode_submit_fps: 90,
                encode_output_fps: 90,
                rendered_fps: Some(90),
                capture_callbacks: 100,
                encode_output_callbacks: 100,
                encode_submit_failures: 0,
                encode_in_flight: 0,
                input_enabled: false,
                input_rate_hz: 120,
                dropped: 0,
                network_dropped: 0,
                network_queue_dropped: 0,
                recovery_frames_dropped: 0,
                udp_send_failures: 0,
                udp_send_retries: 0,
                recovery_keyframes: 0,
                recovery_requests_suppressed: 0,
                capture_queue_dropped: 0,
                capture_to_encode_us: 0,
                max_capture_to_encode_us: 0,
                capture_queue_wait_us: 0,
                max_capture_queue_wait_us: 0,
                encode_output_us: 0,
                max_encode_output_us: 0,
                packetization_us: 0,
                max_packetization_us: 0,
                send_block_us: 0,
                max_send_block_us: 0,
                send_pace_us: 0,
                max_send_pace_us: 0,
                pending_frame: 0,
                pending_frame_bytes: 0,
                pending_frame_oldest_age_us: 0,
                frames: 100,
                bytes: 1_000_000,
                capture_backend: "screenCaptureKit".into(),
                media_transport: "udp".into(),
                first_capture_ms: 20,
                first_encode_ms: 25,
                first_send_ms: 26,
                current_bitrate: 12_000_000,
                encoder_mode: "ave".into(),
                encoder_id: "com.apple.videotoolbox.videoencoder.ave.avc".into(),
                encoder_hardware_accelerated: Some(true),
                encoder_preset: "high-speed".into(),
                encoder_profile: "main".into(),
                encoder_applied_properties: vec!["HighSpeed".into(), "Quality".into()],
                encoder_unsupported_properties: vec!["SuggestedLookAheadFrameCount".into()],
                encoder_rejected_properties: vec![],
                encoder_fallback_reason: None,
                quality_hint: None,
                quality_override: None,
                quality_adaptation_checks: 0,
                quality_adaptation_changes: 0,
                quality_adaptation_rejections: 0,
                quality_adaptation_last_status: "not_checked".into(),
                capture_interval_p95_us: 16_667,
                capture_to_encode_p95_us: 8_000,
                capture_queue_wait_p95_us: 1_000,
                encode_output_p95_us: 7_000,
                packetization_p95_us: 0,
                encode_output_interval_p95_us: 11_111,
                send_block_p95_us: 1_000,
                send_pace_p95_us: 0,
                last_au_bytes: 0,
                last_au_fragments: 0,
                last_au_parity: 0,
                last_au_datagrams: 0,
                last_au_expected_datagrams: 0,
                last_au_send_us: 0,
                last_au_is_keyframe: false,
                max_au_bytes: 0,
                max_au_fragments: 0,
                sent_datagrams: 0,
                sent_parity_datagrams: 0,
                error: None,
                receiver_frame_gaps: 0,
                receiver_input_drops: 0,
                receiver_incomplete_aus: 0,
                receiver_stale_frames: 0,
                receiver_stale_input_drops: None,
                receiver_output_burst_discards: 0,
                receiver_rtt_ms: None,
                receiver_wire_ms: None,
                receiver_feedback_age_ms: None,
                ..SessionView::default()
            }],
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"sourceName\""));
        assert!(s.contains("\"encoderExperimentRequested\":\"auto\""));
        assert!(s.contains("\"encoderExperimentApplied\":\"rateControl\""));
        assert!(s.contains("\"encoderExperimentDiagnosticsAvailable\":true"));
        assert!(s.contains("\"encoderMode\":\"ave\""));
        assert!(s.contains("\"encoderID\":\"com.apple.videotoolbox.videoencoder.ave.avc\""));
        assert!(s.contains("\"encoderHardwareAccelerated\":true"));
        assert!(s.contains("\"encoderAppliedProperties\":[\"HighSpeed\",\"Quality\"]"));
    }

    #[test]
    fn stats_info_serializes_keys() {
        let stats = StatsInfo {
            frames: 1,
            bytes: 2,
            state: "running".into(),
            fps: 90,
            kbps: 12000,
            fps_target: 60,
            encoder_experiment_diagnostics_available: true,
            encoder_experiment_requested: "auto".into(),
            encoder_experiment_applied: "rateControl".into(),
            encoder_experiment_fallback_reason: None,
            encoder_frame_drops: 0,
            encoder_frame_drop_fps: 0,
            valid_encode_output_fps: 90,
            encode_submit_call_p50_us: 0,
            encode_submit_call_p95_us: 0,
            encoder_callback_p50_us: 0,
            encoder_callback_p95_us: 0,
            packetization_in_flight: 0,
            base_frame_qp: None,
            base_frame_qp_changes: 0,
            capture_fps: 90,
            encode_submit_fps: 90,
            encode_output_fps: 90,
            rendered_fps: Some(90),
            capture_callbacks: 100,
            encode_output_callbacks: 100,
            encode_submit_failures: 0,
            encode_in_flight: 0,
            dropped: 0,
            network_dropped: 0,
            network_queue_dropped: 0,
            recovery_frames_dropped: 0,
            udp_send_failures: 0,
            udp_send_retries: 0,
            recovery_keyframes: 0,
            recovery_requests_suppressed: 0,
            capture_queue_dropped: 0,
            capture_to_encode_us: 0,
            max_capture_to_encode_us: 0,
            capture_queue_wait_us: 0,
            max_capture_queue_wait_us: 0,
            encode_output_us: 0,
            max_encode_output_us: 0,
            packetization_us: 0,
            max_packetization_us: 0,
            send_block_us: 0,
            max_send_block_us: 0,
            send_pace_us: 0,
            max_send_pace_us: 0,
            pending_frame: 0,
            pending_frame_bytes: 0,
            pending_frame_oldest_age_us: 0,
            capture_backend: "screenCaptureKit".into(),
            media_transport: "udp".into(),
            first_capture_ms: 20,
            first_encode_ms: 25,
            first_send_ms: 26,
            current_bitrate: 12_000_000,
            bitrate_floor_collapse_count: 7,
            bitrate_floor_collapse_last_reason: "resolution_fallback_floor_reached".into(),
            encoder_mode: "ave".into(),
            encoder_id: "com.apple.videotoolbox.videoencoder.ave.avc".into(),
            encoder_hardware_accelerated: Some(true),
            encoder_preset: "high-speed".into(),
            encoder_profile: "main".into(),
            encoder_applied_properties: vec!["HighSpeed".into(), "Quality".into()],
            encoder_unsupported_properties: vec!["SuggestedLookAheadFrameCount".into()],
            encoder_rejected_properties: vec![],
            encoder_fallback_reason: None,
            quality_hint: None,
            quality_override: None,
            quality_adaptation_checks: 0,
            quality_adaptation_changes: 0,
            quality_adaptation_rejections: 0,
            quality_adaptation_last_status: "not_checked".into(),
            capture_interval_p95_us: 16_667,
            capture_to_encode_p95_us: 8_000,
            capture_queue_wait_p95_us: 1_000,
            encode_output_p95_us: 7_000,
            packetization_p95_us: 0,
            encode_output_interval_p95_us: 11_111,
            send_block_p95_us: 1_000,
            send_pace_p95_us: 0,
            last_au_bytes: 0,
            last_au_fragments: 0,
            last_au_parity: 0,
            last_au_datagrams: 0,
            last_au_expected_datagrams: 0,
            last_au_send_us: 0,
            last_au_is_keyframe: false,
            max_au_bytes: 0,
            max_au_fragments: 0,
            sent_datagrams: 0,
            sent_parity_datagrams: 0,
            error: None,
            receiver_frame_gaps: 0,
            receiver_input_drops: 0,
            receiver_incomplete_aus: 0,
            receiver_stale_frames: 0,
            receiver_stale_input_drops: None,
            receiver_output_burst_discards: 0,
            receiver_rtt_ms: None,
            receiver_wire_ms: None,
            receiver_feedback_age_ms: None,
            ..StatsInfo::default()
        };
        let mut old_payload = serde_json::to_value(&stats).unwrap();
        old_payload
            .as_object_mut()
            .unwrap()
            .remove("encoderExperimentDiagnosticsAvailable");
        let old_stats: StatsInfo = serde_json::from_value(old_payload).unwrap();
        assert!(!old_stats.encoder_experiment_diagnostics_available);

        let s = serde_json::to_string(&stats).unwrap();
        assert!(s.contains("\"frames\"") && s.contains("\"kbps\""));
        assert!(s.contains("\"recoveryFramesDropped\""));
        assert!(s.contains("\"lastAuFragments\""));
        assert!(s.contains("\"sentDatagrams\""));
        assert!(s.contains("\"encoderMode\":\"ave\""));
        assert!(s.contains("\"encoderID\":\"com.apple.videotoolbox.videoencoder.ave.avc\""));
        assert!(s.contains("\"encoderHardwareAccelerated\":true"));
        assert!(s.contains("\"encoderAppliedProperties\":[\"HighSpeed\",\"Quality\"]"));
        assert!(s.contains("\"encoderExperimentDiagnosticsAvailable\":true"));
        assert!(s.contains("\"bitrateFloorCollapseCount\":7"));
        assert!(
            s.contains("\"bitrateFloorCollapseLastReason\":\"resolution_fallback_floor_reached\"")
        );
    }
}
