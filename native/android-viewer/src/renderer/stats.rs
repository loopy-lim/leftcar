#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PairPresentationStats {
    pub joined_rendered_frames: u64,
    pub pair_sync_timeouts: u32,
    pub unmatched_output_drops: u32,
    pub pair_ready_delta_max_us: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SplitFeedbackSnapshot {
    pub frame_gaps: u32,
    pub input_drops: u32,
    pub incomplete_aus: u32,
    pub stale_frames: u32,
    pub rendered_fps: u16,
    pub joined_rendered_fps: u16,
    pub pair_ready_delta_p95_us: u32,
    pub pair_ready_delta_max_us: u32,
    pub pair_sync_timeouts: u32,
    pub unmatched_output_drops: u32,
    pub keyframe_gap_recoveries: u32,
    pub delta_gap_recoveries: u32,
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
    /// Coordinator-wide count of paired recovery episodes where BOTH tiles
    /// observed the same IDR generation. Lives in the length-tolerant v3
    /// suffix (bytes 120..124): a released viewer keeps sending exactly the
    /// legacy 120B body and the Host gates this field on the explicit body
    /// length, so both directions stay compatible.
    pub paired_idr_resumes: u32,
    /// Smoothed clock-corrected capture/send -> decoder-feed ages in
    /// milliseconds. `u16::MAX` means "not measured" (no host clock offset
    /// yet), matching the single-session latency convention. Suffix fields at
    /// bytes 124..126 and 126..128.
    pub wire_to_decoder_ms: u16,
    pub capture_age_ms: u16,
    /// Viewer-measured reliable-input send->ack round trip (EWMA, ms).
    /// `u16::MAX` means no ack has landed yet. Suffix field at 128..130.
    pub input_rtt_ms: u16,
    /// Per-tile gap recovery (R2): per-tile episodes that resumed when THEIR
    /// tile's decoder saw the new keyframe. Suffix field at 130..134; the
    /// extended body length is also the Host's per-tile capability signal.
    pub per_tile_idr_resumes: u32,
}

