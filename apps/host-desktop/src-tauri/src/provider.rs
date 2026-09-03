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
    /// CGVD engine's numeric display id — the only recoverable handle while
    /// shim `remove` is unimplemented by design. None for other engines.
    pub cgvd_display_id: Option<u32>,
}

/// Every non-macOS branch answers with this one message so the guidance is
/// identical whichever provider a caller holds. macOS builds cfg-gate every
/// use away, hence the lint escape.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const MACOS_ONLY_MESSAGE: &str = "가상 디스플레이는 macOS에서만 지원됩니다.";

/// Dev-built shim location (see tools/cgvd-shim/README.md), relative to the
/// process cwd.
const DEFAULT_SHIM_PATH: &str = "tools/cgvd-shim/.build/release/cgvd-shim";

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
// The only non-test caller sits in a macOS-cfg'd block, so the plain Windows
// lib build would flag this as dead; tests still exercise it everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn which_succeeds(binary: &str) -> bool {
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
            Ok(VirtualDisplay {
                name,
                cgvd_display_id: None,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        // No active-display premise here: the premise doc scopes it to
        // creation, and by teardown time the lid may be closed (count == 0)
        // — requiring it would misclassify cleanup as NoActiveDisplay.
        // There is nothing to discard when count == 0 anyway.
        #[cfg(target_os = "macos")]
        {
            super::virtual_display::remove_virtual_display(&display.name)
                .map(|_| ())
                .map_err(ProviderError::EngineFailed)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = display;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
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
        Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
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

/// EXPERIMENT-ONLY provider behind the CGVD opt-in flag (R-015 논골 유지).
/// Calls the Swift shim built from `tools/cgvd-shim/`; never promotes to the
/// default provider without a separate ADR.
pub struct CgvdProvider {
    // Only macOS-cfg'd methods read the path; on other platforms it is
    // written but never read, which would fail Windows `clippy -D warnings`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    binary_path: String,
}

impl CgvdProvider {
    pub fn with_binary_path(binary_path: &str) -> Self {
        Self {
            binary_path: binary_path.into(),
        }
    }

    /// Dev-built shim location; built via `swift build -c release` per
    /// tools/cgvd-shim/README.md. `LEFTCAR_CGVD_SHIM` overrides the path so
    /// a debug build or an alternate checkout can be tested without moving
    /// files — the same pattern as `LEFTCAR_CAPTURE_DYLIB` in ffi.rs.
    pub fn new() -> Self {
        if let Ok(path) = std::env::var("LEFTCAR_CGVD_SHIM") {
            // An explicit override is authoritative, even when the path does
            // not exist — availability then fails loudly instead of silently
            // probing the default.
            return Self::with_binary_path(&path);
        }
        Self::with_binary_path(DEFAULT_SHIM_PATH)
    }

    #[cfg(target_os = "macos")]
    fn classify_stdout(
        &self,
        status: std::process::ExitStatus,
        stdout: &str,
        stderr: &str,
    ) -> Result<u32, ProviderError> {
        // Shim details are untrusted text flowing into user-facing messages —
        // cap every variant that embeds one (usage lines are ~60 chars, but
        // be defensive).
        let exit_label = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into());
        let line = stdout.lines().next().unwrap_or("").trim();
        match parse_cgvd_line(line) {
            Ok(display_id) => Ok(display_id),
            Err(ProviderError::EngineFailed(detail)) => {
                // Empty stdout means the shim crashed before printing (dyld,
                // signal) or produced no output at all — the exit status and
                // the stderr tail are the only diagnosis available then.
                if line.is_empty() {
                    let stderr_tail: String =
                        stderr.lines().rev().take(2).collect::<Vec<_>>().join(" ");
                    Err(ProviderError::EngineFailed(format!(
                        "shim 출력 없음 (exit={exit_label}) {}",
                        Self::truncate_detail(&stderr_tail)
                    )))
                } else {
                    Err(ProviderError::EngineFailed(Self::truncate_detail(&detail)))
                }
            }
            Err(ProviderError::EngineUnavailable(detail)) => Err(ProviderError::EngineUnavailable(
                Self::truncate_detail(&detail),
            )),
            Err(other) => Err(other),
        }
    }

    /// Spawns the shim and returns its (status, stdout, stderr) verbatim.
    /// `probe` speaks a different one-line contract (`EXISTS`/`MISSING`) than
    /// `create`, so both callers share the spawn but classify separately.
    #[cfg(target_os = "macos")]
    fn spawn_shim(
        &self,
        args: &[&str],
    ) -> std::io::Result<(std::process::ExitStatus, String, String)> {
        let output = std::process::Command::new(&self.binary_path)
            .args(args)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((output.status, stdout, stderr))
    }

    #[cfg(target_os = "macos")]
    fn run_shim(&self, args: &[&str]) -> Result<u32, ProviderError> {
        let (status, stdout, stderr) = self.spawn_shim(args).map_err(|error| {
            ProviderError::EngineUnavailable(format!("shim 실행 불가: {error}"))
        })?;
        self.classify_stdout(status, &stdout, &stderr)
    }

    // The only non-test caller (create) is macOS-cfg'd, and remove() goes
    // through spawn_shim directly, so the plain Windows lib build would flag
    // this stub as dead; keeping it preserves the uniform call surface.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    #[cfg(not(target_os = "macos"))]
    fn run_shim(&self, _args: &[&str]) -> Result<u32, ProviderError> {
        Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
    }

    // Shim details flow into user-facing ProviderError messages; an
    // unbounded foreign string could flood the UI, so cap it defensively.
    #[cfg(target_os = "macos")]
    fn truncate_detail(detail: &str) -> String {
        const MAX_CHARS: usize = 200;
        if detail.chars().count() <= MAX_CHARS {
            detail.to_string()
        } else {
            let truncated: String = detail.chars().take(MAX_CHARS).collect();
            format!("{truncated}…")
        }
    }
}

impl Default for CgvdProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// One-line stdout contract of the shim (see tools/cgvd-shim/README.md).
/// Kept compiled on every platform so the contract tests run on Windows CI;
/// the non-macOS build never calls it (the shim is macOS-only).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_cgvd_line(line: &str) -> Result<u32, ProviderError> {
    let mut parts = line.splitn(2, ' ');
    match parts.next() {
        Some("OK") => parts
            .next()
            .and_then(|rest| rest.parse::<u32>().ok())
            .ok_or_else(|| ProviderError::EngineFailed(format!("shim 출력 파싱 실패: {line}"))),
        Some("NOACTIVE") => Err(ProviderError::NoActiveDisplay),
        Some("UNAVAILABLE") => Err(ProviderError::EngineUnavailable(
            parts.next().unwrap_or("").into(),
        )),
        Some("FAILED") => Err(ProviderError::EngineFailed(
            parts.next().unwrap_or("").into(),
        )),
        _ => Err(ProviderError::EngineFailed(format!(
            "shim 출력 파싱 실패: {line}"
        ))),
    }
}

impl VirtualDisplayProvider for CgvdProvider {
    fn name(&self) -> &'static str {
        "cgvirtualdisplay"
    }

    fn available(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Cheap disk check first — spawning a missing binary on every
            // availability poll is wasted work.
            if !std::path::Path::new(&self.binary_path).exists() {
                return false;
            }
            // probe answers EXISTS/MISSING (always exit 0) — its own contract,
            // matched literally instead of through parse_cgvd_line.
            matches!(
                self.spawn_shim(&["probe"]),
                Ok((_status, stdout, _stderr))
                    if stdout.lines().next().map(str::trim) == Some("EXISTS")
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        #[cfg(target_os = "macos")]
        {
            let count = active_display_count();
            ensure_active_display_premise(count)?;
            // Same shared validators as BetterDisplayProvider: the trait
            // promises engine-swap transparency, so ratio-style dimensions
            // and blank names must be rejected with Korean guidance before
            // the shim ever runs — not as an English shim usage line.
            let name = super::virtual_display::validate_name(&spec.name)
                .map_err(ProviderError::EngineFailed)?;
            super::virtual_display::validate_dimensions(spec.width, spec.height)
                .map_err(ProviderError::EngineFailed)?;
            let width = spec.width.to_string();
            let height = spec.height.to_string();
            // The displayID is the only handle that exists while shim `remove`
            // stays unimplemented by design (R-015) — keep it on the handle.
            let display_id = self.run_shim(&[
                "create",
                &format!("--name={name}"),
                &format!("--width={width}"),
                &format!("--height={height}"),
            ])?;
            Ok(VirtualDisplay {
                name,
                cgvd_display_id: Some(display_id),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        // No active-display premise here: cleanup must not require an active
        // display (BetterDisplayProvider lesson — the lid may be closed by
        // teardown time).
        #[cfg(target_os = "macos")]
        {
            match self.spawn_shim(&["remove", &format!("--name={}", display.name)]) {
                // Exit 3 IS the contract: the shim answers the intentional
                // R-015 "remove is not implemented by design" no-op. Detect
                // by exit code, never by the prose — detail text is capped
                // elsewhere and could be cut mid-marker. Never surface this
                // as an error to Drop.
                Ok((status, _stdout, _stderr)) if status.code() == Some(3) => Ok(()),
                Ok((status, stdout, stderr)) => {
                    self.classify_stdout(status, &stdout, &stderr).map(|_| ())
                }
                Err(error) => Err(ProviderError::EngineUnavailable(format!(
                    "shim 실행 불가: {error}"
                ))),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = display;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
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

    #[test]
    fn cgvd_output_contract_is_parsed_into_provider_errors() {
        assert_eq!(
            parse_cgvd_line("NOACTIVE"),
            Err(ProviderError::NoActiveDisplay)
        );
        assert_eq!(
            parse_cgvd_line("UNAVAILABLE shim missing"),
            Err(ProviderError::EngineUnavailable("shim missing".into()))
        );
        assert_eq!(
            parse_cgvd_line("FAILED displayID=0"),
            Err(ProviderError::EngineFailed("displayID=0".into()))
        );
        assert_eq!(parse_cgvd_line("OK 42"), Ok(42));
        assert!(parse_cgvd_line("garbage").is_err());
    }

    #[test]
    fn cgvd_parser_pins_multiformat_and_malformed_lines() {
        // Multi-word FAILED details are part of the shim contract.
        assert_eq!(
            parse_cgvd_line("FAILED registration timeout displayID=42"),
            Err(ProviderError::EngineFailed(
                "registration timeout displayID=42".into()
            ))
        );
        // A non-numeric OK payload is a contract violation, not a display id.
        assert!(parse_cgvd_line("OK abc").is_err());
        // Empty stdout (crash before print) must parse as failure, never as
        // a silently successful creation.
        assert!(parse_cgvd_line("").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cgvd_remove_treats_exit3_not_implemented_as_success() {
        // Pin the shim's exact no-op line (tools/cgvd-shim/main.swift): the
        // remove() decision is made by exit code, but the prose is part of
        // the documented contract and must not drift silently. parse_cgvd_line
        // strips the "FAILED " prefix, so the pinned detail excludes it.
        let line = "FAILED remove is not implemented yet by design (R-015 experiment scope)";
        assert_eq!(
            parse_cgvd_line(line),
            Err(ProviderError::EngineFailed(
                "remove is not implemented yet by design (R-015 experiment scope)".into()
            ))
        );

        // The exit-code path needs a real process — a stand-in script that
        // speaks the exact contract line and exits 3 like the shim does.
        let mut script = std::env::temp_dir();
        script.push(format!("leftcar-cgvd-exit3-{}.sh", std::process::id()));
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' '{line}'\nexit 3\n"),
        )
        .expect("write temp shim stand-in");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod temp shim stand-in");

        let provider = CgvdProvider::with_binary_path(script.to_str().expect("utf8 temp path"));
        let display = VirtualDisplay {
            name: "tablets".into(),
            cgvd_display_id: Some(7),
        };
        let result = provider.remove(&display);
        let _ = std::fs::remove_file(&script);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn cgvd_provider_is_object_safe_and_named() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(CgvdProvider::with_binary_path("missing-binary"));
        assert_eq!(provider.name(), "cgvirtualdisplay");
        // The injected path does not exist, so availability must be false.
        assert!(!provider.available());
    }
}
