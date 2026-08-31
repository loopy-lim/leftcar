#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitGapSignal {
    None,
    Loss,
    Idr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SplitInputPressureDecision {
    pub recover: bool,
}

const SPLIT_INPUT_PRESSURE_GRACE_MS: u64 = 17;

pub(super) const fn split_receive_batch_limit() -> usize {
    4
}

pub(super) fn decide_split_input_pressure(
    keyframe: bool,
    pressure_elapsed_ms: u64,
) -> SplitInputPressureDecision {
    SplitInputPressureDecision {
        recover: keyframe || pressure_elapsed_ms >= SPLIT_INPUT_PRESSURE_GRACE_MS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SplitFrameGapDecision {
    pub missing: u16,
    pub feed: bool,
    pub awaiting_keyframe_after: bool,
    pub signal: SplitGapSignal,
}

pub(super) fn decide_split_frame_gap(
    previous: Option<u16>,
    current: u16,
    keyframe: bool,
    awaiting_keyframe: bool,
) -> SplitFrameGapDecision {
    let missing = previous
        .map(|previous| viewer_decoder::frame_id_missing_count(previous, current))
        .unwrap_or(0);
    if keyframe {
        return SplitFrameGapDecision {
            missing,
            feed: true,
            awaiting_keyframe_after: false,
            signal: SplitGapSignal::Idr,
        };
    }
    if awaiting_keyframe {
        return SplitFrameGapDecision {
            missing,
            feed: false,
            awaiting_keyframe_after: true,
            signal: SplitGapSignal::None,
        };
    }
    if missing > 0 {
        // The Qualcomm low-latency decoder can expose visible block corruption
        // after even one missing reference on high-motion 4K content. Keep the
        // last good Surface image while the paired IDR is in flight instead of
        // displaying damaged deltas. This does not flush or recreate either
        // decoder, so recovery resumes at the independently decodable boundary.
        return SplitFrameGapDecision {
            missing,
            feed: false,
            awaiting_keyframe_after: true,
            signal: SplitGapSignal::Loss,
        };
    }
    SplitFrameGapDecision {
        missing: 0,
        feed: true,
        awaiting_keyframe_after: false,
        signal: SplitGapSignal::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_delta_gap_freezes_last_good_surface_until_paired_idr() {
        assert_eq!(
            decide_split_frame_gap(Some(10), 13, false, false),
            SplitFrameGapDecision {
                missing: 2,
                feed: false,
                awaiting_keyframe_after: true,
                signal: SplitGapSignal::Loss,
            }
        );
    }

    #[test]
    fn one_missing_delta_freezes_last_good_surface_until_paired_idr() {
        assert_eq!(
            decide_split_frame_gap(Some(10), 12, false, false),
            SplitFrameGapDecision {
                missing: 1,
                feed: false,
                awaiting_keyframe_after: true,
                signal: SplitGapSignal::Loss,
            }
        );
    }

    #[test]
    fn repeated_delta_input_miss_within_one_millisecond_does_not_recover() {
        assert!(!decide_split_input_pressure(false, 1).recover);
    }

    #[test]
    fn delta_input_pressure_past_one_frame_requests_recovery() {
        assert!(decide_split_input_pressure(false, 17).recover);
    }

    #[test]
    fn missed_idr_input_requests_recovery_immediately() {
        assert!(decide_split_input_pressure(true, 0).recover);
    }

    #[test]
    fn another_delta_while_awaiting_does_not_repeat_loss() {
        let decision = decide_split_frame_gap(Some(13), 15, false, true);
        assert_eq!(decision.signal, SplitGapSignal::None);
        assert!(!decision.feed);
        assert!(decision.awaiting_keyframe_after);
    }

    #[test]
    fn idr_after_gap_completes_recovery_without_loss() {
        assert_eq!(
            decide_split_frame_gap(Some(15), 24, true, true),
            SplitFrameGapDecision {
                missing: 8,
                feed: true,
                awaiting_keyframe_after: false,
                signal: SplitGapSignal::Idr,
            }
        );
    }

    #[test]
    fn contiguous_delta_stays_on_the_live_path() {
        assert_eq!(
            decide_split_frame_gap(Some(15), 16, false, false),
            SplitFrameGapDecision {
                missing: 0,
                feed: true,
                awaiting_keyframe_after: false,
                signal: SplitGapSignal::None,
            }
        );
    }

    #[test]
    fn uint16_wrap_is_contiguous() {
        assert_eq!(
            decide_split_frame_gap(Some(u16::MAX), 0, false, false).missing,
            0
        );
    }

    #[test]
    fn split_receive_loop_drains_one_wifi_microburst_before_decoder_polling() {
        assert_eq!(split_receive_batch_limit(), 4);
    }
}
