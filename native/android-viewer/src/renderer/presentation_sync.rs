use std::collections::BTreeMap;
use std::sync::mpsc;
use viewer_decoder::ReadyOutput;

const MAX_PENDING_PER_TILE: usize = 1;
const MAX_RELEASED_METADATA_PER_TILE: usize = 2;
const MAX_PENDING_RELEASE_ACKS: usize = 4;
// Hold a leading tile for only one scheduler tick. A matching tile already in
// the coordinator queue is drained in the same loop; anything later may catch
// up independently without adding a visible 4ms receiver queue.
const DECODER_PAIR_WAIT_NS: i64 = 1_000_000;
const DECODER_LATE_PEER_GRACE_NS: i64 = 4_000_000;

pub fn drain_available_events<T>(first: T, receiver: &mpsc::Receiver<T>) -> Vec<T> {
    let mut events = vec![first];
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyFrame {
    pub output: ReadyOutput,
    pub pts_us: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerCommand {
    PresentAt {
        side: TileSide,
        output: ReadyOutput,
        target_present_ns: i64,
    },
    Discard {
        side: TileSide,
        output: ReadyOutput,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncDecision {
    Wait,
    Present {
        left: ReadyFrame,
        right: ReadyFrame,
        target_present_ns: i64,
    },
    PresentSingle {
        side: TileSide,
        frame: ReadyFrame,
        target_present_ns: i64,
        ready_delta_us: Option<u32>,
    },
    PresentMany {
        commands: Vec<WorkerCommand>,
    },
    Discard {
        commands: Vec<WorkerCommand>,
    },
}

#[derive(Debug, Clone, Copy)]
struct PendingFrame {
    frame: ReadyFrame,
    ready_ns: i64,
}

#[derive(Debug, Clone, Copy)]
struct ReleasedFrame {
    ready_ns: i64,
    target_present_ns: i64,
}

pub struct PairPresentationCoordinator {
    wait_budget_ns: i64,
    released_metadata_budget_ns: i64,
    left: BTreeMap<i64, PendingFrame>,
    right: BTreeMap<i64, PendingFrame>,
    released_left: BTreeMap<i64, ReleasedFrame>,
    released_right: BTreeMap<i64, ReleasedFrame>,
}

#[derive(Default)]
pub struct PairReleaseAcknowledgements {
    pending: BTreeMap<i64, u8>,
}

impl PairReleaseAcknowledgements {
    pub fn record(&mut self, side: TileSide, pts_us: i64, succeeded: bool) -> bool {
        if !succeeded {
            self.pending.remove(&pts_us);
            return false;
        }
        let side_bit = match side {
            TileSide::Left => 0b01,
            TileSide::Right => 0b10,
        };
        let acknowledged = self.pending.entry(pts_us).or_default();
        *acknowledged |= side_bit;
        if *acknowledged == 0b11 {
            self.pending.remove(&pts_us);
            return true;
        }
        while self.pending.len() > MAX_PENDING_RELEASE_ACKS {
            self.pending.pop_first();
        }
        false
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

impl PairPresentationCoordinator {
    pub fn new(fps: u32) -> Self {
        let frame_period_ns = 1_000_000_000i64 / i64::from(fps.max(1));
        Self {
            wait_budget_ns: frame_period_ns.min(DECODER_PAIR_WAIT_NS),
            released_metadata_budget_ns: frame_period_ns.saturating_add(DECODER_LATE_PEER_GRACE_NS),
            left: BTreeMap::new(),
            right: BTreeMap::new(),
            released_left: BTreeMap::new(),
            released_right: BTreeMap::new(),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.left.len() + self.right.len()
    }

    pub fn reset(&mut self) {
        self.left.clear();
        self.right.clear();
        self.released_left.clear();
        self.released_right.clear();
    }

    pub fn discard_pending(&mut self) -> SyncDecision {
        self.released_left.clear();
        self.released_right.clear();
        let commands = std::mem::take(&mut self.left)
            .into_values()
            .map(|pending| WorkerCommand::Discard {
                side: TileSide::Left,
                output: pending.frame.output,
            })
            .chain(
                std::mem::take(&mut self.right)
                    .into_values()
                    .map(|pending| WorkerCommand::Discard {
                        side: TileSide::Right,
                        output: pending.frame.output,
                    }),
            )
            .collect::<Vec<_>>();
        if commands.is_empty() {
            SyncDecision::Wait
        } else {
            SyncDecision::Discard { commands }
        }
    }

    pub fn push_ready(&mut self, side: TileSide, frame: ReadyFrame, now_ns: i64) -> SyncDecision {
        let released_peer = match side {
            TileSide::Left => self.released_right.remove(&frame.pts_us),
            TileSide::Right => self.released_left.remove(&frame.pts_us),
        };
        if let Some(peer) = released_peer {
            let ready_delta_us = now_ns.abs_diff(peer.ready_ns) / 1_000;
            return SyncDecision::PresentSingle {
                side,
                frame,
                target_present_ns: peer.target_present_ns.max(self.next_present_time(now_ns)),
                ready_delta_us: Some(ready_delta_us.min(u64::from(u32::MAX)) as u32),
            };
        }

        let matching_peer = match side {
            TileSide::Left => self.right.remove(&frame.pts_us),
            TileSide::Right => self.left.remove(&frame.pts_us),
        };
        if let Some(peer) = matching_peer {
            let (left, right) = match side {
                TileSide::Left => (frame, peer.frame),
                TileSide::Right => (peer.frame, frame),
            };
            return SyncDecision::Present {
                left,
                right,
                target_present_ns: self.next_present_time(now_ns),
            };
        }

        let pending = PendingFrame {
            frame,
            ready_ns: now_ns,
        };
        let slot = match side {
            TileSide::Left => &mut self.left,
            TileSide::Right => &mut self.right,
        };
        if let Some(replaced) = slot.insert(frame.pts_us, pending) {
            return SyncDecision::Discard {
                commands: vec![WorkerCommand::Discard {
                    side,
                    output: replaced.frame.output,
                }],
            };
        }
        if slot.len() > MAX_PENDING_PER_TILE {
            let oldest_pts = *slot.first_key_value().expect("bounded slot is non-empty").0;
            let oldest = slot.remove(&oldest_pts).expect("oldest pending exists");
            let target_present_ns = self.next_present_time(now_ns);
            let released = ReleasedFrame {
                ready_ns: oldest.ready_ns,
                target_present_ns,
            };
            let released_slot = match side {
                TileSide::Left => &mut self.released_left,
                TileSide::Right => &mut self.released_right,
            };
            released_slot.insert(oldest_pts, released);
            while released_slot.len() > MAX_RELEASED_METADATA_PER_TILE {
                released_slot.pop_first();
            }
            return SyncDecision::PresentSingle {
                side,
                frame: oldest.frame,
                target_present_ns,
                ready_delta_us: None,
            };
        }
        SyncDecision::Wait
    }

    pub fn expire(&mut self, now_ns: i64) -> SyncDecision {
        let mut commands = Vec::new();
        let target_present_ns = self.next_present_time(now_ns);
        for (side, pending) in [
            (TileSide::Left, &mut self.left),
            (TileSide::Right, &mut self.right),
        ] {
            let expired = pending
                .iter()
                .filter_map(|(pts, frame)| {
                    (now_ns.saturating_sub(frame.ready_ns) >= self.wait_budget_ns).then_some(*pts)
                })
                .collect::<Vec<_>>();
            for pts in expired {
                let frame = pending.remove(&pts).expect("expired pending exists");
                let released = ReleasedFrame {
                    ready_ns: frame.ready_ns,
                    target_present_ns,
                };
                let released_slot = match side {
                    TileSide::Left => &mut self.released_left,
                    TileSide::Right => &mut self.released_right,
                };
                released_slot.insert(pts, released);
                while released_slot.len() > MAX_RELEASED_METADATA_PER_TILE {
                    released_slot.pop_first();
                }
                commands.push(WorkerCommand::PresentAt {
                    side,
                    output: frame.frame.output,
                    target_present_ns,
                });
            }
        }
        for released in [&mut self.released_left, &mut self.released_right] {
            let expired = released
                .iter()
                .filter_map(|(pts, frame)| {
                    (now_ns.saturating_sub(frame.ready_ns) >= self.released_metadata_budget_ns)
                        .then_some(*pts)
                })
                .collect::<Vec<_>>();
            for pts in expired {
                released.remove(&pts);
            }
        }
        if commands.is_empty() {
            SyncDecision::Wait
        } else {
            SyncDecision::PresentMany { commands }
        }
    }

    fn next_present_time(&self, now_ns: i64) -> i64 {
        // MediaCodec and SurfaceFlinger already latch timed releases on the
        // next display opportunity. Rounding to an artificial 60Hz epoch here
        // could add almost one full frame before the real compositor wait.
        now_ns.saturating_add(1_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_decoder::ReadyOutput;

    fn ready(pts_us: i64) -> ReadyFrame {
        ReadyFrame {
            output: ReadyOutput {
                index: pts_us as usize,
                pts_us,
            },
            pts_us,
        }
    }

    #[test]
    fn matching_pts_release_both_at_the_same_time() {
        let mut sync = PairPresentationCoordinator::new(60);
        assert_eq!(
            sync.push_ready(TileSide::Left, ready(7), 1_000),
            SyncDecision::Wait
        );
        let decision = sync.push_ready(TileSide::Right, ready(7), 1_200);
        let SyncDecision::Present {
            left,
            right,
            target_present_ns,
        } = decision
        else {
            panic!("matching pair was not presented")
        };
        assert_eq!(left.pts_us, right.pts_us);
        assert_eq!(target_present_ns, 1_001_200);
    }

    #[test]
    fn unmatched_frame_is_presented_after_one_millisecond_jitter_grace() {
        let mut sync = PairPresentationCoordinator::new(60);
        sync.push_ready(TileSide::Left, ready(8), 1_000);
        assert!(matches!(sync.expire(1_000_999), SyncDecision::Wait));
        let SyncDecision::PresentMany { commands } = sync.expire(1_001_000) else {
            panic!("expired output must advance its Surface instead of being discarded")
        };
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            WorkerCommand::PresentAt {
                side: TileSide::Left,
                ..
            }
        ));
    }

    #[test]
    fn late_peer_catches_up_within_one_frame_after_leading_tile_is_presented() {
        let mut sync = PairPresentationCoordinator::new(60);
        sync.push_ready(TileSide::Left, ready(8), 1_000);
        assert!(matches!(
            sync.expire(1_001_000),
            SyncDecision::PresentMany { .. }
        ));
        let SyncDecision::PresentSingle {
            side,
            frame,
            ready_delta_us,
            ..
        } = sync.push_ready(TileSide::Right, ready(8), 15_000_000)
        else {
            panic!("late peer must catch up without holding another decoder output")
        };
        assert_eq!(side, TileSide::Right);
        assert_eq!(frame.pts_us, 8);
        assert_eq!(ready_delta_us, Some(14_999));
    }

    #[test]
    fn recovery_discards_every_output_still_owned_by_the_coordinator() {
        let mut coordinator = PairPresentationCoordinator::new(60);
        assert!(matches!(
            coordinator.push_ready(TileSide::Left, ready(9), 1_000),
            SyncDecision::Wait
        ));
        let SyncDecision::Discard { commands } = coordinator.discard_pending() else {
            panic!("pending output must be returned to its decoder worker");
        };
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            WorkerCommand::Discard {
                side: TileSide::Left,
                ..
            }
        ));
        assert!(matches!(coordinator.discard_pending(), SyncDecision::Wait));
    }

    #[test]
    fn different_pts_wait_for_their_exact_peers_within_the_frame_budget() {
        let mut sync = PairPresentationCoordinator::new(60);
        sync.push_ready(TileSide::Left, ready(9), 1_000);
        assert_eq!(
            sync.push_ready(TileSide::Right, ready(10), 1_100),
            SyncDecision::Wait
        );
        assert_eq!(sync.pending_count(), 2);
    }

    #[test]
    fn one_frame_same_side_burst_presents_the_old_output_without_holding_it() {
        let mut sync = PairPresentationCoordinator::new(60);
        assert_eq!(
            sync.push_ready(TileSide::Left, ready(40), 1_000_000),
            SyncDecision::Wait
        );
        let SyncDecision::PresentSingle {
            side,
            frame,
            target_present_ns,
            ready_delta_us,
        } = sync.push_ready(TileSide::Left, ready(41), 1_100_000)
        else {
            panic!("the old output must be scheduled instead of discarded")
        };
        assert_eq!(side, TileSide::Left);
        assert_eq!(frame.pts_us, 40);
        assert!(target_present_ns > 1_100_000);
        assert_eq!(ready_delta_us, None);
        assert_eq!(sync.pending_count(), 1);

        let SyncDecision::PresentSingle {
            side,
            frame,
            target_present_ns: peer_target_ns,
            ready_delta_us,
        } = sync.push_ready(TileSide::Right, ready(40), 1_500_000)
        else {
            panic!("the late peer must catch up without entering the pending queue")
        };
        assert_eq!(side, TileSide::Right);
        assert_eq!(frame.pts_us, 40);
        assert!(peer_target_ns >= target_present_ns);
        assert_eq!(ready_delta_us, Some(500));
        assert_eq!(sync.pending_count(), 1);

        assert!(matches!(
            sync.push_ready(TileSide::Right, ready(41), 1_600_000),
            SyncDecision::Present { .. }
        ));
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn sustained_same_side_burst_keeps_only_one_decoder_output_owned() {
        let mut sync = PairPresentationCoordinator::new(60);
        sync.push_ready(TileSide::Left, ready(40), 1_000);
        let first = sync.push_ready(TileSide::Left, ready(41), 1_100);
        let second = sync.push_ready(TileSide::Left, ready(42), 1_200);
        assert!(matches!(first, SyncDecision::PresentSingle { .. }));
        assert!(matches!(second, SyncDecision::PresentSingle { .. }));
        assert_eq!(sync.pending_count(), 1);
        assert!(matches!(
            sync.push_ready(TileSide::Right, ready(40), 1_300),
            SyncDecision::PresentSingle { .. }
        ));
        assert_eq!(sync.pending_count(), 1);
    }

    #[test]
    fn joined_frame_requires_successful_acknowledgements_from_both_workers() {
        let mut acknowledgements = PairReleaseAcknowledgements::default();
        assert!(!acknowledgements.record(TileSide::Left, 55, true));
        assert!(acknowledgements.record(TileSide::Right, 55, true));
        assert_eq!(acknowledgements.pending_count(), 0);

        assert!(!acknowledgements.record(TileSide::Left, 56, true));
        assert!(!acknowledgements.record(TileSide::Right, 56, false));
        assert_eq!(acknowledgements.pending_count(), 0);
    }

    #[test]
    fn release_acknowledgements_remain_bounded_if_one_worker_stalls() {
        let mut acknowledgements = PairReleaseAcknowledgements::default();
        for pts_us in 0..10 {
            assert!(!acknowledgements.record(TileSide::Left, pts_us, true));
        }
        assert_eq!(acknowledgements.pending_count(), 4);
    }

    #[test]
    fn drains_an_already_queued_peer_before_pair_expiration() {
        let (tx, rx) = mpsc::channel();
        tx.send((TileSide::Right, ready(42), 1_200)).unwrap();

        let events = drain_available_events((TileSide::Left, ready(42), 1_000), &rx);
        assert_eq!(events.len(), 2);

        let mut sync = PairPresentationCoordinator::new(60);
        let mut presented = false;
        for (side, frame, ready_ns) in events {
            if matches!(
                sync.push_ready(side, frame, ready_ns),
                SyncDecision::Present { .. }
            ) {
                presented = true;
            }
        }
        assert!(presented);
        assert!(matches!(sync.expire(20_000_000), SyncDecision::Wait));
    }
}
