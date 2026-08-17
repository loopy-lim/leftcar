//! Viewer control package (docs/04 §6).

use crate::common::*;
use rustra::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePairingOfferInput {
    pub request_id: String,
    pub offer_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequestView {
    pub host_fingerprint: String,
    pub human_verification_code: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListHostsInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    pub host_id: HostId,
    pub display_name: String,
    pub state: PairingState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectHostInput {
    pub request_id: String,
    pub host_id: HostId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub host_id: HostId,
    pub catalog_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectHostInput {
    pub request_id: String,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRemoteSourcesInput {
    pub request_id: String,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateStreamLaunchInput {
    pub request_id: String,
    pub session_id: SessionId,
    pub source_id: SourceId,
    pub expected_catalog_revision: u64,
}

/// Opaque, short-lived launch handle (docs/04 §6.2): no secrets in Intent data.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamLaunchView {
    pub launch_handle: String,
    pub source_id: SourceId,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListStreamWindowsInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamWindowView {
    pub source_id: SourceId,
    pub instance_id: StreamInstanceId,
    pub phase: StreamPhase,
    pub profile: QualityProfileKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestCloseStreamWindowInput {
    pub request_id: String,
    pub instance_id: StreamInstanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetStreamProfileInput {
    pub request_id: String,
    pub instance_id: StreamInstanceId,
    pub profile: QualityProfileKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamSnapshot {
    pub instance_id: StreamInstanceId,
    pub source_id: SourceId,
    pub phase: StreamPhase,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetViewerCapabilitiesInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewerCapabilities {
    pub max_concurrent_decoders_hint: u32,
    pub low_latency_supported: bool,
    pub codec_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetStreamSnapshotInput {
    pub request_id: String,
    pub instance_id: StreamInstanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetBenchmarkReadinessInput {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReadiness {
    pub codec_probe_available: bool,
    pub clock_sync_available: bool,
    pub overlay_available: bool,
}

/// H02/H09 proof command on the viewer path too.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewerAddNumbersInput {
    pub a: i64,
    pub b: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewerAddNumbersOutput {
    pub value: i64,
}

#[command]
fn viewer_add_numbers(input: ViewerAddNumbersInput) -> rustra::Result<ViewerAddNumbersOutput> {
    Ok(ViewerAddNumbersOutput { value: input.a + input.b })
}

/// Events (docs/04 §7).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind", content = "payload")]
pub enum ViewerEvent {
    HostStateChanged { state: PairingState },
    SourceCatalogChanged { host_id: HostId, revision: u64 },
    StreamWindowChanged { instance_id: StreamInstanceId, phase: StreamPhase },
    SessionAlert { error_code: String, retryable: bool },
}

pub fn viewer_package() -> Package {
    Package::builder("leftcar.viewer.control")
        .command_fn(viewer_add_numbers)
        .build()
}
