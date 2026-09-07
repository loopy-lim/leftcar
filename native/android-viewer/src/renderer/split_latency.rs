//! Bounded split-path latency telemetry, measured with the local
//! CLOCK_MONOTONIC clock only.
//!
//! The split renderer has no latency-probe exchange with the Host. The
//! established clock-offset pipeline (LCP1/LCP2 probes producing
//! `host_clock_offset_ms`) lives entirely in the single-session path and its
//! control socket; the split media sockets only carry ACK / input-status /
//! termination control packets, and no viewer-side session in split mode ever
//! establishes a host clock offset. Without an established offset,
//! `capture_wall_ms` carried by media datagrams cannot be converted into a
//! capture age here, so this module deliberately does not touch wall-clock
//! host timestamps. End-to-end capture age therefore remains unknown in
//! split mode; what this module does measure is every *local* monotonic
//! segment of the playback pipeline:
//!
//! `datagram receive -> decoder queue ("feed") -> decoder output ready ->
//! timed surface release submitted`, plus per-episode
//! `gap -> IDR -> first resumed decoder output` durations.
//!
//! Unknown segments are skipped (never recorded as zero) so `count == 0`
//! always means "not measured", never "measured as zero".

use std::collections::{HashMap, VecDeque};

/// Recent-sample window used for the p95 estimate. Older samples are
/// dropped, keeping the series state bounded.
const LATENCY_SAMPLE_WINDOW: usize = 512;

/// Receive timestamps are keyed by the raw u16 frame id, which wraps after
/// 65536 frames (~18 min at 60 FPS). A re-seen id only replaces the old
/// timestamp when the previous entry is older than this, so normal replay
/// of an id keeps the first (correct) fragment arrival.
const RECEIVE_TIMESTAMP_STALE_NS: i64 = 1_000_000_000;

/// Upper bound on frames tracked simultaneously in each pipeline stage.
/// At most one output per tile may sit in the coordinator plus a small
/// decoder backlog, so 64 is far above any legitimate queue depth.
const TRACE_CAPACITY: usize = 64;

/// Upper bound of frame ids tracked for first-fragment arrival times.
const RECEIVE_TRACE_CAPACITY: usize = 128;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LatencySnapshot {
    pub count: u64,
    pub total_us: u64,
    pub max_us: u32,
    pub p95_us: u32,
}

/// Bounded monotonic duration series: count / total / max plus a
/// recent-sample window for p95. Durations saturate at u32 microseconds
/// (about 71.6 minutes) instead of wrapping.
#[derive(Debug, Default)]
pub struct LatencySeries {
    count: u64,
    total_us: u64,
    max_us: u32,
    samples: Vec<u32>,
}

impl LatencySeries {
    pub fn record(&mut self, duration_us: u64) {
        let sample = duration_us.min(u64::from(u32::MAX)) as u32;
        self.count += 1;
        self.total_us = self.total_us.saturating_add(duration_us);
        self.max_us = self.max_us.max(sample);
        self.samples.push(sample);
        if self.samples.len() > LATENCY_SAMPLE_WINDOW {
            let excess = self.samples.len() - LATENCY_SAMPLE_WINDOW;
            self.samples.drain(..excess);
        }
    }

