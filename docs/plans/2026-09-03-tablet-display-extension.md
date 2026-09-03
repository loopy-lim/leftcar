# 태블릿 화면 확장 구현 계획 (Tablet Display Extension)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 태블릿 연결 시 가상 디스플레이를 생성해 스트리밍하고, 덮개를 닫아도 생존하는 "유일 화면 모드"를 제공한다.

**Architecture:** `virtual_display.rs` 위에 `VirtualDisplayProvider` trait 추상화(BetterDisplay/CGVD 듀얼 프로바이더), `caffeinate` 절전 방어(`power_assertion.rs`), 상태 머신 오케스트레이션(`clamshell_mode.rs`), Tauri 명령 3종과 UI 카드 확장. 스트리밍/입력 경로는 기존 코드 무수정.

**Tech Stack:** Rust (Tauri 2, crate `leftcar-host-desktop`), Swift (CGVD shim, SwiftPM), TypeScript/React (App.tsx), i18n (`packages/ui-tokens/src/i18n.ts`).

**설계 문서:** `docs/plans/2026-09-03-tablet-display-extension-design.md`

**검증 명령:**
- Rust: `cargo test -p leftcar-host-desktop --lib`
- TS: `bun run typecheck && bun run test`
- React 게이트: `npx -y react-doctor@latest . --verbose` (100/100 필수)

---

### Task 1: 프로바이더 타입과 NoActiveDisplay 전제 검사

**Files:**
- Create: `apps/host-desktop/src-tauri/src/provider.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs:16` (`pub mod provider;` 추가)

**Step 1: 실패하는 테스트 작성** — `provider.rs`를 새로 만들고 아래를 작성:

```rust
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
    pub cgvd_display_id: Option<u32>,
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
}
```

`lib.rs:16` 근처(`pub mod virtual_display;` 옆)에 `pub mod provider;` 추가.

**Step 2: 테스트 실패 확인**

Run: `cargo test -p leftcar-host-desktop --lib provider`
Expected: FAIL (module not yet wired — compile error 직후 PASS로 전환됨을 확인)

**Step 3: 최소 구현** — 위 코드가 곧 구현이다 (테스트가 로직 전부를 검증).

**Step 4: 테스트 통과 확인**

Run: `cargo test -p leftcar-host-desktop --lib provider`
Expected: PASS (3 tests)

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/provider.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): 가상 디스플레이 프로바이더 추상화 타입과 활성 화면 전제 검사"
```

---

### Task 2: BetterDisplayProvider — 기존 CLI 경로의 trait 구현

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/provider.rs`
- Reuse: `apps/host-desktop/src-tauri/src/virtual_display.rs` (수정 없음 — argv 빌더·validate 재사용)

**Step 1: 실패하는 테스트 작성** — `provider.rs` tests에 추가:

```rust
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
```

그리고 trait 객체 사용 가능성(오케스트레이션이 `Arc<dyn VirtualDisplayProvider>`로 담는다는 설계)을 잠그는 테스트:

```rust
    #[test]
    fn providers_are_object_safe_for_the_session_registry() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(BetterDisplayProvider::new());
        assert_eq!(provider.name(), "betterdisplay");
    }
```

**Step 2: 실패 확인** — Run: `cargo test -p leftcar-host-desktop --lib provider` → Expected: FAIL (`BetterDisplayProvider` 미정의)

**Step 3: 구현** — `provider.rs`에 추가:

```rust
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
        which_succeeds(super::virtual_display::CLI)
    }

    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        ensure_engine_premise()?;
        let name = super::virtual_display::validate_name(&spec.name)
            .map_err(ProviderError::EngineFailed)?;
        super::virtual_display::validate_dimensions(spec.width, spec.height)
            .map_err(ProviderError::EngineFailed)?;
        run_cli_chain(|| {
            let create = super::virtual_display::create_virtual_display(
                &name,
                spec.width,
                spec.height,
            )?;
            Ok(create)
        })
        .map(|_| VirtualDisplay { name, cgvd_display_id: None })
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        ensure_engine_premise()?;
        run_cli_chain(|| {
            super::virtual_display::remove_virtual_display(&display.name)?;
            Ok(())
        })
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

fn run_cli_chain<T>(
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, ProviderError> {
    body().map_err(ProviderError::EngineFailed)
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
```

