//! Leftcar domain crate: pure state, policy, errors, IDs (ADR-0002 root).
//!
//! No platform dependencies. No video bytes. See docs/03 §4.1.

pub mod error;
pub mod ids;
pub mod km;
pub mod lease;
pub mod phase;
pub mod source;

pub use error::{ErrorScope, LeftcarError, RecoveryAction, Retryability, ALL_ERRORS};
pub use ids::{DeviceId, HostId, IdError, SessionId, SourceId, StreamInstanceId};
pub use lease::{LeaseEvent, LeaseTable, PendingStop};
pub use phase::{PairingState, StreamPhase};
pub use source::{SourceCatalogSnapshot, SourceDescriptor, SourceKind, SourceRegistry};
pub use km::{
    AndroidToMacTriggerSource, KmArrangement, KmCapabilities, KmConsent, KmDirection, KmEdgeEntry,
    KmEvent, KmEffect, KmGeometryError, KmState, KmStateMachine, MacDisplayUnion,
};