    pub fn snapshot(&self) -> LatencySnapshot {
        let p95_us = if self.samples.is_empty() {
            0
        } else {
            let mut sorted = self.samples.clone();
            sorted.sort_unstable();
            let index = ((sorted.len() * 95).div_ceil(100)).saturating_sub(1);
            sorted[index]
        };
        LatencySnapshot {
            count: self.count,
            total_us: self.total_us,
            max_us: self.max_us,
            p95_us,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueuedTrace {
    recv_ns: Option<i64>,
    queued_ns: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadyTrace {
    recv_ns: Option<i64>,
    ready_ns: i64,
}

/// Tracks one frame through the local pipeline stages keyed by the raw
/// frame id (fragment arrival) and the expanded presentation timestamp
/// (queue -> ready -> release). Frames without a known receive instant
/// (e.g. fully FEC-restored) still produce queue/ready/release samples but
/// never invent a receive timestamp.
#[derive(Debug, Default)]
pub struct FrameTraceStore {
    recv_ns_by_id: HashMap<u16, i64>,
    recv_order: VecDeque<u16>,
    queued_by_pts: HashMap<i64, QueuedTrace>,
    queued_order: VecDeque<i64>,
    ready_by_pts: HashMap<i64, ReadyTrace>,
    ready_order: VecDeque<i64>,
}

fn elapsed_us(start_ns: i64, now_ns: i64) -> u64 {
    u64::try_from(now_ns.saturating_sub(start_ns))
        .unwrap_or(0)
        .saturating_div(1_000)
}

impl FrameTraceStore {
    /// Records the arrival instant of the first fragment of `id`. Later
    /// fragments of the same frame keep the existing timestamp.
    pub fn note_fragment(&mut self, id: u16, now_ns: i64) {
        if let Some(existing) = self.recv_ns_by_id.get(&id) {
            if now_ns.saturating_sub(*existing) < RECEIVE_TIMESTAMP_STALE_NS {
                return;
            }
        } else {
            self.recv_order.push_back(id);
            while self.recv_order.len() > RECEIVE_TRACE_CAPACITY {
                if let Some(oldest) = self.recv_order.pop_front() {
                    self.recv_ns_by_id.remove(&oldest);
                }
            }
        }
        self.recv_ns_by_id.insert(id, now_ns);
    }

    /// The frame was queued into the decoder. Returns the receive -> feed
    /// duration when the frame's first fragment arrival is known.
    pub fn note_queued(&mut self, id: u16, pts_us: i64, now_ns: i64) -> Option<u64> {
        let recv_ns = self.recv_ns_by_id.remove(&id);
        if !self.queued_by_pts.contains_key(&pts_us) {
            self.queued_order.push_back(pts_us);
            while self.queued_order.len() > TRACE_CAPACITY {
                if let Some(oldest) = self.queued_order.pop_front() {
                    self.queued_by_pts.remove(&oldest);
                }
            }
        }
        let feed_us = recv_ns.map(|recv_ns| elapsed_us(recv_ns, now_ns));
        self.queued_by_pts.insert(
            pts_us,
            QueuedTrace {
                recv_ns,
                queued_ns: now_ns,
            },
        );
        feed_us
    }

    /// The decoder produced the output. Returns the queue -> ready duration
    /// for the matching pts, or None when the pts was never queued here.
    pub fn note_ready(&mut self, pts_us: i64, now_ns: i64) -> Option<u64> {
        let queued = self.queued_by_pts.remove(&pts_us)?;
        if !self.ready_by_pts.contains_key(&pts_us) {
            self.ready_order.push_back(pts_us);
            while self.ready_order.len() > TRACE_CAPACITY {
                if let Some(oldest) = self.ready_order.pop_front() {
                    self.ready_by_pts.remove(&oldest);
                }
            }
        }
        let ready_us = elapsed_us(queued.queued_ns, now_ns);
        self.ready_by_pts.insert(
            pts_us,
            ReadyTrace {
                recv_ns: queued.recv_ns,
                ready_ns: now_ns,
            },
        );
        Some(ready_us)
    }

    /// The output was submitted for a timed surface release. Returns
    /// `(receive -> release, ready -> release)`; ready -> release includes
    /// the coordinator's pair-wait and dispatch hop. Unknown segments stay
    /// None instead of degrading to zero.
    pub fn note_released(&mut self, pts_us: i64, now_ns: i64) -> (Option<u64>, Option<u64>) {
        let Some(ready) = self.ready_by_pts.remove(&pts_us) else {
            return (None, None);
        };
        (
            ready.recv_ns.map(|recv_ns| elapsed_us(recv_ns, now_ns)),
            Some(elapsed_us(ready.ready_ns, now_ns)),
        )
    }

    pub fn note_discarded(&mut self, pts_us: i64) {
        self.queued_by_pts.remove(&pts_us);
        self.ready_by_pts.remove(&pts_us);
    }

    pub fn clear(&mut self) {
        self.recv_ns_by_id.clear();
        self.recv_order.clear();
        self.queued_by_pts.clear();
        self.queued_order.clear();
        self.ready_by_pts.clear();
        self.ready_order.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapEpisodeDurations {
    /// None when the episode's first decoder output appeared before any IDR
    /// was observed on this tile.
    pub gap_to_idr_us: Option<u64>,
    /// None when the first output appeared before an IDR was observed.
    pub idr_to_first_output_us: Option<u64>,
    /// Gap detection -> first resumed decoder output.
    pub total_us: u64,
}

/// One tile's gap-freeze episode: loss detected -> IDR observed -> first
/// decoder output after the freeze. Purely local state; each tile tracks
/// its own stream independently, mirroring the per-tile gating in
/// `split_gap_policy::decide_split_frame_gap`.
#[derive(Debug, Default)]
pub struct GapEpisodeTracker {
    gap_started_ns: Option<i64>,
    idr_ns: Option<i64>,
    idr_pts_us: Option<i64>,
}

impl GapEpisodeTracker {
    /// Starts a new episode when none is open. Returns true when the call
    /// started one.
    pub fn on_gap(&mut self, now_ns: i64) -> bool {
        if self.gap_started_ns.is_some() {
            return false;
        }
        self.gap_started_ns = Some(now_ns);
        self.idr_ns = None;
        self.idr_pts_us = None;
        true
    }

    /// An IDR was fed on this tile. Returns the gap -> IDR duration when an
    /// open episode was awaiting it. Regular IDRs without a preceding gap
    /// return None.
    pub fn on_idr(&mut self, pts_us: i64, now_ns: i64) -> Option<u64> {
        let start = self.gap_started_ns?;
        if self.idr_ns.is_some() {
            return None;
        }
        self.idr_ns = Some(now_ns);
        self.idr_pts_us = Some(pts_us);
        Some(elapsed_us(start, now_ns))
    }

    /// Buffered output predating the queued recovery IDR cannot end a freeze.
    /// Split decoder PTS expands the wire frame sequence monotonically.
    pub fn on_first_output(&mut self, pts_us: i64, now_ns: i64) -> Option<GapEpisodeDurations> {
        if pts_us < self.idr_pts_us? {
            return None;
        }
        let start = self.gap_started_ns.take()?;
        let idr_ns = self.idr_ns.take();
        self.idr_pts_us = None;
        Some(GapEpisodeDurations {
            gap_to_idr_us: idr_ns.map(|idr_ns| elapsed_us(start, idr_ns)),
            idr_to_first_output_us: idr_ns.map(|idr_ns| elapsed_us(idr_ns, now_ns)),
            total_us: elapsed_us(start, now_ns),
        })
    }

    /// An episode that can no longer complete normally (decoder flush or
    /// reset). Returns true when an open episode was discarded.
    pub fn abort(&mut self) -> bool {
        let open = self.gap_started_ns.take().is_some();
        self.idr_ns = None;
        self.idr_pts_us = None;
        open
    }
}

/// Per-tile telemetry bundle owned by one split tile worker thread. Every
/// recording API takes an explicit monotonic `now_ns` so tests can drive
/// time deterministically; unknown stages record nothing at all.
#[derive(Debug, Default)]
pub struct TileLatencyTelemetry {
    pub recv_to_feed_us: LatencySeries,
    pub feed_to_ready_us: LatencySeries,
    pub ready_to_release_us: LatencySeries,
    pub recv_to_release_us: LatencySeries,
    pub gap_to_idr_us: LatencySeries,
    pub idr_to_first_output_us: LatencySeries,
    pub gap_to_first_output_us: LatencySeries,
    pub gap_episodes_started: u64,
    pub gap_episodes_completed: u64,
    pub gap_episodes_aborted: u64,
    /// Sequenced access units withheld from the decoder while the tile was
    /// frozen awaiting a keyframe (only counted once the tile has a
    /// decoder, i.e. the freeze is user-visible). This is the honest
    /// split-path replacement for the single-session stale-frame metric and
    /// stays local: the wire `stale_frames` field feeds the Host ABR loss
    /// signal and must not change meaning from this path.
    pub frozen_inputs: u64,
    traces: FrameTraceStore,
    episode: GapEpisodeTracker,
}

impl TileLatencyTelemetry {
    pub fn note_fragment(&mut self, id: u16, now_ns: i64) {
        self.traces.note_fragment(id, now_ns);
    }

    pub fn note_queued(&mut self, id: u16, pts_us: i64, now_ns: i64) {
        if let Some(feed_us) = self.traces.note_queued(id, pts_us, now_ns) {
            self.recv_to_feed_us.record(feed_us);
        }
    }

    /// Records feed -> ready. When this output is the first one after a
    /// gap-freeze episode, the episode is closed and its durations are
    /// returned for episodic logging.
    pub fn note_output_ready(&mut self, pts_us: i64, now_ns: i64) -> Option<GapEpisodeDurations> {
        if let Some(ready_us) = self.traces.note_ready(pts_us, now_ns) {
            self.feed_to_ready_us.record(ready_us);
        }
        let durations = self.episode.on_first_output(pts_us, now_ns)?;
        self.gap_episodes_completed += 1;
        if let Some(idr_us) = durations.idr_to_first_output_us {
            self.idr_to_first_output_us.record(idr_us);
        }
        self.gap_to_first_output_us.record(durations.total_us);
        Some(durations)
    }

    pub fn note_released(&mut self, pts_us: i64, now_ns: i64) {
        let (recv_us, ready_us) = self.traces.note_released(pts_us, now_ns);
        if let Some(recv_us) = recv_us {
            self.recv_to_release_us.record(recv_us);
        }
        if let Some(ready_us) = ready_us {
            self.ready_to_release_us.record(ready_us);
        }
    }

    pub fn note_discarded(&mut self, pts_us: i64) {
        self.traces.note_discarded(pts_us);
    }

    pub fn note_frozen_input(&mut self) {
        self.frozen_inputs += 1;
    }

    /// A gap-freeze begins on this tile (loss signal while feeding).
    pub fn note_gap_started(&mut self, now_ns: i64) {
        if self.episode.on_gap(now_ns) {
            self.gap_episodes_started += 1;
        }
    }

    /// An IDR was fed after a gap-freeze. Returns the gap -> IDR duration
    /// for episodic logging.
    pub fn note_idr(&mut self, pts_us: i64, now_ns: i64) -> Option<u64> {
        let idr_us = self.episode.on_idr(pts_us, now_ns)?;
        self.gap_to_idr_us.record(idr_us);
        Some(idr_us)
    }

    /// Decoder flush or reset: pending traces cannot complete and an open
    /// episode is discarded as aborted. Returns true when an episode was
    /// aborted, for episodic logging.
    pub fn reset_for_recovery(&mut self) -> bool {
        let aborted = self.episode.abort();
        if aborted {
            self.gap_episodes_aborted += 1;
        }
        self.traces.clear();
        aborted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const US: i64 = 1_000;
    const MS: i64 = 1_000 * US;

    #[test]
    fn latency_series_reports_count_total_max_and_recent_window_p95() {
        let mut series = LatencySeries::default();
        for us in 1..=100u64 {
            series.record(us * US as u64);
        }
        let snapshot = series.snapshot();
        assert_eq!(snapshot.count, 100);
        assert_eq!(snapshot.total_us, 5_050 * US as u64);
        assert_eq!(snapshot.max_us, 100 * US as u64 as u32);
        assert_eq!(snapshot.p95_us, 95 * US as u64 as u32);
    }

    #[test]
    fn latency_series_p95_covers_only_the_bounded_recent_window() {
        let mut series = LatencySeries::default();
        for us in 0..(LATENCY_SAMPLE_WINDOW as u64 + 100) {
            series.record(us + 1);
        }
        let snapshot = series.snapshot();
        assert_eq!(snapshot.count, (LATENCY_SAMPLE_WINDOW + 100) as u64);
        // The window holds the newest 512 samples (values 101..=612), so the
        // p95 comes from that window, not from the discarded 1..=100 head.
        assert_eq!(snapshot.p95_us, 587);
        assert_eq!(snapshot.max_us, 612);
    }

    #[test]
    fn latency_series_saturates_instead_of_wrapping() {
        let mut series = LatencySeries::default();
        series.record(u64::from(u32::MAX) + 1);
        series.record(1);
        let snapshot = series.snapshot();
        assert_eq!(snapshot.max_us, u32::MAX);
        assert_eq!(snapshot.total_us, u64::from(u32::MAX) + 2);
    }

    #[test]
    fn empty_series_stays_zero_so_count_distinguishes_unknown() {
        let series = LatencySeries::default();
        let snapshot = series.snapshot();
        assert_eq!(
            snapshot,
            LatencySnapshot {
                count: 0,
                total_us: 0,
                max_us: 0,
                p95_us: 0,
            }
        );
    }

    #[test]
    fn queued_frame_without_known_receive_time_skips_instead_of_recording_zero() {
        let mut traces = FrameTraceStore::default();
        assert_eq!(traces.note_queued(5, 7_000, 10 * MS), None);
    }

    #[test]
    fn frame_pipeline_keeps_pts_association_across_all_stages() {
        let mut traces = FrameTraceStore::default();
        traces.note_fragment(9, MS);
        assert_eq!(traces.note_queued(9, 90_000, 2 * MS), Some(1_000));
        assert_eq!(traces.note_ready(90_000, 3 * MS), Some(1_000));
        let (recv_release, ready_release) = traces.note_released(90_000, 5 * MS);
        assert_eq!(recv_release, Some(4_000));
        assert_eq!(ready_release, Some(2_000));
        // Consumed exactly once.
        assert_eq!(traces.note_released(90_000, 6 * MS), (None, None));
    }

    #[test]
    fn restored_frame_without_receive_instant_still_measures_feed_and_release() {
        let mut traces = FrameTraceStore::default();
        assert_eq!(traces.note_queued(3, 3_000, MS), None);
        assert_eq!(traces.note_ready(3_000, 2 * MS), Some(1_000));
        let (recv_release, ready_release) = traces.note_released(3_000, 4 * MS);
        assert_eq!(recv_release, None);
        assert_eq!(ready_release, Some(2_000));
    }

    #[test]
    fn trace_maps_evict_bounded_state() {
        let mut traces = FrameTraceStore::default();
        for id in 0..400u16 {
            traces.note_fragment(id, i64::from(id) * MS);
        }
        // Receive-stage eviction: only the newest RECEIVE_TRACE_CAPACITY ids
        // keep their first-fragment arrival.
        assert_eq!(traces.note_queued(0, 0, 401 * MS), None);
        assert_eq!(traces.note_queued(399, 399, 401 * MS), Some(2_000));

        // Queue and ready stages evict their oldest pts entries too.
        for index in 0..(TRACE_CAPACITY as i64 + 10) {
            let pts = index * 1_000;
            traces.note_queued(399, pts, 401 * MS);
            assert!(traces.note_ready(pts, 401 * MS).is_some());
        }
        assert_eq!(traces.note_released(0, 402 * MS), (None, None));
        let newest_pts = (TRACE_CAPACITY as i64 + 9) * 1_000;
        assert_eq!(
            traces.note_released(newest_pts, 402 * MS),
            (None, Some(1_000))
        );
    }

    #[test]
    fn repeated_id_within_stale_window_keeps_first_fragment_arrival() {
        let mut traces = FrameTraceStore::default();
        traces.note_fragment(7, MS);
        traces.note_fragment(7, 2 * MS);
        assert_eq!(traces.note_queued(7, 700, 3 * MS), Some(2_000));
    }

    #[test]
    fn id_wrap_after_stale_window_replaces_the_timestamp() {
        let mut traces = FrameTraceStore::default();
        traces.note_fragment(7, 0);
        traces.note_fragment(7, 2_000 * MS);
        assert_eq!(traces.note_queued(7, 700, 2_001 * MS), Some(1_000));
    }

    #[test]
    fn discard_drops_pending_traces_without_fabricating_samples() {
        let mut traces = FrameTraceStore::default();
        traces.note_fragment(1, 0);
        traces.note_queued(1, 100, MS);
        traces.note_discarded(100);
        assert_eq!(traces.note_ready(100, 2 * MS), None);
        assert_eq!(traces.note_released(100, 3 * MS), (None, None));
    }

    #[test]
    fn buffered_output_before_recovery_idr_does_not_end_the_gap() {
        let mut telemetry = TileLatencyTelemetry::default();
        telemetry.note_gap_started(MS);
        assert_eq!(telemetry.note_output_ready(100, 2 * MS), None);
        assert_eq!(telemetry.gap_episodes_completed, 0);
        assert_eq!(telemetry.gap_to_first_output_us.snapshot().count, 0);
    }

    #[test]
    fn gap_episode_measures_gap_to_idr_to_first_output() {
        let mut episode = GapEpisodeTracker::default();
        assert!(episode.on_gap(MS));
        assert!(!episode.on_gap(2 * MS));
        assert_eq!(episode.on_idr(200_000, 30 * MS), Some(29_000));
        let durations = episode
            .on_first_output(200_000, 33 * MS)
            .expect("episode closes");
        assert_eq!(
            durations,
            GapEpisodeDurations {
                gap_to_idr_us: Some(29_000),
                idr_to_first_output_us: Some(3_000),
                total_us: 32_000,
            }
        );
        // Consumed exactly once.
        assert_eq!(episode.on_first_output(200_000, 34 * MS), None);
    }

    #[test]
    fn regular_idr_without_gap_is_not_an_episode() {
        let mut episode = GapEpisodeTracker::default();
        assert_eq!(episode.on_idr(200_000, 5 * MS), None);
        assert_eq!(episode.on_first_output(200_000, 6 * MS), None);
    }

    #[test]
    fn buffered_outputs_do_not_close_gap_before_the_queued_idr_output() {
        let mut episode = GapEpisodeTracker::default();
        episode.on_gap(MS);
        assert_eq!(episode.on_first_output(100, 20 * MS), None);
        assert_eq!(episode.on_idr(200, 30 * MS), Some(29_000));
        assert_eq!(episode.on_first_output(199, 31 * MS), None);
        let durations = episode
            .on_first_output(200, 33 * MS)
            .expect("IDR output resumes");
        assert_eq!(durations.total_us, 32_000);
        assert_eq!(durations.idr_to_first_output_us, Some(3_000));
    }

    #[test]
    fn abort_reports_only_open_episodes() {
        let mut episode = GapEpisodeTracker::default();
        assert!(!episode.abort());
        episode.on_gap(0);
        assert!(episode.abort());
        // The aborted episode is fully closed.
        assert_eq!(episode.on_idr(200_000, MS), None);
        assert_eq!(episode.on_first_output(200_000, 2 * MS), None);
    }

    #[test]
    fn telemetry_routes_pipeline_and_episode_samples_into_series() {
        let mut telemetry = TileLatencyTelemetry::default();
        telemetry.note_fragment(20, MS);
        telemetry.note_queued(20, 200_000, 2 * MS);
        telemetry.note_gap_started(2 * MS);
        assert_eq!(telemetry.note_idr(200_000, 30 * MS), Some(28_000));
        let closed = telemetry
            .note_output_ready(200_000, 33 * MS)
            .expect("episode closed by first output");
        assert_eq!(closed.total_us, 31_000);
        telemetry.note_released(200_000, 34 * MS);

        assert_eq!(telemetry.recv_to_feed_us.snapshot().count, 1);
        assert_eq!(telemetry.feed_to_ready_us.snapshot().count, 1);
        assert_eq!(telemetry.ready_to_release_us.snapshot().count, 1);
        assert_eq!(telemetry.recv_to_release_us.snapshot().count, 1);
        assert_eq!(telemetry.gap_to_idr_us.snapshot().count, 1);
        assert_eq!(telemetry.idr_to_first_output_us.snapshot().count, 1);
        assert_eq!(telemetry.gap_to_first_output_us.snapshot().count, 1);
        assert_eq!(
            (
                telemetry.gap_episodes_started,
                telemetry.gap_episodes_completed,
                telemetry.gap_episodes_aborted
            ),
            (1, 1, 0)
        );
        assert_eq!(telemetry.recv_to_release_us.snapshot().max_us, 33_000);
    }

    #[test]
    fn telemetry_reset_aborts_open_episode_and_clears_traces() {
        let mut telemetry = TileLatencyTelemetry::default();
        telemetry.note_fragment(1, 0);
        telemetry.note_queued(1, 100, MS);
        telemetry.note_gap_started(MS);
        assert!(telemetry.reset_for_recovery());
        assert_eq!(
            (
                telemetry.gap_episodes_started,
                telemetry.gap_episodes_completed,
                telemetry.gap_episodes_aborted
            ),
            (1, 0, 1)
        );
        // Pending traces were cleared, so nothing fabricates release samples
        // for frames that can no longer be released.
        assert_eq!(telemetry.note_output_ready(100, 2 * MS), None);
        telemetry.note_released(100, 3 * MS);
        assert_eq!(telemetry.ready_to_release_us.snapshot().count, 0);
        assert_eq!(telemetry.recv_to_release_us.snapshot().count, 0);
        // The aborted episode is gone; a later IDR is not misread as recovery.
        assert_eq!(telemetry.note_idr(200_000, 5 * MS), None);
    }
}
