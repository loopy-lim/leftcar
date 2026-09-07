//! V3 paired-IDR dispatch semantics for the split renderer.
//!
//! Contract (Host source verified, read-only):
//! - The Host accepts an authenticated `"IDR"` from EITHER viewer socket and
//!   coalesces concurrent PLIs only while `splitFlowState
//!   .recoveryBoundaryPending` is true. After the boundary completes, a late
//!   PLI from the second port starts a NEW Host recovery generation.
//! - Therefore each coordinator action transmits from exactly ONE selected
//!   path (the request-origin path when usable), with at most ONE bounded
//!   alternate-path attempt within the same action when the selected path
//!   reports itself unready. The 750ms recovery cadence is unchanged.
//! - A worker that cannot send (peer/token not learned) explicitly reports
//!   the unsent request to the coordinator, which retains it until a path is
//!   ready or the cadence retries; logging-and-dropping is not allowed.
//! - A UDP `send_to` attempt is a transmit attempt, never a delivery
//!   confirmation; telemetry and logs must say "attempt".
//! - Every request is stamped with its recovery episode id; the worker
//!   re-checks the id immediately before any wire access, so a request
//!   queued before a pair resume is cancelled instead of becoming a late
//!   pending PLI that would open a new Host generation.
//! - V3 same-episode request-instance ownership: the episode id ALONE is
//!   not enough, because every duplicate issued within one episode carries
//!   the same episode stamp. Each dispatched command therefore also carries
//!   a unique request id minted by the ledger, and exactly ONE command is
//!   outstanding (in flight: queued at a worker, awaiting its pre-wire
//!   decision or outcome) at any time — including through the bounded
//!   alternate. A same-episode retry before the outstanding outcome arrives
//!   mints nothing, an outcome not carrying the outstanding request id is
//!   inert (cannot dispatch, retain, stick or clear), and a queued command
//!   whose request id was superseded is cancelled before any wire access so
//!   an old queued wire copy can never ride a newer tick.

use super::presentation_sync::TileSide;
use super::recovery::RecoveryAction;

/// Truthful outcome of one stamped request as observed by the tile worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdrRequestOutcome {
    /// `send_to` accepted the datagram. This is a transmit attempt; actual
    /// delivery is unknown and never claimed.
    Transmitted,
    /// Socket had not learned the Host peer yet; nothing reached the wire.
    UnsentNoPeer,
    /// Session token not learned yet; nothing reached the wire.
    UnsentNoToken,
    /// `send_to` returned an error; nothing was handed to the kernel.
    SendFailed,
    /// Stamped episode or request id no longer current: cancelled before
    /// any wire access.
    CancelledStale,
}

/// The pre-wire decision a worker makes for one stamped request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdrSendDecision {
    Transmit,
    CancelStale,
    ReportUnsentNoPeer,
    ReportUnsentNoToken,
}

/// Pre-wire decision for one stamped request. Ownership comes first: a
/// request whose episode OR unique request id is no longer current is
/// cancelled before any wire access. The episode check closes everything
/// queued before a pair resume; the request-id check closes an old queued
/// wire copy of a superseded same-episode command, so a newer tick can
/// never be chased by an older copy on the alternate path.
pub fn decide_idr_send(
    current_episode: u64,
    stamped_episode: u64,
    current_request: u64,
    stamped_request: u64,
    peer_learned: bool,
    token_learned: bool,
) -> IdrSendDecision {
    if current_episode != stamped_episode || current_request != stamped_request {
        return IdrSendDecision::CancelStale;
    }
    if !peer_learned {
        IdrSendDecision::ReportUnsentNoPeer
    } else if !token_learned {
        IdrSendDecision::ReportUnsentNoToken
    } else {
        IdrSendDecision::Transmit
    }
}