pub fn split_feedback_body(snapshot: SplitFeedbackSnapshot) -> Vec<u8> {
    let mut body = Vec::with_capacity(134);
    body.extend_from_slice(b"LCF1");
    body.extend_from_slice(&snapshot.frame_gaps.to_be_bytes());
    body.extend_from_slice(&snapshot.input_drops.to_be_bytes());
    body.extend_from_slice(&snapshot.incomplete_aus.to_be_bytes());
    body.extend_from_slice(&snapshot.stale_frames.to_be_bytes());
    body.extend_from_slice(&u16::MAX.to_be_bytes());
    body.extend_from_slice(&u16::MAX.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&snapshot.rendered_fps.to_be_bytes());
    body.extend_from_slice(&snapshot.joined_rendered_fps.to_be_bytes());
    body.extend_from_slice(&snapshot.pair_ready_delta_p95_us.to_be_bytes());
    body.extend_from_slice(&snapshot.pair_ready_delta_max_us.to_be_bytes());
    body.extend_from_slice(&snapshot.pair_sync_timeouts.to_be_bytes());
    body.extend_from_slice(&snapshot.unmatched_output_drops.to_be_bytes());
    body.extend_from_slice(&snapshot.keyframe_gap_recoveries.to_be_bytes());
    body.extend_from_slice(&snapshot.delta_gap_recoveries.to_be_bytes());
    body.extend_from_slice(&snapshot.media_datagrams_received.to_be_bytes());
    body.extend_from_slice(&snapshot.data_datagrams_received.to_be_bytes());
    body.extend_from_slice(&snapshot.parity_datagrams_received.to_be_bytes());
    body.extend_from_slice(&snapshot.fec_restored_fragments.to_be_bytes());
    body.extend_from_slice(&snapshot.unrecoverable_fec_groups.to_be_bytes());
    body.extend_from_slice(&snapshot.max_missing_data_fragments.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&snapshot.one_frame_gap_events.to_be_bytes());
    body.extend_from_slice(&snapshot.multi_frame_gap_events.to_be_bytes());
    body.extend_from_slice(&snapshot.paired_idr_episodes.to_be_bytes());
    body.extend_from_slice(
        &snapshot
            .suppressed_duplicate_recovery_requests
            .to_be_bytes(),
    );
    body.extend_from_slice(&snapshot.fec_decode_failures.to_be_bytes());
    // Length-tolerant v3 suffix. The Host parses the 120B v2 body it knows
    // and reads each suffix field only when the datagram is long enough, so
    // senders may keep appending fields without breaking older receivers.
    body.extend_from_slice(&snapshot.paired_idr_resumes.to_be_bytes());
    body.extend_from_slice(&snapshot.wire_to_decoder_ms.to_be_bytes());
    body.extend_from_slice(&snapshot.capture_age_ms.to_be_bytes());
    body.extend_from_slice(&snapshot.input_rtt_ms.to_be_bytes());
    body.extend_from_slice(&snapshot.per_tile_idr_resumes.to_be_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_feedback_preserves_legacy_offsets_and_appends_pair_metrics() {
        let body = split_feedback_body(SplitFeedbackSnapshot {
            frame_gaps: 1,
            input_drops: 2,
            incomplete_aus: 3,
            stale_frames: 4,
            rendered_fps: 55,
            joined_rendered_fps: 54,
            pair_ready_delta_p95_us: 700,
            pair_ready_delta_max_us: 900,
            pair_sync_timeouts: 5,
            unmatched_output_drops: 6,
            keyframe_gap_recoveries: 7,
            delta_gap_recoveries: 8,
            media_datagrams_received: 9,
            data_datagrams_received: 10,
            parity_datagrams_received: 11,
            fec_restored_fragments: 12,
            unrecoverable_fec_groups: 13,
            max_missing_data_fragments: 14,
            one_frame_gap_events: 15,
            multi_frame_gap_events: 16,
            paired_idr_episodes: 17,
            suppressed_duplicate_recovery_requests: 18,
            fec_decode_failures: 19,
            paired_idr_resumes: 20,
            wire_to_decoder_ms: 21,
            capture_age_ms: 22,
            input_rtt_ms: 23,
            per_tile_idr_resumes: 24,
        });
        assert_eq!(&body[..4], b"LCF1");
        // Bytes 16..20 are the stale-frame field the Host reads into its ABR
        // loss signal. The split worker intentionally keeps sending 0 there
        // (it has no capture-age metric; populating it would change Host
        // bitrate policy), but the wire position itself must stay pinned so
        // that a future intentional change is a visible, deliberate act.
        assert_eq!(u32::from_be_bytes(body[16..20].try_into().unwrap()), 4);
        assert_eq!(u16::from_be_bytes([body[32], body[33]]), 55);
        assert_eq!(u16::from_be_bytes([body[34], body[35]]), 54);
        assert_eq!(u32::from_be_bytes(body[36..40].try_into().unwrap()), 700);
        assert_eq!(u32::from_be_bytes(body[52..56].try_into().unwrap()), 7);
        assert_eq!(u32::from_be_bytes(body[56..60].try_into().unwrap()), 8);
        assert_eq!(u64::from_be_bytes(body[60..68].try_into().unwrap()), 9);
        assert_eq!(u64::from_be_bytes(body[68..76].try_into().unwrap()), 10);
        assert_eq!(u64::from_be_bytes(body[76..84].try_into().unwrap()), 11);
        assert_eq!(u64::from_be_bytes(body[84..92].try_into().unwrap()), 12);
        assert_eq!(u32::from_be_bytes(body[92..96].try_into().unwrap()), 13);
        assert_eq!(u16::from_be_bytes(body[96..98].try_into().unwrap()), 14);
        assert_eq!(&body[98..100], &[0, 0]);
        assert_eq!(u32::from_be_bytes(body[100..104].try_into().unwrap()), 15);
        assert_eq!(u32::from_be_bytes(body[104..108].try_into().unwrap()), 16);
        assert_eq!(u32::from_be_bytes(body[108..112].try_into().unwrap()), 17);
        assert_eq!(u32::from_be_bytes(body[112..116].try_into().unwrap()), 18);
        assert_eq!(u32::from_be_bytes(body[116..120].try_into().unwrap()), 19);
        // The legacy body ends at 120 bytes; the v3 suffix is pure append.
        assert_eq!(u32::from_be_bytes(body[120..124].try_into().unwrap()), 20);
        assert_eq!(u16::from_be_bytes(body[124..126].try_into().unwrap()), 21);
        assert_eq!(u16::from_be_bytes(body[126..128].try_into().unwrap()), 22);
        assert_eq!(u16::from_be_bytes(body[128..130].try_into().unwrap()), 23);
        assert_eq!(u32::from_be_bytes(body[130..134].try_into().unwrap()), 24);
        assert_eq!(body.len(), 134);
        // The first 120 bytes are byte-for-byte the released v2 body: an old
        // Host ignores the suffix and a new Host reads the legacy fields at
        // the exact offsets it always has.
        let legacy = split_feedback_body(SplitFeedbackSnapshot {
            paired_idr_resumes: 0,
            wire_to_decoder_ms: 0,
            capture_age_ms: 0,
            input_rtt_ms: 0,
            per_tile_idr_resumes: 0,
            ..snapshot_fields()
        });
        assert_eq!(&body[..120], &legacy[..120]);
    }

    fn snapshot_fields() -> SplitFeedbackSnapshot {
        SplitFeedbackSnapshot {
            frame_gaps: 1,
            input_drops: 2,
            incomplete_aus: 3,
            stale_frames: 4,
            rendered_fps: 55,
            joined_rendered_fps: 54,
            pair_ready_delta_p95_us: 700,
            pair_ready_delta_max_us: 900,
            pair_sync_timeouts: 5,
            unmatched_output_drops: 6,
            keyframe_gap_recoveries: 7,
            delta_gap_recoveries: 8,
            media_datagrams_received: 9,
            data_datagrams_received: 10,
            parity_datagrams_received: 11,
            fec_restored_fragments: 12,
            unrecoverable_fec_groups: 13,
            max_missing_data_fragments: 14,
            one_frame_gap_events: 15,
            multi_frame_gap_events: 16,
            paired_idr_episodes: 17,
            suppressed_duplicate_recovery_requests: 18,
            fec_decode_failures: 19,
            paired_idr_resumes: 0,
            wire_to_decoder_ms: 0,
            capture_age_ms: 0,
            input_rtt_ms: 0,
            per_tile_idr_resumes: 0,
        }
    }
}
