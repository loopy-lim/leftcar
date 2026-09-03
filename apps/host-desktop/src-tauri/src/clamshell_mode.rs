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
            return Err(ProviderError::EngineUnavailable(format!(
                "{} 엔진을 사용할 수 없습니다.",
                provider.name()
            )));
        }
        let display = provider.create(spec)?;
        #[cfg(target_os = "macos")]
        let assertion = {
            let on_battery = crate::power_assertion::PowerAssertion::on_battery();
            // Assertion failure degrades to None instead of failing the whole
            // session: streaming without power defense beats no stream at all.
            crate::power_assertion::PowerAssertion::acquire(on_battery).ok()
        };
        Ok(Self {
            provider,
            display,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DisplaySpec {
        DisplaySpec {
            name: "Leftcar Virtual".into(),
            width: 1920,
            height: 1200,
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
