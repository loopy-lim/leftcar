//! Shared contract types (docs/04 §3–4).
//!
//! All IDs are opaque; titles/bundle IDs/native handles are never encoded.

use rustra::prelude::*;

#[bridge_type]
#[derive(Clone)]
pub struct HostId(pub String);

#[bridge_type]
#[derive(Clone)]
pub struct DeviceId(pub String);

#[bridge_type]
#[derive(Clone)]
pub struct SourceId(pub String);

#[bridge_type]
#[derive(Clone)]
pub struct StreamInstanceId(pub String);

#[bridge_type]
#[derive(Clone)]
pub struct SessionId(pub String);

#[bridge_type]
#[derive(Clone)]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

#[bridge_type]
#[derive(Clone)]
pub enum SourceKind {
    Display,
    Window,
}

#[bridge_type]
#[derive(Clone)]
pub struct SourceDescriptor {
    pub id: SourceId,
    pub kind: SourceKind,
    pub display_name: String,
    pub application_name: Option<String>,
    pub width_px: u32,
    pub height_px: u32,
    pub is_approved: bool,
    pub is_available: bool,
    pub revision: u64,
}

#[bridge_type]
#[derive(Clone)]
pub enum PairingState {
    Unpaired,
    Advertising,
    AwaitingHostApproval,
    PairedOffline,
    Connecting,
    Connected,
    Revoked,
}

#[bridge_type]
#[derive(Clone)]
pub enum StreamPhase {
    Idle,
    Negotiating,
    WaitingKeyframe,
    Playing,
    Degraded,
    Reconnecting,
    Suspended,
    SourceUnavailable,
    PermissionRevoked,
    DecoderFailed,
    Stopped,
}

#[bridge_type]
#[derive(Clone)]
pub enum QualityProfileKind {
    Focus,
    Normal,
    BackgroundVisible,
    Suspended,
    Custom,
}

#[bridge_type]
#[derive(Clone)]
pub struct CustomQualityProfile {
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u16,
    pub max_bitrate_bps: u64,
}
