//! Media model: encoded frames, fragmentation, assembly, backpressure.
//! Pure Rust; no platform or transport dependencies (ADR-0002).

pub mod assemble;
pub mod backpressure;
pub mod fragment;
pub mod frame;

pub use assemble::{
    AssembleError, AssembledOutput, FragmentAssembler, MAX_ASSEMBLY_BYTES_PER_SOURCE,
    MAX_INCOMPLETE_PER_SOURCE,
};
pub use fragment::{packetize, Fragment, FragmentHeader, DEFAULT_MTU, MAX_FRAME_BYTES};
pub use frame::{CodecProfile, EncodedFrame, FrameKind, StreamEpoch};
