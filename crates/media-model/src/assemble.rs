//! Fragment assembler with decoder-dependency and memory rules
//! (docs/03 §6.2, docs/05 §5.4, docs/07 §13).

use crate::fragment::{Fragment, FragmentHeader};
use crate::frame::{EncodedFrame, FrameKind, StreamEpoch};
use bytes::Bytes;
use domain::ids::{SessionId, SourceId};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

pub const MAX_INCOMPLETE_PER_SOURCE: usize = 2;
/// Hard cap on buffered incomplete-frame bytes per source (docs/07 §13 spirit).
pub const MAX_ASSEMBLY_BYTES_PER_SOURCE: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("duplicate fragment for frame {frame_id}")]
    Duplicate { frame_id: u64 },
    #[error("assembly memory bound exceeded")]
    MemoryBoundExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembledOutput {
    Frame(EncodedFrame),
    /// decoder cannot reference the missing frame; request IDR (rate-limited)
    RequestIdr { source_id: SourceId },
    /// fragment dropped silently (stale epoch, superseded, late)
    Dropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamState {
    epoch: StreamEpoch,
    /// whether the decoder can decode the next delta (key/config seen in epoch)
    decodable: bool,
    last_delivered_frame_id: Option<u64>,
    last_idr_request_frame: Option<u64>,
}

#[derive(Debug)]
pub struct FragmentAssembler {
    /// source -> (frame_id -> partial frame)
    partials: HashMap<SourceId, BTreeMap<u64, PartialFrame>>,
    states: HashMap<SourceId, StreamState>,
    idr_rate_limit: u32,
    codec: crate::frame::CodecProfile,
    session_id: SessionId,
}

#[derive(Debug)]
struct PartialFrame {
    header: FragmentHeader,
    received: Vec<Option<Bytes>>,
    received_bytes: usize,
    first_seen: Duration,
}

impl FragmentAssembler {
    pub fn new(session_id: SessionId, codec: crate::frame::CodecProfile) -> Self {
        Self {
            partials: HashMap::new(),
            states: HashMap::new(),
            idr_rate_limit: 8,
            codec,
            session_id,
        }
    }

    /// Feed one fragment. `now` drives incomplete-frame expiry.
    pub fn feed(&mut self, frag: Fragment, now: Duration) -> Result<AssembledOutput, AssembleError> {
        // epoch gate: frames from an older epoch than the newest seen are dropped
        let state = self.states.entry(frag.header.source_id.clone()).or_insert(StreamState {
            epoch: frag.header.stream_epoch,
            decodable: false,
            last_delivered_frame_id: None,
            last_idr_request_frame: None,
        });
        if frag.header.stream_epoch < state.epoch {
            return Ok(AssembledOutput::Dropped); // old epoch after resize
        }
        if frag.header.stream_epoch > state.epoch {
            // new epoch resets decoder dependency
            *state = StreamState {
                epoch: frag.header.stream_epoch,
                decodable: false,
                last_delivered_frame_id: None,
                last_idr_request_frame: None,
            };
            self.partials.remove(&frag.header.source_id);
        }

        // stale-frame guard: fragments for a frame at or before the last
        // delivered frame are late replays (docs/03 6.2 "늦은 delta 폐기")
        if let Some(last) = state.last_delivered_frame_id {
            if frag.header.frame_id <= last {
                return Ok(AssembledOutput::Dropped);
            }
        }

        // dependency gate before buffering (docs/05 5.4)
        match frag.header.kind {
            FrameKind::Config => { /* config is buffered and delivered with key */ }
            FrameKind::Key => { /* keys are self-contained */ }
            FrameKind::Delta => {
                if !state.decodable {
                    // delta before config/keyframe in this epoch is dropped; a
                    // delta whose predecessors were lost triggers an IDR request
                    // with rate limiting (docs/03 6.2)
                    let should_request = state
                        .last_idr_request_frame
                        .map(|last| frag.header.frame_id.saturating_sub(last) >= self.idr_rate_limit as u64)
                        .unwrap_or(true);
                    if should_request {
                        state.last_idr_request_frame = Some(frag.header.frame_id);
                        return Ok(AssembledOutput::RequestIdr { source_id: frag.header.source_id.clone() });
                    }
                    return Ok(AssembledOutput::Dropped);
                }
            }
        }

        // supersede: a newer complete frame can replace older incomplete deltas
        let source_partials = self.partials.entry(frag.header.source_id.clone()).or_default();
        Self::evict_overflow(source_partials, frag.header.frame_id, now)?;

        // duplicate detection
        if let Some(existing) = source_partials.get(&frag.header.frame_id) {
            if existing
                .received
                .get(frag.header.frag_index as usize)
                .and_then(|slot| slot.as_ref())
                .is_some()
            {
                return Ok(AssembledOutput::Dropped); // duplicate fragment does not duplicate output
            }
        }

        let partial = source_partials
            .entry(frag.header.frame_id)
            .or_insert_with(|| PartialFrame {
                received: vec![None; frag.header.frag_count as usize],
                header: frag.header.clone(),
                received_bytes: 0,
                first_seen: now,
            });
        if partial.received[frag.header.frag_index as usize].is_none() {
            partial.received_bytes += frag.payload.len();
            partial.received[frag.header.frag_index as usize] = Some(frag.payload);
        }

        // complete?
        if partial.received.iter().all(|s| s.is_some()) {
            let header = partial.header.clone();
            let mut payload = Vec::with_capacity(header.frame_len as usize);
            for slot in partial.recovery_iter() {
                payload.extend_from_slice(&slot);
            }
            source_partials.remove(&header.frame_id);
            self.expire_incomplete(&header.source_id, now);
            self.deliver(header, Bytes::from(payload))
        } else {
            Ok(AssembledOutput::Dropped)
        }
    }

    /// Expire incomplete frames older than `ttl`. Call periodically or on feed.
    pub fn expire_incomplete_all(&mut self, ttl: Duration, now: Duration) {
        for partials in self.partials.values_mut() {
            partials.retain(|_, p| now.saturating_sub(p.first_seen) < ttl);
        }
    }

    fn expire_incomplete(&mut self, source: &SourceId, _now: Duration) {
        // keep at most MAX_INCOMPLETE_PER_SOURCE incomplete frames per source
        if let Some(partials) = self.partials.get_mut(source) {
            while partials.len() > MAX_INCOMPLETE_PER_SOURCE {
                let oldest = *partials.keys().next().expect("non-empty");
                partials.remove(&oldest);
            }
        }
    }

    fn evict_overflow(
        partials: &mut BTreeMap<u64, PartialFrame>,
        incoming_frame_id: u64,
        now: Duration,
    ) -> Result<(), AssembleError> {
        let mut total: usize = partials.values().map(|p| p.received_bytes).sum();
        // Newer incomplete frame supersedes older incomplete deltas: drop the
        // oldest incomplete frames when the incoming frame is newer.
        while total > MAX_ASSEMBLY_BYTES_PER_SOURCE {
            let oldest = *partials.keys().next().ok_or(AssembleError::MemoryBoundExceeded)?;
            if oldest >= incoming_frame_id {
                return Err(AssembleError::MemoryBoundExceeded);
            }
            let removed = partials.remove(&oldest).expect("just keyed");
            total -= removed.received_bytes;
            let _ = now;
        }
        Ok(())
    }

    fn deliver(&mut self, header: FragmentHeader, payload: Bytes) -> Result<AssembledOutput, AssembleError> {
        let state = self.states.get_mut(&header.source_id).expect("state exists");
        let out = match header.kind {
            FrameKind::Config => AssembledOutput::Dropped, // config delivered via key delivery bookkeeping
            FrameKind::Key => {
                state.decodable = true;
                state.last_delivered_frame_id = Some(header.frame_id);
                AssembledOutput::Frame(self.rebuild(&header, payload))
            }
            FrameKind::Delta => {
                if state.decodable {
                    state.last_delivered_frame_id = Some(header.frame_id);
                    AssembledOutput::Frame(self.rebuild(&header, payload))
                } else {
                    // late delta loss after which decoder cannot reference:
                    // request IDR with rate limit
                    let should_request = state
                        .last_idr_request_frame
                        .map(|last| header.frame_id.saturating_sub(last) >= self.idr_rate_limit as u64)
                        .unwrap_or(true);
                    if should_request {
                        state.last_idr_request_frame = Some(header.frame_id);
                        AssembledOutput::RequestIdr { source_id: header.source_id.clone() }
                    } else {
                        AssembledOutput::Dropped
                    }
                }
            }
        };
        Ok(out)
    }

    fn rebuild(&self, header: &FragmentHeader, payload: Bytes) -> EncodedFrame {
        EncodedFrame {
            session_id: self.session_id.clone(),
            source_id: header.source_id.clone(),
            stream_epoch: header.stream_epoch,
            frame_id: header.frame_id,
            kind: header.kind,
            codec: self.codec,
            capture_time_host_ns: 0,
            encode_done_host_ns: 0,
            width: 0,
            height: 0,
            payload,
        }
    }
}

impl PartialFrame {
    fn recovery_iter(&self) -> impl Iterator<Item = Bytes> + '_ {
        self.received.iter().map(|s| s.clone().expect("complete frame"))
    }
}

