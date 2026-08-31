use crate::renderer::presentation_sync::TileSide;

const RECOVERY_COOLDOWN_NS: u64 = 750_000_000;

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
}

impl PairedRecoveryGate {
    pub fn start_initial(&mut self, now_ns: u64) -> RecoveryAction {
        self.active = true;
        self.last_request_ns = Some(now_ns);
        self.left_idr_generation = None;
        self.right_idr_generation = None;
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
                RecoveryAction::ResumePair
            }
            (Some(left), Some(right)) => {
                // Keep the newer tile generation. Clearing both here creates
                // an ordering race: if one tile receives generation N before
                // its peer, the peer's later N can never complete the pair.
                if left > right {
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
