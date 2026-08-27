//! Host control package (docs/04 §5).

use crate::common::*;
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsInfo {
    pub frames: i64,
    pub bytes: i64,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
    pub fps_target: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamInput {
    pub source_index: u32,
    pub viewer_port: u16,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: u32,
    pub source_index: u32,
    pub source_name: String,
    pub viewer_addr: String,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
    pub fps_target: u32,
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
        };
        let json = serde_json::to_string(&catalog).unwrap();
        assert!(json.contains("\"platform\":\"windows\""));
        assert!(json.contains("\"captureBackends\""));
        assert!(json.contains("\"mediaHost\":\"192.168.0.134\""));
        assert!(json.contains("\"windowsGraphicsCapture\""));
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
            }],
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"sourceName\""));
        assert!(s.contains("\"encoderMode\":\"ave\""));
        assert!(s.contains("\"encoderID\":\"com.apple.videotoolbox.videoencoder.ave.avc\""));
        assert!(s.contains("\"encoderHardwareAccelerated\":true"));
        assert!(s.contains("\"encoderAppliedProperties\":[\"HighSpeed\",\"Quality\"]"));
    }

    #[test]
    fn stats_info_serializes_keys() {
        let s = serde_json::to_string(&StatsInfo {
            frames: 1,
            bytes: 2,
            state: "running".into(),
            fps: 90,
            kbps: 12000,
            fps_target: 60,
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
        })
        .unwrap();
        assert!(s.contains("\"frames\"") && s.contains("\"kbps\""));
        assert!(s.contains("\"recoveryFramesDropped\""));
        assert!(s.contains("\"lastAuFragments\""));
        assert!(s.contains("\"sentDatagrams\""));
        assert!(s.contains("\"encoderMode\":\"ave\""));
        assert!(s.contains("\"encoderID\":\"com.apple.videotoolbox.videoencoder.ave.avc\""));
        assert!(s.contains("\"encoderHardwareAccelerated\":true"));
        assert!(s.contains("\"encoderAppliedProperties\":[\"HighSpeed\",\"Quality\"]"));
    }
}