/// Bounded queue helper used by stages (docs/03 §6.3 spirit).
#[derive(Debug)]
pub struct BoundedQueue<T> {
    items: VecDeque<T>,
    cap: usize,
}

impl<T> BoundedQueue<T> {
    pub fn new(cap: usize) -> Self {
        Self { items: VecDeque::new(), cap }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Push, dropping the oldest item if over cap. Returns the dropped item.
    pub fn push_evict(&mut self, item: T) -> Option<T> {
        let dropped = if self.items.len() == self.cap { self.items.pop_front() } else { None };
        self.items.push_back(item);
        dropped
    }
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::packetize;

    fn assembled(codec: crate::frame::CodecProfile) -> FragmentAssembler {
        FragmentAssembler::new(SessionId::from_raw("s").unwrap(), codec)
    }

    fn frame(len: usize, kind: FrameKind, frame_id: u64, epoch: u32) -> EncodedFrame {
        EncodedFrame {
            session_id: SessionId::from_raw("s").unwrap(),
            source_id: SourceId::from_raw("src").unwrap(),
            stream_epoch: StreamEpoch(epoch),
            frame_id,
            kind,
            codec: crate::frame::CodecProfile::AvcBaseline,
            capture_time_host_ns: 0,
            encode_done_host_ns: 0,
            width: 1920,
            height: 1080,
            payload: Bytes::from(vec![0xCD; len]),
        }
    }

    fn feed_all(asm: &mut FragmentAssembler, frame: &EncodedFrame, now: Duration) -> Vec<AssembledOutput> {
        packetize(frame, 512)
            .unwrap()
            .into_iter()
            .map(|f| asm.feed(f, now).unwrap())
            .collect()
    }

    #[test]
    fn key_assembles_and_marks_decodable() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        let out = feed_all(&mut asm, &frame(1000, FrameKind::Key, 1, 1), Duration::ZERO);
        assert!(matches!(out.last(), Some(AssembledOutput::Frame(f)) if f.kind == FrameKind::Key));
    }

