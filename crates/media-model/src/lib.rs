//! Media model: encoded frames, fragmentation, assembly, backpressure.
//! Pure Rust; no platform or transport dependencies (ADR-0002).

pub mod assemble;
pub mod backpressure;
pub mod fragment;
pub mod frame;

pub use assemble::{
    AssembledOutput, AssembleError, BoundedQueue, FragmentAssembler,
    MAX_ASSEMBLY_BYTES_PER_SOURCE, MAX_INCOMPLETE_PER_SOURCE,
};
pub use backpressure::{BoundedAuQueue, LatestFrameSlot};
pub use fragment::{packetize, Fragment, FragmentHeader, DEFAULT_MTU, MAX_FRAME_BYTES};
pub use frame::{CodecProfile, EncodedFrame, FrameKind, StreamEpoch};

/// Capture/encode port traits (docs/03 §7.1). Implementations live in
/// platform crates; fakes live in host-core.
pub mod ports {
    use crate::frame::{CodecProfile, EncodedFrame};
    use bytes::Bytes;

    #[derive(Debug, Clone)]
    pub struct CaptureConfig {
        pub max_width: u32,
        pub max_height: u32,
        pub max_fps: u16,
        pub include_cursor: bool,
    }

    #[derive(Debug, Clone)]
    pub struct ApprovedSource {
        pub source_id: domain::SourceId,
        pub kind: domain::SourceKind,
    }

    #[async_trait::async_trait]
    pub trait FrameSink: Send + Sync {
        /// Receive one captured frame (native handle abstracted to bytes here).
        async fn on_frame(&self, frame: RawFrame);
    }

    #[derive(Debug, Clone)]
    pub struct RawFrame {
        pub width: u32,
        pub height: u32,
        pub timestamp_ns: u64,
        pub data: Bytes,
    }

    #[async_trait::async_trait]
    pub trait EncodeSink: Send + Sync {
        async fn on_access_unit(&self, frame: EncodedFrame);
    }

    /// Encoder configuration knobs validated against hardware capabilities.
    #[derive(Debug, Clone)]
    pub struct EncoderConfig {
        pub codec: CodecProfile,
        pub width: u32,
        pub height: u32,
        pub fps: u16,
        pub bitrate_bps: u64,
        pub allow_b_frames: bool,
    }
}
