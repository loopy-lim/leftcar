//! Fragment packetization (docs/03 §6.2).
//!
//! Fragments identify source and epoch, carry frame length and fragment
//! index/count, and respect an MTU. Resource caps follow docs/07 §13.

use crate::frame::{EncodedFrame, FrameKind, StreamEpoch};
use bytes::Bytes;
use domain::ids::SourceId;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MTU: usize = 1200;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024; // 16 MiB (docs/07 §13)

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentHeader {
    pub source_id: SourceId,
    pub stream_epoch: StreamEpoch,
    pub frame_id: u64,
    pub kind: FrameKind,
    pub frame_len: u32,
    pub frag_index: u16,
    pub frag_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub header: FragmentHeader,
    pub payload: Bytes,
}

#[derive(Debug, thiserror::Error)]
pub enum PacketizeError {
    #[error("frame payload exceeds cap: {len} > {MAX_FRAME_BYTES}")]
    FrameTooLarge { len: usize },
}

pub fn packetize(frame: &EncodedFrame, mtu: usize) -> Result<Vec<Fragment>, PacketizeError> {
    let mtu = mtu.max(64);
    if frame.payload.len() > MAX_FRAME_BYTES {
        return Err(PacketizeError::FrameTooLarge { len: frame.payload.len() });
    }
    let payload_len = frame.payload.len();
    let frag_count = if payload_len == 0 {
        1
    } else {
        payload_len.div_ceil(mtu)
    };
    let frag_count = u16::try_from(frag_count).map_err(|_| PacketizeError::FrameTooLarge { len: payload_len })?;
    let mut out = Vec::with_capacity(frag_count as usize);
    for i in 0..frag_count {
        let start = i as usize * mtu;
        let end = ((i as usize) + 1) * mtu;
        out.push(Fragment {
            header: FragmentHeader {
                source_id: frame.source_id.clone(),
                stream_epoch: frame.stream_epoch,
                frame_id: frame.frame_id,
                kind: frame.kind,
                frame_len: payload_len as u32,
                frag_index: i,
                frag_count,
            },
            payload: frame.payload.slice(start..end.min(payload_len)),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::CodecProfile;
    use domain::ids::SessionId;

    fn frame_with_payload(len: usize, kind: FrameKind, frame_id: u64) -> EncodedFrame {
        EncodedFrame {
            session_id: SessionId::from_raw("s").unwrap(),
            source_id: SourceId::from_raw("src").unwrap(),
            stream_epoch: StreamEpoch(1),
            frame_id,
            kind,
            codec: CodecProfile::AvcBaseline,
            capture_time_host_ns: 0,
            encode_done_host_ns: 0,
            width: 1920,
            height: 1080,
            payload: Bytes::from(vec![0xAB; len]),
        }
    }

    #[test]
    fn roundtrip_fragment_count_matches_mtu() {
        let frags = packetize(&frame_with_payload(2500, FrameKind::Key, 1), 1200).unwrap();
        assert_eq!(frags.len(), 3);
        assert_eq!(frags[0].header.frag_count, 3);
        assert_eq!(frags[0].payload.len(), 1200);
        assert_eq!(frags[2].payload.len(), 100);
        // every fragment identifies source and epoch (docs/03 6.2)
        for f in &frags {
            assert_eq!(f.header.source_id, SourceId::from_raw("src").unwrap());
            assert_eq!(f.header.stream_epoch, StreamEpoch(1));
            assert_eq!(f.header.frame_len, 2500);
        }
    }

    #[test]
    fn empty_payload_is_single_fragment() {
        let frags = packetize(&frame_with_payload(0, FrameKind::Config, 1), 1200).unwrap();
        assert_eq!(frags.len(), 1);
        assert_eq!(frags[0].header.frag_index, 0);
    }

    #[test]
    fn oversized_frame_rejected_before_framing() {
        let err = packetize(&frame_with_payload(MAX_FRAME_BYTES + 1, FrameKind::Key, 1), 1200);
        assert!(matches!(err, Err(PacketizeError::FrameTooLarge { .. })));
    }
}