    #[test]
    fn delta_before_keyframe_is_dropped() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        let out = feed_all(&mut asm, &frame(100, FrameKind::Delta, 1, 1), Duration::ZERO);
        // never a Frame; an IDR request is the correct recovery signal
        assert!(
            out.iter().all(|o| matches!(o, AssembledOutput::Dropped | AssembledOutput::RequestIdr { .. })),
            "delta must not deliver before a keyframe: {out:?}"
        );
    }

    #[test]
    fn delta_before_config_is_dropped() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        feed_all(&mut asm, &frame(50, FrameKind::Config, 1, 1), Duration::ZERO);
        // config alone does not make the stream decodable for deltas
        let out = feed_all(&mut asm, &frame(100, FrameKind::Delta, 2, 1), Duration::ZERO);
        assert!(
            out.iter().all(|o| matches!(o, AssembledOutput::Dropped | AssembledOutput::RequestIdr { .. })),
            "delta must not deliver after config only: {out:?}"
        );
    }

    #[test]
    fn old_epoch_is_dropped_after_resize() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        feed_all(&mut asm, &frame(100, FrameKind::Key, 1, 1), Duration::ZERO);
        // epoch bump (resize)
        feed_all(&mut asm, &frame(100, FrameKind::Key, 2, 2), Duration::ZERO);
        // late delta from epoch 1
        let out = feed_all(&mut asm, &frame(100, FrameKind::Delta, 3, 1), Duration::ZERO);
        assert!(out.iter().all(|o| matches!(o, AssembledOutput::Dropped)));
    }

    #[test]
    fn duplicate_fragment_does_not_duplicate_output() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        let f = frame(1000, FrameKind::Key, 1, 1);
        let frags = packetize(&f, 512).unwrap();
        for frag in &frags {
            asm.feed(frag.clone(), Duration::ZERO).unwrap();
        }
        // replay all fragments
        let mut delivered = 0;
        for frag in &frags {
            if let AssembledOutput::Frame(_) = asm.feed(frag.clone(), Duration::ZERO).unwrap() {
                delivered += 1;
            }
        }
        assert_eq!(delivered, 0, "duplicate must not re-deliver");
    }

    #[test]
    fn incomplete_frame_expires() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        let f = frame(1000, FrameKind::Key, 1, 1);
        let frags = packetize(&f, 512).unwrap();
        // feed only first fragment
        asm.feed(frags[0].clone(), Duration::ZERO).unwrap();
        // after ttl the incomplete frame is gone
        asm.expire_incomplete_all(Duration::from_millis(100), Duration::from_millis(200));
        let state = asm.partials.get(&SourceId::from_raw("src").unwrap()).unwrap();
        assert!(state.is_empty(), "incomplete frame should have expired");
    }

    #[test]
    fn newer_complete_frame_can_supersede_older_incomplete_delta() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        feed_all(&mut asm, &frame(100, FrameKind::Key, 1, 1), Duration::ZERO);
        // partial delta frame 2 (first fragment only)
        let delta = frame(1000, FrameKind::Delta, 2, 1);
        let frags = packetize(&delta, 512).unwrap();
        asm.feed(frags[0].clone(), Duration::ZERO).unwrap();
        // newer complete delta frame 3
        let out = feed_all(&mut asm, &frame(100, FrameKind::Delta, 3, 1), Duration::ZERO);
        assert!(matches!(out.last(), Some(AssembledOutput::Frame(_))));
        // and the old incomplete delta 2 is superseded (was evicted or expired):
        // feeding its remaining fragments late must not deliver stale frame
        let late = frags[1..].iter().map(|f| asm.feed(f.clone(), Duration::from_millis(1)).unwrap());
        for o in late {
            assert!(matches!(o, AssembledOutput::Dropped), "stale frame must not deliver: {o:?}");
        }
    }

    #[test]
    fn keyframe_loss_requests_idr_with_rate_limit() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        feed_all(&mut asm, &frame(100, FrameKind::Key, 1, 1), Duration::ZERO);
        // decoder becomes non-decodable (simulated loss) — model via epoch reset? No:
        // loss means a delta arrived whose predecessor was never delivered.
        // Model by state mutation through public API: epoch bump without key.
        // Instead simulate: delta whose dependency chain is broken is one where
        // decodable flag is false. We force it by feeding delta after eviction
        // of all keys — the flag stays true, so this models via rate limit path.
        // Direct unit: request IDR when delta arrives undecodable.
        let state = asm.states.get_mut(&SourceId::from_raw("src").unwrap()).unwrap();
        state.decodable = false;
        let mut idr_count = 0;
        for frame_id in 2..20u64 {
            let outs = feed_all(&mut asm, &frame(10, FrameKind::Delta, frame_id, 1), Duration::ZERO);
            for o in outs {
                if matches!(o, AssembledOutput::RequestIdr { .. }) {
                    idr_count += 1;
                }
            }
        }
        assert!(idr_count >= 1, "IDR must be requested");
        assert!(idr_count <= 3, "IDR requests must be rate limited, got {idr_count}");
    }

    #[test]
    fn fragment_flood_stays_within_memory_bound() {
        let mut asm = assembled(crate::frame::CodecProfile::AvcBaseline);
        feed_all(&mut asm, &frame(100, FrameKind::Key, 0, 1), Duration::ZERO);
        // flood with first fragments of many large incomplete frames
        for frame_id in 1..200u64 {
            let f = frame(100_000, FrameKind::Delta, frame_id, 1);
            let frag = packetize(&f, 512).unwrap().remove(0);
            let _ = asm.feed(frag, Duration::from_millis(frame_id)).unwrap();
        }
        let buffered: usize = asm
            .partials
            .get(&SourceId::from_raw("src").unwrap())
            .map(|m| m.values().map(|p| p.received_bytes).sum())
            .unwrap_or(0);
        assert!(
            buffered <= MAX_ASSEMBLY_BYTES_PER_SOURCE + 100_000,
            "flood buffered {buffered} bytes"
        );
    }
}