/// Coordinator-side routing ledger for paired IDR requests. Owns, per
/// episode: the sticky wire path (first path whose transmit attempt was
/// accepted), the per-action bounded alternate failover, retention of
/// requests no path could carry yet, and the identity of the ONE
/// outstanding stamped request. Retained requests re-dispatch only on the
/// recovery gate's unchanged 750ms cadence — never sooner.
#[derive(Debug, Default)]
pub struct DispatchLedger {
    episode: u64,
    origin: Option<TileSide>,
    sticky_path: Option<TileSide>,
    failover_used: bool,
    transmitted_this_action: bool,
    /// Identity of the single stamped request currently owned by a worker
    /// path: queued, awaiting its pre-wire decision or its outcome. `None`
    /// means no command is on the wire or in any queue.
    outstanding: Option<u64>,
    /// Path the outstanding request was dispatched on. A report from any
    /// other path is a duplicate of an already-consumed request.
    outstanding_path: Option<TileSide>,
    /// Every ready path reported the request unsent: nothing is on the
    /// wire; only the unchanged 750ms cadence re-dispatches it. Distinct
    /// from in-flight, where a stamped command is still owned by a worker.
    retained: bool,
    /// Last minted request id; ids are unique per dispatched command for
    /// the lifetime of the ledger and never reused.
    next_request: u64,
}

fn alternate_path(side: TileSide) -> TileSide {
    match side {
        TileSide::Left => TileSide::Right,
        TileSide::Right => TileSide::Left,
    }
}

impl DispatchLedger {
    pub fn new() -> Self {
        Self {
            next_request: 0,
            ..Default::default()
        }
    }

    /// Adopt the gate's current episode id. An id change closes the old
    /// episode: sticky path, origin, failover budget, retention and the
    /// outstanding request slot reset so nothing from the dead episode can
    /// dispatch or leak into the new one.
    pub fn sync_episode(&mut self, episode: u64) {
        if self.episode != episode {
            self.episode = episode;
            self.origin = None;
            self.sticky_path = None;
            self.failover_used = false;
            self.transmitted_this_action = false;
            self.outstanding = None;
            self.outstanding_path = None;
            self.retained = false;
        }
    }

    /// Record which tile originated the current episode; its socket is the
    /// preferred request path for the whole episode.
    pub fn note_origin(&mut self, origin: TileSide) {
        self.origin = Some(origin);
    }

    /// Select the ONE wire path for this action: the sticky path that
    /// already transmitted within this episode, else the request-origin
    /// path. Returns `None` while a stamped request is still outstanding —
    /// a same-episode retry (cadence tick, fresh gap, duplicate trigger)
    /// must never mint a second wire copy until the outstanding outcome
    /// arrives; that is what keeps a newer Left and an older Right from
    /// ever being queued simultaneously. A dispatched request starts a
    /// fresh per-action failover budget and mints a unique request id.
    pub fn select_path(&mut self, action: RecoveryAction) -> Option<TileSide> {
        if self.outstanding.is_some() {
            return None;
        }
        let preferred = self.sticky_path.or(self.origin).unwrap_or(TileSide::Left);
        let target = super::recovery::dispatch_plan(action, preferred);
        if let Some(target) = target {
            self.outstanding = Some(self.mint_request());
            self.outstanding_path = Some(target);
            self.failover_used = false;
            self.transmitted_this_action = false;
            self.retained = false;
        }
        target
    }

    /// Identity of the single outstanding stamped request. The coordinator
    /// publishes it to the workers (pre-wire ownership) and stamps it on
    /// the queued command. Valid only right after a dispatch; `0` means
    /// nothing is outstanding.
    pub fn current_request(&self) -> u64 {
        self.outstanding.unwrap_or(0)
    }

    /// True while a stamped command is owned by a worker path: queued,
    /// awaiting its pre-wire decision or its outcome.
    pub fn in_flight(&self) -> bool {
        self.outstanding.is_some()
    }

    /// True while the request was reported unsent by every ready path:
    /// nothing is on the wire, and only the unchanged 750ms cadence
    /// re-dispatches it.
    pub fn retained_request(&self) -> bool {
        self.retained
    }

    /// True while the current episode still owes the wire a paired request:
    /// either a stamped request is in flight or an unready one is retained.
    /// Aggregate form of `in_flight` + `retained_request`; the android
    /// wiring logs the two states separately, this stays in the host-tested
    /// pure-state contract.
    #[allow(dead_code)]
    pub fn has_pending_request(&self) -> bool {
        self.outstanding.is_some() || self.retained
    }