**Step 4: 통과 확인** — Run: `cargo test -p leftcar-host-desktop --lib` → Expected: PASS (기존 virtual_display 테스트 포함 전부)

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/provider.rs
git commit -m "feat(host): BetterDisplay 프로바이더 — 기존 CLI 계약의 trait 구현"
```

---

### Task 3: CGVD Swift shim (probe/create/remove)

**Files:**
- Create: `tools/cgvd-shim/Package.swift`
- Create: `tools/cgvd-shim/Sources/cgvd-shim/main.swift`
- Create: `tools/cgvd-shim/CGVD.h` (스파크 `tools/cgvd-spark/CGVD.h`의 저장소 소유 선언 복사 — OpenDisplay 코드 아님)
- Create: `tools/cgvd-shim/.gitignore` (`.build`)
- Create: `tools/cgvd-shim/README.md` (빌드·계약·R-015 논골 경고)

**Step 1: Package.swift 작성**

```swift
// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "cgvd-shim",
    targets: [
        .executableTarget(
            name: "cgvd-shim",
            cSettings: [.unsafeFlags(["-I", "Sources/cgvd-shim"])]
        )
    ]
)
```

**Step 2: main.swift 작성** — 3개 서브커맨드. 스파크의 3분류 진단 출력 계약을 그대로 사용:

```swift
// CGVD shim for the tablet-display CgvdProvider. EXPERIMENT ONLY (R-015):
// promotion to the default provider requires a separate ADR backed by the
// spark evidence (docs/research/2026-09-03_cgvd-spark-results.md).
//
// stdout contract (one line):
//   probe   -> "EXISTS" | "MISSING"
//   create  -> "OK <displayID>" | "NOACTIVE" | "UNAVAILABLE <detail>" | "FAILED <detail>"
//   remove  -> "OK" | "FAILED <detail>"
import Foundation

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    print("FAILED usage: cgvd-shim <probe|create|remove> [options]")
    exit(2)
}

switch arguments[1] {
case "probe":
    print(CGVirtualDisplayDescriptor.cls == nil ? "MISSING" : "EXISTS")
case "create":
    create(arguments)
case "remove":
    print("FAILED remove is not implemented yet by design (R-015 experiment scope)")
    exit(3)
default:
    print("FAILED unknown subcommand \(arguments[1])")
    exit(2)
}
```

`create(_:)`와 CGVD 브리지는 스파크 `tools/cgvd-spark/main.swift`의 생성 코드(모드 1개, 1x 배율)를 축소해 이식한다 — 이름/폭/높이는 `--name= --width= --height=` 플래그로 받고, `CGSessionCopyCurrentDictionary() == nil`이면 `NOACTIVE` 이전에 세션 밖 실행을 `UNAVAILABLE session`으로, `CGGetActiveDisplayList` 0이면 `NOACTIVE`를 출력한다.

**Step 3: 빌드·수동 스모크**

Run: `cd tools/cgvd-shim && swift build`
Expected: BUILD SUCCEEDED
Run: `.build/debug/cgvd-shim probe` → Expected: `EXISTS` (macOS 26에서) — **GUI 세션에서 실행할 것** (docs/dev-environment.md 제약)

**Step 4: 커밋**

```bash
git add tools/cgvd-shim/
git commit -m "feat(spark): CGVD 프로바이더용 Swift shim — probe/create 계약과 3분류 진단 출력"
```

---

### Task 4: CgvdProvider — shim 프로세스 호출

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/provider.rs`

**Step 1: 실패하는 테스트 작성** — `provider.rs` tests에 추가:

```rust
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
    fn cgvd_provider_is_object_safe_and_named() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(CgvdProvider::with_binary_path("missing-binary"));
        assert_eq!(provider.name(), "cgvirtualdisplay");
        // The injected path does not exist, so availability must be false.
        assert!(!provider.available());
    }
```

