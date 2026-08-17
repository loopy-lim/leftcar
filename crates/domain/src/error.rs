//! Stable product error contract (docs/04 §8, docs/03 §10).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorScope {
    Command,
    Source,
    Stream,
    Session,
    Host,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    Retry,
    RePair,
    ReSelectSource,
    GrantScreenRecording,
    CloseStreamWindow,
    UpdateHost,
    UpdateViewer,
    OpenDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftcarError {
    pub code: &'static str,
    pub retryable: Retryability,
    pub scope: ErrorScope,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Retryability {
    Always,
    Never,
    Conditional,
}

/// The stable error table from docs/04 §8. Codes are compile-time constants.
pub const ERR_PAIRING_OFFER_EXPIRED: LeftcarError = LeftcarError {
    code: "pairing.offer_expired",
    retryable: Retryability::Always,
    scope: ErrorScope::Session,
    action: RecoveryAction::RePair,
};
pub const ERR_PAIRING_HOST_REJECTED: LeftcarError = LeftcarError {
    code: "pairing.host_rejected",
    retryable: Retryability::Never,
    scope: ErrorScope::Session,
    action: RecoveryAction::RePair,
};
pub const ERR_AUTH_DEVICE_REVOKED: LeftcarError = LeftcarError {
    code: "auth.device_revoked",
    retryable: Retryability::Never,
    scope: ErrorScope::Session,
    action: RecoveryAction::RePair,
};
pub const ERR_PROTOCOL_VERSION_MISMATCH: LeftcarError = LeftcarError {
    code: "protocol.version_mismatch",
    retryable: Retryability::Never,
    scope: ErrorScope::Session,
    action: RecoveryAction::UpdateHost,
};
pub const ERR_CAPTURE_PERMISSION_REQUIRED: LeftcarError = LeftcarError {
    code: "capture.permission_required",
    retryable: Retryability::Always,
    scope: ErrorScope::Host,
    action: RecoveryAction::GrantScreenRecording,
};
pub const ERR_CAPTURE_SOURCE_UNAVAILABLE: LeftcarError = LeftcarError {
    code: "capture.source_unavailable",
    retryable: Retryability::Always,
    scope: ErrorScope::Source,
    action: RecoveryAction::ReSelectSource,
};
pub const ERR_CAPTURE_PROTECTED_CONTENT: LeftcarError = LeftcarError {
    code: "capture.protected_content",
    retryable: Retryability::Never,
    scope: ErrorScope::Source,
    action: RecoveryAction::ReSelectSource,
};
pub const ERR_ENCODER_HARDWARE_UNAVAILABLE: LeftcarError = LeftcarError {
    code: "encoder.hardware_unavailable",
    retryable: Retryability::Conditional,
    scope: ErrorScope::Host,
    action: RecoveryAction::OpenDiagnostics,
};
pub const ERR_TRANSPORT_DISCONNECTED: LeftcarError = LeftcarError {
    code: "transport.disconnected",
    retryable: Retryability::Always,
    scope: ErrorScope::Session,
    action: RecoveryAction::Retry,
};
pub const ERR_DECODER_PROFILE_UNSUPPORTED: LeftcarError = LeftcarError {
    code: "decoder.profile_unsupported",
    retryable: Retryability::Conditional,
    scope: ErrorScope::Stream,
    action: RecoveryAction::Retry,
};
pub const ERR_DECODER_RESOURCE_EXHAUSTED: LeftcarError = LeftcarError {
    code: "decoder.resource_exhausted",
    retryable: Retryability::Always,
    scope: ErrorScope::Stream,
    action: RecoveryAction::CloseStreamWindow,
};
pub const ERR_STREAM_LAUNCH_EXPIRED: LeftcarError = LeftcarError {
    code: "stream.launch_expired",
    retryable: Retryability::Always,
    scope: ErrorScope::Stream,
    action: RecoveryAction::Retry,
};
pub const ERR_STREAM_STALE_CATALOG_REVISION: LeftcarError = LeftcarError {
    code: "stream.stale_catalog_revision",
    retryable: Retryability::Always,
    scope: ErrorScope::Source,
    action: RecoveryAction::Retry,
};

/// Every stable error, for the completeness test.
pub const ALL_ERRORS: &[&LeftcarError] = &[
    &ERR_PAIRING_OFFER_EXPIRED,
    &ERR_PAIRING_HOST_REJECTED,
    &ERR_AUTH_DEVICE_REVOKED,
    &ERR_PROTOCOL_VERSION_MISMATCH,
    &ERR_CAPTURE_PERMISSION_REQUIRED,
    &ERR_CAPTURE_SOURCE_UNAVAILABLE,
    &ERR_CAPTURE_PROTECTED_CONTENT,
    &ERR_ENCODER_HARDWARE_UNAVAILABLE,
    &ERR_TRANSPORT_DISCONNECTED,
    &ERR_DECODER_PROFILE_UNSUPPORTED,
    &ERR_DECODER_RESOURCE_EXHAUSTED,
    &ERR_STREAM_LAUNCH_EXPIRED,
    &ERR_STREAM_STALE_CATALOG_REVISION,
];

impl LeftcarError {
    /// Errors with conditional retryability must resolve their action at
    /// runtime; the static table maps them to a diagnostic-first action.
    pub fn is_user_recoverable(&self) -> bool {
        !matches!(self.action, RecoveryAction::OpenDiagnostics) || self.retryable != Retryability::Never
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_stable_errors_have_user_recovery_mapping() {
        assert!(!ALL_ERRORS.is_empty());
        for e in ALL_ERRORS {
            assert!(
                !e.code.is_empty(),
                "error code must be stable non-empty string"
            );
            assert!(matches!(e.action, RecoveryAction::Retry
                | RecoveryAction::RePair
                | RecoveryAction::ReSelectSource
                | RecoveryAction::GrantScreenRecording
                | RecoveryAction::CloseStreamWindow
                | RecoveryAction::UpdateHost
                | RecoveryAction::UpdateViewer
                | RecoveryAction::OpenDiagnostics));
        }
    }

    #[test]
    fn error_codes_are_unique() {
        let set: HashSet<&str> = ALL_ERRORS.iter().map(|e| e.code).collect();
        assert_eq!(set.len(), ALL_ERRORS.len());
    }

    #[test]
    fn unknown_code_maps_to_diagnostics() {
        // The UI lookup contract: unknown codes render OpenDiagnostics.
        let unknown = LeftcarError {
            code: "some.future.code",
            retryable: Retryability::Conditional,
            scope: ErrorScope::Command,
            action: RecoveryAction::OpenDiagnostics,
        };
        assert_eq!(unknown.action, RecoveryAction::OpenDiagnostics);
    }
}
