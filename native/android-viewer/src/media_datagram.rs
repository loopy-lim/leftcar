//! Low-latency UDP media datagram framing shared by the Android receiver and
//! host-side protocol tests.
//!
//! A video access unit can be much larger than the network MTU. Each datagram
//! therefore carries one fragment using this fixed header:
//!
//! ```text
//! G | fragment_index:u16 BE | fragment_count:u16 BE | au_id:u16 LE
//!   | L2 | capture_ms:u64 BE | encode_ms:u64 BE | send_ms:u64 BE
//!   | Annex-B bytes
//! ```
//!
//! The legacy `LT | send_ms` header is still accepted during rolling upgrades.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub const FRAME_MARKER: u8 = b'G';
pub const PARITY_MARKER: u8 = b'P';
pub const FRAME_HEADER_V1_LEN: usize = 17;
pub const FRAME_HEADER_V2_LEN: usize = 33;
pub const PARITY_HEADER_LEN: usize = 19;
pub const MAX_DATAGRAM_BYTES: usize = 1_400;

/// Per-socket receive buffer capacity on the viewer; larger than
/// MAX_DATAGRAM so reassembly never truncates a legal datagram.
pub const MEDIA_BUFFER_BYTES: usize = 2_048;

/// Viewer → host media-socket command bodies. Each one is AEAD-sealed with
/// the session media key at the socket boundary (`media_crypto`), which is
/// what authenticates it — there is no inline session token.
pub const COMMAND_IDR: &[u8] = b"IDR";
pub const COMMAND_BYE: &[u8] = b"BYE";
/// Viewer → host selective-retransmit request (NACK/RTX). Rides the same
/// sealed media-socket command path as [`COMMAND_IDR`]. Body:
/// `NAK | au_id:u16 LE | count:u8 | count × fragment_index:u16 LE` with
/// `count <= MAX_NACK_FRAGMENTS`. A host that predates NAK never matches the
/// prefix and drops the body, so the sender's grace simply expires into the
/// existing loss path — no compatibility shim needed in either direction.
pub const COMMAND_NAK: &[u8] = b"NAK";
/// Hard cap on fragments per NACK datagram. Larger requests are split across
/// several datagrams by the sender (see [`MAX_NACK_REQUEST_TOTAL`] for when
/// requesting is abandoned entirely).
pub const MAX_NACK_FRAGMENTS: usize = 20;
/// Widest hole worth repairing by retransmit. Beyond this the loss is a
/// burst, the host ring has likely evicted the fragments anyway, and the
/// existing gated IDR is the cheaper repair.
pub const MAX_NACK_REQUEST_TOTAL: usize = 40;
pub const MAX_FRAGMENT_PAYLOAD: usize = MAX_DATAGRAM_BYTES - FRAME_HEADER_V2_LEN;
// Four AUs tolerate a short Wi-Fi scheduling/reordering burst. Completed AUs
// are still returned immediately, so this does not create a playback queue.
const MAX_IN_FLIGHT_AUS: usize = 4;
const MAX_FRAGMENTS_PER_AU: usize = 16_384;
const MAX_AU_BYTES: usize = 16 * 1024 * 1024;
// Normal 1080p access units use far fewer fragments. Give that hot path one
// contiguous slab and retain the sparse guarded representation only for very
// large recovery keyframes.
const CONTIGUOUS_FRAGMENT_LIMIT: usize = 1_024;
const RECENT_COMPLETED_AUS: usize = 64;
const COMPLETION_REORDER_WAIT: Duration = Duration::from_millis(8);
const MAX_COMPLETED_REORDER: usize = 3;
/// While a NACK retransmit hold keeps the reorder window open, the
/// subsequent-AU count flush in `push_at` is suspended and the pending queue
/// may grow past MAX_COMPLETED_REORDER — but only to this hard cap, beyond
/// which delivery proceeds anyway. The hold itself is bounded (<= 25ms), so
/// at normal frame rates only a few extra AUs accumulate; the cap bounds
/// memory and latency even if the NACK ticks stop entirely.
const MAX_HELD_REORDER: usize = MAX_COMPLETED_REORDER * 2;
pub const BASE_STALE_FRAME_BUDGET_MS: u64 = 80;
pub const RECOVERY_REQUEST_COOLDOWN: Duration = Duration::from_millis(250);
/// Number of consecutive over-budget delta frames that used to trigger
/// resync. Kept as a telemetry threshold for compatibility; lateness alone
/// must not invalidate a video codec reference chain.
pub const STALE_RESYNC_THRESHOLD: u32 = 3;
/// The receive-side completion batch remains bounded for memory and work
/// accounting. Completed AUs are still submitted in order so delta references
/// are not broken; the decoder output pump owns latest-frame selection.
pub const MAX_LIVE_EDGE_BATCH: usize = 3;

pub struct LiveEdgeSelection<T> {
    pub frames: Vec<T>,
    pub discarded: usize,
}