**Step 2: 실패 확인** — Run: `cargo test -p leftcar-host-desktop --lib provider` → Expected: FAIL

**Step 3: 구현** — `provider.rs`에 추가:

```rust
/// EXPERIMENT-ONLY provider behind the CGVD opt-in flag (R-015 논골 유지).
/// Calls the Swift shim built from `tools/cgvd-shim/`; never promotes to the
/// default provider without a separate ADR.
pub struct CgvdProvider {
    binary_path: String,
}

impl CgvdProvider {
    pub fn with_binary_path(binary_path: &str) -> Self {
        Self { binary_path: binary_path.into() }
    }

    /// Dev-built shim location; documented in tools/cgvd-shim/README.md.
    pub fn new() -> Self {
        Self::with_binary_path("tools/cgvd-shim/.build/release/cgvd-shim")
    }

    #[cfg(target_os = "macos")]
    fn run_shim(&self, args: &[&str]) -> Result<String, ProviderError> {
        let output = std::process::Command::new(&self.binary_path)
            .args(args)
            .output()
            .map_err(|error| {
                ProviderError::EngineUnavailable(format!("shim 실행 불가: {error}"))
            })?;
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        parse_cgvd_line(&line)
    }
    #[cfg(not(target_os = "macos"))]
    fn run_shim(&self, _args: &[&str]) -> Result<String, ProviderError> {
        Err(ProviderError::EngineUnavailable("macOS 전용입니다.".into()))
    }
}

impl Default for CgvdProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// One-line stdout contract of the shim (see tools/cgvd-shim/main.swift).
#[cfg_attr(not(test), allow(dead_code))]
fn parse_cgvd_line(line: &str) -> Result<u32, ProviderError> {
    let mut parts = line.splitn(2, ' ');
    match parts.next() {
        Some("OK") => parts
            .next()
            .and_then(|rest| rest.parse::<u32>().ok())
            .ok_or_else(|| ProviderError::EngineFailed(format!("shim 출력 파싱 실패: {line}"))),
        Some("NOACTIVE") => Err(ProviderError::NoActiveDisplay),
        Some("UNAVAILABLE") => {
            Err(ProviderError::EngineUnavailable(parts.next().unwrap_or("").into()))
        }
        Some("FAILED") => Err(ProviderError::EngineFailed(parts.next().unwrap_or("").into())),
        _ => Err(ProviderError::EngineFailed(format!("shim 출력 파싱 실패: {line}"))),
    }
}

impl VirtualDisplayProvider for CgvdProvider {
    fn name(&self) -> &'static str {
        "cgvirtualdisplay"
    }

    fn available(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            std::path::Path::new(&self.binary_path).exists()
                && matches!(self.run_shim(&["probe"]), Ok(_))
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
            let width = spec.width.to_string();
            let height = spec.height.to_string();
            self.run_shim(&[
                "create",
                &format!("--name={}", spec.name),
                &format!("--width={width}"),
                &format!("--height={height}"),
            ])?;
            Ok(VirtualDisplay { name: spec.name.clone(), cgvd_display_id: None })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable("macOS 전용입니다.".into()))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        self.run_shim(&["remove", &format!("--name={}", display.name)])
            .map(|_| ())
    }
}
```

**Step 4: 통과 확인** — Run: `cargo test -p leftcar-host-desktop --lib` → Expected: PASS

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/provider.rs
git commit -m "feat(host): CgvdProvider — shim stdout 계약 파싱과 실험 옵트인 구현"
```

---

### Task 5: 절전 방어 (power_assertion.rs)

**Files:**
- Create: `apps/host-desktop/src-tauri/src/power_assertion.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (`pub mod power_assertion;`)

**Step 1: 실패하는 테스트 작성** — `power_assertion.rs`:

