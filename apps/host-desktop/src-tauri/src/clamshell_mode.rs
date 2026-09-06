//! Orchestrates the tablet display mode: create VD -> stream -> survive lid
//! close. Streaming/input paths are untouched; this module only sequences
//! provider calls and power assertions, and guarantees cleanup on every exit
//! path via Drop (same pattern as the Windows backend InputInjector).

use crate::provider::{DisplaySpec, ProviderError, VirtualDisplay, VirtualDisplayProvider};
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ModeState {
    Idle,
    Creating,
    Streaming,
    ClamshellActive,
    Failed(String),
}

/// Test seam: provider calls are recorded instead of shelling out.
pub struct MockProvider {
    pub available: bool,
    pub create_result: Result<(), ProviderError>,
    pub created: Mutex<Vec<String>>,
    pub removed: Mutex<Vec<String>>,
}

impl MockProvider {
    pub fn ok() -> Self {
        Self {
            available: true,
            create_result: Ok(()),
            created: Mutex::new(Vec::new()),
            removed: Mutex::new(Vec::new()),
        }
    }
}

impl VirtualDisplayProvider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn available(&self) -> bool {
        self.available
    }
    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        self.create_result.clone()?;
        self.created.lock().unwrap().push(spec.name.clone());
        Ok(VirtualDisplay {
            name: spec.name.clone(),
            cgvd_display_id: None,
        })
    }
    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        self.removed.lock().unwrap().push(display.name.clone());
        Ok(())
    }
}

pub struct TabletDisplaySession {
    pub provider: Arc<dyn VirtualDisplayProvider>,
    pub display: VirtualDisplay,
    /// Recorded at start from the same probe that picks the caffeinate flag:
    /// true means the assertion degraded to `-i` (battery), which the UI
    /// surfaces as a stability warning. Cfg-independent (false off macOS) so
    /// the status contract and this struct are identical on every target.
    pub on_battery: bool,
    #[cfg(target_os = "macos")]
    pub assertion: Option<crate::power_assertion::PowerAssertion>,
    pub state: ModeState,
}

impl TabletDisplaySession {
    /// Creates the virtual display FIRST (spark premise: zero active displays
    /// cannot create), then acquires the power assertion.
    pub fn start(
        provider: Arc<dyn VirtualDisplayProvider>,
        spec: &DisplaySpec,
    ) -> Result<Self, ProviderError> {
        if !provider.available() {
            // Bare engine name: message() already carries the "사용할 수 없습니다"
            // sentence — duplicating it here produced
            // "...사용할 수 없습니다: <name> 엔진을 사용할 수 없습니다."
            return Err(ProviderError::EngineUnavailable(provider.name().into()));
        }
        let display = provider.create(spec)?;
        // The battery probe exists only on macOS; off it there is no battery to
        // degrade on, so the field stays false and the UI shows no warning.
        #[cfg(target_os = "macos")]
        let on_battery = crate::power_assertion::PowerAssertion::on_battery();
        #[cfg(not(target_os = "macos"))]
        let on_battery = false;
        #[cfg(target_os = "macos")]
        let assertion = {
            // Assertion failure degrades to None instead of failing the whole
            // session: streaming without power defense beats no stream at all.
            crate::power_assertion::PowerAssertion::acquire(on_battery).ok()
        };
        Ok(Self {
            provider,
            display,
            on_battery,
            #[cfg(target_os = "macos")]
            assertion,
            state: ModeState::Streaming,
        })
    }
}

impl Drop for TabletDisplaySession {
    fn drop(&mut self) {
        // Best-effort cleanup: a failed remove must never panic or block app
        // shutdown; the engine-side VD is opt-in state, not user data.
        let _ = self.provider.remove(&self.display);
        // Idle is set after remove for symmetry/debuggability only — a dropped
        // object can never be observed, so this is not a state guarantee.
        self.state = ModeState::Idle;
    }
}

/// Pure parser for `ioreg -r -k AppleClamshellState` output — UI display only,
/// never a control branch. None = indeterminate (show nothing).
pub fn parse_clamshell_state(ioreg_output: &str) -> Option<bool> {
    let line = ioreg_output
        .lines()
        .find(|line| line.contains("AppleClamshellState"))?;
    let value = line.rsplit('=').next()?.trim();
    match value {
        "Yes" => Some(true),
        "No" => Some(false),
        _ => None,
    }
}

/// Wall-clock budget for one `ioreg` lid probe. Generous for a registry read
/// (it normally finishes in milliseconds) but short enough that a hung spawn
/// cannot pile up across the UI's 5-second status polls.
#[cfg(target_os = "macos")]
const IOREG_TIMEOUT_MS: u64 = 1000;

/// Reads the lid state by spawning `ioreg -r -k AppleClamshellState`.
/// macOS-only (the registry key does not exist elsewhere). Any failure —
/// spawn error, timeout, unparseable output — is indeterminate (`None`),
/// never an error: the caller degrades to `streaming`.
#[cfg(target_os = "macos")]
pub async fn read_lid_closed() -> Option<bool> {
    let child = tokio::process::Command::new("ioreg")
        .args(["-r", "-k", "AppleClamshellState"])
        // Killing on drop makes the timeout enforce itself: a hung ioreg is
        // reaped when the timed-out future is dropped.
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_millis(IOREG_TIMEOUT_MS), child)
        .await
        .ok()?
        .ok()?;
    parse_clamshell_state(&String::from_utf8_lossy(&output.stdout))
}

