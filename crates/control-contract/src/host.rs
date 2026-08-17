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
