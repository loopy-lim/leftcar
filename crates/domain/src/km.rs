//! KM (keyboard/mouse share) exclusive-ownership state machine and edge geometry.
//!
//! Pure domain, no platform dependencies (ADR-0002). Contract sources:
//! `docs/superpowers/specs/2026-09-25-wifi-bidirectional-km-design.md` §4.2 (state machine),
//! §7 (escape + release-all), §9 (disconnect/lock/sleep recovery), §11 Stage 0;
//! `docs/superpowers/specs/2026-10-03-wifi-km-optional-usb-roadmap.json` (transport contract:
//! Wi-Fi direct is default, wireless ADB is not requested, USB is an optional
//! future extension and is not modeled here).
//!
//! Invariants:
//! - I1: at most one active direction — `Active(MacToAndroid)` XOR `Active(AndroidToMac)`
//! - I2: only `Active(MacToAndroid)` suppresses Mac physical pointer input
//! - I3: every exit to [`KmState::Off`] emits [`KmEffect::ReleaseAllInjectedInput`]
//!   first, then per-subsystem teardown, then [`KmEffect::RestoreLocalInput`]
//! - I4: reconnect never auto-activates; explicit re-arm with fresh bidirectional
//!   consent is required after any fail-closed exit
//!
//! Direction-specific gates: each active direction re-checks its own consent and
//! its own trigger-source capability at activation time. Granting one direction
//! never activates the other (design §3).

use serde::{Deserialize, Serialize};

/// Tolerance for treating a Mac pointer position as touching the shared edge.
pub const EDGE_ENTRY_TOLERANCE_PX: f32 = 2.0;

/// Active KM direction. At most one can be active at any time (I1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KmDirection {
    /// Mac physical input is suppressed and forwarded to the tablet (KM_MAC_TO_ANDROID).
    MacToAndroid,
    /// Tablet physical keyboard/mouse is captured and forwarded to the Mac (KM_ANDROID_TO_MAC).
    AndroidToMac,
}

/// KM session state (design §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KmState {
    /// No capture anywhere; local input on both peers.
    Off,
    /// Bidirectional consent + preflight passed; edge watch is observe-only.
    Armed,
    /// Exclusive directional ownership.
    Active(KmDirection),
}

impl KmState {
    pub fn is_off(&self) -> bool {
        matches!(self, KmState::Off)
    }

    pub fn active_direction(&self) -> Option<KmDirection> {
        match self {
            KmState::Active(direction) => Some(*direction),
            _ => None,
        }
    }
}

/// Per-direction session consent (design §3: independent per direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KmConsent {
    pub mac_to_android: bool,
    pub android_to_mac: bool,
}

impl KmConsent {
    pub fn both(&self) -> bool {
        self.mac_to_android && self.android_to_mac
    }
}

/// Capability preflight flags (design §4.2, §2). All default false (fail-closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KmCapabilities {
    /// Mac suppress tap + input injection preflight passed.
    pub mac_to_android: bool,
    /// Accessibility key filter preflight passed (KeyBridge-style path).
    pub android_to_mac: bool,
    /// Actual tablet mouse/cursor injection authority via the approved local UHID helper.
    pub android_cursor_injection: bool,
    /// Verified physical mouse observe-and-pass: local Android input stays usable in Armed.
    pub android_observe_and_pass: bool,
    /// Physical mouse consuming capture, usable only during Active(AndroidToMac).
    pub android_mouse_capture: bool,
    /// Physical edge source via the approved USB-started local Binder UHID helper.
    /// This never grants authority to treat a remote synthetic return as activation.
    pub android_virtual_cursor: bool,
    /// Public accessibility motion subscription consumes events. Never an Armed observer.
    pub android_motion_observation: bool,
}

/// Why a directional activation request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KmActivationRejection {
    /// KM_OFF → KM_ARMED requires both directions' consent.
    MissingBidirectionalConsent,
    /// KM_OFF → KM_ARMED requires both directions' base capability preflight.
    MissingPreflightCapability,
    /// The target direction's consent is not granted.
    MissingDirectionConsent,
    /// The target direction's base capability preflight is not passed.
    MissingDirectionCapability,
    /// Trigger source requires a capability that is off (virtual cursor / motion observation).
    UnsupportedTriggerSource,
    /// Opposite direction already owns the active slot (invariant I1).
    ConflictingActiveDirection,
    /// Trigger arrived while not Armed.
    NotArmed,
}

/// Tablet-side trigger that requests AndroidToMac ownership (design §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidToMacTriggerSource {
    /// Explicit operator toggle (QS tile / notification button). Always-gated fallback.
    ExplicitToggle,
    /// Verified physical cursor edge, distinct from a synthetic remote return.
    VirtualCursorEdge,
    /// Separately verified observe-and-pass physical mouse detected edge reach.
    MotionObservationEdge,
}

/// Per-direction consent grant (used for re-consent after partial revoke).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmConsentDirection {
    MacToAndroid,
    AndroidToMac,
    Both,
}

// ---------------------------------------------------------------------------
// Edge geometry (design §4.1): arrangement, negative-origin union bounds,
// rotation-aware entry mapping and delta scaling.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrangementSide {
    /// The Mac desktop union's LEFT edge touches the tablet.
    Left,
    /// The Mac desktop union's RIGHT edge touches the tablet.
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabletOrientation {
    /// Tablet held upright: its near edge is the long (vertical) edge.
    Portrait,
    /// Tablet rotated 90° clockwise: portrait-top edge faces physically right,
    /// portrait-left edge faces physically up, tablet +x axis points physically down.
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MacDisplayUnion {
    /// Left bound of the multi-display union. May be negative (displays placed
    /// to the left of the origin) — negative origins are supported, not errors.
    pub min_x: f32,
    /// Top bound of the union. May be negative.
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KmGeometryError {
    /// NaN/infinite coordinate, extent, or delta.
    NonFinite,
    /// Width/height/tablet extent must be strictly positive.
    NonPositiveExtent,
    /// Scale must be finite and strictly positive.
    InvalidScale,
    /// Edge band must satisfy 0 <= start < end <= 1.
    InvalidEdgeBand,
    /// Entry position is not touching the configured shared edge.
    EntryNotAtEdge,
    /// Entry position is outside the configured edge band.
    EntryOutsideBand,
}

impl MacDisplayUnion {
    pub fn new(min_x: f32, min_y: f32, width: f32, height: f32) -> Result<Self, KmGeometryError> {
        let union = Self {
            min_x,
            min_y,
            width,
            height,
        };
        union.validate()?;
        Ok(union)
    }

    fn validate(&self) -> Result<(), KmGeometryError> {
        if !self.min_x.is_finite() || !self.min_y.is_finite() {
            return Err(KmGeometryError::NonFinite);
        }
        if !self.width.is_finite() || !self.height.is_finite() {
            return Err(KmGeometryError::NonFinite);
        }
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(KmGeometryError::NonPositiveExtent);
        }
        Ok(())
    }

    /// Absolute x of the shared edge, negative-origin aware.
    pub fn shared_edge_x(&self, side: ArrangementSide) -> f32 {
        match side {
            ArrangementSide::Left => self.min_x,
            ArrangementSide::Right => self.min_x + self.width,
        }
    }
}

/// KM arrangement: user-configured shared-edge band, scale and tablet orientation
/// (design §4.1 `km_arrangement`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KmArrangement {
    pub side: ArrangementSide,
    /// Start of the tablet's band along the shared Mac edge, 0.0–1.0 from the
    /// union's top.
    pub edge_norm_start: f32,
    /// End of the band; must satisfy `edge_norm_start < edge_norm_end <= 1.0`.
    pub edge_norm_end: f32,
    /// Mac px → tablet normalized velocity coefficient. Must be > 0.
    pub scale: f32,
    pub tablet_orientation: TabletOrientation,
}

