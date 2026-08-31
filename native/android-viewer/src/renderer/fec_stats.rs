use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FecStatsSnapshot {
    pub media_datagrams_received: u64,
    pub data_datagrams_received: u64,
    pub parity_datagrams_received: u64,
    pub fec_restored_fragments: u64,
    pub unrecoverable_fec_groups: u32,
    pub max_missing_data_fragments: u16,
    pub one_frame_gap_events: u32,
    pub multi_frame_gap_events: u32,
    pub paired_idr_episodes: u32,
    pub suppressed_duplicate_recovery_requests: u32,
    pub fec_decode_failures: u32,
}

#[derive(Debug, Default)]
pub struct FecRuntimeStats {
    media_datagrams_received: AtomicU64,
    data_datagrams_received: AtomicU64,
    parity_datagrams_received: AtomicU64,
    fec_restored_fragments: AtomicU64,
    unrecoverable_fec_groups: AtomicU32,
    max_missing_data_fragments: AtomicU32,
    one_frame_gap_events: AtomicU32,
    multi_frame_gap_events: AtomicU32,
    paired_idr_episodes: AtomicU32,
    suppressed_duplicate_recovery_requests: AtomicU32,
    fec_decode_failures: AtomicU32,
}

impl FecRuntimeStats {
    pub fn record_data_datagram(&self) {
        self.media_datagrams_received
            .fetch_add(1, Ordering::Relaxed);
        self.data_datagrams_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_parity_datagram(&self) {
        self.media_datagrams_received
            .fetch_add(1, Ordering::Relaxed);
        self.parity_datagrams_received
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_restored_fragments(&self, count: usize) {
        self.fec_restored_fragments
            .fetch_add(count.min(u64::MAX as usize) as u64, Ordering::Relaxed);
    }

    pub fn record_unrecoverable_group(&self, missing_data_fragments: usize) {
        if missing_data_fragments == 0 {
            return;
        }
        self.unrecoverable_fec_groups
            .fetch_add(1, Ordering::Relaxed);
        self.max_missing_data_fragments.fetch_max(
            missing_data_fragments.min(usize::from(u16::MAX)) as u32,
            Ordering::Relaxed,
        );
    }

    pub fn record_gap_event(&self, missing_frames: u16) {
        match missing_frames {
            0 => {}
            1 => {
                self.one_frame_gap_events.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.multi_frame_gap_events.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn record_paired_idr_episode(&self) {
        self.paired_idr_episodes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_suppressed_recovery_request(&self) {
        self.suppressed_duplicate_recovery_requests
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_decode_failure(&self) {
        self.fec_decode_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> FecStatsSnapshot {
        FecStatsSnapshot {
            media_datagrams_received: self.media_datagrams_received.load(Ordering::Relaxed),
            data_datagrams_received: self.data_datagrams_received.load(Ordering::Relaxed),
            parity_datagrams_received: self.parity_datagrams_received.load(Ordering::Relaxed),
            fec_restored_fragments: self.fec_restored_fragments.load(Ordering::Relaxed),
            unrecoverable_fec_groups: self.unrecoverable_fec_groups.load(Ordering::Relaxed),
            max_missing_data_fragments: self
                .max_missing_data_fragments
                .load(Ordering::Relaxed)
                .min(u32::from(u16::MAX)) as u16,
            one_frame_gap_events: self.one_frame_gap_events.load(Ordering::Relaxed),
            multi_frame_gap_events: self.multi_frame_gap_events.load(Ordering::Relaxed),
            paired_idr_episodes: self.paired_idr_episodes.load(Ordering::Relaxed),
            suppressed_duplicate_recovery_requests: self
                .suppressed_duplicate_recovery_requests
                .load(Ordering::Relaxed),
            fec_decode_failures: self.fec_decode_failures.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_real_loss_shape_as_saturating_cumulative_counters() {
        let stats = FecRuntimeStats::default();
        stats.record_data_datagram();
        stats.record_parity_datagram();
        stats.record_restored_fragments(3);
        stats.record_unrecoverable_group(2);
        stats.record_unrecoverable_group(5);
        stats.record_gap_event(1);
        stats.record_gap_event(4);
        stats.record_paired_idr_episode();
        stats.record_suppressed_recovery_request();
        stats.record_decode_failure();

        assert_eq!(
            stats.snapshot(),
            FecStatsSnapshot {
                media_datagrams_received: 2,
                data_datagrams_received: 1,
                parity_datagrams_received: 1,
                fec_restored_fragments: 3,
                unrecoverable_fec_groups: 2,
                max_missing_data_fragments: 5,
                one_frame_gap_events: 1,
                multi_frame_gap_events: 1,
                paired_idr_episodes: 1,
                suppressed_duplicate_recovery_requests: 1,
                fec_decode_failures: 1,
            }
        );
    }
}