/// Derives the status string `tablet_display_status` reports from the stored
/// `ModeState` plus a lid reading. Pure and total (never panics), so the
/// reporting decision is unit-testable without spawning `ioreg`.
///
/// The session state is never mutated here: per the design, lid detection is
/// for UI display only, never a control branch. An indeterminate lid reading
/// (spawn failure, timeout, unparseable output) reports `streaming` — the
/// label may only advance to `clamshell` when the lid is provably closed.
///
/// Battery rides the same string as a `;battery` suffix on the live states
/// only. Rationale: the Tauri command returns `Result<String, String>`, and
/// existing builds of the UI already parse the bare prefixes, so extending
/// the string keeps old parsers working (they match exact strings like
/// "streaming" and fall through to their else branch otherwise) while the
/// current UI opt-in reads the suffix. Idle/Creating/Failed never carry it:
/// with no live session the warning would outlive its cause.
pub fn reported_status(state: &ModeState, lid_closed: Option<bool>, on_battery: bool) -> String {
    // The suffix is meaningful only while the session is live — Streaming or
    // an active clamshell; the warning must not linger after stop.
    let base = match state {
        ModeState::Idle => return "idle".to_string(),
        ModeState::Creating => return "creating".to_string(),
        ModeState::Failed(detail) => return format!("failed: {detail}"),
        // A stored ClamshellActive (defensive; no code path produces it
        // today) reports clamshell regardless of a fresh lid reading.
        ModeState::ClamshellActive => "clamshell",
        ModeState::Streaming => match lid_closed {
            Some(true) => "clamshell",
            Some(false) | None => "streaming",
        },
    };
    if on_battery {
        format!("{base};battery")
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DisplaySpec {
        DisplaySpec {
            name: "Leftcar Virtual".into(),
            width: 1920,
            height: 1200,
            scale: 1,
        }
    }

    #[test]
    fn successful_start_streams_and_records_creation() {
        let provider = Arc::new(MockProvider::ok());
        let session = TabletDisplaySession::start(provider.clone(), &spec()).unwrap();
        assert_eq!(session.state, ModeState::Streaming);
        assert_eq!(provider.created.lock().unwrap().len(), 1);
        drop(session);
        assert_eq!(
            provider.removed.lock().unwrap().len(),
            1,
            "Drop must remove the VD"
        );
    }

    #[test]
    fn unavailable_engine_fails_before_any_creation() {
        let mut provider = MockProvider::ok();
        provider.available = false;
        // let-else instead of unwrap_err: the session is not Debug (trait
        // object + non-Debug PowerAssertion), and it need not be.
        let Err(error) = TabletDisplaySession::start(Arc::new(provider), &spec()) else {
            panic!("start must fail when the engine is unavailable");
        };
        assert!(matches!(error, ProviderError::EngineUnavailable(_)));
    }

    #[test]
    fn failed_creation_propagates_without_assertion_leak() {
        let mut provider = MockProvider::ok();
        provider.create_result = Err(ProviderError::NoActiveDisplay);
        let Err(error) = TabletDisplaySession::start(Arc::new(provider), &spec()) else {
            panic!("start must fail when creation fails");
        };
        assert_eq!(error, ProviderError::NoActiveDisplay);
    }

    #[test]
    fn streaming_with_closed_lid_reports_clamshell() {
        assert_eq!(
            reported_status(&ModeState::Streaming, Some(true), false),
            "clamshell"
        );
    }

    #[test]
    fn streaming_with_open_lid_reports_streaming() {
        assert_eq!(
            reported_status(&ModeState::Streaming, Some(false), false),
            "streaming"
        );
    }

    #[test]
    fn indeterminate_lid_reading_never_claims_clamshell() {
        // Spawn failure/timeout/parse miss degrade to None: the label may
        // only advance when the lid is provably closed.
        assert_eq!(
            reported_status(&ModeState::Streaming, None, false),
            "streaming"
        );
    }

    #[test]
    fn battery_suffix_rides_the_live_states_only() {
        assert_eq!(
            reported_status(&ModeState::Streaming, Some(false), true),
            "streaming;battery"
        );
        assert_eq!(
            reported_status(&ModeState::Streaming, Some(true), true),
            "clamshell;battery"
        );
        // Idle/Creating/Failed carry no suffix: with no live session (or
        // before the session exists / after it failed) a battery warning
        // would outlive its cause.
        assert_eq!(reported_status(&ModeState::Idle, None, true), "idle");
        assert_eq!(
            reported_status(&ModeState::Creating, None, true),
            "creating"
        );
        assert_eq!(
            reported_status(&ModeState::Failed("boom".into()), None, true),
            "failed: boom"
        );
    }

    #[test]
    fn ac_power_reports_no_battery_suffix() {
        assert_eq!(
            reported_status(&ModeState::Streaming, Some(true), false),
            "clamshell"
        );
    }

    #[test]
    fn stored_states_report_verbatim() {
        assert_eq!(reported_status(&ModeState::Idle, None, false), "idle");
        assert_eq!(
            reported_status(&ModeState::Creating, None, false),
            "creating"
        );
        // A stored ClamshellActive (never produced today) must still report
        // clamshell, lid reading or not.
        assert_eq!(
            reported_status(&ModeState::ClamshellActive, Some(false), false),
            "clamshell"
        );
        assert_eq!(
            reported_status(&ModeState::Failed("boom".into()), Some(true), false),
            "failed: boom"
        );
    }

    #[test]
    fn clamshell_output_is_parsed_for_ui_only() {
        assert_eq!(
            parse_clamshell_state("\"AppleClamshellState\" = Yes"),
            Some(true)
        );
        assert_eq!(
            parse_clamshell_state("\"AppleClamshellState\" = No"),
            Some(false)
        );
        assert_eq!(parse_clamshell_state("no such key"), None);
    }
}