impl KmArrangement {
    pub fn new(
        side: ArrangementSide,
        edge_norm_start: f32,
        edge_norm_end: f32,
        scale: f32,
        tablet_orientation: TabletOrientation,
    ) -> Result<Self, KmGeometryError> {
        let arrangement = Self {
            side,
            edge_norm_start,
            edge_norm_end,
            scale,
            tablet_orientation,
        };
        arrangement.validate()?;
        Ok(arrangement)
    }

    fn validate(&self) -> Result<(), KmGeometryError> {
        if !self.edge_norm_start.is_finite()
            || !self.edge_norm_end.is_finite()
            || !self.scale.is_finite()
        {
            return Err(KmGeometryError::NonFinite);
        }
        if !(0.0..=1.0).contains(&self.edge_norm_start)
            || !(0.0..=1.0).contains(&self.edge_norm_end)
            || self.edge_norm_start >= self.edge_norm_end
        {
            return Err(KmGeometryError::InvalidEdgeBand);
        }
        if self.scale <= 0.0 {
            return Err(KmGeometryError::InvalidScale);
        }
        Ok(())
    }

    /// Absolute y range of the configured band on the shared edge.
    pub fn band_y_range(&self, union: &MacDisplayUnion) -> (f32, f32) {
        let top = union.min_y + self.edge_norm_start * union.height;
        let bottom = union.min_y + self.edge_norm_end * union.height;
        (top, bottom)
    }

    /// Validates a Mac pointer entry at the shared edge and maps it to the
    /// tablet-normalized virtual cursor position.
    ///
    /// Placement convention (rotation-aware, design §4.1):
    /// - Portrait: near edge is vertical → cursor lands at
    ///   `(near_edge_x, entry_norm)` where `near_edge_x` is 0.0 (side Right) or 1.0 (side Left).
    /// - Landscape (90° clockwise): the physically-near edge becomes the tablet's
    ///   top edge (y = 0.0) when side is Right, bottom edge (y = 1.0) when side is
    ///   Left; the entry position maps onto the tablet x axis.
    pub fn try_enter(
        &self,
        union: &MacDisplayUnion,
        abs_x: f32,
        abs_y: f32,
    ) -> Result<KmEdgeEntry, KmGeometryError> {
        union.validate()?;
        if !abs_x.is_finite() || !abs_y.is_finite() {
            return Err(KmGeometryError::NonFinite);
        }
        let edge_x = union.shared_edge_x(self.side);
        if (abs_x - edge_x).abs() > EDGE_ENTRY_TOLERANCE_PX {
            return Err(KmGeometryError::EntryNotAtEdge);
        }
        let (band_top, band_bottom) = self.band_y_range(union);
        if abs_y < band_top || abs_y > band_bottom {
            return Err(KmGeometryError::EntryOutsideBand);
        }
        let entry_norm = (abs_y - band_top) / (band_bottom - band_top);
        let (tablet_x_norm, tablet_y_norm) = match self.tablet_orientation {
            TabletOrientation::Portrait => match self.side {
                ArrangementSide::Right => (0.0, entry_norm),
                ArrangementSide::Left => (1.0, entry_norm),
            },
            TabletOrientation::Landscape => match self.side {
                ArrangementSide::Right => (entry_norm, 0.0),
                ArrangementSide::Left => (entry_norm, 1.0),
            },
        };
        Ok(KmEdgeEntry {
            entry_norm,
            tablet_x_norm,
            tablet_y_norm,
        })
    }

    /// Maps a Mac physical pointer delta into tablet-normalized virtual cursor
    /// motion, applying the configured scale, rotation and tablet dimensions.
    ///
    /// Portrait: `dx_tablet = ±dx_mac · scale / width_px` (sign flips for side Left,
    /// where moving deeper into the tablet is physically −x).
    /// Landscape (90° CW, tablet +x points physically down):
    /// `dx_tablet = dy_mac · scale / width_px`, `dy_tablet = −dx_mac · scale / height_px`.
    pub fn map_delta(
        &self,
        dx: f32,
        dy: f32,
        tablet_width_px: f32,
        tablet_height_px: f32,
    ) -> Result<(f32, f32), KmGeometryError> {
        self.validate()?;
        if !dx.is_finite() || !dy.is_finite() {
            return Err(KmGeometryError::NonFinite);
        }
        if !tablet_width_px.is_finite()
            || !tablet_height_px.is_finite()
            || tablet_width_px <= 0.0
            || tablet_height_px <= 0.0
        {
            return Err(KmGeometryError::NonPositiveExtent);
        }
        let mapped = match self.tablet_orientation {
            TabletOrientation::Portrait => match self.side {
                ArrangementSide::Right => (dx, dy),
                ArrangementSide::Left => (-dx, dy),
            },
            TabletOrientation::Landscape => (dy, -dx),
        };
        Ok((
            mapped.0 * self.scale / tablet_width_px,
            mapped.1 * self.scale / tablet_height_px,
        ))
    }
}