/// Preserve every completed AU in receive order.
///
/// Dropping completed delta AUs here looks like a latency optimization, but
/// it breaks the codec reference chain and forces an IDR recovery. The
/// decoder's output pump already keeps the newest renderable outputs and
/// discards older decoded images, so the input side must remain lossless
/// within the bounded receive batch.
pub fn select_live_edge_frames<T>(frames: Vec<T>) -> LiveEdgeSelection<T> {
    LiveEdgeSelection {
        discarded: 0,
        frames,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameGapReason {
    None,
    NetworkLoss { missing: u16 },
    LiveEdgeDiscard { missing: u16 },
    RecoverySkip { missing: u16 },
}

/// Classify a frame-id jump using the receiver-side event that caused it.
///
/// A live-edge discard, when produced by an older caller, is a deliberate
/// local discard, not packet loss. Once recovery is already active, skipped
/// deltas are also not a new loss event; counting either as network loss feeds
/// the Host's recovery/bitrate loop with a false positive and can create
/// another IDR burst.
pub fn classify_frame_gap(
    previous: Option<u16>,
    current: u16,
    intentional_live_edge_discard: bool,
    awaiting_keyframe: bool,
) -> FrameGapReason {
    let Some(previous) = previous else {
        return FrameGapReason::None;
    };
    let missing = viewer_decoder::frame_id_missing_count(previous, current);
    if missing == 0 {
        return FrameGapReason::None;
    }
    if awaiting_keyframe {
        FrameGapReason::RecoverySkip { missing }
    } else if intentional_live_edge_discard {
        FrameGapReason::LiveEdgeDiscard { missing }
    } else {
        FrameGapReason::NetworkLoss { missing }
    }
}

/// Decide whether a completed AU may enter MediaCodec after a frame-id jump.
///
/// Any missing delta AU breaks the low-latency reference chain. Keep the last
/// good Surface image visible while the receiver requests a fresh keyframe
/// instead of feeding visibly corrupted dependent frames to MediaCodec.
pub fn should_feed_frame(reason: FrameGapReason, keyframe: bool) -> bool {
    match reason {
        FrameGapReason::None => true,
        FrameGapReason::NetworkLoss { .. }
        | FrameGapReason::LiveEdgeDiscard { .. }
        | FrameGapReason::RecoverySkip { .. } => keyframe,
    }
}

/// Unified gap freeze policy (Q8/T9): the single path now matches the split
/// renderer's no-flush behavior. A missing delta AU freezes the last good
/// Surface image and requests one IDR through the recovery gate — MediaCodec
/// is NOT flushed, and feeding resumes at the next independently decodable
/// keyframe (`should_feed_frame` + `classify_frame_gap` RecoverySkip).
pub fn should_freeze_after_network_loss(missing: u16, keyframe: bool) -> bool {
    !keyframe && missing > 0
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverPressure {
    pub live_edge_discards: u64,
    pub decoder_input_drops: u64,
    pub decoder_output_discards: u64,
    pub fec_recovered_fragments: u64,
    pub unrecoverable_fec_groups: u64,
}

impl ReceiverPressure {
    pub fn record_live_edge_discards(&mut self, count: usize) {
        self.live_edge_discards = self.live_edge_discards.saturating_add(count as u64);
    }

    pub fn record_decoder_input_drop(&mut self) {
        self.decoder_input_drops = self.decoder_input_drops.saturating_add(1);
    }

    pub fn record_decoder_output_discards(&mut self, total: u64) {
        self.decoder_output_discards = total;
    }

    pub fn record_fec_recovery(&mut self, count: usize) {
        self.fec_recovered_fragments = self.fec_recovered_fragments.saturating_add(count as u64);
    }

    /// Parity with the split renderer's telemetry: an evicted group still
    /// owing fragments is unrecoverable, not merely forgotten.
    pub fn record_unrecoverable_group(&mut self, missing_data_fragments: usize) {
        if missing_data_fragments == 0 {
            return;
        }
        self.unrecoverable_fec_groups = self.unrecoverable_fec_groups.saturating_add(1);
    }
}

/// Advance stale-frame telemetry without requiring a decoder.
///
/// A frame that arrives late is still a valid reference input. The decoder
/// output pump is responsible for discarding old decoded images, while the
/// input side must not request an IDR merely because wall-clock age crossed a
/// display budget.
pub fn stale_streak_advance(
    consecutive_stale: u32,
    is_keyframe: bool,
    over_budget: bool,
) -> (u32, bool) {
    if is_keyframe || !over_budget {
        return (0, false);
    }
    let next = consecutive_stale.saturating_add(1);
    (next, false)
}

pub fn recovery_request_suppressed(now_us: u64, suppressed_until_us: u64) -> bool {
    now_us < suppressed_until_us
}

pub fn stale_frame_budget_ms(network_rtt_ms: Option<u64>) -> u64 {
    BASE_STALE_FRAME_BUDGET_MS + network_rtt_ms.unwrap_or(0).saturating_div(2).min(120)
}

/// Convert a rendered-frame counter delta into a rate for the actual feedback
/// interval. The media loop can be delayed by FEC/reassembly or a decoder
/// wake-up, so treating every feedback packet as exactly one second makes the
/// Host see false 1-30 FPS dips even while the Surface is rendering steadily.
pub fn rendered_fps_from_feedback(
    rendered_frames: u64,
    previous_rendered_frames: u64,
    elapsed_ms: u64,
) -> u16 {
    if elapsed_ms == 0 {
        return 0;
    }
    let delta = rendered_frames.saturating_sub(previous_rendered_frames);
    let fps = (u128::from(delta) * 1_000).div_ceil(u128::from(elapsed_ms));
    fps.min(u128::from(u16::MAX)) as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFragment<'a> {
    pub index: u16,
    pub count: u16,
    pub id: u16,
    pub capture_wall_ms: Option<u64>,
    pub encode_wall_ms: Option<u64>,
    pub send_wall_ms: u64,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParityFragment<'a> {
    pub id: u16,
    pub k: u8,
    pub index: u8,
    pub base: u16,
    pub total: u16,
    pub send_wall_ms: u64,
    pub payload: &'a [u8],
}

pub fn parse_parity(datagram: &[u8]) -> Option<ParityFragment<'_>> {
    if datagram.len() <= PARITY_HEADER_LEN || datagram[0] != PARITY_MARKER {
        return None;
    }
    let id = u16::from_le_bytes(datagram[1..3].try_into().ok()?);
    let k = datagram[3];
    let index = datagram[4];
    let base = u16::from_be_bytes(datagram[5..7].try_into().ok()?);
    let total = u16::from_be_bytes(datagram[7..9].try_into().ok()?);
    if !(1..=8).contains(&k)
        || usize::from(index) >= fec_core::MAX_PARITY_SHARDS.min(usize::from(k).saturating_sub(1))
        || total == 0
        || usize::from(base) + usize::from(k) > usize::from(total)
        || &datagram[9..11] != b"LT"
    {
        return None;
    }
    Some(ParityFragment {
        id,
        k,
        index,
        base,
        total,
        send_wall_ms: u64::from_be_bytes(datagram[11..19].try_into().ok()?),
        payload: &datagram[PARITY_HEADER_LEN..],
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredFragment {
    pub index: u16,
    pub count: u16,
    pub id: u16,
    pub capture_wall_ms: Option<u64>,
    pub encode_wall_ms: Option<u64>,
    pub send_wall_ms: u64,
    pub payload: Vec<u8>,
}

/// Recently completed FEC groups. Data shards normally arrive before their
/// parity tail, so a complete group can be removed before those parity
/// datagrams reach the receiver. Remembering the key prevents that harmless
/// tail from recreating an all-missing group and being reported as loss when
/// the bounded FEC map evicts it.
#[derive(Default)]
pub struct CompletedFecGroups {
    order: std::collections::VecDeque<(u16, u16)>,
}

impl CompletedFecGroups {
    const CAPACITY: usize = 64;

    pub fn contains(&self, key: &(u16, u16)) -> bool {
        self.order.contains(key)
    }

    pub fn remember(&mut self, key: (u16, u16)) {
        if self.contains(&key) {
            return;
        }
        if self.order.len() == Self::CAPACITY {
            self.order.pop_front();
        }
        self.order.push_back(key);
    }

    pub fn clear(&mut self) {
        self.order.clear();
    }
}

/// Drop receiver-local FEC assembly state at an explicit recovery boundary.
///
/// A decoder recovery already has its own gap and IDR telemetry. Treating the
/// partially assembled groups discarded by that reset as fresh packet loss
/// feeds the same recovery back into Host adaptation and can hold the UDP
/// burst at its most conservative value indefinitely.
pub fn discard_fec_groups(
    groups: &mut HashMap<(u16, u16), FecGroup>,
    order: &mut VecDeque<(u16, u16)>,
) {
    groups.clear();
    order.clear();
}

/// Bounded FEC state for one AU group. Data fragments may arrive before the
/// parity width is known; they are retained in raw form until a P datagram
/// supplies the fixed-width shard size.
pub const MAX_ACTIVE_FEC_GROUPS: usize = 48;

pub struct FecGroup {
    id: u16,
    k: usize,
    base: u16,
    total: u16,
    data: Vec<Option<Vec<u8>>>,
    parity: Vec<Option<Vec<u8>>>,
    capture_wall_ms: Option<u64>,
    encode_wall_ms: Option<u64>,
    send_wall_ms: u64,
    width: Option<usize>,
}

impl FecGroup {
    pub fn new(id: u16, k: u8, base: u16, total: u16) -> Option<Self> {
        let k = usize::from(k);
        let parity = fec_core::MAX_PARITY_SHARDS.min(k.saturating_sub(1));
        if !(1..=8).contains(&k) || parity == 0 || usize::from(base) + k > usize::from(total) {
            return None;
        }
        Some(Self {
            id,
            k,
            base,
            total,
            data: vec![None; k],
            parity: vec![None; parity],
            capture_wall_ms: None,
            encode_wall_ms: None,
            send_wall_ms: 0,
            width: None,
        })
    }

    pub fn push_data(&mut self, fragment: FrameFragment<'_>) {
        if fragment.id != self.id
            || fragment.count != self.total
            || fragment.index < self.base
            || usize::from(fragment.index - self.base) >= self.k
        {
            return;
        }
        let index = usize::from(fragment.index - self.base);
        if self.data[index].is_none() {
            self.data[index] = Some(fragment.payload.to_vec());
            self.capture_wall_ms = fragment.capture_wall_ms;
            self.encode_wall_ms = fragment.encode_wall_ms;
            self.send_wall_ms = fragment.send_wall_ms;
        }
    }

    pub fn push_data_and_restore(
        &mut self,
        fragment: FrameFragment<'_>,
    ) -> Option<Vec<RestoredFragment>> {
        self.push_data(fragment);
        self.try_restore()
    }

    pub fn push_parity(&mut self, fragment: ParityFragment<'_>) {
        if fragment.id != self.id
            || fragment.k as usize != self.k
            || fragment.base != self.base
            || fragment.total != self.total
            || usize::from(fragment.index) >= self.parity.len()
            || !(2..=MAX_DATAGRAM_BYTES).contains(&fragment.payload.len())
            || self
                .width
                .is_some_and(|width| width != fragment.payload.len())
            || self.parity[usize::from(fragment.index)].is_some()
        {
            return;
        }
        self.width = Some(fragment.payload.len());
        self.parity[usize::from(fragment.index)] = Some(fragment.payload.to_vec());
        if self.send_wall_ms == 0 {
            self.send_wall_ms = fragment.send_wall_ms;
        }
    }

    pub fn push_parity_and_restore(
        &mut self,
        fragment: ParityFragment<'_>,
    ) -> Option<Vec<RestoredFragment>> {
        self.push_parity(fragment);
        self.try_restore()
    }

    pub fn try_restore(&mut self) -> Option<Vec<RestoredFragment>> {
        self.try_restore_result().ok().flatten()
    }

    pub fn try_restore_result(
        &mut self,
    ) -> Result<Option<Vec<RestoredFragment>>, fec_core::FecError> {
        let Some(width) = self.width else {
            return Ok(None);
        };
        // Every data fragment is present: there is nothing to restore. The
        // common late-parity arrival used to pay a full identity matrix
        // inversion plus a k*width multiply pass here on the completing
        // datagram before returning None.
        // Count unique usable slots before pack_shard/clone. Retries with
        // fewer than k valid shards do no heap work (including bad widths).
        let packable = |payload: &Vec<u8>| {
            width >= 2 && payload.len() <= width - 2 && payload.len() <= usize::from(u16::MAX)
        };
        let available_data = self.data.iter().flatten().filter(|p| packable(p)).count();
        if available_data == self.k {
            return Ok(None);
        }
        let available_parity = self
            .parity
            .iter()
            .flatten()
            .filter(|p| p.len() == width)
            .count();
        if available_data + available_parity < self.k {
            return Ok(None);
        }
        let received = self
            .data
            .iter()
            .map(|payload| {
                payload
                    .as_deref()
                    .and_then(|payload| fec_core::pack_shard(payload, width).ok())
            })
            .chain(self.parity.iter().cloned())
            .collect::<Vec<_>>();
        if received.iter().flatten().count() < self.k {
            return Ok(None);
        }
        let restored =
            fec_core::decode_group_with_max_parity(received, self.k, width, self.parity.len())?;
        let mut output = Vec::new();
        for (index, payload) in restored.into_iter().enumerate() {
            if !self.data[index].as_ref().is_some_and(packable) {
                self.data[index] = Some(payload.clone());
                output.push(RestoredFragment {
                    index: self.base + index as u16,
                    count: self.total,
                    id: self.id,
                    capture_wall_ms: self.capture_wall_ms,
                    encode_wall_ms: self.encode_wall_ms,
                    send_wall_ms: self.send_wall_ms,
                    payload,
                });
            }
        }
        Ok((!output.is_empty()).then_some(output))
    }

    pub fn is_complete(&self) -> bool {
        self.data.iter().all(Option::is_some)
    }

    pub fn missing_data_fragments(&self) -> usize {
        self.data
            .iter()
            .filter(|fragment| fragment.is_none())
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassembledFrame {
    pub id: u16,
    pub capture_wall_ms: Option<u64>,
    pub encode_wall_ms: Option<u64>,
    pub send_wall_ms: u64,
    pub au: Vec<u8>,
}

/// Restores short cross-AU UDP reordering without becoming a playback queue.
/// A genuinely missing frame is skipped after at most three completed AUs or
/// eight milliseconds, whichever comes first.
pub struct CompletedFrameSequencer {
    last_delivered: Option<u16>,
    pending: HashMap<u16, (Instant, ReassembledFrame)>,
    codec: viewer_decoder::VideoCodec,
    /// While set and still in the future, both reorder flush paths wait: the
    /// time path via `expiry_due` and the subsequent-AU count path in
    /// `push_at` (bounded by `MAX_HELD_REORDER`). The NACK tick sets it to
    /// the retransmit grace deadline so a retransmit in flight can still
    /// complete the blocked hole contiguously instead of the hole being
    /// forgotten by an early count-based flush. It is an absolute arm-time
    /// deadline (never extended past its grace) and is cleared by any
    /// clear/epoch reset, so it can never stall delivery.
    hold_until: Option<Instant>,
    completion_nack_grace: Option<Duration>,
    // Reserve at completion, before a receive batch can flush the hole. Keep
    // an expired reservation for this hole so later arrivals cannot renew it.
    nack_reservation: Option<(u16, Instant)>,
}

impl Default for CompletedFrameSequencer {
    fn default() -> Self {
        Self {
            last_delivered: None,
            pending: HashMap::new(),
            codec: viewer_decoder::VideoCodec::H264,
            hold_until: None,
            completion_nack_grace: None,
            nack_reservation: None,
        }
    }
}

impl CompletedFrameSequencer {
    pub fn clear(&mut self) {
        self.last_delivered = None;
        self.pending.clear();
        self.hold_until = None;
        self.nack_reservation = None;
    }

    pub fn establish_epoch(&mut self, last_delivered: u16) {
        self.last_delivered = Some(last_delivered);
        self.pending.clear();
        self.hold_until = None;
        self.nack_reservation = None;
    }

    pub fn set_codec(&mut self, codec: viewer_decoder::VideoCodec) {
        self.codec = codec;
    }

    /// The access-unit id the sequencer is blocked on: the next expected id
    /// when at least one later completed AU is queued but the next one itself
    /// has not completed. This is the only hole a retransmit can still heal —
    /// once expiry delivers past it, presentation classifies the loss.
    pub fn blocked_hole(&self) -> Option<u16> {
        let last = self.last_delivered?;
        if self.pending.is_empty() {
            return None;
        }
        let expected = last.wrapping_add(1);
        (!self.pending.contains_key(&expected)).then_some(expected)
    }

    /// Hold the reorder window open until `deadline` (absolute). Callers pass
    /// the NACK grace computed at arm time; expiry resumes automatically once
    /// the deadline passes even if the ticks stop.
    pub fn hold_reorder_expiry_until(&mut self, deadline: Instant) {
        self.hold_until = Some(deadline);
    }

    pub fn clear_hold(&mut self) {
        self.hold_until = None;
    }

    /// Reorder expiry for one pending AU: the usual COMPLETION_REORDER_WAIT,
    /// extended while a NACK retransmit grace is pending (the grace is capped
    /// at 25ms, so a never-answered NACK only adds bounded latency before the
    /// unchanged freeze + IDR path runs).
    fn expiry_due(&self, received_at: Instant, now: Instant) -> bool {
        if now.duration_since(received_at) < COMPLETION_REORDER_WAIT {
            return false;
        }
        match self.hold_until {
            Some(deadline) => now >= deadline,
            None => true,
        }
    }

    pub fn drain_expired(&mut self) -> Vec<ReassembledFrame> {
        self.drain_expired_at(Instant::now())
    }

    fn drain_expired_at(&mut self, now: Instant) -> Vec<ReassembledFrame> {
        // A cleared epoch is waiting for an independently decodable boundary.
        // Never advance it from a delta merely because the reorder timer elapsed;
        // that would make a slightly later recovery IDR look stale.
        if self.last_delivered.is_none() {
            return Vec::new();
        }
        let waited_long_enough = self
            .pending
            .values()
            .any(|(received_at, _)| self.expiry_due(*received_at, now));
        if !waited_long_enough {
            return Vec::new();
        }
        self.drain_next_available()
    }

    /// Set receiver eligibility before processing completions (including FEC).
    /// This reserves time only; the normal requester tick sends the NACK.
    pub fn configure_nack_grace(&mut self, eligible: bool, rtt: Option<u64>) {
        self.completion_nack_grace = eligible.then(|| nack_grace_duration(rtt));
        if !eligible {
            self.nack_reservation = None;
            self.clear_hold();
        }
    }

    pub fn push_reassembled(
        &mut self,
        frame: ReassembledFrame,
        reassembler: &FrameReassembler,
    ) -> Vec<ReassembledFrame> {
        self.push_reassembled_at(frame, reassembler, Instant::now())
    }

    fn push_reassembled_at(
        &mut self,
        frame: ReassembledFrame,
        reassembler: &FrameReassembler,
        now: Instant,
    ) -> Vec<ReassembledFrame> {
        if let (Some(grace), Some(last)) = (self.completion_nack_grace, self.last_delivered) {
            let distance = frame.id.wrapping_sub(last);
            let hole = last.wrapping_add(1);
            if distance > 1 && distance <= i16::MAX as u16 {
                if self.nack_reservation.is_some_and(|(id, _)| id != hole) {
                    self.nack_reservation = None;
                    self.clear_hold();
                }
                if self.nack_reservation.is_none()
                    && reassembler
                        .missing_fragment_indexes(hole)
                        .is_some_and(|missing| {
                            !missing.is_empty() && missing.len() <= MAX_NACK_REQUEST_TOTAL
                        })
                {
                    let deadline = self.hold_until.unwrap_or(now + grace).min(now + grace);
                    self.nack_reservation = Some((hole, deadline));
                    self.hold_reorder_expiry_until(deadline);
                }
            }
        }
        let ready = self.push_at(frame, now);
        if self
            .nack_reservation
            .is_some_and(|(hole, _)| self.blocked_hole() != Some(hole))
        {
            self.nack_reservation = None;
            self.clear_hold();
        }
        ready
    }

    pub fn push(&mut self, frame: ReassembledFrame) -> Vec<ReassembledFrame> {
        self.push_at(frame, Instant::now())
    }

    fn push_at(&mut self, frame: ReassembledFrame, now: Instant) -> Vec<ReassembledFrame> {
        let Some(last) = self.last_delivered else {
            if is_keyframe(&frame.au, self.codec) {
                self.last_delivered = Some(frame.id);
                let mut expected = frame.id.wrapping_add(1);
                let mut contiguous = Vec::with_capacity(MAX_COMPLETED_REORDER);
                while self.pending.contains_key(&expected) {
                    contiguous.push(expected);
                    expected = expected.wrapping_add(1);
                }
                self.pending.retain(|id, _| contiguous.contains(id));
                let mut ready = vec![frame];
                self.drain_contiguous(&mut ready);
                return ready;
            }
            if !self.pending.contains_key(&frame.id) {
                while self.pending.len() >= MAX_COMPLETED_REORDER {
                    let Some(oldest) = self
                        .pending
                        .iter()
                        .min_by_key(|(_, (received_at, _))| *received_at)
                        .map(|(id, _)| *id)
                    else {
                        break;
                    };
                    self.pending.remove(&oldest);
                }
                self.pending.insert(frame.id, (now, frame));
            }
            return self.drain_expired_at(now);
        };
        let distance = frame.id.wrapping_sub(last);
        if distance == 0 || distance > i16::MAX as u16 {
            return Vec::new();
        }
        self.pending.entry(frame.id).or_insert((now, frame));

        let mut ready = Vec::with_capacity(self.pending.len().min(MAX_COMPLETED_REORDER));
        self.drain_contiguous(&mut ready);
        if !ready.is_empty() {
            return ready;
        }

        // While a NACK retransmit hold is armed, the count-based flush must
        // not run at MAX_COMPLETED_REORDER: delivering past the blocked hole
        // mid-grace would forget it (blocked_hole() -> None clears the
        // request state) and a late retransmit would be dropped as stale.
        // Only the time path below may flush, and it waits for the hold
        // deadline. The wait stays bounded both ways: the hold is an
        // absolute deadline, and the queue may only grow to MAX_HELD_REORDER
        // before flushing anyway.
        let hold_active = self.hold_until.is_some_and(|deadline| now < deadline);
        let flush_threshold = if hold_active {
            MAX_HELD_REORDER
        } else {
            MAX_COMPLETED_REORDER
        };
        let waited_long_enough = self
            .pending
            .values()
            .any(|(received_at, _)| self.expiry_due(*received_at, now));
        if waited_long_enough {
            return self.drain_next_available();
        }
        if self.pending.len() < flush_threshold {
            return ready;
        }

        self.drain_next_available()
    }

    fn drain_next_available(&mut self) -> Vec<ReassembledFrame> {
        let next_id = match self.last_delivered {
            Some(last) => self
                .pending
                .keys()
                .copied()
                .min_by_key(|id| id.wrapping_sub(last)),
            None => self.pending.keys().copied().min(),
        };
        let Some(next_id) = next_id else {
            return Vec::new();
        };
        let Some((_, frame)) = self.pending.remove(&next_id) else {
            return Vec::new();
        };
        self.last_delivered = Some(frame.id);
        let mut ready = vec![frame];
        self.drain_contiguous(&mut ready);
        ready
    }

    fn drain_contiguous(&mut self, ready: &mut Vec<ReassembledFrame>) {
        loop {
            let Some(last) = self.last_delivered else {
                return;
            };
            let expected = last.wrapping_add(1);
            let Some((_, frame)) = self.pending.remove(&expected) else {
                return;
            };
            self.last_delivered = Some(frame.id);
            ready.push(frame);
        }
    }
}

/// One random-access access unit (H264 IDR / HEVC IRAP). Canonical
/// keyframe test shared by host reassembly and both renderers.
pub fn is_keyframe(au: &[u8], codec: viewer_decoder::VideoCodec) -> bool {
    viewer_decoder::split_annexb(au)
        .iter()
        .any(|nal| match codec {
            viewer_decoder::VideoCodec::H264 => {
                viewer_decoder::nal_type(nal.bytes) == Some(viewer_decoder::NAL_IDR)
            }
            viewer_decoder::VideoCodec::Hevc => viewer_decoder::hevc_nal_type(nal.bytes)
                .is_some_and(|nal_type| matches!(nal_type, 19..=21)),
        })
}

enum PartialPayload {
    Contiguous { data: Vec<u8>, lengths: Vec<u16> },
    Sparse(Vec<Option<Vec<u8>>>),
}

impl PartialPayload {
    fn new(fragment_count: usize) -> Self {
        if fragment_count <= CONTIGUOUS_FRAGMENT_LIMIT {
            Self::Contiguous {
                data: vec![0; fragment_count * MAX_FRAGMENT_PAYLOAD],
                lengths: vec![0; fragment_count],
            }
        } else {
            Self::Sparse(vec![None; fragment_count])
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Contiguous { lengths, .. } => lengths.len(),
            Self::Sparse(fragments) => fragments.len(),
        }
    }

    fn insert(&mut self, index: usize, payload: &[u8]) -> bool {
        match self {
            Self::Contiguous { data, lengths } => {
                let Some(length) = lengths.get_mut(index) else {
                    return false;
                };
                if *length != 0 {
                    return false;
                }
                let start = index * MAX_FRAGMENT_PAYLOAD;
                data[start..start + payload.len()].copy_from_slice(payload);
                *length = payload.len() as u16;
                true
            }
            Self::Sparse(fragments) => {
                let Some(slot) = fragments.get_mut(index) else {
                    return false;
                };
                if slot.is_some() {
                    return false;
                }
                *slot = Some(payload.to_vec());
                true
            }
        }
    }

    fn missing_fragment_indexes(&self) -> Vec<u16> {
        match self {
            Self::Contiguous { lengths, .. } => lengths
                .iter()
                .enumerate()
                .filter(|(_, length)| **length == 0)
                .map(|(index, _)| index as u16)
                .collect(),
            Self::Sparse(fragments) => fragments
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.is_none())
                .map(|(index, _)| index as u16)
                .collect(),
        }
    }

    fn contains(&self, index: usize) -> bool {
        match self {
            Self::Contiguous { lengths, .. } => {
                lengths.get(index).is_some_and(|length| *length != 0)
            }
            Self::Sparse(fragments) => fragments.get(index).is_some_and(Option::is_some),
        }
    }

    fn into_au(self, bytes: usize) -> Option<Vec<u8>> {
        match self {
            Self::Contiguous { mut data, lengths } => {
                let last = lengths.len().checked_sub(1)?;
                let protocol_contiguous = lengths[..last]
                    .iter()
                    .all(|length| usize::from(*length) == MAX_FRAGMENT_PAYLOAD);
                if protocol_contiguous {
                    let final_len = last * MAX_FRAGMENT_PAYLOAD + usize::from(lengths[last]);
                    data.truncate(final_len);
                    return Some(data);
                }
                let mut compact = Vec::with_capacity(bytes);
                for (index, length) in lengths.into_iter().enumerate() {
                    let length = usize::from(length);
                    let start = index * MAX_FRAGMENT_PAYLOAD;
                    compact.extend_from_slice(&data[start..start + length]);
                }
                Some(compact)
            }
            Self::Sparse(mut fragments) => {
                let mut compact = Vec::with_capacity(bytes);
                for fragment in &mut fragments {
                    compact.extend(fragment.take()?);
                }
                Some(compact)
            }
        }
    }
}

struct PartialFrame {
    capture_wall_ms: Option<u64>,
    encode_wall_ms: Option<u64>,
    send_wall_ms: u64,
    payload: PartialPayload,
    received: usize,
    bytes: usize,
}

#[derive(Default)]
pub struct FrameReassembler {
    partial: HashMap<u16, PartialFrame>,
    insertion_order: VecDeque<u16>,
    recently_completed: VecDeque<u16>,
    incomplete_evictions: u64,
}

pub fn parse_fragment(datagram: &[u8]) -> Option<FrameFragment<'_>> {
    if datagram.len() <= FRAME_HEADER_V1_LEN || datagram[0] != FRAME_MARKER {
        return None;
    }
    let index = u16::from_be_bytes(datagram[1..3].try_into().ok()?);
    let count = u16::from_be_bytes(datagram[3..5].try_into().ok()?);
    let id = u16::from_le_bytes(datagram[5..7].try_into().ok()?);
    if count == 0 || index >= count {
        return None;
    }
    if usize::from(count) > MAX_FRAGMENTS_PER_AU {
        return None;
    }
    let (capture_wall_ms, encode_wall_ms, send_wall_ms, header_len) = match datagram.get(7..9)? {
        [b'L', b'2'] if datagram.len() > FRAME_HEADER_V2_LEN => (
            Some(u64::from_be_bytes(datagram[9..17].try_into().ok()?)),
            Some(u64::from_be_bytes(datagram[17..25].try_into().ok()?)),
            u64::from_be_bytes(datagram[25..33].try_into().ok()?),
            FRAME_HEADER_V2_LEN,
        ),
        [b'L', b'T'] => (
            None,
            None,
            u64::from_be_bytes(datagram[9..17].try_into().ok()?),
            FRAME_HEADER_V1_LEN,
        ),
        _ => return None,
    };
    Some(FrameFragment {
        index,
        count,
        id,
        capture_wall_ms,
        encode_wall_ms,
        send_wall_ms,
        payload: &datagram[header_len..],
    })
}

impl FrameReassembler {
    pub fn clear(&mut self) {
        self.partial.clear();
        self.insertion_order.clear();
        self.recently_completed.clear();
    }

    /// Number of incomplete access units evicted due to reordering pressure,
    /// malformed resets, or the bounded AU-size guard.
    pub fn incomplete_evictions(&self) -> u64 {
        self.incomplete_evictions
    }

    pub fn push(&mut self, fragment: FrameFragment<'_>) -> Option<ReassembledFrame> {
        // Recovery keyframes may be transmitted twice. Once the first copy is
        // complete, fragments from the redundant copy must not create a new
        // partial frame or emit the same AU a second time. A small recent-ID
        // window is sufficient because the 16-bit AU sequence takes minutes
        // to wrap even at high refresh rates.
        if self.recently_completed.contains(&fragment.id) {
            return None;
        }
        let expected_count = usize::from(fragment.count);
        if fragment.payload.is_empty() || fragment.payload.len() > MAX_FRAGMENT_PAYLOAD {
            self.drop_incomplete(fragment.id);
            return None;
        }
        let needs_reset = self
            .partial
            .get(&fragment.id)
            .map(|partial| {
                partial.payload.len() != expected_count
                    || partial.capture_wall_ms != fragment.capture_wall_ms
                    || partial.encode_wall_ms != fragment.encode_wall_ms
                    || partial.send_wall_ms != fragment.send_wall_ms
            })
            .unwrap_or(false);
        if needs_reset {
            self.drop_incomplete(fragment.id);
        }

        if !self.partial.contains_key(&fragment.id) {
            while self.partial.len() >= MAX_IN_FLIGHT_AUS {
                let Some(oldest) = self.insertion_order.pop_front() else {
                    break;
                };
                if self.partial.remove(&oldest).is_some() {
                    self.incomplete_evictions = self.incomplete_evictions.saturating_add(1);
                }
            }
            self.insertion_order.push_back(fragment.id);
            self.partial.insert(
                fragment.id,
                PartialFrame {
                    capture_wall_ms: fragment.capture_wall_ms,
                    encode_wall_ms: fragment.encode_wall_ms,
                    send_wall_ms: fragment.send_wall_ms,
                    payload: PartialPayload::new(expected_count),
                    received: 0,
                    bytes: 0,
                },
            );
        }

        let fragment_index = usize::from(fragment.index);
        let exceeds_size_limit = self.partial.get(&fragment.id).is_some_and(|partial| {
            !partial.payload.contains(fragment_index)
                && partial.bytes.saturating_add(fragment.payload.len()) > MAX_AU_BYTES
        });
        if exceeds_size_limit {
            self.drop_incomplete(fragment.id);
            return None;
        }

        let partial = self.partial.get_mut(&fragment.id)?;
        if partial.payload.insert(fragment_index, fragment.payload) {
            partial.bytes = partial.bytes.saturating_add(fragment.payload.len());
            partial.received += 1;
        }
        if partial.received != partial.payload.len() {
            return None;
        }

        let completed = self.partial.remove(&fragment.id)?;
        self.insertion_order.retain(|id| *id != fragment.id);
        self.recently_completed.push_back(fragment.id);
        if self.recently_completed.len() > RECENT_COMPLETED_AUS {
            self.recently_completed.pop_front();
        }
        let au = completed.payload.into_au(completed.bytes)?;
        Some(ReassembledFrame {
            id: fragment.id,
            capture_wall_ms: completed.capture_wall_ms,
            encode_wall_ms: completed.encode_wall_ms,
            send_wall_ms: completed.send_wall_ms,
            au,
        })
    }

    /// Fragment indexes of `id` still missing, when `id` is partially
    /// assembled. `None` when the AU is unknown (never seen, already
    /// completed, or evicted) — nothing retransmittable is known about it.
    /// These are the payloads a NACK can ask the host to re-send.
    pub fn missing_fragment_indexes(&self, id: u16) -> Option<Vec<u16>> {
        let partial = self.partial.get(&id)?;
        Some(partial.payload.missing_fragment_indexes())
    }

    /// True when `id` completed recently enough to sit in the dedup window.
    /// The NACK tick reads this to attribute a heal: the armed AU completed
    /// (original delivery, FEC restore, or retransmit) inside the grace.
    pub fn recently_completed_contains(&self, id: u16) -> bool {
        self.recently_completed.contains(&id)
    }

    fn drop_incomplete(&mut self, id: u16) {
        if self.partial.remove(&id).is_some() {
            self.incomplete_evictions = self.incomplete_evictions.saturating_add(1);
        }
        self.insertion_order.retain(|queued| *queued != id);
    }
}

/// How long the resequencer holds its reorder window open for a retransmit
/// after a NACK was sent. 2×RTT covers request + retransmit on the same
/// link; the floor matches the existing 8ms reorder window, so an unmeasured
/// RTT changes nothing; the 25ms cap bounds the added freeze latency when
/// the retransmit never arrives (versus today's 150-400ms gap freeze).
pub fn nack_grace_duration(network_rtt_ms: Option<u64>) -> Duration {
    let rtt = network_rtt_ms.unwrap_or(0);
    Duration::from_millis(rtt.saturating_mul(2).saturating_add(5).clamp(8, 25))
}

/// Encode one NACK request body (to be AEAD-sealed by the caller like any
/// other viewer command).
pub fn encode_nack_request(au_id: u16, fragment_indexes: &[u16]) -> Option<Vec<u8>> {
    if fragment_indexes.is_empty() || fragment_indexes.len() > MAX_NACK_FRAGMENTS {
        return None;
    }
    let mut body = Vec::with_capacity(6 + 2 * fragment_indexes.len());
    body.extend_from_slice(COMMAND_NAK);
    body.extend_from_slice(&au_id.to_le_bytes());
    body.push(fragment_indexes.len() as u8);
    for index in fragment_indexes {
        body.extend_from_slice(&index.to_le_bytes());
    }
    Some(body)
}

/// Decode one NACK request body. Returns `(au_id, fragment_indexes)`; `None`
/// for any malformed or oversized body (hosts must skip silently).
pub fn parse_nack_request(body: &[u8]) -> Option<(u16, Vec<u16>)> {
    if body.len() < 6 || &body[..3] != COMMAND_NAK {
        return None;
    }
    let au_id = u16::from_le_bytes(body[3..5].try_into().ok()?);
    let count = usize::from(body[5]);
    if count == 0 || count > MAX_NACK_FRAGMENTS || body.len() < 6 + 2 * count {
        return None;
    }
    let indexes = (0..count)
        .map(|index| u16::from_le_bytes(body[6 + 2 * index..8 + 2 * index].try_into().unwrap()))
        .collect();
    Some((au_id, indexes))
}

/// One viewer retransmit request slot. Single attempt per access unit: when
/// the first grace expires the pre-existing loss path (freeze + gated IDR)
/// runs unchanged, and a second NACK would only lose the race against that
/// IDR. `attempted` outlives `armed` so the same hole is never re-requested.
#[derive(Debug, Default)]
pub struct NackRequester {
    attempted: Option<u16>,
    armed: Option<(u16, Instant)>,
    fragments_requested: usize,
}

impl NackRequester {
    pub fn fragments_requested(&self) -> usize {
        self.fragments_requested
    }
}

/// What one [`tick_nack_requester`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NackTickOutcome {
    pub au_id: Option<u16>,
    pub sent_messages: u64,
    /// The armed access unit completed inside the grace window: the freeze
    /// (and IDR) this loss would have cost was avoided.
    pub healed: bool,
}

/// Viewer-side selective-retransmit tick, shared by the single-session
/// renderer and the split tile workers. Call every loop iteration (workers
/// wake at least every 2ms).
///
/// Detection point: [`CompletedFrameSequencer::blocked_hole`] — the one AU
/// the sequencer is waiting on while later completed AUs queue behind it.
/// Only that hole is still healable; once expiry delivers past it the
/// presentation layer classifies the loss exactly as before.
pub fn tick_nack_requester(
    requester: &mut NackRequester,
    sequencer: &mut CompletedFrameSequencer,
    reassembler: &FrameReassembler,
    awaiting_keyframe: bool,
    network_rtt_ms: Option<u64>,
    now: Instant,
    mut send: impl FnMut(&[u8]),
) -> NackTickOutcome {
    let mut outcome = NackTickOutcome::default();

    // 1. Settle an armed request first: a completion inside the grace window
    // is a heal; the deadline passing hands the hole back to the unchanged
    // loss classification.
    if let Some((au_id, deadline)) = requester.armed {
        if reassembler.recently_completed_contains(au_id) {
            outcome = NackTickOutcome {
                au_id: Some(au_id),
                sent_messages: 0,
                healed: true,
            };
            requester.armed = None;
        } else if now >= deadline {
            requester.armed = None;
        }
    }

    // 2. Nothing healable right now (or a recovery is already active and a
    // healed hole would not resume feeding): release the hold and state.
    let Some(hole) = sequencer.blocked_hole() else {
        sequencer.clear_hold();
        requester.attempted = None;
        requester.armed = None;
        return outcome;
    };
    if awaiting_keyframe {
        sequencer.clear_hold();
        requester.attempted = None;
        requester.armed = None;
        return outcome;
    }

    // 3. Keep the existing hold alive for the hole already asked for. The
    // stored deadline is absolute, so re-arming cannot creep it forward.
    if let Some((armed_hole, deadline)) = requester.armed {
        if armed_hole == hole {
            sequencer.hold_reorder_expiry_until(deadline);
            return outcome;
        }
    }

    // 4. Single attempt per access unit (see NackRequester).
    if requester.attempted == Some(hole) {
        sequencer.clear_hold();
        return outcome;
    }
    requester.attempted = None;

    // 5. Arm a new request for the fragments the reassembler is still owed.
    let Some(missing) = reassembler.missing_fragment_indexes(hole) else {
        // Nothing retransmittable is known: the AU never started or was
        // evicted. The existing loss path handles it.
        sequencer.clear_hold();
        return outcome;
    };
    requester.attempted = Some(hole);
    if missing.is_empty() || missing.len() > MAX_NACK_REQUEST_TOTAL {
        sequencer.clear_hold();
        return outcome;
    }
    let deadline = sequencer
        .nack_reservation
        .filter(|(id, _)| *id == hole)
        .map_or(
            now + nack_grace_duration(network_rtt_ms),
            |(_, deadline)| deadline,
        );
    if now >= deadline {
        sequencer.clear_hold();
        return outcome;
    }
    let mut sent = 0u64;
    for chunk in missing.chunks(MAX_NACK_FRAGMENTS) {
        if let Some(body) = encode_nack_request(hole, chunk) {
            send(&body);
            sent += 1;
        }
    }
    requester.armed = Some((hole, deadline));
    requester.fragments_requested = missing.len();
    sequencer.hold_reorder_expiry_until(deadline);
    NackTickOutcome {
        au_id: Some(hole),
        sent_messages: sent,
        healed: false,
    }
}

#[derive(Debug)]
pub struct RecoveryRequestGate {
    last_request: Option<Instant>,
    awaiting_keyframe: bool,
    cooldown: Duration,
}
impl Default for RecoveryRequestGate {
    fn default() -> Self {
        Self {
            last_request: None,
            awaiting_keyframe: false,
            cooldown: RECOVERY_REQUEST_COOLDOWN,
        }
    }
}

impl RecoveryRequestGate {
    pub fn should_request(&mut self, now: Instant) -> bool {
        let cooldown_elapsed = self
            .last_request
            .is_none_or(|last| now.saturating_duration_since(last) >= self.cooldown);
        if self.awaiting_keyframe && !cooldown_elapsed {
            return false;
        }
        if !cooldown_elapsed {
            return false;
        }
        self.last_request = Some(now);
        self.awaiting_keyframe = true;
        true
    }

    pub fn recovered(&mut self) {
        self.awaiting_keyframe = false;
    }

    #[cfg(test)]
    fn with_cooldown(cooldown: Duration) -> Self {
        Self {
            cooldown,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_media_datagram_budget_stays_below_ethernet_mtu() {
        assert_eq!(MAX_DATAGRAM_BYTES, 1_400);
        assert_eq!(MAX_FRAGMENT_PAYLOAD, 1_367);
        const { assert!(MAX_DATAGRAM_BYTES + 28 <= 1_500) };
    }

    fn legacy_datagram(index: u16, count: u16, id: u16, wall: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![FRAME_MARKER];
        bytes.extend_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(b"LT");
        bytes.extend_from_slice(&wall.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn datagram(
        index: u16,
        count: u16,
        id: u16,
        capture: u64,
        encode: u64,
        send: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![FRAME_MARKER];
        bytes.extend_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(b"L2");
        bytes.extend_from_slice(&capture.to_be_bytes());
        bytes.extend_from_slice(&encode.to_be_bytes());
        bytes.extend_from_slice(&send.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn parity_datagram(
        id: u16,
        k: u8,
        index: u8,
        base: u16,
        total: u16,
        wall: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![PARITY_MARKER];
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.push(k);
        bytes.push(index);
        bytes.extend_from_slice(&base.to_be_bytes());
        bytes.extend_from_slice(&total.to_be_bytes());
        bytes.extend_from_slice(b"LT");
        bytes.extend_from_slice(&wall.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn fec_fixed_wire_vector_preserves_length_prefix_and_short_tail() {
        // First parity row is XOR: [0,1,10,0] ^ [0,2,20,21].
        // Literal bytes, independent of the encoder used by production.
        let parity = [0, 3, 30, 21];
        let encoded = fec_core::encode_group(&[vec![10], vec![20, 21]]).unwrap();
        assert_eq!(encoded.data, vec![vec![0, 1, 10, 0], vec![0, 2, 20, 21]]);
        assert_eq!(encoded.parity, vec![parity.to_vec()]);
        let mut group = FecGroup::new(0x0102, 2, 0, 2).unwrap();
        group.push_data(FrameFragment {
            index: 0,
            count: 2,
            id: 0x0102,
            capture_wall_ms: Some(1),
            encode_wall_ms: Some(2),
            send_wall_ms: 3,
            payload: &[10],
        });
        group.push_parity(ParityFragment {
            id: 0x0102,
            k: 2,
            index: 0,
            base: 0,
            total: 2,
            send_wall_ms: 3,
            payload: &parity,
        });
        let restored = group.try_restore_result().unwrap().unwrap();
        assert_eq!(
            restored,
            vec![RestoredFragment {
                index: 1,
                count: 2,
                id: 0x0102,
                capture_wall_ms: Some(1),
                encode_wall_ms: Some(2),
                send_wall_ms: 3,
                payload: vec![20, 21]
            }]
        );
    }

    #[test]
    fn fec_rejects_invalid_parity_index_width_and_duplicate_before_retaining() {
        let mut group = FecGroup::new(1, 8, 0, 8).unwrap();
        let parity = |index, payload| ParityFragment {
            id: 1,
            k: 8,
            index,
            base: 0,
            total: 8,
            send_wall_ms: 0,
            payload,
        };
        group.push_parity(parity(255, &[0; 4]));
        group.push_parity(parity(0, &[0]));
        assert_eq!(group.width, None);
        group.push_parity(parity(0, &[0; 4]));
        group.push_parity(parity(0, &[9; 6]));
        group.push_parity(parity(1, &[9; 6]));
        assert_eq!(group.width, Some(4));
        assert_eq!(group.parity.iter().flatten().count(), 1);
        assert_eq!(group.parity[0].as_deref(), Some(&[0; 4][..]));
        assert!(group.try_restore_result().unwrap().is_none());
    }

    #[test]
    fn fec_counts_only_unique_packable_data_and_full_width_parity() {
        let mut group = FecGroup::new(1, 2, 0, 2).unwrap();
        let fragment = FrameFragment {
            index: 0,
            count: 2,
            id: 1,
            capture_wall_ms: None,
            encode_wall_ms: None,
            send_wall_ms: 0,
            payload: &[1, 2, 3],
        };
        group.push_data(fragment);
        group.push_data(fragment);
        group.push_parity(ParityFragment {
            id: 1,
            k: 2,
            index: 0,
            base: 0,
            total: 2,
            send_wall_ms: 0,
            payload: &[0; 4],
        });
        assert!(group.try_restore_result().unwrap().is_none());
    }

    #[test]
    fn parity_restores_one_lost_fragment_before_reassembly() {
        let payloads = (0..8).map(|i| vec![i as u8; 64]).collect::<Vec<_>>();
        let encoded = fec_core::encode_group(&payloads).unwrap();
        let mut group = FecGroup::new(77, 8, 0, 8).unwrap();
        for (index, payload) in payloads.iter().enumerate() {
            if index != 3 {
                group.push_data(
                    parse_fragment(&datagram(index as u16, 8, 77, 1, 2, 3, payload)).unwrap(),
                );
            }
        }
        for (index, parity) in encoded.parity.iter().enumerate() {
            group.push_parity(
                parse_parity(&parity_datagram(77, 8, index as u8, 0, 8, 3, parity)).unwrap(),
            );
        }
        let restored = group.try_restore().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].index, 3);
        assert_eq!(restored[0].payload, payloads[3]);
    }

    #[test]
    fn four_parity_datagrams_restore_four_lost_fragments() {
        let payloads = (0..8).map(|i| vec![i as u8; 64]).collect::<Vec<_>>();
        let encoded = fec_core::encode_group_with_parity(&payloads, 4).unwrap();
        let mut group = FecGroup::new(79, 8, 0, 8).unwrap();
        for (index, payload) in payloads.iter().enumerate() {
            if ![0, 2, 5, 7].contains(&index) {
                group.push_data(
                    parse_fragment(&datagram(index as u16, 8, 79, 1, 2, 3, payload)).unwrap(),
                );
            }
        }
        for (index, parity) in encoded.parity.iter().enumerate() {
            let datagram = parity_datagram(79, 8, index as u8, 0, 8, 3, parity);
            let parsed =
                parse_parity(&datagram).expect("new receiver accepts parity indexes 0 through 3");
            group.push_parity(parsed);
        }
        let restored = group.try_restore_result().unwrap().unwrap();
        assert_eq!(restored.len(), 4);
        assert!(group.is_complete());
    }

    #[test]
    fn four_k_recovery_window_retains_every_group_in_the_observed_maximum_idr() {
        // Physical 4K60 split validation reached 317 aggregate fragments,
        // or at most 40 RS(8, 12) groups on one tile. The receiver must not
        // evict an early group merely because its parity arrived late.
        const { assert!(MAX_ACTIVE_FEC_GROUPS >= 40) };
    }

    #[test]
    fn parity_first_arrival_retries_after_late_data() {
        let payloads = (0..7).map(|i| vec![i as u8; 64]).collect::<Vec<_>>();
        let encoded = fec_core::encode_group(&payloads).unwrap();
        let mut group = FecGroup::new(78, 7, 0, 7).unwrap();

        for (index, parity) in encoded.parity.iter().enumerate() {
            assert!(group
                .push_parity_and_restore(
                    parse_parity(&parity_datagram(78, 7, index as u8, 0, 7, 3, parity)).unwrap()
                )
                .is_none());
        }
        for (index, payload) in payloads.iter().enumerate() {
            if index == 3 {
                continue;
            }
            let restored = group.push_data_and_restore(
                parse_fragment(&datagram(index as u16, 7, 78, 1, 2, 3, payload)).unwrap(),
            );
            if index == 6 {
                let restored = restored.expect("late data must trigger FEC retry");
                assert_eq!(restored.len(), 1);
                assert_eq!(restored[0].index, 3);
                assert_eq!(restored[0].payload, payloads[3]);
            } else {
                assert!(restored.is_none());
            }
        }
    }

    /// Parity supplying the shard width before the data must not make the
    /// completing data fragment pay an identity-matrix decode: a group whose
    /// data all arrived has nothing to restore. (Earlier fragments may still
    /// legitimately recover the tail from parity — that is normal FEC.)
    #[test]
    fn completing_data_after_parity_skips_the_identity_decode() {
        let payloads = (0..8).map(|i| vec![i as u8; 64]).collect::<Vec<_>>();
        let encoded = fec_core::encode_group(&payloads).unwrap();
        let mut group = FecGroup::new(80, 8, 0, 8).unwrap();
        group.push_parity(
            parse_parity(&parity_datagram(80, 8, 0, 0, 8, 3, &encoded.parity[0])).unwrap(),
        );
        let mut last_restored = None;
        for (index, payload) in payloads.iter().enumerate() {
            last_restored = group.push_data_and_restore(
                parse_fragment(&datagram(index as u16, 8, 80, 1, 2, 3, payload)).unwrap(),
            );
            if let Some(restored) = last_restored.as_ref() {
                for fragment in restored {
                    assert_eq!(usize::from(fragment.index), index + 1);
                }
            }
        }
        // The final fragment completes the group in order: nothing may be
        // reported as restored, and every slot is present.
        assert!(last_restored.is_none(), "complete data never restores");
        assert!(group.is_complete());
    }

    #[test]
    fn parity_parser_rejects_unknown_group_shape() {
        let packet = parity_datagram(77, 8, 0, 8, 8, 3, &[1; 64]);
        assert!(parse_parity(&packet).is_none());
    }

    #[test]
    fn completed_fec_groups_suppress_late_parity_without_growing_forever() {
        let mut completed = CompletedFecGroups::default();
        completed.remember((7, 0));
        completed.remember((7, 0));
        assert!(completed.contains(&(7, 0)));
        assert_eq!(completed.order.len(), 1);

        for id in 8..=(8 + CompletedFecGroups::CAPACITY as u16) {
            completed.remember((id, 0));
        }
        assert!(!completed.contains(&(7, 0)));
        assert_eq!(completed.order.len(), CompletedFecGroups::CAPACITY);

        completed.clear();
        assert!(!completed.contains(&(72, 0)));
    }

    #[test]
    fn recovery_discards_partial_fec_state_without_classifying_network_loss() {
        let mut groups = HashMap::new();
        let mut order = VecDeque::new();
        let key = (7, 0);
        groups.insert(key, FecGroup::new(7, 8, 0, 8).unwrap());
        order.push_back(key);

        discard_fec_groups(&mut groups, &mut order);

        assert!(groups.is_empty());
        assert!(order.is_empty());
    }

    #[test]
    fn parses_and_reassembles_out_of_order_fragments() {
        let mut reassembler = FrameReassembler::default();
        let second_bytes = datagram(1, 2, 42, 1_200, 1_220, 1_225, b"world");
        let first_bytes = datagram(0, 2, 42, 1_200, 1_220, 1_225, b"hello ");
        let second = parse_fragment(&second_bytes).unwrap();
        let first = parse_fragment(&first_bytes).unwrap();
        assert!(reassembler.push(second).is_none());
        assert_eq!(
            reassembler.push(first),
            Some(ReassembledFrame {
                id: 42,
                capture_wall_ms: Some(1_200),
                encode_wall_ms: Some(1_220),
                send_wall_ms: 1_225,
                au: b"hello world".to_vec(),
            })
        );
    }

    #[test]
    fn duplicate_fragment_does_not_finish_early() {
        let mut reassembler = FrameReassembler::default();
        let first_bytes = datagram(0, 2, 7, 1, 2, 3, b"a");
        let second_bytes = datagram(1, 2, 7, 1, 2, 3, b"b");
        let first = parse_fragment(&first_bytes).unwrap();
        assert!(reassembler.push(first).is_none());
        assert!(reassembler.push(first).is_none());
        let second = parse_fragment(&second_bytes).unwrap();
        assert_eq!(reassembler.push(second).unwrap().au, b"ab");
    }

    #[test]
    fn rejects_malformed_or_oversized_fragment_headers() {
        assert!(parse_fragment(b"short").is_none());
        assert!(parse_fragment(&datagram(2, 2, 1, 1, 2, 3, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, 0, 1, 1, 2, 3, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, u16::MAX, 1, 1, 2, 3, b"x")).is_none());
    }

    #[test]
    fn accepts_legacy_send_only_timestamp() {
        let bytes = legacy_datagram(0, 1, 9, 7_777, b"frame");
        let fragment = parse_fragment(&bytes).unwrap();
        assert_eq!(fragment.capture_wall_ms, None);
        assert_eq!(fragment.encode_wall_ms, None);
        assert_eq!(fragment.send_wall_ms, 7_777);
        assert_eq!(fragment.payload, b"frame");
    }

    #[test]
    fn counts_incomplete_access_units_evicted_by_reordering_pressure() {
        let mut reassembler = FrameReassembler::default();
        for id in 1..=5 {
            let bytes = datagram(0, 2, id, 1, 2, 3, b"partial");
            let fragment = parse_fragment(&bytes).unwrap();
            assert!(reassembler.push(fragment).is_none());
        }
        assert_eq!(reassembler.incomplete_evictions(), 1);
    }

    #[test]
    fn stale_budget_allows_half_an_rtt_but_remains_bounded() {
        assert_eq!(stale_frame_budget_ms(None), 80);
        assert_eq!(stale_frame_budget_ms(Some(20)), 90);
        assert_eq!(stale_frame_budget_ms(Some(400)), 200);
    }

    #[test]
    fn contiguous_hot_path_reuses_one_au_slab() {
        let mut reassembler = FrameReassembler::default();
        let full = vec![7; MAX_FRAGMENT_PAYLOAD];
        let first_bytes = datagram(0, 2, 77, 1, 2, 3, &full);
        let second_bytes = datagram(1, 2, 77, 1, 2, 3, b"tail");
        assert!(reassembler
            .push(parse_fragment(&first_bytes).unwrap())
            .is_none());
        let frame = reassembler
            .push(parse_fragment(&second_bytes).unwrap())
            .unwrap();
        assert_eq!(frame.au.len(), MAX_FRAGMENT_PAYLOAD + 4);
        assert_eq!(&frame.au[MAX_FRAGMENT_PAYLOAD..], b"tail");
    }

    #[test]
    fn recovery_gate_coalesces_requests_until_keyframe_or_cooldown() {
        let mut gate = RecoveryRequestGate::with_cooldown(Duration::from_millis(100));
        let now = Instant::now();
        assert!(gate.should_request(now));
        assert!(!gate.should_request(now + Duration::from_millis(99)));
        assert!(gate.should_request(now + Duration::from_millis(100)));
        gate.recovered();
        assert!(!gate.should_request(now + Duration::from_millis(150)));
        assert!(gate.should_request(now + Duration::from_millis(200)));
    }

    #[test]
    fn recovery_cooldown_allows_four_requests_per_second() {
        assert_eq!(RECOVERY_REQUEST_COOLDOWN, Duration::from_millis(250));
    }

    #[test]
    fn single_stale_frame_renders_late_without_resync() {
        assert_eq!(stale_streak_advance(0, false, true), (1, false));
        assert_eq!(stale_streak_advance(1, false, true), (2, false));
    }

    #[test]
    fn stale_frames_do_not_break_the_decoder_reference_chain() {
        assert_eq!(stale_streak_advance(2, false, true), (3, false));
    }

    #[test]
    fn rendered_feedback_rate_uses_elapsed_interval() {
        assert_eq!(rendered_fps_from_feedback(30, 0, 500), 60);
        assert_eq!(rendered_fps_from_feedback(31, 30, 1_000), 1);
        assert_eq!(rendered_fps_from_feedback(30, 0, 1_000), 30);
        assert_eq!(rendered_fps_from_feedback(30, 0, 0), 0);
    }

    #[test]
    fn fresh_frame_resets_the_streak() {
        assert_eq!(stale_streak_advance(2, false, false), (0, false));
    }

    #[test]
    fn keyframe_resets_the_streak_and_never_resyncs() {
        assert_eq!(stale_streak_advance(2, true, true), (0, false));
    }

    #[test]
    fn live_edge_selection_keeps_all_frames_when_batch_is_small() {
        let frames = vec![10u16, 11u16];
        let selection = select_live_edge_frames(frames);
        assert_eq!(selection.frames.as_slice(), [10, 11]);
        assert_eq!(selection.discarded, 0);
    }

    #[test]
    fn live_edge_selection_preserves_a_large_batch_for_decoder_references() {
        let frames = (10..20).collect::<Vec<_>>();
        let selection = select_live_edge_frames(frames);
        assert_eq!(
            selection.frames.as_slice(),
            [10, 11, 12, 13, 14, 15, 16, 17, 18, 19]
        );
        assert_eq!(selection.discarded, 0);
    }

    #[test]
    fn receiver_pressure_separates_live_edge_and_decoder_discards() {
        let mut pressure = ReceiverPressure::default();
        pressure.record_live_edge_discards(1);
        pressure.record_decoder_input_drop();
        pressure.record_decoder_input_drop();
        pressure.record_decoder_output_discards(3);
        pressure.record_fec_recovery(2);

        assert_eq!(pressure.live_edge_discards, 1);
        assert_eq!(pressure.decoder_input_drops, 2);
        assert_eq!(pressure.decoder_output_discards, 3);
        assert_eq!(pressure.fec_recovered_fragments, 2);
    }

    #[test]
    fn resize_recovery_waits_until_geometry_is_stable() {
        assert!(recovery_request_suppressed(1_000, 351_000));
        assert!(!recovery_request_suppressed(351_000, 351_000));
    }

    #[test]
    fn ignores_redundant_copy_after_access_unit_completed() {
        let first = datagram(0, 2, 42, 100, 110, 120, b"a");
        let second = datagram(1, 2, 42, 100, 110, 120, b"b");
        let mut reassembler = FrameReassembler::default();

        assert!(reassembler.push(parse_fragment(&first).unwrap()).is_none());
        assert_eq!(
            reassembler
                .push(parse_fragment(&second).unwrap())
                .unwrap()
                .au,
            b"ab"
        );
        assert!(reassembler.push(parse_fragment(&first).unwrap()).is_none());
        assert!(reassembler.push(parse_fragment(&second).unwrap()).is_none());
        assert_eq!(reassembler.incomplete_evictions(), 0);
    }

    #[test]
    fn sequences_short_cross_access_unit_reordering() {
        fn frame(id: u16) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![id as u8],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(9);
        assert_eq!(sequencer.push_at(frame(10), start)[0].id, 10);
        assert!(sequencer
            .push_at(frame(12), start + Duration::from_millis(1))
            .is_empty());
        let ready = sequencer.push_at(frame(11), start + Duration::from_millis(2));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [11, 12]
        );
    }

    #[test]
    fn holds_short_four_k_fec_reordering_within_one_frame() {
        fn frame(id: u16) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![id as u8],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(9);
        assert_eq!(sequencer.push_at(frame(10), start)[0].id, 10);
        assert!(sequencer
            .push_at(frame(12), start + Duration::from_millis(1))
            .is_empty());
        assert!(sequencer
            .push_at(frame(13), start + Duration::from_millis(6))
            .is_empty());
        let ready = sequencer.push_at(frame(11), start + Duration::from_millis(7));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [11, 12, 13]
        );
    }

    #[test]
    fn skips_a_real_gap_without_growing_a_playback_queue() {
        fn frame(id: u16) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![id as u8],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(19);
        assert_eq!(sequencer.push_at(frame(20), start)[0].id, 20);
        assert!(sequencer
            .push_at(frame(22), start + Duration::from_millis(1))
            .is_empty());
        let ready = sequencer.push_at(frame(23), start + Duration::from_millis(10));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [22, 23]
        );
    }

    #[test]
    fn expires_a_lone_completed_frame_without_another_push() {
        fn frame(id: u16) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![0, 0, 0, 1, 0x41],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(40);
        assert!(sequencer
            .push_at(frame(42), start + Duration::from_millis(1))
            .is_empty());
        let ready = sequencer.drain_expired_at(start + Duration::from_millis(10));
        assert_eq!(ready.iter().map(|frame| frame.id).collect::<Vec<_>>(), [42]);
    }

    #[test]
    fn recovery_idr_arriving_after_delta_starts_the_new_epoch() {
        fn frame(id: u16, nal: u8) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![0, 0, 0, 1, nal],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        assert!(sequencer.push_at(frame(49, 0x41), start).is_empty());
        assert!(sequencer
            .push_at(frame(51, 0x41), start + Duration::from_millis(1))
            .is_empty());
        assert!(sequencer
            .drain_expired_at(start + Duration::from_millis(10))
            .is_empty());
        let ready = sequencer.push_at(frame(50, 0x65), start + Duration::from_millis(12));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [50, 51]
        );
        assert!(sequencer
            .drain_expired_at(start + Duration::from_millis(20))
            .is_empty());
    }

    #[test]
    fn cleared_epoch_bounds_thousands_of_deltas_while_waiting_for_idr() {
        fn delta(id: u16) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![0, 0, 0, 1, 0x41],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        for id in 1..=1_000 {
            assert!(sequencer
                .push_at(delta(id), start + Duration::from_micros(u64::from(id)))
                .is_empty());
            assert!(sequencer.pending.len() <= MAX_COMPLETED_REORDER);
        }
        assert_eq!(sequencer.pending.len(), MAX_COMPLETED_REORDER);
    }

    #[test]
    fn late_recovery_idr_discards_noncontiguous_deltas_across_wrap() {
        fn frame(id: u16, nal: u8) -> ReassembledFrame {
            ReassembledFrame {
                id,
                capture_wall_ms: None,
                encode_wall_ms: None,
                send_wall_ms: 0,
                au: vec![0, 0, 0, 1, nal],
            }
        }

        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        for (offset, id) in [8, 9, 10, u16::MAX, 0].into_iter().enumerate() {
            assert!(sequencer
                .push_at(
                    frame(id, 0x41),
                    start + Duration::from_micros(offset as u64)
                )
                .is_empty());
        }
        let ready = sequencer.push_at(frame(u16::MAX - 1, 0x65), start);
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [u16::MAX - 1, u16::MAX, 0]
        );
        assert!(sequencer.pending.is_empty());
    }

    #[test]
    fn random_access_detection_is_codec_specific() {
        let h264_pps_with_nri_one = [0, 0, 0, 1, 0x28, 0x01];
        let h264_idr = [0, 0, 0, 1, 0x65, 0x01];
        let hevc_idr = [0, 0, 0, 1, 0x28, 0x01];
        assert!(!is_keyframe(
            &h264_pps_with_nri_one,
            viewer_decoder::VideoCodec::H264
        ));
        assert!(is_keyframe(&h264_idr, viewer_decoder::VideoCodec::H264));
        assert!(is_keyframe(&hevc_idr, viewer_decoder::VideoCodec::Hevc));
        assert!(!is_keyframe(&h264_idr, viewer_decoder::VideoCodec::Hevc));
    }

    #[test]
    fn live_edge_skip_is_not_reported_as_network_loss() {
        assert_eq!(
            classify_frame_gap(Some(10), 20, true, false),
            FrameGapReason::LiveEdgeDiscard { missing: 9 }
        );
    }

    #[test]
    fn real_gap_is_reported_as_network_loss() {
        assert_eq!(
            classify_frame_gap(Some(10), 20, false, false),
            FrameGapReason::NetworkLoss { missing: 9 }
        );
    }

    #[test]
    fn frames_skipped_while_waiting_for_keyframe_are_not_new_loss() {
        assert_eq!(
            classify_frame_gap(Some(10), 20, false, true),
            FrameGapReason::RecoverySkip { missing: 9 }
        );
    }

    #[test]
    fn first_frame_has_no_gap_reason() {
        assert_eq!(
            classify_frame_gap(None, 20, true, false),
            FrameGapReason::None
        );
    }

    #[test]
    fn live_edge_delta_is_not_fed_after_reference_chain_collapse() {
        assert!(!should_feed_frame(
            FrameGapReason::LiveEdgeDiscard { missing: 9 },
            false
        ));
        assert!(should_feed_frame(
            FrameGapReason::LiveEdgeDiscard { missing: 9 },
            true
        ));
    }

    #[test]
    fn one_real_missing_delta_freezes_and_requests_recovery() {
        assert!(!should_feed_frame(
            FrameGapReason::NetworkLoss { missing: 1 },
            false
        ));
        assert!(!should_feed_frame(
            FrameGapReason::NetworkLoss { missing: 2 },
            false
        ));
        assert!(should_freeze_after_network_loss(1, false));
        assert!(should_freeze_after_network_loss(2, false));
        assert!(!should_freeze_after_network_loss(99, true));
    }

    /// Q8: the single-session gap policy is the split renderer's no-flush
    /// freeze. A missing delta is never fed, skips while frozen classify as
    /// recovery (not new loss), and the recovery keyframe resumes feeding and
    /// ends the freeze — without any decoder flush in the gap path itself.
    #[test]
    fn gap_freezes_without_flush_and_resumes_on_keyframe() {
        // The gap itself: freeze and request IDR (never feed the damaged delta).
        assert!(should_freeze_after_network_loss(1, false));
        assert!(!should_feed_frame(
            FrameGapReason::NetworkLoss { missing: 1 },
            false
        ));
        // While frozen, subsequent deltas are recovery skips, not new losses.
        assert_eq!(
            classify_frame_gap(Some(10), 12, false, true),
            FrameGapReason::RecoverySkip { missing: 1 }
        );
        assert!(!should_feed_frame(
            FrameGapReason::RecoverySkip { missing: 1 },
            false
        ));
        // The recovery keyframe is always fed and clears the freeze.
        assert!(should_feed_frame(
            FrameGapReason::RecoverySkip { missing: 9 },
            true
        ));
        assert!(!should_freeze_after_network_loss(9, true));
    }

    #[test]
    fn nack_grace_clamps_rtt_to_the_reorder_floor_and_bounded_ceiling() {
        assert_eq!(nack_grace_duration(None), Duration::from_millis(8));
        assert_eq!(nack_grace_duration(Some(0)), Duration::from_millis(8));
        assert_eq!(nack_grace_duration(Some(2)), Duration::from_millis(9));
        assert_eq!(nack_grace_duration(Some(5)), Duration::from_millis(15));
        assert_eq!(nack_grace_duration(Some(100)), Duration::from_millis(25));
        assert_eq!(
            nack_grace_duration(Some(u64::MAX)),
            Duration::from_millis(25)
        );
    }

    #[test]
    fn nack_request_bodies_round_trip_and_reject_malformed() {
        let body = encode_nack_request(0x1234, &[7, 9, 11]).unwrap();
        assert_eq!(&body[..3], b"NAK");
        assert_eq!(parse_nack_request(&body), Some((0x1234, vec![7, 9, 11])));
        assert!(encode_nack_request(1, &[]).is_none());
        assert!(encode_nack_request(1, &[0u16; MAX_NACK_FRAGMENTS + 1]).is_none());
        assert!(parse_nack_request(b"NAK").is_none());
        assert!(parse_nack_request(b"NAX0000").is_none());
        // count field claims more fragments than the body carries.
        let mut short = encode_nack_request(5, &[1, 2]).unwrap();
        short.truncate(6 + 2);
        assert!(parse_nack_request(&short).is_none());
        // count over the wire cap is rejected even with a long enough body.
        let mut oversized = encode_nack_request(5, &[1]).unwrap();
        oversized[5] = (MAX_NACK_FRAGMENTS + 1) as u8;
        assert!(parse_nack_request(&oversized).is_none());
    }

    /// Shared NACK-tick fixture: AU 10 delivered, AU 11 partial (fragment 0
    /// of 2 missing), AU 12 completed and queued behind the hole.
    struct NackFixture {
        reassembler: FrameReassembler,
        sequencer: CompletedFrameSequencer,
        start: Instant,
    }

    impl NackFixture {
        fn new() -> Self {
            let mut reassembler = FrameReassembler::default();
            assert!(reassembler
                .push(parse_fragment(&datagram(1, 2, 11, 1, 2, 3, b"b")).unwrap())
                .is_none());
            let mut sequencer = CompletedFrameSequencer::default();
            sequencer.establish_epoch(10);
            let start = Instant::now();
            let frame12 = reassembler
                .push(parse_fragment(&datagram(0, 1, 12, 1, 2, 3, b"c")).unwrap())
                .unwrap();
            assert!(sequencer.push_at(frame12, start).is_empty());
            assert_eq!(sequencer.blocked_hole(), Some(11));
            Self {
                reassembler,
                sequencer,
                start,
            }
        }

        fn missing(&self) -> Vec<u16> {
            self.reassembler.missing_fragment_indexes(11).unwrap()
        }
    }

    #[test]
    fn nack_completion_boundary_holds_one_rx_batch_and_repaired_order() {
        let start = Instant::now();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(10);
        sequencer.configure_nack_grace(true, Some(3));
        let mut reassembler = FrameReassembler::default();
        assert!(reassembler
            .push(parse_fragment(&datagram(1, 2, 11, 1, 2, 3, b"b")).unwrap())
            .is_none());
        // No requester tick between these completions: exactly one receive batch.
        for id in [12, 13, 14] {
            let frame = reassembler
                .push(parse_fragment(&datagram(0, 1, id, 1, 2, 3, b"x")).unwrap())
                .unwrap();
            assert!(
                sequencer
                    .push_reassembled_at(frame, &reassembler, start)
                    .is_empty(),
                "AU {id} escaped before NACK had a chance to run"
            );
        }
        let mut requester = NackRequester::default();
        let mut sent = Vec::new();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut sequencer,
            &reassembler,
            false,
            Some(3),
            start + Duration::from_millis(1),
            |body| sent.push(body.to_vec()),
        );
        assert_eq!(outcome.sent_messages, 1);
        assert_eq!(sent, vec![encode_nack_request(11, &[0]).unwrap()]);
        let repaired = reassembler
            .push(parse_fragment(&datagram(0, 2, 11, 1, 2, 3, b"a")).unwrap())
            .unwrap();
        let ready =
            sequencer.push_reassembled_at(repaired, &reassembler, start + Duration::from_millis(9));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            vec![11, 12, 13, 14]
        );
    }

    fn push_nack_test_au(
        sequencer: &mut CompletedFrameSequencer,
        reassembler: &mut FrameReassembler,
        id: u16,
        now: Instant,
    ) -> Vec<ReassembledFrame> {
        let Some(frame) =
            reassembler.push(parse_fragment(&datagram(0, 1, id, 1, 2, 3, b"x")).unwrap())
        else {
            return Vec::new();
        };
        sequencer.push_reassembled_at(frame, reassembler, now)
    }

    #[test]
    fn nack_completion_boundary_duplicate_reorder_keeps_original_deadline() {
        let mut fixture = NackFixture::new();
        // The fixture has AU12 queued, but no tick or completion reservation yet.
        fixture.sequencer.configure_nack_grace(true, Some(3));
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            14,
            fixture.start
        )
        .is_empty());
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            13,
            fixture.start + Duration::from_millis(1)
        )
        .is_empty());
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            13,
            fixture.start + Duration::from_millis(9)
        )
        .is_empty());
        // A later RTT update and request tick must not give this hole a new 25ms.
        fixture.sequencer.configure_nack_grace(true, Some(100));
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(100),
            fixture.start + Duration::from_millis(9),
            |_| {},
        );
        assert_eq!(outcome.sent_messages, 1);
        assert!(fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(10))
            .is_empty());
        let ready = fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(11));
        assert_eq!(
            ready.iter().map(|f| f.id).collect::<Vec<_>>(),
            vec![12, 13, 14]
        );
        let late = fixture
            .reassembler
            .push(parse_fragment(&datagram(0, 2, 11, 1, 2, 3, b"a")).unwrap())
            .unwrap();
        assert!(fixture
            .sequencer
            .push_reassembled_at(
                late,
                &fixture.reassembler,
                fixture.start + Duration::from_millis(12)
            )
            .is_empty());
    }

    #[test]
    fn nack_completion_boundary_hard_cap_without_any_tick() {
        let mut fixture = NackFixture::new();
        fixture.sequencer.configure_nack_grace(true, Some(100));
        for id in [13, 14, 15, 16] {
            assert!(push_nack_test_au(
                &mut fixture.sequencer,
                &mut fixture.reassembler,
                id,
                fixture.start
            )
            .is_empty());
        }
        let ready = push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            17,
            fixture.start,
        );
        assert_eq!(
            ready.iter().map(|f| f.id).collect::<Vec<_>>(),
            vec![12, 13, 14, 15, 16, 17]
        );
    }

    #[test]
    fn nack_completion_boundary_expired_reservation_cannot_be_rearmed() {
        let mut fixture = NackFixture::new();
        fixture.sequencer.configure_nack_grace(true, None);
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            13,
            fixture.start
        )
        .is_empty());
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(100),
            fixture.start + Duration::from_millis(8),
            |_| panic!("expired hole must not send a fresh NACK"),
        );
        assert_eq!(outcome.sent_messages, 0);
        let ready = push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            14,
            fixture.start + Duration::from_millis(9),
        );
        assert_eq!(
            ready.iter().map(|f| f.id).collect::<Vec<_>>(),
            vec![12, 13, 14]
        );
    }

    #[test]
    fn nack_completion_boundary_recovery_and_idr_reset_the_old_hold() {
        let mut fixture = NackFixture::new();
        fixture.sequencer.configure_nack_grace(true, Some(100));
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            13,
            fixture.start
        )
        .is_empty());
        fixture.sequencer.configure_nack_grace(false, Some(100));
        let ready = push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            14,
            fixture.start,
        );
        assert_eq!(
            ready.iter().map(|f| f.id).collect::<Vec<_>>(),
            vec![12, 13, 14]
        );
        fixture.sequencer.clear();
        fixture.reassembler.clear();
        let idr = ReassembledFrame {
            id: 50,
            capture_wall_ms: None,
            encode_wall_ms: None,
            send_wall_ms: 0,
            au: vec![0, 0, 0, 1, 0x65, 1],
        };
        assert_eq!(
            fixture
                .sequencer
                .push_reassembled_at(idr, &fixture.reassembler, fixture.start)[0]
                .id,
            50
        );
        fixture.sequencer.configure_nack_grace(true, None);
        fixture
            .reassembler
            .push(parse_fragment(&datagram(1, 2, 51, 1, 2, 3, b"b")).unwrap());
        assert!(push_nack_test_au(
            &mut fixture.sequencer,
            &mut fixture.reassembler,
            52,
            fixture.start + Duration::from_millis(1)
        )
        .is_empty());
        assert!(fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(8))
            .is_empty());
        assert_eq!(
            fixture
                .sequencer
                .drain_expired_at(fixture.start + Duration::from_millis(9))[0]
                .id,
            52
        );
    }

    #[test]
    fn nack_completion_boundary_only_holds_retransmittable_holes() {
        for missing_count in [0, 41] {
            let start = Instant::now();
            let mut sequencer = CompletedFrameSequencer::default();
            sequencer.establish_epoch(10);
            sequencer.configure_nack_grace(true, Some(100));
            let mut reassembler = FrameReassembler::default();
            if missing_count > 0 {
                reassembler.push(parse_fragment(&datagram(41, 42, 11, 1, 2, 3, b"b")).unwrap());
            }
            assert!(push_nack_test_au(&mut sequencer, &mut reassembler, 12, start).is_empty());
            assert!(push_nack_test_au(&mut sequencer, &mut reassembler, 13, start).is_empty());
            let ready = push_nack_test_au(&mut sequencer, &mut reassembler, 14, start);
            assert_eq!(
                ready.iter().map(|f| f.id).collect::<Vec<_>>(),
                vec![12, 13, 14]
            );
        }
    }

    #[test]
    fn nack_tick_requests_missing_fragments_once_and_heals_on_completion() {
        let mut fixture = NackFixture::new();
        assert_eq!(fixture.missing(), vec![0]);
        let mut sent = Vec::new();
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(3),
            fixture.start,
            |body| sent.push(body.to_vec()),
        );
        assert_eq!(outcome.au_id, Some(11));
        assert_eq!(outcome.sent_messages, 1);
        assert!(!outcome.healed);
        assert_eq!(sent, vec![encode_nack_request(11, &[0]).unwrap()]);

        // The grace (2×3+5 = 11ms) holds the hole past the plain 8ms reorder
        // window: at +9ms the pending AU 12 must not have been delivered yet.
        assert!(fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(9))
            .is_empty());

        // The retransmitted fragment completes AU 11; contiguous delivery
        // happens immediately even though the hold is still active.
        let frame11 = fixture
            .reassembler
            .push(parse_fragment(&datagram(0, 2, 11, 1, 2, 3, b"a")).unwrap())
            .unwrap();
        let ready = fixture
            .sequencer
            .push_at(frame11, fixture.start + Duration::from_millis(9));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [11, 12]
        );
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(3),
            fixture.start + Duration::from_millis(9),
            |_| {},
        );
        assert!(outcome.healed);
        assert_eq!(outcome.au_id, Some(11));
        assert_eq!(fixture.sequencer.blocked_hole(), None);
    }

    /// F14: the 3-subsequent-AU count flush must not bypass an armed NACK
    /// hold. A burst reaching the count threshold behind the blocked hole
    /// used to drain immediately, forget the hole mid-grace, and drop the
    /// retransmit when it finally arrived.
    #[test]
    fn nack_hold_gates_the_subsequent_au_count_flush_until_the_deadline() {
        let mut fixture = NackFixture::new();
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(3), // grace = 2×3+5 = 11ms
            fixture.start,
            |_| {},
        );
        assert_eq!(outcome.au_id, Some(11));

        // AU 12 is already pending; AU 13/14 push the queue to the count
        // threshold (MAX_COMPLETED_REORDER = 3). While the hold is armed,
        // that must not flush anything.
        for (offset, id) in [13u16, 14u16].into_iter().enumerate() {
            let frame = fixture
                .reassembler
                .push(parse_fragment(&datagram(0, 1, id, 1, 2, 3, b"x")).unwrap())
                .unwrap();
            assert!(fixture
                .sequencer
                .push_at(frame, fixture.start + Duration::from_millis(offset as u64))
                .is_empty());
        }
        assert_eq!(fixture.sequencer.blocked_hole(), Some(11));
        // Still held short of the 11ms deadline (past the plain 8ms window).
        assert!(fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(9))
            .is_empty());

        // Once the deadline passes, the expiry path delivers the burst in
        // order (the hole is handed to the pre-existing loss path).
        let ready = fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(11));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [12, 13, 14]
        );
    }

    /// The held wait is bounded: pending AUs may exceed MAX_COMPLETED_REORDER
    /// but only up to MAX_HELD_REORDER, beyond which delivery proceeds even
    /// mid-grace so a stopped NACK tick cannot grow the queue unbounded.
    #[test]
    fn nack_hold_bounds_the_pending_queue_with_a_hard_cap() {
        let mut fixture = NackFixture::new();
        let mut requester = NackRequester::default();
        tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(3),
            fixture.start,
            |_| {},
        );

        let mut delivered = Vec::new();
        for (offset, id) in (13u16..=17).enumerate() {
            let frame = fixture
                .reassembler
                .push(parse_fragment(&datagram(0, 1, id, 1, 2, 3, b"x")).unwrap())
                .unwrap();
            delivered.extend(
                fixture
                    .sequencer
                    .push_at(frame, fixture.start + Duration::from_millis(offset as u64))
                    .iter()
                    .map(|frame| frame.id),
            );
            assert!(fixture.sequencer.pending.len() <= MAX_HELD_REORDER);
        }
        // The queue reached the cap (6) at +4ms — mid-grace, well before the
        // 11ms deadline — and flushed as one ordered run.
        assert_eq!(delivered, (12u16..=17).collect::<Vec<_>>());
        assert!(fixture.sequencer.pending.is_empty());
    }

    /// A retransmit completing the hole inside the grace still delivers the
    /// held burst contiguously and immediately: the contiguous path is never
    /// gated by the hold.
    #[test]
    fn nack_hold_retransmit_inside_the_grace_delivers_the_whole_burst() {
        let mut fixture = NackFixture::new();
        let mut requester = NackRequester::default();
        tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            false,
            Some(3),
            fixture.start,
            |_| {},
        );
        for id in [13u16, 14u16] {
            let frame = fixture
                .reassembler
                .push(parse_fragment(&datagram(0, 1, id, 1, 2, 3, b"x")).unwrap())
                .unwrap();
            assert!(fixture
                .sequencer
                .push_at(frame, fixture.start + Duration::from_millis(1))
                .is_empty());
        }

        let frame11 = fixture
            .reassembler
            .push(parse_fragment(&datagram(0, 2, 11, 1, 2, 3, b"a")).unwrap())
            .unwrap();
        let ready = fixture
            .sequencer
            .push_at(frame11, fixture.start + Duration::from_millis(9));
        assert_eq!(
            ready.iter().map(|frame| frame.id).collect::<Vec<_>>(),
            [11, 12, 13, 14]
        );
        assert!(fixture.sequencer.pending.is_empty());
        assert_eq!(fixture.sequencer.blocked_hole(), None);
    }

    #[test]
    fn nack_tick_single_attempt_expires_into_the_existing_loss_path() {
        let mut fixture = NackFixture::new();
        let mut sent = 0;
        let mut requester = NackRequester::default();
        let mut tick = |now: Instant| {
            let outcome = tick_nack_requester(
                &mut requester,
                &mut fixture.sequencer,
                &fixture.reassembler,
                false,
                Some(3),
                now,
                |_| sent += 1,
            );
            outcome.sent_messages
        };
        assert_eq!(tick(fixture.start), 1);
        // Inside the grace: hold kept alive, no further sends.
        assert_eq!(tick(fixture.start + Duration::from_millis(10)), 0);
        // After the grace: the request is settled; the same hole is never
        // re-requested (single attempt per AU).
        assert_eq!(tick(fixture.start + Duration::from_millis(12)), 0);
        assert_eq!(tick(fixture.start + Duration::from_millis(13)), 0);
        assert_eq!(sent, 1);
        // Expiry resumed: the pending AU is delivered past the hole exactly
        // like the pre-NACK path, and presentation classifies the loss.
        let ready = fixture
            .sequencer
            .drain_expired_at(fixture.start + Duration::from_millis(14));
        assert_eq!(ready.iter().map(|frame| frame.id).collect::<Vec<_>>(), [12]);
    }

    #[test]
    fn nack_tick_skips_awaiting_keyframe_and_refuses_wide_holes() {
        let mut fixture = NackFixture::new();
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut fixture.sequencer,
            &fixture.reassembler,
            true,
            Some(3),
            fixture.start,
            |_| panic!("must not request while awaiting a keyframe"),
        );
        assert_eq!(outcome.sent_messages, 0);
        assert!(!outcome.healed);

        // A 41-fragment hole exceeds MAX_NACK_REQUEST_TOTAL: no request, and
        // the same hole is never retried.
        let mut wide = FrameReassembler::default();
        assert!(wide
            .push(parse_fragment(&datagram(41, 42, 21, 1, 2, 3, b"x")).unwrap())
            .is_none());
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(20);
        let frame22 = wide
            .push(parse_fragment(&datagram(0, 1, 22, 1, 2, 3, b"y")).unwrap())
            .unwrap();
        assert!(sequencer.push_at(frame22, fixture.start).is_empty());
        let outcome = tick_nack_requester(
            &mut requester,
            &mut sequencer,
            &wide,
            false,
            Some(3),
            fixture.start,
            |_| panic!("must not request a hole wider than the cap"),
        );
        assert_eq!(outcome.sent_messages, 0);
        let outcome = tick_nack_requester(
            &mut requester,
            &mut sequencer,
            &wide,
            false,
            Some(3),
            fixture.start + Duration::from_millis(30),
            |_| panic!("must not retry the refused hole"),
        );
        assert_eq!(outcome.sent_messages, 0);
    }

    #[test]
    fn nack_tick_ignores_holes_without_retransmittable_state() {
        // AU 11 was never seen at all (no partial state): nothing to request,
        // so the tick must not send and must leave the existing loss path.
        let mut reassembler = FrameReassembler::default();
        let mut sequencer = CompletedFrameSequencer::default();
        sequencer.establish_epoch(10);
        let start = Instant::now();
        let frame12 = reassembler
            .push(parse_fragment(&datagram(0, 1, 12, 1, 2, 3, b"y")).unwrap())
            .unwrap();
        assert!(sequencer.push_at(frame12, start).is_empty());
        let mut requester = NackRequester::default();
        let outcome = tick_nack_requester(
            &mut requester,
            &mut sequencer,
            &reassembler,
            false,
            Some(3),
            start,
            |_| panic!("must not request fragments with no partial state"),
        );
        assert_eq!(outcome.sent_messages, 0);
    }

    #[test]
    fn reassembler_reports_missing_indexes_and_recent_completion() {
        let mut reassembler = FrameReassembler::default();
        assert!(reassembler
            .push(parse_fragment(&datagram(1, 4, 9, 1, 2, 3, b"p")).unwrap())
            .is_none());
        assert!(reassembler
            .push(parse_fragment(&datagram(3, 4, 9, 1, 2, 3, b"p")).unwrap())
            .is_none());
        assert_eq!(reassembler.missing_fragment_indexes(9), Some(vec![0, 2]));
        assert_eq!(reassembler.missing_fragment_indexes(8), None);
        assert!(!reassembler.recently_completed_contains(9));
        assert!(reassembler
            .push(parse_fragment(&datagram(0, 4, 9, 1, 2, 3, b"p")).unwrap())
            .is_none());
        assert!(reassembler
            .push(parse_fragment(&datagram(2, 4, 9, 1, 2, 3, b"p")).unwrap())
            .is_some());
        assert!(reassembler.recently_completed_contains(9));
        assert_eq!(reassembler.missing_fragment_indexes(9), None);
    }
}