```rust
//! Power assertions keeping the Mac awake after the lid closes.
//!
//! Primary defense is a `caffeinate` child process: dropping the handle
//! terminates the child, so the assertion can never outlive the app.

/// `-s` (keep system awake) is only valid on AC power; on battery we degrade
/// to `-i` (prevent idle sleep) and the UI shows a stability warning.
pub fn caffeinate_args(on_battery: bool) -> Vec<String> {
    vec![if on_battery { "-i".into() } else { "-s".into() }]
}

/// Parses `pmset -g batt` output. Unknown formats conservatively assume AC
/// (stronger assertion) — the failure mode is a louder warning, not a crash.
pub fn parse_pmset_reports_battery(pmset_output: &str) -> bool {
    pmset_output.to_ascii_lowercase().contains("discharging")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_power_uses_the_system_sleep_assertion() {
        assert_eq!(caffeinate_args(false), vec!["-s".to_string()]);
    }

    #[test]
    fn battery_power_degrades_to_idle_prevention() {
        assert_eq!(caffeinate_args(true), vec!["-i".to_string()]);
    }

    #[test]
    fn pmset_discharging_means_battery() {
        assert!(parse_pmset_reports_battery(
            "Now drawing from 'Battery'\n -InternalBattery-0 87%; discharging; 4:20 remaining"
        ));
        assert!(!parse_pmset_reports_battery(
            "Now drawing from 'AC Power'\n -InternalBattery-0 100%; charged; 0:00 remaining"
        ));
        // Unknown format: conservatively AC.
        assert!(!parse_pmset_reports_battery("unexpected"));
    }
}
```

그리고 macos 전용 Drop 계약은 spawn 없이 검증할 수 없으므로 cfg(macos) 통합 테스트는 Task 10의 실기 절차로 넘긴다(유닛 레벨에서는 argv와 파싱만 잠근다).

**Step 2: 실패 확인** — Run: `cargo test -p leftcar-host-desktop --lib power_assertion` → Expected: FAIL (모듈 미등록 컴파일 에러)

**Step 3: 구현** — 같은 파일에 spawn 추가:

```rust
#[cfg(target_os = "macos")]
pub struct PowerAssertion {
    child: std::process::Child,
}

#[cfg(target_os = "macos")]
impl PowerAssertion {
    /// Spawns `caffeinate` so the Mac stays usable after the lid closes.
    pub fn acquire(on_battery: bool) -> Result<Self, String> {
        let child = std::process::Command::new("caffeinate")
            .args(caffeinate_args(on_battery))
            .spawn()
            .map_err(|error| format!("caffeinate 실행 실패: {error}"))?;
        Ok(Self { child })
    }

    pub fn on_battery() -> bool {
        parse_pmset_reports_battery(
            &std::process::Command::new("pmset")
                .args(["-g", "batt"])
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
                .unwrap_or_default(),
        )
    }
}

#[cfg(target_os = "macos")]
impl Drop for PowerAssertion {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

`lib.rs`에 `pub mod power_assertion;` 추가.

**Step 4: 통과 확인** — Run: `cargo test -p leftcar-host-desktop --lib` → Expected: PASS

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/power_assertion.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): caffeinate 절전 방어 — AC/-s, 배터리/-i 강등과 Drop 정리"
```

---

### Task 6: 상태 머신 (clamshell_mode.rs)

**Files:**
- Create: `apps/host-desktop/src-tauri/src/clamshell_mode.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (`pub mod clamshell_mode;`)

**Step 1: 실패하는 테스트 작성** — `clamshell_mode.rs`:

```rust
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
        Ok(VirtualDisplay { name: spec.name.clone(), cgvd_display_id: None })
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
        assert_eq!(provider.removed.lock().unwrap().len(), 1, "Drop must remove the VD");
    }

    #[test]
    fn unavailable_engine_fails_before_any_creation() {
        let mut provider = MockProvider::ok();
        provider.available = false;
        let error = TabletDisplaySession::start(Arc::new(provider), &spec()).unwrap_err();
        assert!(matches!(error, ProviderError::EngineUnavailable(_)));
    }

    #[test]
    fn failed_creation_propagates_without_assertion_leak() {
        let mut provider = MockProvider::ok();
        provider.create_result = Err(ProviderError::NoActiveDisplay);
        let error = TabletDisplaySession::start(Arc::new(provider), &spec()).unwrap_err();
        assert_eq!(error, ProviderError::NoActiveDisplay);
    }

    #[test]
    fn clamshell_output_is_parsed_for_ui_only() {
        assert_eq!(parse_clamshell_state("\"AppleClamshellState\" = Yes"), Some(true));
        assert_eq!(parse_clamshell_state("\"AppleClamshellState\" = No"), Some(false));
        assert_eq!(parse_clamshell_state("no such key"), None);
    }
}
```

**Step 2: 실패 확인** — Run: `cargo test -p leftcar-host-desktop --lib clamshell_mode` → Expected: FAIL (컴파일 에러)

**Step 3: 구현** — 위 코드가 곧 구현. `lib.rs`에 `pub mod clamshell_mode;` 추가.

**Step 4: 통과 확인** — Run: `cargo test -p leftcar-host-desktop --lib` → Expected: PASS

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/clamshell_mode.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): 태블릿 화면 세션 상태 머신 — 생성→스트리밍→Drop 정리 보장"
```