/// A validated edge entry: normalized position along the shared band plus the
/// tablet-normalized virtual cursor placement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KmEdgeEntry {
    pub entry_norm: f32,
    pub tablet_x_norm: f32,
    pub tablet_y_norm: f32,
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// KM machine event set. Carries already-validated geometry (see `KmArrangement::try_enter`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KmEvent {
    /// Operator request to leave Off. Requires bidirectional consent and both
    /// base capability preflights (design §4.2).
    Arm { consent: KmConsent },
    /// Return the original physical source to its local screen, preserving consent.
    ReturnToLocal,
    /// Explicit operator disarm.
    Disarm,
    /// Grant (re-)consent for one or both directions.
    GrantConsent { direction: KmConsentDirection },
    /// Revoke consent. Revoking the active direction fails closed to Off;
    /// revoking one direction while Armed keeps Armed but that direction can
    /// no longer activate; revoking both always disarms.
    RevokeConsent { direction: KmConsentDirection },
    /// Mac pointer crossed the shared edge inside the configured band.
    MacPointerCrossedSharedEdge { entry: KmEdgeEntry },
    /// Tablet-side trigger requesting AndroidToMac ownership.
    AndroidToMacTrigger { source: AndroidToMacTriggerSource },
    /// Emergency escape combo (design §7).
    Escape,
    /// Wi-Fi connection lost (design §9).
    Disconnected,
    /// Mac locked / secure input active (design §9).
    HostLocked,
    /// Mac entered sleep (design §9).
    HostSleep,
    /// Host stop-all / revoke contract (design §11, §7).
    StopAll,
    /// KM_STATE/ACK watchdog expired (design §7: 500 ms host / 1 s viewer).
    WatchdogTimeout,
    /// Display/arrangement revalidation failed (design §9: safe disarm).
    ArrangementInvalid,
    /// Transport reconnected. Must never auto-activate (invariant I4).
    Reconnected,
}

/// Emitted effects, in contract order. The exit ordering (I3) is:
/// `ReleaseAllInjectedInput` → per-subsystem stops → `RestoreLocalInput` →
/// `StopWatchdog` → `BroadcastKmState`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum KmEffect {
    /// Observe-only Mac pointer edge watch (Armed).
    StartEdgeWatch,
    StopEdgeWatch,
    /// KM_STATE/ACK watchdog (Armed and Active).
    StartWatchdog,
    StopWatchdog,
    /// Consume + forward Mac physical pointer input (only in MacToAndroid, I2).
    SuppressMacPhysicalPointer,
    StopMacPhysicalSuppression,
    /// Place the tablet virtual KM cursor at the mapped entry position.
    MoveKmCursor {
        x_norm: f32,
        y_norm: f32,
    },
    /// Tablet key filter gate ON (AndroidToMac).
    EnableAndroidKeyFilter,
    DisableAndroidKeyFilter,
    /// Verified observe-and-pass physical mouse edge watch (Armed, separately gated).
    StartAndroidMouseObservation,
    StopAndroidMouseObservation,
    /// Consuming physical mouse capture starts only in Active(AndroidToMac).
    StartAndroidMouseCapture,
    StopAndroidMouseCapture,
    /// kind-5 release-all on both peers. Always the FIRST effect on every exit path (I3).
    ReleaseAllInjectedInput,
    /// Local input restoration after release-all.
    RestoreLocalInput,
    /// Re-broadcast KM_STATE so both peers converge.
    BroadcastKmState,
    /// A directional activation request was refused (with the reason).
    ActivationRejected(KmActivationRejection),
}

/// Host-owned KM exclusive-ownership state machine (design §4.2). The Host owns
/// this instance and broadcasts the resulting state; the Viewer obeys state only.
#[derive(Debug, Clone)]
pub struct KmStateMachine {
    state: KmState,
    consent: KmConsent,
    capabilities: KmCapabilities,
    mouse_observation_active: bool,
}

impl KmStateMachine {
    pub fn new(capabilities: KmCapabilities) -> Self {
        Self {
            state: KmState::Off,
            consent: KmConsent::default(),
            capabilities,
            mouse_observation_active: false,
        }
    }

    pub fn state(&self) -> KmState {
        self.state
    }

    pub fn consent(&self) -> KmConsent {
        self.consent
    }

    pub fn capabilities(&self) -> KmCapabilities {
        self.capabilities
    }

    pub fn on(&mut self, event: KmEvent) -> Vec<KmEffect> {
        match event {
            KmEvent::Arm { consent } => self.arm(consent),
            KmEvent::ReturnToLocal => self.return_to_local(),
            KmEvent::Disarm => self.exit_to_off(),
            KmEvent::GrantConsent { direction } => {
                self.apply_consent(direction, true);
                Vec::new()
            }
            KmEvent::RevokeConsent { direction } => self.revoke_consent(direction),
            KmEvent::MacPointerCrossedSharedEdge { entry } => self.enter_mac_to_android(entry),
            KmEvent::AndroidToMacTrigger { source } => self.enter_android_to_mac(source),
            KmEvent::Escape
            | KmEvent::Disconnected
            | KmEvent::HostLocked
            | KmEvent::HostSleep
            | KmEvent::StopAll
            | KmEvent::WatchdogTimeout
            | KmEvent::ArrangementInvalid => self.exit_to_off(),
            KmEvent::Reconnected => Vec::new(),
        }
    }

    /// Consent bookkeeping shared by Arm and GrantConsent.
    fn apply_consent(&mut self, direction: KmConsentDirection, granted: bool) {
        match direction {
            KmConsentDirection::MacToAndroid => self.consent.mac_to_android = granted,
            KmConsentDirection::AndroidToMac => self.consent.android_to_mac = granted,
            KmConsentDirection::Both => {
                self.consent.mac_to_android = granted;
                self.consent.android_to_mac = granted;
            }
        }
    }

