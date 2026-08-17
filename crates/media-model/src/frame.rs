//! Encoded frame model (docs/03 §6.1).

use bytes::Bytes;
use domain::ids::{SessionId, SourceId};
use serde::{Deserialize, Serialize};

/// Increments on encoder restart, source resize, codec reconfiguration.
/// Viewers drop packets from earlier epochs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamEpoch(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    Key,
    Delta,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodecProfile {
    AvcBaseline,
    AvcMain,
    AvcHigh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodedFrame {
    pub session_id: SessionId,
    pub source_id: SourceId,
    pub stream_epoch: StreamEpoch,
    pub frame_id: u64,
    pub kind: FrameKind,
    pub codec: CodecProfile,
    pub capture_time_host_ns: u64,
    pub encode_done_host_ns: u64,
    pub width: u32,
    pub height: u32,
    #[serde(with = "bytes_serde")]
    pub payload: Bytes,
}

mod bytes_serde {
    use bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(value: &Bytes, s: S) -> Result<S::Ok, S::Error> {
        value.as_ref().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Bytes, D::Error> {
        let v: Vec<u8> = Vec::deserialize(d)?;
        Ok(Bytes::from(v))
    }
}

impl EncodedFrame {
    pub fn is_decodable_after(&self, prior: Option<&EncodedFrame>) -> bool {
        match (prior, self.kind) {
            // keyframes are self-contained
            (_, FrameKind::Key) => true,
            // a delta needs a prior decodable frame in the same epoch
            (Some(p), FrameKind::Delta) => p.stream_epoch == self.stream_epoch,
            (None, FrameKind::Delta) => false,
            // config alone is not decodable
            (_, FrameKind::Config) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(epoch: u32, kind: FrameKind) -> EncodedFrame {
        EncodedFrame {
            session_id: SessionId::from_raw("s").unwrap(),
            source_id: SourceId::from_raw("src").unwrap(),
            stream_epoch: StreamEpoch(epoch),
            frame_id: 1,
            kind,
            codec: CodecProfile::AvcBaseline,
            capture_time_host_ns: 0,
            encode_done_host_ns: 0,
            width: 1920,
            height: 1080,
            payload: Bytes::new(),
        }
    }

    #[test]
    fn delta_without_prior_is_not_decodable() {
        assert!(!frame(1, FrameKind::Delta).is_decodable_after(None));
    }

    #[test]
    fn key_is_always_decodable() {
        assert!(frame(1, FrameKind::Key).is_decodable_after(None));
    }

    #[test]
    fn delta_across_epoch_is_not_decodable() {
        let key = frame(1, FrameKind::Key);
        let mut delta = frame(2, FrameKind::Delta);
        delta.frame_id = 2;
        assert!(!delta.is_decodable_after(Some(&key)));
    }
}
