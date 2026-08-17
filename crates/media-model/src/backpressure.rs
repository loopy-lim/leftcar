//! Bounded stage queues implementing "latest screen wins" (docs/03 §6.3).

use crate::frame::{EncodedFrame, FrameKind};
use std::collections::VecDeque;

/// capture -> encoder boundary: hold only the newest un-encoded frame.
#[derive(Debug, Default)]
pub struct LatestFrameSlot {
    pending: Option<EncodedFrame>,
}

impl LatestFrameSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer a new frame; a not-yet-consumed older frame is returned to the
    /// caller (deltas are simply dropped upstream; keys are handed back so the
    /// caller can keep them for recovery, docs/03 §6.3).
    pub fn offer(&mut self, frame: EncodedFrame) -> Option<EncodedFrame> {
        self.pending.replace(frame)
    }

    pub fn take(&mut self) -> Option<EncodedFrame> {
        self.pending.take()
    }

    pub fn len(&self) -> usize {
        usize::from(self.pending.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_none()
    }
}

/// encoder -> packetizer boundary: at most 2 access units; when full, evict the
/// oldest delta and keep keys.
#[derive(Debug)]
pub struct BoundedAuQueue {
    items: VecDeque<EncodedFrame>,
    cap: usize,
}

impl BoundedAuQueue {
    pub fn new(cap: usize) -> Self {
        Self { items: VecDeque::new(), cap: cap.max(1) }
    }

    /// Push; returns evicted frames (oldest deltas first) when over cap.
    pub fn push(&mut self, frame: EncodedFrame) -> Vec<EncodedFrame> {
        let mut evicted = Vec::new();
        self.items.push_back(frame);
        while self.items.len() > self.cap {
            // evict the oldest delta; keys are preserved for recovery
            if let Some(pos) = self.items.iter().position(|f| matches!(f.kind, FrameKind::Delta)) {
                let removed = self.items.remove(pos).expect("position exists");
                evicted.push(removed);
            } else {
                let removed = self.items.pop_front().expect("over cap implies non-empty");
                evicted.push(removed);
            }
        }
        evicted
    }

    pub fn pop(&mut self) -> Option<EncodedFrame> {
        self.items.pop_front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{CodecProfile, StreamEpoch};
    use bytes::Bytes;
    use domain::ids::{SessionId, SourceId};

    fn frame(id: u64, kind: FrameKind) -> EncodedFrame {
        EncodedFrame {
            session_id: SessionId::from_raw("s").unwrap(),
            source_id: SourceId::from_raw("src").unwrap(),
            stream_epoch: StreamEpoch(1),
            frame_id: id,
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
    fn latest_frame_slot_drops_stale() {
        let mut slot = LatestFrameSlot::new();
        let dropped = slot.offer(frame(1, FrameKind::Delta));
        assert!(dropped.is_none());
        let dropped = slot.offer(frame(2, FrameKind::Delta));
        assert!(matches!(dropped, Some(f) if f.kind == FrameKind::Delta), "older unconsumed delta is dropped");
        assert_eq!(slot.take().unwrap().frame_id, 2, "newest survives");
    }

    #[test]
    fn latest_frame_slot_keeps_key_when_displaced() {
        let mut slot = LatestFrameSlot::new();
        slot.offer(frame(1, FrameKind::Key));
        // displacing a key returns it (caller may need it for recovery)
        let dropped = slot.offer(frame(2, FrameKind::Delta));
        assert!(matches!(dropped, Some(f) if f.kind == FrameKind::Key));
    }

    #[test]
    fn au_queue_drops_oldest_delta_keeps_key() {
        let mut q = BoundedAuQueue::new(2);
        q.push(frame(1, FrameKind::Key));
        q.push(frame(2, FrameKind::Delta));
        let evicted = q.push(frame(3, FrameKind::Delta));
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].frame_id, 2, "oldest delta evicted");
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop().unwrap().frame_id, 1, "key preserved");
    }

    #[test]
    fn au_queue_never_exceeds_cap() {
        let mut q = BoundedAuQueue::new(2);
        for id in 1..10 {
            q.push(frame(id, FrameKind::Delta));
            assert!(q.len() <= 2);
        }
    }
}