    /// Report one stamped outcome; returns the single alternate path to try
    /// within the same action, or `None` (nothing more this action).
    /// Outcomes stamped with a dead episode, or not carrying the identity
    /// of the outstanding request, are ignored entirely.
    pub fn on_outcome_stamped(
        &mut self,
        request: u64,
        side: TileSide,
        episode: u64,
        outcome: IdrRequestOutcome,
    ) -> Option<TileSide> {
        if episode != self.episode {
            return None;
        }
        // Outcome ownership: only the report of the ONE outstanding stamped
        // request can mutate routing state. Anything else — a superseded
        // queued copy's report, a late duplicate of an already-consumed
        // request — must not dispatch, retain, stick or clear.
        if self.outstanding != Some(request) {
            return None;
        }
        match outcome {
            IdrRequestOutcome::Transmitted => {
                if self.sticky_path.is_none() {
                    self.sticky_path = Some(side);
                }
                // The request left this coordinator: consume the failover
                // budget so no second copy can chase it.
                self.failover_used = true;
                self.transmitted_this_action = true;
                self.outstanding = None;
                self.outstanding_path = None;
                self.retained = false;
                None
            }
            IdrRequestOutcome::CancelledStale => {
                self.outstanding = None;
                self.outstanding_path = None;
                None
            }
            IdrRequestOutcome::UnsentNoPeer
            | IdrRequestOutcome::UnsentNoToken
            | IdrRequestOutcome::SendFailed => {
                // A transmit attempt was already accepted for this action:
                // the request is on the wire (delivery still unknown), the
                // episode owes nothing more, and even an odd late failure
                // report must not resurrect retention or dispatch a copy.
                if self.transmitted_this_action {
                    return None;
                }
                // This stamped command is fully consumed: its slot clears
                // whether we fail over (a fresh mint takes it over) or
                // retain the request unready.
                self.outstanding = None;
                self.outstanding_path = None;
                // The selected path cannot carry the request yet. Exactly
                // one bounded alternate attempt within this action; if that
                // also fails the request stays retained for the unchanged
                // 750ms cadence instead of any faster retry.
                self.retained = true;
                if self.failover_used {
                    return None;
                }
                self.failover_used = true;
                let target = alternate_path(side);
                // The alternate is a NEW stamped command: it takes over the
                // single outstanding slot with its own unique identity.
                self.retained = false;
                self.outstanding = Some(self.mint_request());
                self.outstanding_path = Some(target);
                Some(target)
            }
        }
    }

    /// Report one outcome resolved against the outstanding request by path:
    /// a report from any path other than the outstanding request's path is
    /// a duplicate of an already-consumed request (each stamped command
    /// reports exactly once) and is inert. The production wiring uses
    /// `on_outcome_stamped` with the id carried on the event; this path
    /// form keeps the pure 2-tick contract testable standalone.
    #[allow(dead_code)]
    pub fn on_outcome(
        &mut self,
        side: TileSide,
        episode: u64,
        outcome: IdrRequestOutcome,
    ) -> Option<TileSide> {
        let request = match (self.outstanding, self.outstanding_path) {
            (Some(request), Some(path)) if path == side => request,
            _ => return None,
        };
        self.on_outcome_stamped(request, side, episode, outcome)
    }

