//! Pairing and stream phase state machines (docs/01 §6, docs/04 §4).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingState {
    Unpaired,
    Advertising,
    AwaitingHostApproval,
    PairedOffline,
    Connecting,
    Connected,
    Revoked,
}

impl PairingState {
    pub fn can_transition_to(&self, next: Self) -> bool {
        use PairingState::*;
        if next == Revoked {
            return *self != Revoked;
        }
        matches!(
            (self, next),
            (Unpaired, Advertising)
                | (Advertising, AwaitingHostApproval)
                | (Advertising, Unpaired)
                | (AwaitingHostApproval, PairedOffline)
                | (AwaitingHostApproval, Unpaired)
                | (PairedOffline, Connecting)
                | (Connecting, Connected)
                | (Connecting, PairedOffline)
                | (Connected, PairedOffline)
                | (Connected, Connecting) // reconnecting
                | (Revoked, Unpaired) // re-pairing after revocation
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamPhase {
    Idle,
    Negotiating,
    WaitingKeyframe,
    Playing,
    Degraded,
    Reconnecting,
    Suspended,
    SourceUnavailable,
    PermissionRevoked,
    DecoderFailed,
    Stopped,
}

impl StreamPhase {
    /// Terminal-for-window states reachable from any state (docs/01 §6).
    pub fn is_any_reachable(&self) -> bool {
        matches!(
            self,
            Self::SourceUnavailable | Self::PermissionRevoked | Self::DecoderFailed | Self::Stopped
        )
    }

    pub fn can_transition_to(&self, next: Self) -> bool {
        if next.is_any_reachable() && !self.is_any_reachable() {
            return true;
        }
        use StreamPhase::*;
        matches!(
            (self, next),
            (Idle, Negotiating)
                | (Negotiating, WaitingKeyframe)
                | (WaitingKeyframe, Playing)
                | (Playing, Degraded)
                | (Degraded, Playing)
                | (Playing, Reconnecting)
                | (Degraded, Reconnecting)
                | (Reconnecting, Playing)
                | (Playing, Suspended)
                | (Degraded, Suspended)
                | (Suspended, Negotiating) // resume from suspension
                | (Stopped, Idle) // reopen
                | (Idle, Stopped)
                | (Negotiating, Stopped)
                | (WaitingKeyframe, Stopped)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_transitions_match_product_requirements() {
        // docs/01 §6 graph
        assert!(StreamPhase::Idle.can_transition_to(StreamPhase::Negotiating));
        assert!(StreamPhase::Negotiating.can_transition_to(StreamPhase::WaitingKeyframe));
        assert!(StreamPhase::WaitingKeyframe.can_transition_to(StreamPhase::Playing));
        assert!(StreamPhase::Playing.can_transition_to(StreamPhase::Degraded));
        assert!(StreamPhase::Degraded.can_transition_to(StreamPhase::Playing));
        assert!(StreamPhase::Playing.can_transition_to(StreamPhase::Reconnecting));
        assert!(StreamPhase::Reconnecting.can_transition_to(StreamPhase::Playing));
        assert!(StreamPhase::Playing.can_transition_to(StreamPhase::Suspended));
        assert!(StreamPhase::Suspended.can_transition_to(StreamPhase::Negotiating));
        // any -> terminal
        for from in [
            StreamPhase::Idle,
            StreamPhase::Negotiating,
            StreamPhase::Playing,
            StreamPhase::Suspended,
        ] {
            assert!(from.can_transition_to(StreamPhase::SourceUnavailable), "{from:?}");
            assert!(from.can_transition_to(StreamPhase::PermissionRevoked), "{from:?}");
            assert!(from.can_transition_to(StreamPhase::DecoderFailed), "{from:?}");
            assert!(from.can_transition_to(StreamPhase::Stopped), "{from:?}");
        }
        // forbidden
        assert!(!StreamPhase::Idle.can_transition_to(StreamPhase::Playing));
        assert!(!StreamPhase::Suspended.can_transition_to(StreamPhase::Playing));
        assert!(!StreamPhase::Playing.can_transition_to(StreamPhase::Idle));
    }

    #[test]
    fn pairing_revoked_from_any_state() {
        for from in [
            PairingState::Unpaired,
            PairingState::Advertising,
            PairingState::AwaitingHostApproval,
            PairingState::PairedOffline,
            PairingState::Connecting,
            PairingState::Connected,
        ] {
            assert!(from.can_transition_to(PairingState::Revoked), "{from:?}");
        }
        assert!(!PairingState::Revoked.can_transition_to(PairingState::Connected));
        // revoke leaves via re-pairing only
        assert!(PairingState::Revoked.can_transition_to(PairingState::Unpaired));
    }
}