---

### Task 7: Tauri 명령 3종과 프로바이더 선택

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (명령 3종 + 상태 + 핸들러 등록 92-93행 부근)

**Step 1: 실패하는 테스트 작성** — `lib.rs` tests 모듈(475행)에 추가:

```rust
    #[test]
    fn provider_kind_selects_the_registered_engines() {
        assert!(super::provider_kind_error("betterdisplay").is_none());
        assert!(super::provider_kind_error("cgvirtualdisplay").is_none());
        let unknown = super::provider_kind_error("duet").unwrap();
        assert!(unknown.contains("지원하지 않는"));
    }
```

**Step 2: 실패 확인** — Run: `cargo test -p leftcar-host-desktop --lib provider_kind` → Expected: FAIL

**Step 3: 구현** — `lib.rs`에 추가:

```rust
/// Validates the provider kind sent from the UI. CGVD stays opt-in behind the
/// `cgvirtualdisplay` kind; R-015 forbids promoting it to the default.
fn provider_kind_error(kind: &str) -> Option<String> {
    match kind {
        "betterdisplay" | "cgvirtualdisplay" => None,
        other => Some(format!("지원하지 않는 엔진입니다: {other}")),
    }
}

type SessionRegistry = std::sync::Mutex<Option<clamshell_mode::TabletDisplaySession>>;

fn provider_for_kind(kind: &str) -> std::sync::Arc<dyn provider::VirtualDisplayProvider> {
    match kind {
        "cgvirtualdisplay" => std::sync::Arc::new(provider::CgvdProvider::new()),
        _ => std::sync::Arc::new(provider::BetterDisplayProvider::new()),
    }
}

/// Async so blocking engine spawns run off the main thread (same rationale as
/// create_virtual_display above).
#[tauri::command]
async fn tablet_display_start(
    state: tauri::State<'_, SessionRegistry>,
    provider_kind: String,
    name: String,
    width: u32,
    height: u32,
) -> Result<String, String> {
    provider_kind_error(&provider_kind).map_err(|error| error.clone())?;
    #[cfg(target_os = "macos")]
    {
        let mut guard = state.lock().map_err(|error| error.to_string())?;
        if guard.is_some() {
            return Err("태블릿 화면 세션이 이미 실행 중입니다.".into());
        }
        let session = clamshell_mode::TabletDisplaySession::start(
            provider_for_kind(&provider_kind),
            &provider::DisplaySpec { name, width, height },
        )
        .map_err(|error| error.message())?;
        *guard = Some(session);
        Ok("태블릿 화면 세션 시작됨".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, name, width, height);
        Err("태블릿 화면 확장은 macOS에서만 지원됩니다.".into())
    }
}

#[tauri::command]
async fn tablet_display_stop(state: tauri::State<'_, SessionRegistry>) -> Result<String, String> {
    let mut guard = state.lock().map_err(|error| error.to_string())?;
    match guard.take() {
        Some(session) => {
            drop(session); // Drop removes the VD and kills caffeinate.
            Ok("태블릿 화면 세션을 정리했습니다.".into())
        }
        None => Ok("실행 중인 세션이 없습니다.".into()),
    }
}

#[tauri::command]
fn tablet_display_status(state: tauri::State<'_, SessionRegistry>) -> Result<String, String> {
    let guard = state.lock().map_err(|error| error.to_string())?;
    Ok(match guard.as_ref() {
        Some(session) => format!("{:?}", session.state),
        None => "Idle".into(),
    })
}
```