    fn arm(&mut self, consent: KmConsent) -> Vec<KmEffect> {
        if !self.state.is_off() {
            // Arm is only an Off-exit; in Armed/Active it is a no-op.
            return Vec::new();
        }
        if !consent.both() {
            return vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingBidirectionalConsent,
            )];
        }
        if !self.capabilities.mac_to_android
            || !self.capabilities.android_to_mac
            || !self.capabilities.android_cursor_injection
            || !self.capabilities.android_mouse_capture
        {
            return vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingPreflightCapability,
            )];
        }
        self.consent = consent;
        self.state = KmState::Armed;
        let mut effects = vec![KmEffect::StartEdgeWatch, KmEffect::StartWatchdog];
        if self.capabilities.android_observe_and_pass {
            self.mouse_observation_active = true;
            effects.push(KmEffect::StartAndroidMouseObservation);
        }
        effects.push(KmEffect::BroadcastKmState);
        effects
    }

    /// Revoke consent. While Armed (observe-only) a partial revoke keeps Armed
    /// but the revoked direction can no longer activate (its gate re-checks
    /// consent). Losing the ACTIVE direction's consent fails closed to Off.
    /// Revoking both directions always disarms.
    fn revoke_consent(&mut self, direction: KmConsentDirection) -> Vec<KmEffect> {
        self.apply_consent(direction, false);
        match self.state {
            KmState::Off => Vec::new(),
            // Armed is observe-only: a single-direction revoke keeps Armed but the
            // revoked direction's gate refuses activation. Both directions revoked
            // means the bidirectional arming contract is gone → fail closed.
            KmState::Armed => {
                if matches!(direction, KmConsentDirection::Both) {
                    self.exit_to_off()
                } else {
                    Vec::new()
                }
            }
            KmState::Active(active) => {
                let active_consent = match active {
                    KmDirection::MacToAndroid => self.consent.mac_to_android,
                    KmDirection::AndroidToMac => self.consent.android_to_mac,
                };
                if active_consent {
                    Vec::new()
                } else {
                    self.exit_to_off()
                }
            }
        }
    }

    fn enter_mac_to_android(&mut self, entry: KmEdgeEntry) -> Vec<KmEffect> {
        if !matches!(self.state, KmState::Armed) {
            return self.reject_activation(KmActivationRejection::NotArmed);
        }
        if !self.consent.mac_to_android {
            return self.reject_activation(KmActivationRejection::MissingDirectionConsent);
        }
        if !self.capabilities.mac_to_android || !self.capabilities.android_cursor_injection {
            return self.reject_activation(KmActivationRejection::MissingDirectionCapability);
        }
        self.state = KmState::Active(KmDirection::MacToAndroid);
        vec![
            KmEffect::SuppressMacPhysicalPointer,
            KmEffect::MoveKmCursor {
                x_norm: entry.tablet_x_norm,
                y_norm: entry.tablet_y_norm,
            },
            KmEffect::BroadcastKmState,
        ]
    }

    fn enter_android_to_mac(&mut self, source: AndroidToMacTriggerSource) -> Vec<KmEffect> {
        if !matches!(self.state, KmState::Armed) {
            return self.reject_activation(KmActivationRejection::NotArmed);
        }
        if !self.consent.android_to_mac {
            return self.reject_activation(KmActivationRejection::MissingDirectionConsent);
        }
        if !self.capabilities.android_to_mac || !self.capabilities.android_mouse_capture {
            return self.reject_activation(KmActivationRejection::MissingDirectionCapability);
        }
        match source {
            AndroidToMacTriggerSource::ExplicitToggle => {}
            AndroidToMacTriggerSource::VirtualCursorEdge => {
                if !self.capabilities.android_virtual_cursor
                    || !self.capabilities.android_observe_and_pass
                {
                    return self.reject_activation(KmActivationRejection::UnsupportedTriggerSource);
                }
            }
            AndroidToMacTriggerSource::MotionObservationEdge => {
                if !self.capabilities.android_observe_and_pass || !self.mouse_observation_active {
                    return self.reject_activation(KmActivationRejection::UnsupportedTriggerSource);
                }
            }
        }
        self.state = KmState::Active(KmDirection::AndroidToMac);
        vec![
            KmEffect::EnableAndroidKeyFilter,
            KmEffect::StartAndroidMouseCapture,
            KmEffect::BroadcastKmState,
        ]
    }

    /// Exclusive-ownership guard: while a direction is active, any other
    /// directional activation is refused and the state is broadcast (invariant I1).
    fn reject_activation(&self, rejection: KmActivationRejection) -> Vec<KmEffect> {
        if self.state.active_direction().is_some() {
            return vec![
                KmEffect::ActivationRejected(KmActivationRejection::ConflictingActiveDirection),
                KmEffect::BroadcastKmState,
            ];
        }
        vec![KmEffect::ActivationRejected(rejection)]
    }

    /// A return uses the original physical source's edge, never an opposite
    /// physical-origin activation. Release held remote input before restoring
    /// that source locally, while the session watchdog and consent remain armed.
    fn return_to_local(&mut self) -> Vec<KmEffect> {
        let Some(direction) = self.state.active_direction() else {
            return Vec::new();
        };
        let mut effects = vec![KmEffect::ReleaseAllInjectedInput];
        match direction {
            KmDirection::MacToAndroid => effects.push(KmEffect::StopMacPhysicalSuppression),
            KmDirection::AndroidToMac => {
                effects.push(KmEffect::DisableAndroidKeyFilter);
                effects.push(KmEffect::StopAndroidMouseCapture);
            }
        }
        effects.push(KmEffect::RestoreLocalInput);
        self.state = KmState::Armed;
        effects.push(KmEffect::BroadcastKmState);
        effects
    }

    /// Every exit to Off follows invariant I3: release-all first, then
    /// per-subsystem teardown, then local restoration. Idempotent from Off.
    fn exit_to_off(&mut self) -> Vec<KmEffect> {
        if self.state.is_off() {
            return Vec::new();
        }
        let was_mac_to_android = self.state.active_direction() == Some(KmDirection::MacToAndroid);
        let was_android_to_mac = self.state.active_direction() == Some(KmDirection::AndroidToMac);
        let was_mouse_observation = self.mouse_observation_active;
        self.state = KmState::Off;
        self.mouse_observation_active = false;
        let mut effects = vec![KmEffect::ReleaseAllInjectedInput];
        if was_mac_to_android {
            effects.push(KmEffect::StopMacPhysicalSuppression);
        }
        if was_android_to_mac {
            effects.push(KmEffect::DisableAndroidKeyFilter);
            effects.push(KmEffect::StopAndroidMouseCapture);
        }
        if was_mouse_observation {
            effects.push(KmEffect::StopAndroidMouseObservation);
        }
        effects.push(KmEffect::StopEdgeWatch);
        effects.push(KmEffect::StopWatchdog);
        effects.push(KmEffect::RestoreLocalInput);
        effects.push(KmEffect::BroadcastKmState);
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn full_capabilities() -> KmCapabilities {
        KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: true,
            android_mouse_capture: true,
            android_virtual_cursor: true,
            android_motion_observation: true,
        }
    }

    fn both_consent() -> KmConsent {
        KmConsent {
            mac_to_android: true,
            android_to_mac: true,
        }
    }

    fn armed() -> KmStateMachine {
        let mut machine = KmStateMachine::new(full_capabilities());
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(machine.state(), KmState::Armed, "{effects:?}");
        machine
    }

    fn entry(entry_norm: f32) -> KmEdgeEntry {
        KmEdgeEntry {
            entry_norm,
            tablet_x_norm: 0.0,
            tablet_y_norm: entry_norm,
        }
    }

    #[test]
    fn state_accessors_report_off_and_directions() {
        assert!(KmState::Off.is_off());
        assert_eq!(KmState::Off.active_direction(), None);
        assert!(!KmState::Armed.is_off());
        assert_eq!(KmState::Armed.active_direction(), None);
        assert_eq!(
            KmState::Active(KmDirection::MacToAndroid).active_direction(),
            Some(KmDirection::MacToAndroid)
        );
    }

    #[test]
    fn normal_return_releases_before_restoring_and_preserves_armed_consent() {
        for direction in [KmDirection::MacToAndroid, KmDirection::AndroidToMac] {
            let mut machine = armed();
            match direction {
                KmDirection::MacToAndroid => {
                    machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
                }
                KmDirection::AndroidToMac => {
                    machine.on(KmEvent::AndroidToMacTrigger {
                        source: AndroidToMacTriggerSource::ExplicitToggle,
                    });
                }
            }
            let effects = machine.on(KmEvent::ReturnToLocal);
            assert_eq!(machine.state(), KmState::Armed);
            assert_eq!(machine.consent(), both_consent());
            assert_eq!(effects.first(), Some(&KmEffect::ReleaseAllInjectedInput));
            assert!(effects.contains(&KmEffect::RestoreLocalInput));
            assert!(!effects.contains(&KmEffect::StopWatchdog));
        }
    }

    #[test]
    fn public_motion_subscription_never_consumes_while_armed() {
        let mut machine = KmStateMachine::new(KmCapabilities {
            android_observe_and_pass: false,
            ..full_capabilities()
        });
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert!(!effects.contains(&KmEffect::StartAndroidMouseObservation));
        assert!(!effects.contains(&KmEffect::StartAndroidMouseCapture));
    }

    #[test]
    fn cursor_injection_authority_is_required_before_arm() {
        let mut machine = KmStateMachine::new(KmCapabilities {
            android_cursor_injection: false,
            ..full_capabilities()
        });
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(machine.state(), KmState::Off);
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingPreflightCapability
            )]
        );
    }

    #[test]
    fn consuming_mouse_capture_is_required_before_activation() {
        let mut machine = KmStateMachine::new(KmCapabilities {
            android_mouse_capture: false,
            ..full_capabilities()
        });
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        assert_ne!(machine.state(), KmState::Active(KmDirection::AndroidToMac));
        assert!(!effects.contains(&KmEffect::EnableAndroidKeyFilter));
    }

    // -- geometry ----------------------------------------------------------

    #[test]
    fn union_rejects_nonfinite_and_nonpositive_geometry() {
        assert_eq!(
            MacDisplayUnion::new(f32::NAN, 0.0, 100.0, 100.0),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            MacDisplayUnion::new(0.0, f32::INFINITY, 100.0, 100.0),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            MacDisplayUnion::new(0.0, 0.0, f32::INFINITY, 100.0),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            MacDisplayUnion::new(0.0, 0.0, 0.0, 100.0),
            Err(KmGeometryError::NonPositiveExtent)
        );
        assert_eq!(
            MacDisplayUnion::new(0.0, 0.0, 100.0, -1.0),
            Err(KmGeometryError::NonPositiveExtent)
        );
    }

    #[test]
    fn negative_origin_union_is_valid_and_maps_shared_edge() {
        let union = MacDisplayUnion::new(-1920.0, -500.0, 3840.0, 1000.0).expect("valid");
        assert_eq!(union.shared_edge_x(ArrangementSide::Right), 1920.0);
        assert_eq!(union.shared_edge_x(ArrangementSide::Left), -1920.0);
    }

    #[test]
    fn arrangement_rejects_invalid_band_and_scale() {
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                -0.1,
                0.5,
                1.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::InvalidEdgeBand)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.5,
                1.1,
                1.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::InvalidEdgeBand)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.6,
                0.5,
                1.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::InvalidEdgeBand)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.0,
                f32::NAN,
                1.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.0,
                1.0,
                0.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::InvalidScale)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.0,
                1.0,
                -1.0,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::InvalidScale)
        );
        assert_eq!(
            KmArrangement::new(
                ArrangementSide::Right,
                0.0,
                1.0,
                f32::INFINITY,
                TabletOrientation::Portrait
            ),
            Err(KmGeometryError::NonFinite)
        );
    }

    #[test]
    fn entry_inside_band_normalizes_with_negative_origins() {
        let union = MacDisplayUnion::new(-1920.0, -500.0, 3840.0, 1000.0).expect("valid");
        let arrangement = KmArrangement::new(
            ArrangementSide::Right,
            0.25,
            0.75,
            1.0,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        // band spans y in [-250, 250]; entry at y=0 is the band middle.
        let crossing = arrangement
            .try_enter(&union, 1920.0, 0.0)
            .expect("inside band");
        assert!((crossing.entry_norm - 0.5).abs() < 1e-6);
        assert!((crossing.tablet_y_norm - 0.5).abs() < 1e-6);
        assert_eq!(crossing.tablet_x_norm, 0.0);
    }

    #[test]
    fn entry_outside_band_or_off_edge_is_rejected() {
        let union = MacDisplayUnion::new(0.0, 0.0, 1000.0, 1000.0).expect("valid");
        let arrangement = KmArrangement::new(
            ArrangementSide::Right,
            0.25,
            0.75,
            1.0,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        assert_eq!(
            arrangement.try_enter(&union, 1000.0, 100.0),
            Err(KmGeometryError::EntryOutsideBand)
        );
        assert_eq!(
            arrangement.try_enter(&union, 1000.0, 900.0),
            Err(KmGeometryError::EntryOutsideBand)
        );
        assert_eq!(
            arrangement.try_enter(&union, 500.0, 500.0),
            Err(KmGeometryError::EntryNotAtEdge)
        );
        assert_eq!(
            arrangement.try_enter(&union, 1003.0, 500.0),
            Err(KmGeometryError::EntryNotAtEdge)
        );
        // tolerance boundary accepts near-edge crossings
        assert!(arrangement
            .try_enter(&union, 1000.0 - EDGE_ENTRY_TOLERANCE_PX, 500.0)
            .is_ok());
        assert!(arrangement.try_enter(&union, f32::NAN, 500.0).is_err());
    }

    #[test]
    fn portrait_entry_places_cursor_at_near_vertical_edge() {
        let union = MacDisplayUnion::new(0.0, 0.0, 1000.0, 800.0).expect("valid");
        let right = KmArrangement::new(
            ArrangementSide::Right,
            0.0,
            1.0,
            1.0,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        let e = right.try_enter(&union, 1000.0, 200.0).expect("entry");
        assert_eq!((e.tablet_x_norm, e.tablet_y_norm), (0.0, 0.25));

        let left = KmArrangement::new(
            ArrangementSide::Left,
            0.0,
            1.0,
            1.0,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        let e = left.try_enter(&union, 0.0, 200.0).expect("entry");
        assert_eq!((e.tablet_x_norm, e.tablet_y_norm), (1.0, 0.25));
    }

    #[test]
    fn landscape_entry_rotates_axes_onto_tablet_x() {
        let union = MacDisplayUnion::new(0.0, 0.0, 1000.0, 800.0).expect("valid");
        let right = KmArrangement::new(
            ArrangementSide::Right,
            0.0,
            1.0,
            1.0,
            TabletOrientation::Landscape,
        )
        .expect("valid");
        let e = right.try_enter(&union, 1000.0, 200.0).expect("entry");
        assert_eq!((e.tablet_x_norm, e.tablet_y_norm), (0.25, 0.0));

        let left = KmArrangement::new(
            ArrangementSide::Left,
            0.0,
            1.0,
            1.0,
            TabletOrientation::Landscape,
        )
        .expect("valid");
        let e = left.try_enter(&union, 0.0, 200.0).expect("entry");
        assert_eq!((e.tablet_x_norm, e.tablet_y_norm), (0.25, 1.0));
    }

    #[test]
    fn delta_mapping_applies_scale_side_sign_and_rotation() {
        let portrait_right = KmArrangement::new(
            ArrangementSide::Right,
            0.0,
            1.0,
            0.5,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        assert_eq!(
            portrait_right.map_delta(100.0, 50.0, 1000.0, 500.0),
            Ok((0.05, 0.05))
        );

        let portrait_left = KmArrangement::new(
            ArrangementSide::Left,
            0.0,
            1.0,
            0.5,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        assert_eq!(
            portrait_left.map_delta(100.0, 50.0, 1000.0, 500.0),
            Ok((-0.05, 0.05))
        );

        let landscape = KmArrangement::new(
            ArrangementSide::Right,
            0.0,
            1.0,
            0.5,
            TabletOrientation::Landscape,
        )
        .expect("valid");
        // dx_tablet = dy·scale/width = 50·0.5/1000 = 0.025
        // dy_tablet = -dx·scale/height = -100·0.5/500 = -0.1
        assert_eq!(
            landscape.map_delta(100.0, 50.0, 1000.0, 500.0),
            Ok((0.025, -0.1))
        );
    }

    #[test]
    fn delta_mapping_rejects_nonfinite_and_bad_tablet_extents() {
        let arrangement = KmArrangement::new(
            ArrangementSide::Right,
            0.0,
            1.0,
            1.0,
            TabletOrientation::Portrait,
        )
        .expect("valid");
        assert_eq!(
            arrangement.map_delta(f32::NAN, 0.0, 1000.0, 500.0),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            arrangement.map_delta(f32::INFINITY, 0.0, 1000.0, 500.0),
            Err(KmGeometryError::NonFinite)
        );
        assert_eq!(
            arrangement.map_delta(1.0, 1.0, 0.0, 500.0),
            Err(KmGeometryError::NonPositiveExtent)
        );
        assert_eq!(
            arrangement.map_delta(1.0, 1.0, f32::NAN, 500.0),
            Err(KmGeometryError::NonPositiveExtent)
        );
    }

    // -- state machine -----------------------------------------------------

    #[test]
    fn machine_starts_off_and_default_capabilities_are_fail_closed() {
        let machine = KmStateMachine::new(KmCapabilities::default());
        assert_eq!(machine.state(), KmState::Off);
        assert_eq!(machine.consent(), KmConsent::default());
        assert!(!machine.capabilities().mac_to_android);
        assert!(!machine.capabilities().android_to_mac);
        assert!(!machine.capabilities().android_virtual_cursor);
        assert!(!machine.capabilities().android_motion_observation);
    }

    #[test]
    fn arm_requires_bidirectional_consent() {
        let mut machine = KmStateMachine::new(full_capabilities());
        let effects = machine.on(KmEvent::Arm {
            consent: KmConsent {
                mac_to_android: true,
                android_to_mac: false,
            },
        });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingBidirectionalConsent
            )]
        );
        assert_eq!(machine.state(), KmState::Off);
    }

    #[test]
    fn arm_requires_base_capability_preflight() {
        let mut machine = KmStateMachine::new(KmCapabilities {
            mac_to_android: true,
            android_to_mac: false,
            android_cursor_injection: true,
            android_observe_and_pass: true,
            android_mouse_capture: true,
            android_virtual_cursor: true,
            android_motion_observation: true,
        });
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingPreflightCapability
            )]
        );
        assert_eq!(machine.state(), KmState::Off);
    }

    #[test]
    fn arm_emits_edge_watch_watchdog_observation_and_broadcast() {
        let mut machine = KmStateMachine::new(full_capabilities());
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::StartEdgeWatch,
                KmEffect::StartWatchdog,
                KmEffect::StartAndroidMouseObservation,
                KmEffect::BroadcastKmState,
            ]
        );
        assert_eq!(machine.state(), KmState::Armed);
    }

    #[test]
    fn arm_without_observation_capability_omits_observation_start() {
        let mut machine = KmStateMachine::new(KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: false,
            android_mouse_capture: true,
            android_virtual_cursor: false,
            android_motion_observation: false,
        });
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::StartEdgeWatch,
                KmEffect::StartWatchdog,
                KmEffect::BroadcastKmState,
            ]
        );
    }

    #[test]
    fn edge_crossing_activates_mac_to_android_with_cursor_placement() {
        let mut machine = armed();
        let effects = machine.on(KmEvent::MacPointerCrossedSharedEdge {
            entry: KmEdgeEntry {
                entry_norm: 0.5,
                tablet_x_norm: 0.0,
                tablet_y_norm: 0.5,
            },
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::SuppressMacPhysicalPointer,
                KmEffect::MoveKmCursor {
                    x_norm: 0.0,
                    y_norm: 0.5,
                },
                KmEffect::BroadcastKmState,
            ]
        );
        assert_eq!(machine.state(), KmState::Active(KmDirection::MacToAndroid));
    }

    #[test]
    fn edge_crossing_without_direction_consent_is_rejected_and_stays_armed() {
        let mut machine = armed();
        let effects = machine.on(KmEvent::RevokeConsent {
            direction: KmConsentDirection::MacToAndroid,
        });
        // Armed observe-only state is safe; the revoked direction simply cannot activate.
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(machine.state(), KmState::Armed);

        let effects = machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::MissingDirectionConsent
            )]
        );
        assert_eq!(machine.state(), KmState::Armed);

        // Re-granting that direction re-enables its activation gate.
        machine.on(KmEvent::GrantConsent {
            direction: KmConsentDirection::MacToAndroid,
        });
        assert!(machine
            .on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) })
            .contains(&KmEffect::SuppressMacPhysicalPointer));
    }

    #[test]
    fn toggle_activates_android_to_mac_and_revoking_active_direction_fails_closed() {
        let mut machine = armed();
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::EnableAndroidKeyFilter,
                KmEffect::StartAndroidMouseCapture,
                KmEffect::BroadcastKmState,
            ]
        );
        assert_eq!(machine.state(), KmState::Active(KmDirection::AndroidToMac));

        // Revoking the ACTIVE direction's consent fails closed to Off (I3).
        let effects = machine.on(KmEvent::RevokeConsent {
            direction: KmConsentDirection::AndroidToMac,
        });
        assert_eq!(effects.first(), Some(&KmEffect::ReleaseAllInjectedInput));
        assert!(effects.contains(&KmEffect::DisableAndroidKeyFilter));
        assert_eq!(machine.state(), KmState::Off);
    }

    #[test]
    fn revoking_inactive_direction_consent_does_not_disturb_active_direction() {
        let mut machine = armed();
        machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        let effects = machine.on(KmEvent::RevokeConsent {
            direction: KmConsentDirection::MacToAndroid,
        });
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(machine.state(), KmState::Active(KmDirection::AndroidToMac));
    }

    #[test]
    fn virtual_cursor_trigger_requires_capability() {
        let caps = KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: false,
            android_mouse_capture: true,
            android_virtual_cursor: false,
            android_motion_observation: false,
        };
        let mut machine = KmStateMachine::new(caps);
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::VirtualCursorEdge,
        });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::UnsupportedTriggerSource
            )]
        );
        assert_eq!(machine.state(), KmState::Armed);

        let mut machine = KmStateMachine::new(KmCapabilities {
            android_virtual_cursor: true,
            android_observe_and_pass: true,
            ..caps
        });
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::VirtualCursorEdge,
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::EnableAndroidKeyFilter,
                KmEffect::StartAndroidMouseCapture,
                KmEffect::BroadcastKmState,
            ]
        );
        assert_eq!(machine.state(), KmState::Active(KmDirection::AndroidToMac));
    }

    #[test]
    fn motion_observation_trigger_requires_running_observation() {
        let caps = KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: true,
            android_mouse_capture: true,
            android_virtual_cursor: false,
            android_motion_observation: true,
        };
        let mut machine = KmStateMachine::new(caps);
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        // Observation was started at Arm (capability on) — trigger allowed.
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::MotionObservationEdge,
        });
        assert!(effects.contains(&KmEffect::EnableAndroidKeyFilter));

        // Without the capability the same trigger is refused.
        let mut machine = KmStateMachine::new(KmCapabilities {
            android_motion_observation: false,
            android_observe_and_pass: false,
            ..caps
        });
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::MotionObservationEdge,
        });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::UnsupportedTriggerSource
            )]
        );
    }

    #[test]
    fn second_direction_activation_is_rejected_while_one_is_active() {
        let mut machine = armed();
        machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        assert_eq!(
            effects,
            vec![
                KmEffect::ActivationRejected(KmActivationRejection::ConflictingActiveDirection),
                KmEffect::BroadcastKmState,
            ]
        );
        assert_eq!(machine.state(), KmState::Active(KmDirection::MacToAndroid));

        // And vice versa.
        let mut machine = armed();
        machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        let effects = machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        assert_eq!(
            effects.first(),
            Some(&KmEffect::ActivationRejected(
                KmActivationRejection::ConflictingActiveDirection
            ))
        );
        assert_eq!(machine.state(), KmState::Active(KmDirection::AndroidToMac));
    }

    #[test]
    fn triggers_outside_armed_are_rejected() {
        let mut machine = KmStateMachine::new(full_capabilities());
        let effects = machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::NotArmed
            )]
        );
        let effects = machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::NotArmed
            )]
        );
    }

    #[test]
    fn arm_while_armed_or_active_is_a_noop_and_disarm_from_off_is_a_noop() {
        let mut machine = armed();
        assert!(machine
            .on(KmEvent::Arm {
                consent: both_consent()
            })
            .is_empty());
        assert_eq!(machine.state(), KmState::Armed);

        machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        assert!(machine
            .on(KmEvent::Arm {
                consent: both_consent()
            })
            .is_empty());
        assert_eq!(machine.state(), KmState::Active(KmDirection::MacToAndroid));

        let mut off = KmStateMachine::new(full_capabilities());
        assert!(off.on(KmEvent::Disarm).is_empty());
        assert!(off.on(KmEvent::Escape).is_empty());
        assert_eq!(off.state(), KmState::Off);
    }

    #[test]
    fn escape_from_any_non_off_state_releases_all_first_and_restores_local() {
        for start in [
            KmState::Armed,
            KmState::Active(KmDirection::MacToAndroid),
            KmState::Active(KmDirection::AndroidToMac),
        ] {
            let mut machine = match start {
                KmState::Armed => armed(),
                other => {
                    let mut m = armed();
                    match other.active_direction() {
                        Some(KmDirection::MacToAndroid) => {
                            m.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
                        }
                        _ => {
                            m.on(KmEvent::AndroidToMacTrigger {
                                source: AndroidToMacTriggerSource::ExplicitToggle,
                            });
                        }
                    }
                    m
                }
            };
            let effects = machine.on(KmEvent::Escape);
            assert_eq!(
                effects.first(),
                Some(&KmEffect::ReleaseAllInjectedInput),
                "{start:?}: {effects:?}"
            );
            assert!(effects.contains(&KmEffect::RestoreLocalInput), "{start:?}");
            assert!(effects.contains(&KmEffect::BroadcastKmState), "{start:?}");
            assert_eq!(machine.state(), KmState::Off, "{start:?}");
        }
    }

    #[test]
    fn fail_closed_events_disarm_from_any_non_off_state_with_release_all_first() {
        let fail_closed = [
            KmEvent::Escape,
            KmEvent::Disconnected,
            KmEvent::HostLocked,
            KmEvent::HostSleep,
            KmEvent::StopAll,
            KmEvent::WatchdogTimeout,
            KmEvent::ArrangementInvalid,
            KmEvent::RevokeConsent {
                direction: KmConsentDirection::Both,
            },
        ];
        for event in fail_closed {
            for active in [
                None,
                Some(KmDirection::MacToAndroid),
                Some(KmDirection::AndroidToMac),
            ] {
                let mut machine = armed();
                match active {
                    None => {}
                    Some(KmDirection::MacToAndroid) => {
                        machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
                    }
                    Some(KmDirection::AndroidToMac) => {
                        machine.on(KmEvent::AndroidToMacTrigger {
                            source: AndroidToMacTriggerSource::ExplicitToggle,
                        });
                    }
                }
                let effects = machine.on(event);
                assert_eq!(
                    effects.first(),
                    Some(&KmEffect::ReleaseAllInjectedInput),
                    "{event:?} from {active:?}: {effects:?}"
                );
                assert!(effects.contains(&KmEffect::RestoreLocalInput));
                assert_eq!(machine.state(), KmState::Off, "{event:?}");
                // idempotent: exiting again from Off is a no-op
                assert!(machine.on(event).is_empty(), "{event:?}");
            }
        }
    }

    #[test]
    fn teardown_stops_exactly_the_subsystems_that_were_started() {
        // Armed with observation: exit stops observation too.
        let mut machine = armed();
        let effects = machine.on(KmEvent::Escape);
        assert_eq!(
            effects,
            vec![
                KmEffect::ReleaseAllInjectedInput,
                KmEffect::StopAndroidMouseObservation,
                KmEffect::StopEdgeWatch,
                KmEffect::StopWatchdog,
                KmEffect::RestoreLocalInput,
                KmEffect::BroadcastKmState,
            ]
        );

        // Armed without observation capability: no observation stop.
        let mut machine = KmStateMachine::new(KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: false,
            android_mouse_capture: true,
            android_virtual_cursor: false,
            android_motion_observation: false,
        });
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        let effects = machine.on(KmEvent::Escape);
        assert_eq!(
            effects,
            vec![
                KmEffect::ReleaseAllInjectedInput,
                KmEffect::StopEdgeWatch,
                KmEffect::StopWatchdog,
                KmEffect::RestoreLocalInput,
                KmEffect::BroadcastKmState,
            ]
        );

        // MacToAndroid active: suppression stop included, key filter not.
        let mut machine = armed();
        machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        let effects = machine.on(KmEvent::StopAll);
        assert_eq!(
            effects,
            vec![
                KmEffect::ReleaseAllInjectedInput,
                KmEffect::StopMacPhysicalSuppression,
                KmEffect::StopAndroidMouseObservation,
                KmEffect::StopEdgeWatch,
                KmEffect::StopWatchdog,
                KmEffect::RestoreLocalInput,
                KmEffect::BroadcastKmState,
            ]
        );

        // AndroidToMac active: key filter stop included, suppression not.
        let mut machine = armed();
        machine.on(KmEvent::AndroidToMacTrigger {
            source: AndroidToMacTriggerSource::ExplicitToggle,
        });
        let effects = machine.on(KmEvent::Disconnected);
        assert_eq!(
            effects,
            vec![
                KmEffect::ReleaseAllInjectedInput,
                KmEffect::DisableAndroidKeyFilter,
                KmEffect::StopAndroidMouseCapture,
                KmEffect::StopAndroidMouseObservation,
                KmEffect::StopEdgeWatch,
                KmEffect::StopWatchdog,
                KmEffect::RestoreLocalInput,
                KmEffect::BroadcastKmState,
            ]
        );
    }

    #[test]
    fn full_session_lifecycle_emits_release_all_exactly_once_per_exit() {
        let mut machine = KmStateMachine::new(full_capabilities());
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        let effects = machine.on(KmEvent::Escape);
        let release_count = effects
            .iter()
            .filter(|e| **e == KmEffect::ReleaseAllInjectedInput)
            .count();
        assert_eq!(release_count, 1);
        assert_eq!(effects.first(), Some(&KmEffect::ReleaseAllInjectedInput));

        // Re-arm after a fail-closed exit is allowed and does not auto-activate.
        machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert_eq!(machine.state(), KmState::Armed);
    }

    #[test]
    fn reconnect_never_auto_activates_and_requires_explicit_rearm() {
        let mut machine = armed();
        machine.on(KmEvent::Disconnected);
        assert_eq!(machine.state(), KmState::Off);

        // Reconnect alone: no effects, no activation.
        assert_eq!(machine.on(KmEvent::Reconnected), Vec::new());
        assert_eq!(machine.state(), KmState::Off);

        // Trigger after reconnect without re-arm: refused.
        let effects = machine.on(KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) });
        assert_eq!(
            effects,
            vec![KmEffect::ActivationRejected(
                KmActivationRejection::NotArmed
            )]
        );

        // Explicit re-arm with fresh consent re-enters Armed.
        let effects = machine.on(KmEvent::Arm {
            consent: both_consent(),
        });
        assert!(effects.contains(&KmEffect::StartEdgeWatch));
        assert_eq!(machine.state(), KmState::Armed);
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(256))]

        #[test]
        fn exit_paths_always_release_all_first_and_land_off(
            ops in proptest::collection::vec(proptest::bool::ANY, 0..32)
        ) {
            let mut machine = KmStateMachine::new(full_capabilities());
            let mut activated = false;
            for (i, &toggle) in ops.iter().enumerate() {
                let event = if !activated {
                    if toggle {
                        KmEvent::Arm { consent: both_consent() }
                    } else {
                        KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) }
                    }
                } else if toggle {
                    KmEvent::WatchdogTimeout
                } else {
                    KmEvent::HostLocked
                };
                let was_active = !machine.state().is_off();
                let effects = machine.on(event);
                if was_active && matches!(event, KmEvent::WatchdogTimeout | KmEvent::HostLocked) {
                    prop_assert_eq!(effects.first(), Some(&KmEffect::ReleaseAllInjectedInput));
                    prop_assert_eq!(machine.state(), KmState::Off);
                    activated = false;
                }
                if machine.state() == KmState::Active(KmDirection::MacToAndroid) {
                    activated = true;
                }
                if machine.state() == KmState::Off {
                    activated = false;
                }
                let _ = i;
            }
            // Final safety: force exit must always land Off with release-all when non-Off.
            if !machine.state().is_off() {
                let effects = machine.on(KmEvent::StopAll);
                prop_assert_eq!(effects.first(), Some(&KmEffect::ReleaseAllInjectedInput));
                prop_assert_eq!(machine.state(), KmState::Off);
            }
        }

        #[test]
        fn no_event_sequence_auto_activates_after_reconnect(
            ops in proptest::collection::vec(proptest::bool::ANY, 0..16)
        ) {
            let mut machine = armed();
            machine.on(KmEvent::Disconnected);
            for &toggle in ops.iter() {
                let event = if toggle {
                    KmEvent::Reconnected
                } else {
                    KmEvent::MacPointerCrossedSharedEdge { entry: entry(0.5) }
                };
                let effects = machine.on(event);
                prop_assert!(machine.state() != KmState::Active(KmDirection::MacToAndroid));
                prop_assert!(machine.state() != KmState::Active(KmDirection::AndroidToMac));
                prop_assert!(!effects.contains(&KmEffect::SuppressMacPhysicalPointer));
                prop_assert!(!effects.contains(&KmEffect::EnableAndroidKeyFilter));
            }
        }
    }
}
