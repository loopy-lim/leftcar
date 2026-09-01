use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub(super) const RENDER_IDR_DEADLINE: Duration = Duration::from_millis(250);
pub(super) const RENDER_REBUILD_DEADLINE: Duration = Duration::from_millis(750);
pub(super) const RENDER_RECOVERY_RETRY_INTERVAL: Duration = Duration::from_millis(1_500);
pub(super) const RENDER_MAX_RECOVERY_RETRIES: u8 = 4;
pub(super) const RENDER_TERMINATE_DEADLINE: Duration = Duration::from_secs(12);
pub(super) const MISSED_CONTROL_PROBE_LIMIT: u8 = 3;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct InitialControlState {
    pub(super) host_peer: Option<SocketAddr>,
    pub(super) input_endpoint: Option<(SocketAddr, Vec<u8>)>,
}

pub(super) fn initial_control_state(
    tcp_control_peer: Option<SocketAddr>,
    prepared_peer: Option<SocketAddr>,
    token: &[u8],
) -> InitialControlState {
    if token.is_empty() {
        return InitialControlState::default();
    }
    let host_peer = tcp_control_peer.or(prepared_peer);
    InitialControlState {
        host_peer,
        input_endpoint: host_peer.map(|peer| (peer, token.to_vec())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderHealthAction {
    None,
    RequestIdr,
    RebuildDecoder,
    TerminateRenderStalled,
}

#[derive(Debug, Default)]
pub(super) struct RenderHealthState {
    last_completed_access_units: u64,
    last_rendered_frames: u64,
    incident_started_at: Option<Instant>,
    idr_requested: bool,
    recovery_attempts: u8,
    last_recovery_at: Option<Instant>,
    terminated: bool,
}

impl RenderHealthState {
    pub(super) fn rebase(&mut self, completed_access_units: u64, rendered_frames: u64) {
        self.last_completed_access_units = completed_access_units;
        self.last_rendered_frames = rendered_frames;
        self.reset_incident();
    }

    pub(super) fn observe(
        &mut self,
        now: Instant,
        completed_access_units: u64,
        rendered_frames: u64,
    ) -> RenderHealthAction {
        let access_units_advanced = completed_access_units > self.last_completed_access_units;
        let surface_advanced = rendered_frames > self.last_rendered_frames;
        self.last_completed_access_units = completed_access_units;
        self.last_rendered_frames = rendered_frames;

        if surface_advanced {
            self.reset_incident();
            return RenderHealthAction::None;
        }
        if !access_units_advanced {
            return RenderHealthAction::None;
        }
        if self.terminated {
            return RenderHealthAction::None;
        }

        let incident_started_at = *self.incident_started_at.get_or_insert(now);
        let incident_age = now.saturating_duration_since(incident_started_at);
        if incident_age >= RENDER_TERMINATE_DEADLINE {
            self.terminated = true;
            return RenderHealthAction::TerminateRenderStalled;
        }
        let recovery_retry_due = self.last_recovery_at.is_none_or(|last| {
            now.saturating_duration_since(last) >= RENDER_RECOVERY_RETRY_INTERVAL
        });
        if incident_age >= RENDER_REBUILD_DEADLINE
            && recovery_retry_due
            && self.recovery_attempts < RENDER_MAX_RECOVERY_RETRIES
        {
            self.recovery_attempts = self.recovery_attempts.saturating_add(1);
            self.last_recovery_at = Some(now);
            return RenderHealthAction::RebuildDecoder;
        }
        if incident_age >= RENDER_IDR_DEADLINE && !self.idr_requested {
            self.idr_requested = true;
            return RenderHealthAction::RequestIdr;
        }
        RenderHealthAction::None
    }

    fn reset_incident(&mut self) {
        self.incident_started_at = None;
        self.idr_requested = false;
        self.recovery_attempts = 0;
        self.last_recovery_at = None;
        self.terminated = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlHealthAction {
    None,
    TerminateHostUnreachable,
}

#[derive(Debug, Default)]
pub(super) struct ControlHealthState {
    outstanding_sequence: Option<u32>,
    consecutive_misses: u8,
    terminated: bool,
}

impl ControlHealthState {
    pub(super) fn probe_send_completed(
        &mut self,
        sequence: u32,
        sent: bool,
    ) -> ControlHealthAction {
        if sent {
            self.probe_sent(sequence)
        } else {
            ControlHealthAction::None
        }
    }

    pub(super) fn probe_sent(&mut self, sequence: u32) -> ControlHealthAction {
        if self.terminated {
            return ControlHealthAction::None;
        }
        if self.outstanding_sequence.is_some() {
            self.consecutive_misses = self.consecutive_misses.saturating_add(1);
        }
        self.outstanding_sequence = Some(sequence);
        if self.consecutive_misses >= MISSED_CONTROL_PROBE_LIMIT {
            self.terminated = true;
            ControlHealthAction::TerminateHostUnreachable
        } else {
            ControlHealthAction::None
        }
    }

    pub(super) fn probe_acknowledged(&mut self, sequence: u32) -> bool {
        if self.terminated || self.outstanding_sequence != Some(sequence) {
            return false;
        }
        self.outstanding_sequence = None;
        self.consecutive_misses = 0;
        true
    }
}

pub(super) fn run_control_probe_cycle<Drain, Send>(
    control_health: &mut ControlHealthState,
    next_probe_sequence: Option<u32>,
    drain_responses: Drain,
    send_probe: Send,
) -> ControlHealthAction
where
    Drain: FnOnce(&mut ControlHealthState),
    Send: FnOnce(u32) -> bool,
{
    drain_responses(control_health);
    let Some(sequence) = next_probe_sequence else {
        return ControlHealthAction::None;
    };
    let sent = send_probe(sequence);
    control_health.probe_send_completed(sequence, sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_silence_never_starts_render_recovery() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        assert_eq!(health.observe(start, 0, 0), RenderHealthAction::None);
        for elapsed in [
            RENDER_IDR_DEADLINE,
            RENDER_REBUILD_DEADLINE,
            RENDER_TERMINATE_DEADLINE,
            Duration::from_secs(30),
        ] {
            assert_eq!(
                health.observe(start + elapsed, 0, 0),
                RenderHealthAction::None
            );
        }
    }

    #[test]
    fn stalled_render_emits_each_incident_action_once_at_exact_deadlines() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        assert_eq!(health.observe(start, 1, 0), RenderHealthAction::None);
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE - Duration::from_nanos(1), 2, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE, 3, 0),
            RenderHealthAction::RequestIdr
        );
        assert_eq!(
            health.observe(start + Duration::from_millis(500), 4, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + RENDER_REBUILD_DEADLINE, 5, 0),
            RenderHealthAction::RebuildDecoder
        );
        assert_eq!(
            health.observe(start + Duration::from_secs(2), 6, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + RENDER_TERMINATE_DEADLINE, 7, 0),
            RenderHealthAction::TerminateRenderStalled
        );
        assert_eq!(
            health.observe(start + Duration::from_secs(4), 8, 0),
            RenderHealthAction::None
        );
    }

    #[test]
    fn stalled_render_retries_decoder_before_using_the_long_grace_deadline() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        assert_eq!(health.observe(start, 1, 0), RenderHealthAction::None);
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE, 2, 0),
            RenderHealthAction::RequestIdr
        );
        assert_eq!(
            health.observe(start + RENDER_REBUILD_DEADLINE, 3, 0),
            RenderHealthAction::RebuildDecoder
        );
        assert_eq!(
            health.observe(start + Duration::from_millis(2_250), 4, 0),
            RenderHealthAction::RebuildDecoder
        );
        assert_eq!(
            health.observe(start + Duration::from_millis(3_750), 5, 0),
            RenderHealthAction::RebuildDecoder
        );
        assert_eq!(
            health.observe(start + Duration::from_millis(5_250), 6, 0),
            RenderHealthAction::RebuildDecoder
        );
        assert_eq!(
            health.observe(start + Duration::from_secs(6), 7, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + Duration::from_secs(12), 8, 0),
            RenderHealthAction::TerminateRenderStalled
        );
    }

    #[test]
    fn timer_only_observations_after_one_au_never_trigger_recovery() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        assert_eq!(health.observe(start, 1, 0), RenderHealthAction::None);
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE, 1, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + RENDER_REBUILD_DEADLINE, 1, 0),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(start + RENDER_TERMINATE_DEADLINE, 1, 0),
            RenderHealthAction::None
        );
    }

    #[test]
    fn surface_release_resets_incident_and_allows_one_future_incident() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        assert_eq!(health.observe(start, 1, 0), RenderHealthAction::None);
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE, 2, 0),
            RenderHealthAction::RequestIdr
        );
        assert_eq!(
            health.observe(start + Duration::from_millis(300), 3, 1),
            RenderHealthAction::None
        );

        let second_start = start + Duration::from_secs(1);
        assert_eq!(health.observe(second_start, 4, 1), RenderHealthAction::None);
        assert_eq!(
            health.observe(second_start + RENDER_IDR_DEADLINE, 5, 1),
            RenderHealthAction::RequestIdr
        );
    }

    #[test]
    fn rebase_discards_old_incident_and_starts_first_post_pause_au_fresh() {
        let start = Instant::now();
        let mut health = RenderHealthState::default();

        health.rebase(100, 50);
        assert_eq!(health.observe(start, 101, 50), RenderHealthAction::None);
        assert_eq!(
            health.observe(start + RENDER_IDR_DEADLINE, 102, 50),
            RenderHealthAction::RequestIdr
        );

        health.rebase(102, 50);
        assert_eq!(
            health.observe(start + Duration::from_secs(30), 102, 50),
            RenderHealthAction::None
        );

        let first_post_pause_au = start + Duration::from_secs(60);
        assert_eq!(
            health.observe(first_post_pause_au, 103, 50),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(
                first_post_pause_au + RENDER_IDR_DEADLINE - Duration::from_nanos(1),
                104,
                50,
            ),
            RenderHealthAction::None
        );
        assert_eq!(
            health.observe(first_post_pause_au + RENDER_IDR_DEADLINE, 105, 50),
            RenderHealthAction::RequestIdr
        );
    }

    #[test]
    fn matching_probe_ack_resets_superseded_miss_count() {
        let mut health = ControlHealthState::default();

        assert_eq!(health.probe_sent(10), ControlHealthAction::None);
        assert!(!health.probe_acknowledged(9));
        assert_eq!(health.probe_sent(11), ControlHealthAction::None);
        assert!(health.probe_acknowledged(11));
        assert_eq!(health.probe_sent(12), ControlHealthAction::None);
        assert_eq!(health.probe_sent(13), ControlHealthAction::None);
        assert_eq!(health.probe_sent(14), ControlHealthAction::None);
    }

    #[test]
    fn authenticated_preflight_seeds_control_peer_and_input_endpoint() {
        let peer = "192.0.2.10:5002".parse().unwrap();

        let initial = initial_control_state(None, Some(peer), b"viewer-token");

        assert_eq!(initial.host_peer, Some(peer));
        assert_eq!(
            initial.input_endpoint,
            Some((peer, b"viewer-token".to_vec()))
        );
    }

    #[test]
    fn preflight_peer_without_authenticated_token_seeds_no_control_state() {
        let peer = "192.0.2.10:5002".parse().unwrap();

        assert_eq!(
            initial_control_state(None, Some(peer), b""),
            Default::default()
        );
    }

    #[test]
    fn queued_ack_is_drained_before_third_probe_miss_is_committed() {
        let mut health = ControlHealthState::default();
        assert_eq!(health.probe_sent(40), ControlHealthAction::None);
        assert_eq!(health.probe_sent(41), ControlHealthAction::None);
        assert_eq!(health.probe_sent(42), ControlHealthAction::None);

        let action = run_control_probe_cycle(
            &mut health,
            Some(43),
            |health| assert!(health.probe_acknowledged(42)),
            |_sequence| true,
        );

        assert_eq!(action, ControlHealthAction::None);
    }

    #[test]
    fn third_genuinely_unacked_probe_cycle_terminates_after_drain() {
        let mut health = ControlHealthState::default();
        assert_eq!(health.probe_sent(50), ControlHealthAction::None);
        assert_eq!(health.probe_sent(51), ControlHealthAction::None);
        assert_eq!(health.probe_sent(52), ControlHealthAction::None);

        let action = run_control_probe_cycle(&mut health, Some(53), |_| {}, |_sequence| true);

        assert_eq!(action, ControlHealthAction::TerminateHostUnreachable);
    }

    #[test]
    fn failed_send_keeps_previous_successful_probe_acknowledgeable() {
        let mut health = ControlHealthState::default();

        assert_eq!(
            health.probe_send_completed(41, true),
            ControlHealthAction::None
        );
        assert_eq!(
            health.probe_send_completed(42, false),
            ControlHealthAction::None
        );
        assert!(health.probe_acknowledged(41));

        assert_eq!(
            health.probe_send_completed(43, true),
            ControlHealthAction::None
        );
        assert_eq!(
            health.probe_send_completed(44, true),
            ControlHealthAction::None
        );
        assert_eq!(
            health.probe_send_completed(45, true),
            ControlHealthAction::None
        );
    }

    #[test]
    fn third_superseded_unacknowledged_probe_terminates_once() {
        let mut health = ControlHealthState::default();

        assert_eq!(health.probe_sent(20), ControlHealthAction::None);
        assert_eq!(health.probe_sent(21), ControlHealthAction::None);
        assert_eq!(health.probe_sent(22), ControlHealthAction::None);
        assert_eq!(
            health.probe_sent(23),
            ControlHealthAction::TerminateHostUnreachable
        );
        assert_eq!(health.probe_sent(24), ControlHealthAction::None);
        assert!(!health.probe_acknowledged(23));
    }
}
