//! Virtual display provider abstraction for the tablet display feature.
//!
//! The rest of the pipeline (streaming, input, power) only sees this trait,
//! so the engine can be swapped without touching callers. Existing CLI argv
//! builders in `virtual_display.rs` remain the single source of truth for
//! BetterDisplay contracts.

/// What callers ask a provider to materialize.
pub struct DisplaySpec {
    pub name: String,
    /// Pixel dimensions, not ratios (see `virtual_display::validate_dimensions`).
    pub width: u32,
    pub height: u32,
}

/// Handle to a display a provider created.
pub struct VirtualDisplay {
    pub name: String,
}

/// Failure causes map 1:1 to UI guidance (spark 3-way classification).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ProviderError {
    /// Zero active displays — macOS cannot create a virtual display.
    /// UI must tell the user to open the lid or wake an external monitor.
    NoActiveDisplay,
    /// Engine missing: BetterDisplay not installed / CGVD shim absent.
    EngineUnavailable(String),
    /// Engine ran and failed (abort, displayID=0, CLI stderr...).
    EngineFailed(String),
}

impl ProviderError {
    pub fn message(&self) -> String {
        match self {
            ProviderError::NoActiveDisplay => {
                "활성 화면이 없습니다. 덮개를 열거나 외장 모니터를 켠 후 시작하세요.".into()
            }
            ProviderError::EngineUnavailable(detail) => {
                format!("가상 디스플레이 엔진을 사용할 수 없습니다: {detail}")
            }
            ProviderError::EngineFailed(detail) => format!("가상 디스플레이 생성 실패: {detail}"),
        }
    }
}

/// The tablet-display pipeline depends only on this trait.
pub trait VirtualDisplayProvider: Send + Sync {
    fn name(&self) -> &'static str;
    /// True when the engine is installed/reachable. Must not create anything.
    fn available(&self) -> bool;
    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError>;
    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError>;
}

/// Premise check shared by every provider: creation is impossible with zero
/// active displays (spark-verified on macOS 26.6.2). Must run BEFORE the
/// engine is invoked so headless attempts fail with actionable guidance.
pub fn ensure_active_display_premise(active_count: u32) -> Result<(), ProviderError> {
    if active_count == 0 {
        Err(ProviderError::NoActiveDisplay)
    } else {
        Ok(())
    }
}

/// Wraps the existing BetterDisplay CLI contracts in `virtual_display.rs`.
/// This module never bundles BetterDisplay — availability is probed, and the
/// user-facing error tells them what to install.
pub struct BetterDisplayProvider;

impl BetterDisplayProvider {
    pub fn new() -> Self {
        Self
    }
}

/// PATH probe shared by provider availability checks.
pub fn which_succeeds(binary: &str) -> bool {
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

impl VirtualDisplayProvider for BetterDisplayProvider {
    fn name(&self) -> &'static str {
        "betterdisplay"
    }

    fn available(&self) -> bool {
        // `CLI` and the spawn helpers are macOS-only in `virtual_display.rs`,
        // and Windows CI compiles and tests this crate, so the engine probe is
        // cfg-gated: on other platforms the engine is simply absent.
        #[cfg(target_os = "macos")]
        {
            which_succeeds(super::virtual_display::CLI)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        ensure_engine_premise()?;
        #[cfg(target_os = "macos")]
        {
            // The name is validated BEFORE the handle is built so `VirtualDisplay`
            // always carries a non-empty name — `remove` resolves by `-namelike=`,
            // and a blank name could discard ALL discardable devices.
            let name = super::virtual_display::validate_name(&spec.name)
                .map_err(ProviderError::EngineFailed)?;
            super::virtual_display::validate_dimensions(spec.width, spec.height)
                .map_err(ProviderError::EngineFailed)?;
            super::virtual_display::create_virtual_display(&name, spec.width, spec.height)
                .map_err(ProviderError::EngineFailed)?;
            Ok(VirtualDisplay { name })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable(
                "가상 디스플레이는 macOS에서만 지원됩니다.".into(),
            ))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        ensure_engine_premise()?;
        #[cfg(target_os = "macos")]
        {
            super::virtual_display::remove_virtual_display(&display.name)
                .map(|_| ())
                .map_err(ProviderError::EngineFailed)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = display;
            Err(ProviderError::EngineUnavailable(
                "가상 디스플레이는 macOS에서만 지원됩니다.".into(),
            ))
        }
    }
}

fn ensure_engine_premise() -> Result<(), ProviderError> {
    #[cfg(target_os = "macos")]
    {
        let count = active_display_count();
        ensure_active_display_premise(count)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(ProviderError::EngineUnavailable(
            "가상 디스플레이는 macOS에서만 지원됩니다.".into(),
        ))
    }
}

#[cfg(target_os = "macos")]
fn active_display_count() -> u32 {
    use std::os::raw::c_uint;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(
            max_displays: c_uint,
            active_displays: *mut c_uint,
            display_count: *mut c_uint,
        ) -> i32;
    }
    let mut count: c_uint = 0;
    // Safe: count pointer is valid; null list with max=0 only writes count.
    unsafe {
        CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count);
    }
    count
}

impl Default for BetterDisplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_active_displays_is_rejected_before_engine_call() {
        assert_eq!(
            ensure_active_display_premise(0),
            Err(ProviderError::NoActiveDisplay)
        );
    }

    #[test]
    fn any_positive_display_count_passes_the_premise() {
        assert_eq!(ensure_active_display_premise(1), Ok(()));
        assert_eq!(ensure_active_display_premise(3), Ok(()));
    }

    #[test]
    fn error_messages_are_user_actionable() {
        assert!(ProviderError::NoActiveDisplay
            .message()
            .contains("덮개를 열거나"));
        assert!(ProviderError::EngineUnavailable("BD 미설치".into())
            .message()
            .contains("BD 미설치"));
    }

    #[test]
    fn betterdisplay_provider_reports_engine_unavailable_without_cli() {
        // No mocking of PATH here: betterdisplaycli is not on CI's PATH, so
        // available() must be false there — that IS the contract under test.
        let provider = BetterDisplayProvider::new();
        if which_succeeds("betterdisplaycli") {
            assert!(provider.available());
        } else {
            assert!(!provider.available());
        }
    }

    #[test]
    fn providers_are_object_safe_for_the_session_registry() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(BetterDisplayProvider::new());
        assert_eq!(provider.name(), "betterdisplay");
    }
}
