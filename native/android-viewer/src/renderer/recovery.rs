use crate::renderer::presentation_sync::TileSide;

const RECOVERY_COOLDOWN_NS: u64 = 750_000_000;

/// V2 dispatch: the paired IDR request leaves through exactly ONE tile
/// socket per coordinator action — the request-origin path when usable, with
/// the coordinator's dispatch ledger owning a bounded single alternate-path
/// retry when the selected path reports itself unready. The Host control
/// listener accepts "IDR" from either viewer socket and coalesces concurrent
/// PLIs ONLY while `splitFlowState.recoveryBoundaryPending` is true; once
/// that recovery boundary completes, a delayed PLI from the second port
/// starts a NEW Host recovery generation (`beginRecovery()` increments the
/// generation), erasing the pair that just resumed. Two near-simultaneous
/// copies per action (the rejected V1 BOTH plan) make that race reachable
/// from the viewer itself, so both copies per action are forbidden here.
/// Returns the single wire path for the action, or `None` for actions that
/// never touch the wire.
pub const fn dispatch_plan(action: RecoveryAction, origin: TileSide) -> Option<TileSide> {
    match action {
        RecoveryAction::RequestPair => Some(origin),
        RecoveryAction::Suppress | RecoveryAction::WaitForPeer | RecoveryAction::ResumePair => None,
    }
}

