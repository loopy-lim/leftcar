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
    pub displays: Vec<DisplayInfo>,
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
    pub dropped: i64,
    pub network_dropped: i64,
    pub capture_queue_dropped: i64,
    pub capture_to_encode_us: u64,
    pub max_capture_to_encode_us: u64,
    pub capture_queue_wait_us: u64,
    pub max_capture_queue_wait_us: u64,
    pub encode_output_us: u64,
    pub max_encode_output_us: u64,
    pub send_block_us: u64,
    pub max_send_block_us: u64,
    pub pending_frame: u32,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamInput {
    pub source_index: u32,
    pub viewer_port: u16,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Legacy redirection hint from early viewers. Kept for wire
    /// compatibility (the server still deserializes it) but ignored: the
    /// viewer's claimed IPs are untrusted on an authenticated-but-unverified
    /// channel, so the server pushes only to the control connection peer.
    #[serde(default)]
    #[deprecated(note = "server pushes only to the control connection peer; ignored")]
    pub viewer_ips: Vec<String>,
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
    pub dropped: i64,
    pub network_dropped: i64,
    pub capture_queue_dropped: i64,
    pub capture_to_encode_us: u64,
    pub max_capture_to_encode_us: u64,
    pub capture_queue_wait_us: u64,
    pub max_capture_queue_wait_us: u64,
    pub encode_output_us: u64,
    pub max_encode_output_us: u64,
    pub send_block_us: u64,
    pub max_send_block_us: u64,
    pub pending_frame: u32,
    #[serde(default)]
    pub error: Option<String>,
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
        let back = serde_json::to_string(&v).unwrap();
        assert!(back.contains("\"sourceIndex\""));
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
                dropped: 0,
                network_dropped: 0,
                capture_queue_dropped: 0,
                capture_to_encode_us: 0,
                max_capture_to_encode_us: 0,
                capture_queue_wait_us: 0,
                max_capture_queue_wait_us: 0,
                encode_output_us: 0,
                max_encode_output_us: 0,
                send_block_us: 0,
                max_send_block_us: 0,
                pending_frame: 0,
                error: None,
            }],
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"sourceName\""));
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
            dropped: 0,
            network_dropped: 0,
            capture_queue_dropped: 0,
            capture_to_encode_us: 0,
            max_capture_to_encode_us: 0,
            capture_queue_wait_us: 0,
            max_capture_queue_wait_us: 0,
            encode_output_us: 0,
            max_encode_output_us: 0,
            send_block_us: 0,
            max_send_block_us: 0,
            pending_frame: 0,
            error: None,
        })
        .unwrap();
        assert!(s.contains("\"frames\"") && s.contains("\"kbps\""));
    }
}