    fn mint_request(&mut self) -> u64 {
        self.next_request = self.next_request.wrapping_add(1);
        self.next_request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::PairedRecoveryGate;

    #[test]
    fn stale_episode_request_is_cancelled_before_any_wire_attempt() {
        // Episode ownership: the coordinator stamped this request with the
        // episode that issued it, but the gate has since moved on (pair
        // resumed, partial restart, or a newer episode). The worker must
        // cancel BEFORE any wire access even when its path is fully ready —
        // a late pending PLI after a pair resumes would start a new Host
        // recovery generation.
        assert_eq!(
            decide_idr_send(2, 1, 1, 1, true, true),
            IdrSendDecision::CancelStale
        );
        assert_eq!(
            decide_idr_send(9_999, 9_998, 7, 7, true, true),
            IdrSendDecision::CancelStale
        );
    }

    #[test]
    fn superseded_request_identity_is_cancelled_before_any_wire_attempt() {
        // V3 same-episode ownership: a queued wire copy whose unique request
        // id was already superseded by a newer stamped command must be
        // cancelled BEFORE any wire access even though its episode stamp is
        // still current and its path is fully ready — an old copy chasing a
        // newer tick is exactly the late-second-PLI Host generation bump.
        assert_eq!(
            decide_idr_send(1, 1, 2, 1, true, true),
            IdrSendDecision::CancelStale
        );
        assert_eq!(
            decide_idr_send(1, 1, 2, 1, false, false),
            IdrSendDecision::CancelStale
        );
        // The owned request on a fully ready path transmits.
        assert_eq!(
            decide_idr_send(1, 1, 2, 2, true, true),
            IdrSendDecision::Transmit
        );
    }

    #[test]
    fn current_episode_transmits_only_when_peer_and_token_are_learned() {
        // Delayed readiness: before the worker learns its peer/token the
        // request must be explicitly reported unsent (never silently
        // dropped), and only a fully ready path produces a transmit attempt.
        assert_eq!(
            decide_idr_send(1, 1, 1, 1, false, false),
            IdrSendDecision::ReportUnsentNoPeer
        );
        assert_eq!(
            decide_idr_send(1, 1, 1, 1, false, true),
            IdrSendDecision::ReportUnsentNoPeer
        );
        assert_eq!(
            decide_idr_send(1, 1, 1, 1, true, false),
            IdrSendDecision::ReportUnsentNoToken
        );
        assert_eq!(
            decide_idr_send(1, 1, 1, 1, true, true),
            IdrSendDecision::Transmit
        );
    }

    #[test]
    fn request_origin_is_preferred_and_first_success_becomes_sticky() {
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Right);
        // Request-origin path carries the request: exactly one path, never
        // both.
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Right)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Right, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert!(!ledger.has_pending_request());
        // The next cadence tick of the same episode stays on the path that
        // actually transmitted: one wire request per action, one stable
        // origin per episode.
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Right)
        );
    }

    #[test]
    fn unsent_selected_path_fails_over_exactly_once_per_action() {
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        // Initial path unavailable, other path usable: exactly ONE bounded
        // alternate-path attempt within the same action.
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        // The alternate also reported unsent: the request is explicitly
        // retained for the unchanged 750ms cadence — never a faster retry,
        // never a third path.
        assert_eq!(
            ledger.on_outcome(TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert!(ledger.has_pending_request());
        // A fresh action (next cadence tick) may select again; with no path
        // having transmitted yet the request-origin preference stands. The
        // newly stamped request is in flight until an outcome arrives.
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        assert!(ledger.has_pending_request());
    }

    #[test]
    fn send_failure_is_undelivered_and_gets_the_same_single_failover_bound() {
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(3);
        ledger.note_origin(TileSide::Right);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Right)
        );
        // A failed send_to handed nothing to the kernel: it must be reported
        // as unsent, never counted as delivered, and it may fail over once.
        assert_eq!(
            ledger.on_outcome(TileSide::Right, 3, IdrRequestOutcome::SendFailed),
            Some(TileSide::Left)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 3, IdrRequestOutcome::SendFailed),
            None
        );
        assert!(ledger.has_pending_request());
    }

    #[test]
    fn transmitted_outcome_never_triggers_an_alternate_dispatch() {
        // Once a path's transmit attempt was accepted, the action is done:
        // no alternate copy may be sent, which is what makes the Host's
        // second-port late-PLI generation bump unreachable from the viewer.
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(2);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 2, IdrRequestOutcome::Transmitted),
            None
        );
        // Even an odd later failure report from the same action must not
        // dispatch anything: the request left this coordinator already.
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 2, IdrRequestOutcome::SendFailed),
            None
        );
        assert!(!ledger.has_pending_request());
    }

    #[test]
    fn episode_change_resets_routing_and_ignores_stale_outcomes() {
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert!(ledger.has_pending_request());
        // The gate moved to a new episode (pair resumed or restarted):
        // retention from the dead episode must not leak into it, and no
        // request stays in flight across the epoch boundary.
        ledger.sync_episode(2);
        assert!(!ledger.has_pending_request());
        assert!(!ledger.in_flight());
        assert!(!ledger.retained_request());
        // Outcomes stamped with the dead episode are ignored entirely: no
        // failover, no retention, no sticky-path update — even when stamped
        // with the exact request id the dead episode dispatched.
        let dead_request = ledger.current_request();
        assert_eq!(
            ledger.on_outcome_stamped(dead_request, TileSide::Left, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert_eq!(
            ledger.on_outcome_stamped(dead_request, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            None
        );
        assert!(!ledger.has_pending_request());
        // The fresh episode selects with its own budget.
        ledger.note_origin(TileSide::Right);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Right)
        );
    }

    #[test]
    fn same_episode_retry_select_is_blocked_until_the_outstanding_outcome() {
        // V3 root repro (dispatch-probe): select the first path, then a
        // same-episode retry BEFORE the first outcome arrives. The retry
        // must mint nothing — exactly one outstanding command until its
        // outcome — so a newer Left and an older Right can never be queued
        // simultaneously within one episode.
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        let first = ledger.select_path(RecoveryAction::RequestPair);
        assert_eq!(first, Some(TileSide::Left));
        assert!(ledger.in_flight());
        assert!(!ledger.retained_request());
        let first_request = ledger.current_request();
        let retry = ledger.select_path(RecoveryAction::RequestPair);
        assert_eq!(retry, None, "same-episode retry while outstanding mints nothing");
        assert_eq!(
            ledger.current_request(),
            first_request,
            "the blocked retry must not mint a request id"
        );
        assert!(ledger.in_flight());
        // The single outstanding request still owns the ONE bounded
        // alternate of its action — there is no newer request to collide
        // with.
        assert_eq!(
            ledger.on_outcome(TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        // And the alternate is again the only outstanding command: another
        // retry before ITS outcome is equally blocked.
        assert_eq!(ledger.select_path(RecoveryAction::RequestPair), None);
        assert_eq!(
            ledger.on_outcome(TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert!(ledger.retained_request());
        assert!(!ledger.in_flight());
    }

    #[test]
    fn old_success_and_old_unsent_reports_after_a_new_attempt_are_inert() {
        // V3 request-instance ownership: once a newer attempt owns the
        // outstanding slot, a report stamped with an older request id —
        // success OR unsent — cannot dispatch an alternate, flip retention,
        // claim the sticky path or clear the newer attempt.
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        let first = ledger.current_request();
        assert_eq!(
            ledger.on_outcome_stamped(first, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        let alternate = ledger.current_request();
        assert_ne!(alternate, first, "the bounded alternate is a new stamped command");
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert!(ledger.retained_request());
        // Next cadence tick: a NEW attempt owns the slot.
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        let newer = ledger.current_request();
        assert!(newer > alternate);
        assert!(ledger.in_flight());
        // Old reports after the new attempt: all inert.
        assert_eq!(
            ledger.on_outcome_stamped(first, TileSide::Left, 1, IdrRequestOutcome::Transmitted),
            None,
            "old success must not complete the newer attempt"
        );
        assert!(ledger.in_flight(), "old success must not clear the outstanding slot");
        assert_eq!(
            ledger.on_outcome_stamped(first, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            None,
            "old unsent must not dispatch a second alternate"
        );
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert_eq!(ledger.current_request(), newer, "slot still owned by the newer attempt");
        assert!(ledger.has_pending_request());
        // The newer attempt completes normally with its own report.
        assert_eq!(
            ledger.on_outcome_stamped(newer, TileSide::Left, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert!(!ledger.has_pending_request());
    }

    #[test]
    fn superseded_queued_copy_reports_inert_and_never_rides_the_newer_tick() {
        // V3 queued-command ownership: an old queued copy cancelled pre-wire
        // (superseded request id, current episode) reports CancelledStale —
        // and that report must not consume or clear the newer attempt's
        // outstanding slot.
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        let first = ledger.current_request();
        assert_eq!(
            ledger.on_outcome_stamped(first, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        let alternate = ledger.current_request();
        // The superseded copy finally drains from the worker queue: it is
        // cancelled before the wire (episode current, id superseded)...
        assert_eq!(
            decide_idr_send(1, 1, alternate, first, true, true),
            IdrSendDecision::CancelStale
        );
        // ...and its report leaves the newer attempt untouched.
        assert_eq!(
            ledger.on_outcome_stamped(first, TileSide::Left, 1, IdrRequestOutcome::CancelledStale),
            None
        );
        assert_eq!(ledger.current_request(), alternate);
        assert!(ledger.in_flight());
        // The owned alternate still completes its action.
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert!(!ledger.has_pending_request());
    }

    #[test]
    fn retained_unready_is_distinguishable_from_in_flight() {
        // V3 retention semantics: after select the request is IN FLIGHT (a
        // stamped command is owned by a worker path). Once the selected path
        // AND the bounded alternate both report unsent, the request is
        // RETAINED: nothing is on the wire, no command is outstanding, and
        // only the unchanged 750ms cadence re-dispatches it. While retained,
        // every report is inert (nothing is outstanding to own it).
        let mut ledger = DispatchLedger::new();
        ledger.sync_episode(1);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        assert!(ledger.in_flight());
        assert!(!ledger.retained_request());
        let selected = ledger.current_request();
        assert_eq!(
            ledger.on_outcome_stamped(selected, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        // The bounded alternate is in flight, not retained.
        assert!(ledger.in_flight());
        assert!(!ledger.retained_request());
        let alternate = ledger.current_request();
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::UnsentNoToken),
            None
        );
        // Retained-unready: no outstanding command, nothing on the wire.
        assert!(ledger.retained_request());
        assert!(!ledger.in_flight());
        assert!(ledger.has_pending_request());
        assert_eq!(ledger.current_request(), 0, "retention owns no outstanding request");
        // Reports while retained are inert.
        assert_eq!(
            ledger.on_outcome_stamped(selected, TileSide::Left, 1, IdrRequestOutcome::UnsentNoPeer),
            None
        );
        assert_eq!(
            ledger.on_outcome_stamped(alternate, TileSide::Right, 1, IdrRequestOutcome::Transmitted),
            None
        );
        assert!(ledger.retained_request());
        assert!(!ledger.in_flight());
    }

    #[test]
    fn no_late_pending_pli_after_pair_resumes() {
        // End-to-end episode ownership across gate + ledger + worker
        // decision: a request queued while the episode was active is
        // cancelled, and its late outcome cannot dispatch anything, once the
        // pair has resumed.
        let mut gate = PairedRecoveryGate::default();
        let mut ledger = DispatchLedger::new();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        let stamped = gate.episode();
        ledger.sync_episode(stamped);
        ledger.note_origin(TileSide::Left);
        let path = ledger.select_path(RecoveryAction::RequestPair);
        assert_eq!(path, Some(TileSide::Left));
        let stamped_request = ledger.current_request();
        // The pair completes before the worker drains the queued command.
        assert_eq!(gate.on_idr(TileSide::Left, 9), RecoveryAction::WaitForPeer);
        assert_eq!(gate.on_idr(TileSide::Right, 9), RecoveryAction::ResumePair);
        ledger.sync_episode(gate.episode());
        // The worker finally processes the stale stamped command with a
        // fully ready path: it must cancel before the wire (dead episode,
        // regardless of the request id it carries).
        assert_eq!(
            decide_idr_send(gate.episode(), stamped, 0, stamped_request, true, true),
            IdrSendDecision::CancelStale
        );
        // Even if something slipped through, the late outcome is stale and
        // must not dispatch or retain anything.
        assert_eq!(
            ledger.on_outcome_stamped(stamped_request, path.unwrap(), stamped, IdrRequestOutcome::Transmitted),
            None
        );
        assert_eq!(
            ledger.on_outcome_stamped(stamped_request, path.unwrap(), stamped, IdrRequestOutcome::UnsentNoPeer),
            None
        );
        assert!(!ledger.has_pending_request());
    }

    #[test]
    fn retained_request_redispatches_on_the_existing_cadence_when_paths_are_ready_later() {
        // Delayed readiness with unchanged cadence: both paths report unsent
        // in the first action; the request stays retained and is retried by
        // the SAME 750ms gate cadence — not sooner, not on a faster timer.
        let mut gate = PairedRecoveryGate::default();
        let mut ledger = DispatchLedger::new();
        assert_eq!(gate.start_initial(0), RecoveryAction::RequestPair);
        let episode = gate.episode();
        ledger.sync_episode(episode);
        ledger.note_origin(TileSide::Left);
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Left, episode, IdrRequestOutcome::UnsentNoPeer),
            Some(TileSide::Right)
        );
        assert_eq!(
            ledger.on_outcome(TileSide::Right, episode, IdrRequestOutcome::UnsentNoToken),
            None
        );
        assert!(ledger.has_pending_request());
        // Before the cadence deadline there is no new dispatch action.
        assert_eq!(gate.retry_due(750_000_000 - 1), RecoveryAction::Suppress);
        // The 750ms tick fires exactly one action; the episode is unchanged,
        // and the ledger selects still exactly one path.
        assert_eq!(gate.retry_due(750_000_000), RecoveryAction::RequestPair);
        assert_eq!(gate.episode(), episode);
        ledger.sync_episode(gate.episode());
        assert_eq!(
            ledger.select_path(RecoveryAction::RequestPair),
            Some(TileSide::Left)
        );
        // Now the path is ready and reports a transmit attempt (not a
        // delivery): retention clears and no alternate is dispatched.
        assert_eq!(
            ledger.on_outcome(TileSide::Left, episode, IdrRequestOutcome::Transmitted),
            None
        );
        assert!(!ledger.has_pending_request());
    }
}