/// Paired generations are the shared Host frame sequence observed as a raw
/// u16 on each wire, so "newer" must follow that sequence across its wrap
/// (serial-number arithmetic in the u16 domain). Equal generations are never
/// "newer"; a distance in the ambiguous far half keeps the incumbent, which
/// only ever costs one more retry tick and never skips a valid pair.
fn generation_is_newer(candidate: u64, incumbent: u64) -> bool {
    let forward = candidate.wrapping_sub(incumbent) & u64::from(u16::MAX);
    forward != 0 && forward <= u64::from(u16::MAX / 2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    RequestPair,
    Suppress,
    WaitForPeer,
    ResumePair,
}

#[derive(Debug, Default)]
pub struct PairedRecoveryGate {
    active: bool,
    last_request_ns: Option<u64>,
    left_idr_generation: Option<u64>,
    right_idr_generation: Option<u64>,
    /// Episode ownership id. Bumped at every episode start and at every
    /// episode close (pair resumed or superseded), so any request stamped
    /// with a retired id is stale and must be cancelled before the wire —
    /// that is what keeps a queued request from becoming a late pending PLI
    /// after the pair resumes (which would start a new Host recovery
    /// generation, because the Host only coalesces PLIs while its recovery
    /// boundary is pending).
    episode: u64,
}

impl PairedRecoveryGate {
    /// Current episode id. `0` means no episode has ever started.
    pub fn episode(&self) -> u64 {
        self.episode
    }

    fn begin_episode(&mut self) {
        self.episode = self.episode.wrapping_add(1);
    }
}

impl PairedRecoveryGate {
    pub fn start_initial(&mut self, now_ns: u64) -> RecoveryAction {
        self.active = true;
        self.last_request_ns = Some(now_ns);
        self.left_idr_generation = None;
        self.right_idr_generation = None;
        self.begin_episode();
        RecoveryAction::RequestPair
    }

    pub fn on_loss(&mut self, _side: TileSide, now_ns: u64) -> RecoveryAction {
        if self.active {
            // A decoder can reject an input immediately after its first IDR
            // while the peer IDR is still queued. That partial generation can
            // no longer resume as a valid pair, so restart it immediately.
            if self.left_idr_generation.is_some() || self.right_idr_generation.is_some() {
                self.left_idr_generation = None;
                self.right_idr_generation = None;
                self.last_request_ns = Some(now_ns);
                self.begin_episode();
                return RecoveryAction::RequestPair;
            }
            return RecoveryAction::Suppress;
        }
        // A matched startup IDR can be followed immediately by a real packet
        // gap. `active` already coalesces duplicate losses; reusing the prior
        // request cooldown here leaves both decoders waiting forever because
        // no recovery is activated and therefore no retry timer runs.
        self.active = true;
        self.last_request_ns = Some(now_ns);
        self.left_idr_generation = None;
        self.right_idr_generation = None;
        self.begin_episode();
        RecoveryAction::RequestPair
    }

    pub fn on_idr(&mut self, side: TileSide, generation: u64) -> RecoveryAction {
        if !self.active {
            return RecoveryAction::Suppress;
        }
        match side {
            TileSide::Left => self.left_idr_generation = Some(generation),
            TileSide::Right => self.right_idr_generation = Some(generation),
        }
        match (self.left_idr_generation, self.right_idr_generation) {
            (Some(left), Some(right)) if left == right => {
                self.active = false;
                // Close the episode: any request still queued behind the
                // completed pair is now stale and will be cancelled by the
                // workers instead of transmitted.
                self.begin_episode();
                RecoveryAction::ResumePair
            }
            (Some(left), Some(right)) => {
                // Keep the newer tile generation (u16-wrap aware). Clearing
                // both here creates an ordering race: if one tile receives
                // generation N before its peer, the peer's later N can never
                // complete the pair.
                if generation_is_newer(left, right) {
                    self.right_idr_generation = None;
                } else {
                    self.left_idr_generation = None;
                }
                RecoveryAction::WaitForPeer
            }
            _ => RecoveryAction::WaitForPeer,
        }
    }

    pub fn retry_due(&mut self, now_ns: u64) -> RecoveryAction {
        if !self.active {
            return RecoveryAction::Suppress;
        }
        let due = self
            .last_request_ns
            .is_none_or(|last| now_ns.saturating_sub(last) >= RECOVERY_COOLDOWN_NS);
        if !due {
            return RecoveryAction::Suppress;
        }
        self.last_request_ns = Some(now_ns);
        RecoveryAction::RequestPair
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_tile_loss_requests_one_paired_idr() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_000),
            RecoveryAction::RequestPair
        );
        assert_eq!(
            gate.on_loss(TileSide::Left, 1_100),
            RecoveryAction::Suppress
        );
        assert_eq!(gate.on_idr(TileSide::Left, 3), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 3), RecoveryAction::ResumePair);
    }

    #[test]
    fn active_recovery_retries_one_paired_idr_after_cooldown() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Left, 1_000),
            RecoveryAction::RequestPair
        );
        assert_eq!(gate.retry_due(750_000_999), RecoveryAction::Suppress);
        assert_eq!(gate.retry_due(750_001_000), RecoveryAction::RequestPair);
        assert_eq!(gate.retry_due(750_001_001), RecoveryAction::Suppress);

        assert_eq!(gate.on_idr(TileSide::Left, 7), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 7), RecoveryAction::ResumePair);
        assert_eq!(gate.retry_due(1_500_001_000), RecoveryAction::Suppress);
    }

    #[test]
    fn initial_recovery_retries_until_both_tiles_receive_the_same_idr() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(1_000), RecoveryAction::RequestPair);
        assert_eq!(
            gate.on_idr(TileSide::Right, 24),
            RecoveryAction::WaitForPeer
        );
        assert_eq!(gate.retry_due(750_001_000), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Left, 45), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 45), RecoveryAction::ResumePair);
        assert_eq!(gate.retry_due(1_500_001_000), RecoveryAction::Suppress);
    }

    #[test]
    fn loss_immediately_after_initial_pair_starts_a_new_recovery() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(1_000), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Left, 5), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 5), RecoveryAction::ResumePair);
        assert_eq!(
            gate.on_loss(TileSide::Left, 1_100),
            RecoveryAction::RequestPair
        );
    }

    #[test]
    fn wrapped_generation_from_the_peer_is_kept_as_the_newer_recovery_target() {
        // Frame ids are raw u16 wire sequence numbers and wrap every ~18min
        // at 60 FPS. A recovery episode spanning the wrap must follow the
        // sequence, not the u64 magnitudes: generation 0 is NEWER than
        // 65535, so the frozen tile's wrapped IDR must be retained and must
        // still complete the pair when its peer reports the same frame.
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(1_000), RecoveryAction::RequestPair);
        assert_eq!(
            gate.on_idr(TileSide::Left, u64::from(u16::MAX)),
            RecoveryAction::WaitForPeer
        );
        assert_eq!(gate.on_idr(TileSide::Right, 0), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Left, 0), RecoveryAction::ResumePair);
    }

    #[test]
    fn request_pair_selects_exactly_one_wire_path_per_action() {
        // V1 BOTH dispatch is rejected: the Host coalesces viewer PLIs only
        // while its recovery boundary is pending, so a delayed PLI from the
        // second port — arriving after the first recovery boundary completed
        // — starts a NEW Host generation and erases the resumed pair. The
        // paired request must therefore carry exactly one selected
        // request-origin path per action, and non-request actions must never
        // touch the wire.
        assert_eq!(
            dispatch_plan(RecoveryAction::RequestPair, TileSide::Left),
            Some(TileSide::Left)
        );
        assert_eq!(
            dispatch_plan(RecoveryAction::RequestPair, TileSide::Right),
            Some(TileSide::Right)
        );
        for action in [
            RecoveryAction::Suppress,
            RecoveryAction::WaitForPeer,
            RecoveryAction::ResumePair,
        ] {
            assert_eq!(dispatch_plan(action, TileSide::Left), None);
        }
    }

    #[test]
    fn episode_identity_is_fresh_per_episode_and_survives_only_that_episode() {
        // Episode ownership: every episode start (initial, loss-activated,
        // partial-restart) issues a fresh id, retries keep the id of the
        // episode they belong to, and completing the pair closes the episode
        // by retiring its id so any queued-but-unprocessed request becomes
        // stale and must be cancelled before the wire.
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.episode(), 0);
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        let initial_episode = gate.episode();
        assert_ne!(initial_episode, 0);
        // A cadence retry is the same episode: it must not invalidate the
        // request it retries.
        assert_eq!(gate.retry_due(750_000_000), RecoveryAction::RequestPair);
        assert_eq!(gate.episode(), initial_episode);
        assert_eq!(gate.on_idr(TileSide::Left, 3), RecoveryAction::WaitForPeer);
        assert_eq!(gate.episode(), initial_episode);
        assert_eq!(gate.on_idr(TileSide::Right, 3), RecoveryAction::ResumePair);
        assert_ne!(gate.episode(), initial_episode);
    }

    #[test]
    fn every_new_recovery_episode_issues_a_fresh_identity() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        let first = gate.episode();
        // Partial generation restart: a new episode id must invalidate any
        // request stamped with the dead episode.
        assert_eq!(gate.on_idr(TileSide::Left, 5), RecoveryAction::WaitForPeer);
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_050),
            RecoveryAction::RequestPair
        );
        let second = gate.episode();
        assert_ne!(second, first);
        assert_eq!(gate.on_idr(TileSide::Right, 7), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Left, 7), RecoveryAction::ResumePair);
        let closed = gate.episode();
        assert_ne!(closed, second);
        // Loss after the pair resumed: another fresh episode.
        assert_eq!(
            gate.on_loss(TileSide::Left, 2_000),
            RecoveryAction::RequestPair
        );
        assert_ne!(gate.episode(), closed);
    }

    #[test]
    fn coalesced_and_mismatched_states_keep_the_current_episode_identity() {
        // Suppress (duplicate loss while active) and WaitForPeer (mismatched
        // or half-arrived IDR) are episode-internal states: they must not
        // invalidate in-flight requests for the running episode.
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        let episode = gate.episode();
        assert_eq!(
            gate.on_loss(TileSide::Right, 100_000),
            RecoveryAction::Suppress
        );
        assert_eq!(gate.episode(), episode);
        assert_eq!(gate.on_idr(TileSide::Left, 9), RecoveryAction::WaitForPeer);
        assert_eq!(gate.episode(), episode);
        assert_eq!(
            gate.on_idr(TileSide::Right, 11),
            RecoveryAction::WaitForPeer
        );
        assert_eq!(gate.episode(), episode);
    }

    #[test]
    fn duplicate_losses_during_one_episode_stay_coalesced_on_the_fixed_cadence() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.on_loss(TileSide::Left, 0), RecoveryAction::RequestPair);
        // Repeated loss signals while active without any IDR generation must
        // neither restart the episode nor move the retry deadline: exactly
        // one request per 750ms tick, so a sustained loss burst cannot become
        // a PLI storm.
        for offset_ms in [100u64, 200, 300, 400, 500, 600] {
            assert_eq!(
                gate.on_loss(TileSide::Right, offset_ms * 1_000_000),
                RecoveryAction::Suppress
            );
        }
        assert_eq!(gate.retry_due(749_999_999), RecoveryAction::Suppress);
        assert_eq!(gate.retry_due(750_000_000), RecoveryAction::RequestPair);
        assert_eq!(gate.retry_due(750_000_001), RecoveryAction::Suppress);
        assert_eq!(gate.retry_due(1_500_000_000), RecoveryAction::RequestPair);
        assert_eq!(gate.retry_due(1_500_000_001), RecoveryAction::Suppress);
    }

    #[test]
    fn mismatch_bookkeeping_never_delays_the_next_scheduled_retry() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Left, 5), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 7), RecoveryAction::WaitForPeer);
        // Unmatched generations resolve only through newer pairs; they must
        // not push the 750ms retry deadline backwards.
        assert_eq!(gate.retry_due(750_000_000), RecoveryAction::RequestPair);
    }

    #[test]
    fn late_peer_idr_completes_a_quiet_stream_recovery_without_a_new_pair() {
        // Quiet-stream incident shape: one tile receives the recovery IDR
        // immediately, the stream goes silent, and the peer's copy arrives
        // much later. The held generation must stay completable by the late
        // peer instead of forcing a fresh pair.
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Left, 10), RecoveryAction::WaitForPeer);
        assert_eq!(gate.retry_due(750_000_000), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Right, 10), RecoveryAction::ResumePair);
        assert_eq!(gate.retry_due(1_500_000_000), RecoveryAction::Suppress);
    }

    #[test]
    fn loss_after_one_recovery_idr_restarts_the_pair_generation() {
        let mut gate = PairedRecoveryGate::default();
        assert_eq!(gate.start_initial(1_000), RecoveryAction::RequestPair);
        assert_eq!(
            gate.on_idr(TileSide::Right, 33),
            RecoveryAction::WaitForPeer
        );
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_050),
            RecoveryAction::RequestPair
        );
        assert_eq!(gate.on_idr(TileSide::Left, 33), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Left, 45), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 45), RecoveryAction::ResumePair);
    }
}