`tauri::Builder`의 `.manage(...)` 체인에 `SessionRegistry: std::sync::Mutex::new(None)` 등록하고, `invoke_handler` 목록(92-93행)에 `tablet_display_start, tablet_display_stop, tablet_display_status` 추가.

**Step 4: 통과 확인** — Run: `cargo test -p leftcar-host-desktop --lib` → Expected: PASS. 이어서 `cargo check -p leftcar-host-desktop` → Expected: 완료 (에러 없음)

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): 태블릿 화면 명령 3종 — 시작/중지/상태와 프로바이더 선택"
```

---

### Task 8: i18n 키 추가 (ko/en)

**Files:**
- Modify: `packages/ui-tokens/src/i18n.ts:223-235` (ko), `i18n.ts:450-462` (en)

**Step 1: 실패하는 테스트 작성** — `packages/ui-tokens/src/i18n.test.ts`에 키 정합 테스트가 이미 있을 것(로캘 간 키 대칭 검증). 새 키 추가 전 먼저 실행해 현황 확인:

Run: `bun x vitest run packages/ui-tokens/src/i18n.test.ts`
Expected: PASS (기존)

**Step 2: 키 추가** — ko 블록(`virtualDisplayToggleOff: "꺼짐",` 뒤):

```typescript
      tabletDisplayTitle: "태블릿 화면 확장",
      tabletDisplayStart: "화면 확장 시작",
      tabletDisplayStop: "중지",
      tabletDisplayIdle: "대기 중",
      tabletDisplayStreaming: "스트리밍 중",
      tabletDisplayClamshell: "유일 화면 모드",
      tabletDisplayStarted: "태블릿 화면 세션이 시작되었습니다. 덮개를 닫아도 유지됩니다.",
      tabletDisplayStopped: "세션을 정리했습니다.",
      tabletDisplayNoEngine: "BetterDisplay가 필요합니다. 설치 후 CLI 접근을 허용하세요.",
      tabletDisplayBattery: "배터리 사용 중 — 덮개 닫힘 유지가 불안정할 수 있습니다.",
      tabletDisplayClamshellHint: "덮개를 닫으면 태블릿이 유일한 화면이 됩니다. 외장 키보드·마우스는 그대로 동작합니다.",
```

en 블록(`virtualDisplayToggleOff: "Off",` 뒤)에 대응 영문:

```typescript
      tabletDisplayTitle: "Tablet Display",
      tabletDisplayStart: "Start Display Extension",
      tabletDisplayStop: "Stop",
      tabletDisplayIdle: "Idle",
      tabletDisplayStreaming: "Streaming",
      tabletDisplayClamshell: "Sole Display Mode",
      tabletDisplayStarted: "Tablet display session started. It survives closing the lid.",
      tabletDisplayStopped: "Session cleaned up.",
      tabletDisplayNoEngine: "BetterDisplay is required. Install it and allow CLI access.",
      tabletDisplayBattery: "On battery — keeping the display alive may be unreliable.",
      tabletDisplayClamshellHint: "Close the lid and the tablet becomes the sole display. External keyboards and mice keep working.",
