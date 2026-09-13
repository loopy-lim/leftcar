use crate::renderer::presentation_sync::TileSide;

const RECOVERY_COOLDOWN_NS: u64 = 750_000_000;

/// Two losses inside this window are one burst: the second tile's gap
/// converts the first tile's per-tile recovery episode into a PAIRED episode
/// (today's both-generation resume). Outside the window the tiles recover
/// independently.
pub const PAIRED_LOSS_WINDOW_NS: u64 = 100_000_000;

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
/// never touch the paired wire path — including the per-tile actions, whose
/// wire path is semantically FIXED to the gapped tile's own socket (the Host
/// identifies the requesting tile by source port) and never uses the paired
/// origin/alternate routing.
pub const fn dispatch_plan(action: RecoveryAction, origin: TileSide) -> Option<TileSide> {
    match action {
        RecoveryAction::RequestPair => Some(origin),
        RecoveryAction::RequestTile(_)
        | RecoveryAction::Suppress
        | RecoveryAction::WaitForPeer
        | RecoveryAction::ResumePair
        | RecoveryAction::ResumeTile(_) => None,
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
    /// Issue ONE "IDR" through the gapped tile's OWN socket (per-tile
    /// recovery). The Host resolves the requesting side by source port.
    RequestTile(TileSide),
    Suppress,
    WaitForPeer,
    ResumePair,
    /// The gapped tile's decoder saw its new keyframe: resume that tile
    /// ALONE and close its episode. The peer is never consulted.
    ResumeTile(TileSide),
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

    /// True while a paired episode is open (startup request, gap-activated
    /// request, or partial restart).
    pub fn is_active(&self) -> bool {
        self.active
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

/// One open per-tile recovery episode (experiment 3 / R2 per-tile decoupled
/// gap recovery). The gapped tile freezes alone, issues its IDR through its
/// OWN socket, and resumes alone when its decoder sees the new keyframe.
#[derive(Debug)]
struct TileRecoveryEpisode {
    #[allow(dead_code)]
    side: TileSide,
    started_ns: u64,
    last_request_ns: u64,
    idr_generation: Option<u64>,
    episode: u64,
}

/// Split recovery coordinator gate: PAIRED episodes (startup, burst loss on
/// both tiles inside [`PAIRED_LOSS_WINDOW_NS`], partial-generation restarts —
/// delegated to the unchanged [`PairedRecoveryGate`]) plus per-tile episodes
/// that freeze, request, and resume ONE tile independently. Invariants:
/// - a paired episode and a tile episode never coexist (`start_initial` and
///   window conversion clear tile slots; `on_loss` delegates to the paired
///   gate while it is active);
/// - an unexpected keyframe with no open episode (a peer tile's IDR from an
///   old host that answered a per-tile request with a PAIRED keyframe, a
///   straggler, or a spontaneous IDR) opens nothing and suppresses — the
///   tile worker already fed the keyframe, the presentation pairing accepts
///   keyframe-carrying pairs like any other, and frame ids stay continuous;
/// - every episode (paired or tile) owns a fresh episode id; closing it
///   retires the id so the dispatch ledger cancels any queued wire copy
///   before transmission (no late pending PLI after a resume).
#[derive(Debug, Default)]
pub struct SplitRecoveryGate {
    paired: PairedRecoveryGate,
    tiles: [Option<TileRecoveryEpisode>; 2],
    next_tile_episode: u64,
}

impl SplitRecoveryGate {
    /// Paired-scope episode id (dispatch ledger + worker ownership for
    /// paired requests).
    pub fn paired_episode(&self) -> u64 {
        self.paired.episode()
    }

    /// Per-tile-scope episode id for one side. `0` = no open episode; the
    /// coordinator publishes it per side so a closed episode's queued
    /// request is cancelled before any wire access.
    pub fn tile_episode(&self, side: TileSide) -> u64 {
        self.tiles[side.slot()]
            .as_ref()
            .map(|episode| episode.episode)
            .unwrap_or(0)
    }

    fn mint_tile_episode(&mut self) -> u64 {
        // Tile ids stay strictly ahead of the paired counter so the two id
        // spaces never collide (workers still disambiguate by scope register
        // and the globally unique request id; this just keeps stamps
        // readable).
        self.next_tile_episode = self
            .next_tile_episode
            .max(self.paired.episode())
            .wrapping_add(1)
            .max(1);
        self.next_tile_episode
    }

    /// Startup: both decoders need the first keyframe pair, so this stays a
    /// PAIRED episode exactly like the pre-R2 behavior.
    pub fn start_initial(&mut self, now_ns: u64) -> RecoveryAction {
        self.tiles = [None, None];
        self.paired.start_initial(now_ns)
    }

    pub fn on_loss(&mut self, side: TileSide, now_ns: u64) -> RecoveryAction {
        if self.paired.is_active() {
            return self.paired.on_loss(side, now_ns);
        }
        // The peer opened a per-tile episode moments ago: one burst hit both
        // tiles, so recover as a pair (today's both-generation resume). The
        // peer's episode is retired — its queued-but-untransmitted request
        // becomes stale and is cancelled before the wire.
        if let Some(peer_episode) = self.tiles[side.peer().slot()].as_ref() {
            if now_ns.saturating_sub(peer_episode.started_ns) <= PAIRED_LOSS_WINDOW_NS {
                self.tiles = [None, None];
                return self.paired.on_loss(side, now_ns);
            }
        }
        if self.tiles[side.slot()].is_some() {
            // This tile's episode already owns the recovery: coalesce, the
            // unchanged 750ms cadence retries.
            return RecoveryAction::Suppress;
        }
        let episode = self.mint_tile_episode();
        self.tiles[side.slot()] = Some(TileRecoveryEpisode {
            side,
            started_ns: now_ns,
            last_request_ns: now_ns,
            idr_generation: None,
            episode,
        });
        RecoveryAction::RequestTile(side)
    }

    pub fn on_idr(&mut self, side: TileSide, generation: u64) -> RecoveryAction {
        if self.paired.is_active() {
            return self.paired.on_idr(side, generation);
        }
        if self.tiles[side.slot()].is_some() {
            // Per-tile resume: THIS tile's decoder saw its new keyframe.
            // The peer is never waited for — it either kept streaming or
            // runs its own episode. Closing the episode retires its id, so
            // any still-queued retry request is cancelled pre-wire.
            if let Some(episode) = self.tiles[side.slot()].as_mut() {
                episode.idr_generation = Some(generation);
            }
            self.tiles[side.slot()] = None;
            return RecoveryAction::ResumeTile(side);
        }
        RecoveryAction::Suppress
    }

    /// Cadence tick across all open episodes. Returns at most one action
    /// per scope: the paired gate's decision plus one retry per open tile
    /// episode (each side's cooldown is independent).
    pub fn retry_due(&mut self, now_ns: u64) -> Vec<RecoveryAction> {
        let mut actions = Vec::new();
        if self.paired.retry_due(now_ns) == RecoveryAction::RequestPair {
            actions.push(RecoveryAction::RequestPair);
        }
        for slot in 0..self.tiles.len() {
            let due = self.tiles[slot].as_ref().is_some_and(|episode| {
                now_ns.saturating_sub(episode.last_request_ns) >= RECOVERY_COOLDOWN_NS
            });
            if due {
                let episode = self.tiles[slot].as_mut().expect("due episode exists");
                episode.last_request_ns = now_ns;
                actions.push(RecoveryAction::RequestTile(episode.side));
            }
        }
        actions
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

#[cfg(test)]
mod per_tile_tests {
    use super::*;

    #[test]
    fn single_tile_gap_recovers_and_resumes_alone() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_000),
            RecoveryAction::RequestTile(TileSide::Right)
        );
        let right_episode = gate.tile_episode(TileSide::Right);
        assert_ne!(right_episode, 0);
        assert_eq!(gate.tile_episode(TileSide::Left), 0);
        // The tile resumes on ITS OWN keyframe; the peer is not consulted.
        assert_eq!(
            gate.on_idr(TileSide::Right, 40),
            RecoveryAction::ResumeTile(TileSide::Right)
        );
        // Closing the episode retires its id: queued retries are cancelled.
        assert_eq!(gate.tile_episode(TileSide::Right), 0);
        // The peer's keyframe from the same (unrequested) recovery arrives
        // late: no episode is open, so nothing may restart.
        assert_eq!(gate.on_idr(TileSide::Left, 40), RecoveryAction::Suppress);
    }

    #[test]
    fn peer_gap_inside_the_window_converts_to_a_paired_episode() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Left, 1_000),
            RecoveryAction::RequestTile(TileSide::Left)
        );
        assert_ne!(gate.tile_episode(TileSide::Left), 0);
        // One burst, both tiles: the second loss converts to today's paired
        // recovery (both-generation resume).
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_000 + PAIRED_LOSS_WINDOW_NS),
            RecoveryAction::RequestPair
        );
        assert_eq!(gate.tile_episode(TileSide::Left), 0);
        assert_eq!(gate.tile_episode(TileSide::Right), 0);
        assert_ne!(
            gate.paired_episode(),
            0,
            "the conversion opened a fresh paired episode"
        );
        assert_eq!(gate.on_idr(TileSide::Left, 7), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 7), RecoveryAction::ResumePair);
    }

    #[test]
    fn peer_gap_outside_the_window_runs_an_independent_per_tile_episode() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Left, 0),
            RecoveryAction::RequestTile(TileSide::Left)
        );
        assert_eq!(
            gate.on_loss(TileSide::Right, PAIRED_LOSS_WINDOW_NS + 1),
            RecoveryAction::RequestTile(TileSide::Right)
        );
        // Each tile resumes independently, in either order.
        assert_eq!(
            gate.on_idr(TileSide::Right, 30),
            RecoveryAction::ResumeTile(TileSide::Right)
        );
        assert_eq!(gate.tile_episode(TileSide::Left), 1);
        assert_eq!(
            gate.on_idr(TileSide::Left, 31),
            RecoveryAction::ResumeTile(TileSide::Left)
        );
        assert_eq!(gate.tile_episode(TileSide::Left), 0);
    }

    #[test]
    fn duplicate_loss_inside_a_per_tile_episode_is_suppressed() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Left, 0),
            RecoveryAction::RequestTile(TileSide::Left)
        );
        for offset_ms in [10u64, 30, 60, 90] {
            assert_eq!(
                gate.on_loss(TileSide::Left, offset_ms * 1_000_000),
                RecoveryAction::Suppress
            );
        }
    }

    #[test]
    fn per_tile_episode_cooldown_retries_through_its_own_side_only() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Left, 0),
            RecoveryAction::RequestTile(TileSide::Left)
        );
        assert!(gate.retry_due(749_999_999).is_empty());
        assert_eq!(
            gate.retry_due(750_000_000),
            vec![RecoveryAction::RequestTile(TileSide::Left)]
        );
        // Second tile gaps independently (outside the window): its cooldown
        // is its own (750ms after its own request, not a shared timer).
        assert_eq!(
            gate.on_loss(TileSide::Right, 800_000_000),
            RecoveryAction::RequestTile(TileSide::Right)
        );
        assert_eq!(
            gate.retry_due(1_500_000_000),
            vec![RecoveryAction::RequestTile(TileSide::Left)]
        );
        assert_eq!(
            gate.retry_due(1_550_000_000),
            vec![RecoveryAction::RequestTile(TileSide::Right)]
        );
        // Left resumes; its retries stop for good (episode closed, queued
        // copies cancelled) while Right's own cadence continues.
        assert_eq!(
            gate.on_idr(TileSide::Left, 60),
            RecoveryAction::ResumeTile(TileSide::Left)
        );
        assert!(gate.retry_due(2_250_000_000).is_empty());
        assert_eq!(
            gate.retry_due(2_300_000_000),
            vec![RecoveryAction::RequestTile(TileSide::Right)]
        );
    }

    #[test]
    fn old_host_paired_answer_heals_the_gapped_tile_per_tile() {
        // An OLD host answers the per-tile "IDR" with a PAIRED keyframe. The
        // gapped tile resumes per-tile on its own keyframe; the peer's
        // unrequested mid-stream keyframe opens nothing and breaks nothing.
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(
            gate.on_loss(TileSide::Right, 0),
            RecoveryAction::RequestTile(TileSide::Right)
        );
        assert_eq!(
            gate.on_idr(TileSide::Left, 50),
            RecoveryAction::Suppress,
            "peer keyframe must not open or disturb an episode"
        );
        assert_eq!(
            gate.on_idr(TileSide::Right, 50),
            RecoveryAction::ResumeTile(TileSide::Right)
        );
    }

    #[test]
    fn unexpected_keyframe_without_any_episode_never_starts_one() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(gate.on_idr(TileSide::Left, 3), RecoveryAction::Suppress);
        assert_eq!(gate.tile_episode(TileSide::Left), 0);
        assert_eq!(gate.tile_episode(TileSide::Right), 0);
        // A later real gap still starts a fresh per-tile episode.
        assert_eq!(
            gate.on_loss(TileSide::Left, 1_000),
            RecoveryAction::RequestTile(TileSide::Left)
        );
    }

    #[test]
    fn paired_episode_still_coalesces_and_resumes_on_matching_generations() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_000),
            RecoveryAction::Suppress,
            "paired gate delegation keeps today's coalescing"
        );
        assert_eq!(gate.on_idr(TileSide::Right, 5), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Left, 5), RecoveryAction::ResumePair);
        // After the pair resumes a fresh gap starts a per-tile episode.
        assert_eq!(
            gate.on_loss(TileSide::Left, 2_000),
            RecoveryAction::RequestTile(TileSide::Left)
        );
    }

    #[test]
    fn paired_partial_restart_keeps_today_semantics() {
        let mut gate = SplitRecoveryGate::default();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        assert_eq!(gate.on_idr(TileSide::Left, 5), RecoveryAction::WaitForPeer);
        // Loss while one generation already landed: the paired gate's
        // partial restart fires unchanged through the delegation.
        assert_eq!(
            gate.on_loss(TileSide::Right, 1_050),
            RecoveryAction::RequestPair
        );
        assert_eq!(gate.on_idr(TileSide::Left, 7), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 7), RecoveryAction::ResumePair);
    }

    #[test]
    fn per_tile_requests_never_use_the_paired_dispatch_plan() {
        // The per-tile wire path is FIXED to the gapped side's own socket;
        // the paired origin/alternate routing must never select it.
        assert_eq!(
            dispatch_plan(
                RecoveryAction::RequestTile(TileSide::Right),
                TileSide::Right
            ),
            None
        );
        assert_eq!(
            dispatch_plan(RecoveryAction::ResumeTile(TileSide::Left), TileSide::Left),
            None
        );
        assert_eq!(
            dispatch_plan(RecoveryAction::RequestPair, TileSide::Left),
            Some(TileSide::Left)
        );
    }
}