```

기존 `virtualDisplayExperiment` 값을 ko `"태블릿 화면 확장 (실험)"`, en `"Tablet Display (Experiment)"`로 변경해 카드 이름을 사용자 승인 이름으로 맞춘다.

**Step 3: 테스트** — Run: `bun x vitest run packages/ui-tokens/src/i18n.test.ts` → Expected: PASS (ko/en 키 대칭 포함)

**Step 4: 커밋**

```bash
git add packages/ui-tokens/src/i18n.ts
git commit -m "feat(ui): 태블릿 화면 확장 i18n 키 — 시작/중지/유일 화면 모드 문구"
```

---

### Task 9: UI 카드 확장 (App.tsx)

**Files:**
- Modify: `apps/host-desktop/src/App.tsx:608-705` (VirtualDisplayCard → 태블릿 화면 확장 워크플로)

**Step 1: 카드 확장** — `VirtualDisplayCard`에 세션 상태와 시작/중지를 추가한다. 기존 수동 생성/제거 버튼은 디버깅용으로 유지한다:

- `useState` 추가: `sessionState: "Idle" | "Streaming" | ...`, `clamshell: boolean | null`
- 시작 버튼: `invoke("tablet_display_start", { providerKind: "betterdisplay", name, width: 1920, height: 1200 })` → 성공 시 `tabletDisplayStarted` 표시, `sessionState`를 `"Streaming"`으로
- 중지 버튼: `invoke("tablet_display_stop")` → `tabletDisplayStopped` 표시, 상태 `"Idle"`
- 상태 pill: `statusPillVariants` 사용 — `t.host.tabletDisplayStreaming` / `t.host.tabletDisplayClamshell` 표시
- 오류 매핑: `provider::ProviderError::message()` 문자열이 그대로 내려오므로 그대로 표시 (3분류 안내 포함)
- 스타일은 DESIGN.md 준수: 기존 토큰(`var(--text-secondary)` 등), `tabular-nums`, 44px 터치 타깃

**Step 2: 타입 체크와 유닛 테스트**

Run: `bun run typecheck && bun run test`
Expected: PASS (기존 회귀 없음)

**Step 3: React 품질 게이트 (저장소 규칙)**

Run: `npx -y react-doctor@latest . --verbose`
Expected: **100 / 100** — 미달이면 구현을 수정해 해결한다 (체커 튜닝·억제 금지)

**Step 4: 커밋**

```bash
git add apps/host-desktop/src/App.tsx
git commit -m "feat(ui): 태블릿 화면 확장 카드 — 시작/중지 오케스트레이션과 상태 표시"
```

---

### Task 10: 실기 검증 절차 문서 + 전체 게이트

**Files:**
- Create: `docs/tablet-display-physical-validation.md`
- Modify: `docs/plans/2026-09-03-tablet-display-extension-design.md` (검증 절차 링크 추가)

**Step 1: 절차 문서 작성** — 설계의 실기 검증 5항목(확장 모니터 / 유일 화면 모드 / 태블릿 터치 / 절전 방어 10분 / EVIDENCE 기록)을 `docs/usb-physical-validation.md` 형식(절차·합격 기준·진단 가이드·E등급 결과 표)으로 작성. 각 항목의 합격 기준:

1. 확장 모니터: VD 생성 + 태블릿 렌더 + 조작 가능
2. 유일 화면 모드: 덮개 닫힘 후 VD 생존, 스트리밍 지속, USB 키보드 입력이 태블릿 화면에 반영
3. 태블릿 터치: `inputEnabled` 옵트인 후 LCI1 주입으로 VD 위 커서 이동·클릭
4. 절전: 덮개 닫힘 10분 후에도 세션 생존, `pmset -g assertions`에 caffeinate 표시
5. 모든 결과 E등급으로 `docs/EVIDENCE.md` 기록

실행은 사람이 직접 (자동화 셸은 GUI 세션 밖 — `docs/dev-environment.md` 제약 문서화).

**Step 2: 전체 게이트 실행**

```bash
cargo test -p leftcar-host-desktop --lib
bun run typecheck && bun run test
npx -y react-doctor@latest . --verbose
```

Expected: 전부 PASS, react-doctor 100/100

**Step 3: 커밋**

```bash
git add docs/tablet-display-physical-validation.md docs/plans/2026-09-03-tablet-display-extension-design.md
git commit -m "docs(validation): 태블릿 화면 확장 실기 검증 절차 — 5항목 합격 기준 고정"
```

---

## 의도적 제외 (YAGNI)

- 자동 재시도, 다중 태블릿, Windows 지원
- CGVD 기본 프로바이더 승격 — 실기 evidence 후 별도 ADR (R-015 논골)
- shim `remove` 서브커맨드 구현 — 스파크 이후 확정 (현재 `FAILED not implemented` 계약)
