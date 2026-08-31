# 4K60 Encoder Experiments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep a true 3840x2160 stream while fixing VideoToolbox drop/recovery semantics and exposing reproducible encoder experiment profiles from Android Viewer through macOS Host diagnostics.

**Architecture:** `EncoderExperiment` is a backward-compatible start-time control-contract value advertised by Host capabilities. The macOS shim applies one RTVC input/rate-control policy for the full session, separates encoder and packetization work, and reports valid output, frame drops, submit latency, callback latency, and Base QP end to end. Viewer chooses the experiment before connection; Desktop remains a read-only inspector of the requested and applied strategy.

**Tech Stack:** Swift 6 and VideoToolbox/CoreVideo, Rust and serde/schemars/Tauri, React Native 0.86 with Expo 57 and Uniwind, React 19/Tailwind Desktop, Vitest, Cargo tests, direct `swiftc` policy tests.

**Spec:** `docs/superpowers/specs/2026-08-28-4k60-encoder-experiments-design.md`

## Global Constraints

- Preserve requested 3840x2160 resolution and target 60fps; do not silently downgrade resolution or FPS.
- Use hardware VideoToolbox encoders only; do not add a software fallback.
- `auto` is backward compatible and initially applies `rateControl`.
- Explicit experiments fail closed when the Host or shim cannot apply the requested path.
- Base QP is supplied to every frame in an `adaptiveQp` session, including forced keyframes.
- Ordinary VideoToolbox `frameDropped` callbacks never request recovery IDR or reset CSD.
- Keep encoder submission capacity independent from packetization and network work.
- Phase A does not change the UDP/FEC/AOAP media wire or Android decoder.
- `splitHorizontal` and `splitVertical` remain outside Phase A until a real-capture Host probe passes the spec gate.
- Do not stage or commit changes during this execution, per user direction.
- After React/React Native/TSX/style changes, run root React Doctor and require `100 / 100`, then rerun typecheck and relevant tests.
- Preserve unrelated dirty work and edit only the paths named in each task.

## File Responsibility Map

| File | Responsibility |
| --- | --- |
| `crates/control-contract/src/host.rs` | Canonical experiment enum, advertised capability, start request, stats/session schema |
| `native/macos-capture-shim/Sources/CaptureShim.swift` | Experiment policy, v6 FFI, callback classification, QP controller, timings, runtime stats |
| `native/macos-capture-shim/Tests/EncodePolicyTests.swift` | Deterministic experiment, QP, callback, and slot-release policy assertions |
| `apps/host-desktop/src-tauri/src/backend.rs` | Backend start interface and complete fake stats fixture |
| `apps/host-desktop/src-tauri/src/ffi.rs` | v6-first shim dispatch, fail-closed compatibility, stats JSON parsing |
| `apps/host-desktop/src-tauri/src/control.rs` | Capability advertisement, request validation, experiment forwarding, SessionView mapping |
| `apps/host-desktop/src-tauri/tests/control_e2e.rs` | Control protocol experiment round-trip and rejection tests |
| `apps/viewer-expo/src/control.ts` | Catalog experiment capability TypeScript shape |
| `apps/viewer-expo/src/stream-profile.ts` | Viewer experiment IDs and default selection |
| `apps/viewer-expo/src/launch-stream.ts` | Start request experiment field |
| `apps/viewer-expo/app/catalog.tsx` | 4K-only advanced experiment selector and reconnect preservation |
| `apps/host-desktop/src/sessionTypes.ts` | Desktop metrics boundary |
| `apps/host-desktop/src/encoderDiagnostics.ts` | Pure read-only experiment diagnostics presentation |
| `apps/host-desktop/src/SessionInspector.tsx` | Applied experiment/drop/timing/QP UI |
| `docs/11-low-latency-investigation.md` | Corrected synthetic evidence and physical validation receipt |

---

### Task 1: Define the canonical experiment and diagnostics contract

**Files:**

- Modify: `crates/control-contract/src/host.rs`
- Test: `crates/control-contract/src/host.rs`
- Test: `crates/control-contract/tests/contract.rs`

**Interfaces:**

- Produces `EncoderExperiment::{Auto, RateControl, AdaptiveQp, EncoderPool, SplitHorizontal, SplitVertical}` with camelCase JSON values; only the first four are advertised in Phase A.
- Produces `EncoderExperimentInfo { id, label, hint, requires_reconnect }` in `CatalogView.encoder_experiments`.
- Adds `StartStreamInput.encoder_experiment: EncoderExperiment` defaulting to `Auto`.
- Adds the 13 experiment/timing fields from the spec to both `StatsInfo` and `SessionView`.

- [ ] **Step 1: Add failing serde/default and capability assertions**

Add tests that parse an old request and an explicit QP request:

```rust
#[test]
fn start_stream_defaults_to_auto_encoder_experiment() {
    let input: StartStreamInput = serde_json::from_str(
        r#"{"sourceIndex":0,"viewerPort":5001,"width":3840,"height":2160,"fps":60}"#,
    )
    .unwrap();
    assert_eq!(input.encoder_experiment, EncoderExperiment::Auto);
}

#[test]
fn start_stream_roundtrips_adaptive_qp() {
    let input: StartStreamInput = serde_json::from_str(
        r#"{"sourceIndex":0,"viewerPort":5001,"width":3840,"height":2160,"fps":60,"encoderExperiment":"adaptiveQp"}"#,
    )
    .unwrap();
    assert_eq!(input.encoder_experiment, EncoderExperiment::AdaptiveQp);
    assert!(serde_json::to_string(&input)
        .unwrap()
        .contains("\"encoderExperiment\":\"adaptiveQp\""));
}
```

Extend the catalog contract test to assert IDs `auto`, `rateControl`,
`adaptiveQp`, and `encoderPool`, all with `requiresReconnect=true`.

- [ ] **Step 2: Run the focused contract tests RED**

Run:

```bash
cargo test -p control-contract start_stream -- --nocapture
cargo test -p control-contract catalog_advertises -- --nocapture
```

Expected: compilation fails because `EncoderExperiment`, the new input field, and catalog capability do not exist.

- [ ] **Step 3: Implement the enum and capability schema**

Add these canonical types near `StartStreamInput`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EncoderExperiment {
    Auto,
    RateControl,
    AdaptiveQp,
    EncoderPool,
    SplitHorizontal,
    SplitVertical,
}

impl Default for EncoderExperiment {
    fn default() -> Self {
        Self::Auto
    }
}

impl EncoderExperiment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::RateControl => "rateControl",
            Self::AdaptiveQp => "adaptiveQp",
            Self::EncoderPool => "encoderPool",
            Self::SplitHorizontal => "splitHorizontal",
            Self::SplitVertical => "splitVertical",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncoderExperimentInfo {
    pub id: EncoderExperiment,
    pub label: String,
    pub hint: String,
    pub requires_reconnect: bool,
}
```

Add `#[serde(default)] pub encoder_experiment: EncoderExperiment` to
`StartStreamInput`, `#[serde(default)] pub encoder_experiments:
Vec<EncoderExperimentInfo>` to `CatalogView`, and the exact stats fields:

```rust
pub encoder_experiment_requested: String,
pub encoder_experiment_applied: String,
pub encoder_experiment_fallback_reason: Option<String>,
pub encoder_frame_drops: i64,
pub encoder_frame_drop_fps: u32,
pub valid_encode_output_fps: u32,
pub encode_submit_call_p50_us: u64,
pub encode_submit_call_p95_us: u64,
pub encoder_callback_p50_us: u64,
pub encoder_callback_p95_us: u64,
pub packetization_in_flight: u32,
pub base_frame_qp: Option<i32>,
pub base_frame_qp_changes: i64,
```

Mark new diagnostics fields with `#[serde(default)]`; keep catalog generation and all fixtures compile-complete.
Add a contract assertion that `splitHorizontal` deserializes successfully but
is absent from Phase A catalog capabilities, so control validation can return a
purpose-specific unsupported-experiment error instead of a generic JSON error.

- [ ] **Step 4: Run contract tests GREEN**

Run:

```bash
cargo test -p control-contract -- --nocapture
```

Expected: all control-contract unit and integration tests pass.

- [ ] **Step 5: Record the unstaged checkpoint**

Run:

```bash
git diff --check -- crates/control-contract/src/host.rs crates/control-contract/tests/contract.rs
git status --short
```

Expected: no whitespace errors; changes remain unstaged.

---

### Task 2: Add deterministic Swift experiment, QP, and callback policies

**Files:**

- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

- Consumes the wire IDs from Task 1 as matching Swift raw values.
- Produces `EncoderExperiment.parse(_:)`, `appliedEncoderExperiment(_:)`, `baseFrameQp(forSliderPercent:)`, `nextAdaptiveBaseFrameQp(...)`, and `encoderCallbackDisposition(...)`.
- Produces an input policy where `encoderPool` always selects pixel-transfer staging and the other Phase A profiles retain direct 4K NV12 input.

- [ ] **Step 1: Add failing policy assertions**

Add deterministic assertions to `EncodePolicyTests.main()`:

```swift
precondition(EncoderExperiment.parse(nil) == .auto)
precondition(EncoderExperiment.parse("rateControl") == .rateControl)
precondition(EncoderExperiment.parse("adaptiveQp") == .adaptiveQp)
precondition(EncoderExperiment.parse("encoderPool") == .encoderPool)
precondition(EncoderExperiment.parse("splitHorizontal") == nil)
precondition(appliedEncoderExperiment(.auto) == .rateControl)
precondition(baseFrameQp(forSliderPercent: 25) == 42)
precondition(baseFrameQp(forSliderPercent: 50) == 26)
precondition(baseFrameQp(forSliderPercent: 0) == nil)
precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: true, stableWindows: 0) == 34)
precondition(nextAdaptiveBaseFrameQp(current: 41, pressured: true, stableWindows: 0) == 42)
precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: false, stableWindows: 2) == 32)
precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: false, stableWindows: 3) == 31)
precondition(nextAdaptiveBaseFrameQp(current: 26, pressured: false, stableWindows: 3) == 26)
precondition(encoderCallbackDisposition(status: noErr, flags: [.frameDropped], hasSample: false) == .dropped)
precondition(encoderCallbackDisposition(status: noErr, flags: [], hasSample: true) == .valid)
precondition(encoderCallbackDisposition(status: -1, flags: [], hasSample: false) == .failed)
```

Add input-policy assertions proving `.encoderPool` selects `.pixelTransfer` at 4K and `.rateControl`/`.adaptiveQp` select `.direct`.

- [ ] **Step 2: Compile the policy test RED**

Run:

```bash
xcrun swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
```

Expected: compile errors identify the missing enum and pure policy functions.

- [ ] **Step 3: Implement the pure policies**

Add the following shapes near the existing encoder policy helpers:

```swift
enum EncoderExperiment: String, Equatable {
    case auto
    case rateControl
    case adaptiveQp
    case encoderPool

    static func parse(_ raw: String?) -> EncoderExperiment? {
        guard let raw else { return .auto }
        return EncoderExperiment(rawValue: raw)
    }
}

enum EncoderCallbackDisposition: Equatable {
    case valid
    case dropped
    case failed
}

func appliedEncoderExperiment(_ requested: EncoderExperiment) -> EncoderExperiment {
    requested == .auto ? .rateControl : requested
}

func baseFrameQp(forSliderPercent percent: Int32) -> Int32? {
    guard (25...50).contains(percent) else { return nil }
    let ratio = Double(percent - 25) / 25.0
    return Int32((42.0 - ratio * 16.0).rounded())
}

func nextAdaptiveBaseFrameQp(
    current: Int32,
    pressured: Bool,
    stableWindows: Int
) -> Int32 {
    if pressured { return min(42, current + 2) }
    if stableWindows >= 3 { return max(26, current - 1) }
    return current
}

func encoderCallbackDisposition(
    status: OSStatus,
    flags: VTEncodeInfoFlags,
    hasSample: Bool
) -> EncoderCallbackDisposition {
    if status == noErr, flags.contains(.frameDropped) { return .dropped }
    if status == noErr, hasSample { return .valid }
    return .failed
}
```

Extend `encoderInputSurfacePolicy` with the experiment argument and return
`.pixelTransfer` for `.encoderPool` before the existing mode/resolution rules.

- [ ] **Step 4: Run the Swift policy test GREEN**

Run:

```bash
xcrun swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
/tmp/leftcar-encode-policy-tests
```

Expected: compile and execution both exit 0.

- [ ] **Step 5: Record the unstaged checkpoint**

Run:

```bash
git diff --check -- native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift
git status --short
```

Expected: no whitespace errors; existing and new Swift changes remain unstaged.

---

### Task 3: Fix runtime callback semantics and split encoder/packetization capacity

**Files:**

- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

- Consumes `encoderCallbackDisposition` from Task 2.
- Produces valid-output-only FPS, encoder drop metrics, synchronous submit timings, callback timings, and independent packetization in-flight state.
- Preserves recovery retry only when a requested recovery keyframe is dropped.

- [ ] **Step 1: Add failing recovery and slot-release model assertions**

Extract pure decisions and assert them:

```swift
precondition(recoveryActionForDroppedFrame(requestedRecoveryKeyframe: false) == .none)
precondition(recoveryActionForDroppedFrame(requestedRecoveryKeyframe: true) == .retryAfterCooldown)
precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .valid))
precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .dropped))
precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .failed))
```

- [ ] **Step 2: Run the Swift policy test RED**

Run the Task 2 `swiftc` command.

Expected: compile errors for the missing recovery and slot-release helpers.

- [ ] **Step 3: Implement callback classification and release ordering**

Change the VideoToolbox output closure to name `infoFlags`, classify before
unwrapping the sample, and handle each branch explicitly:

```swift
) { [weak self] status, infoFlags, encodedSample in
    guard let self else { return }
    let callbackNs = DispatchTime.now().uptimeNanoseconds
    let disposition = encoderCallbackDisposition(
        status: status,
        flags: infoFlags,
        hasSample: encodedSample != nil
    )
    self.recordEncoderCallbackTiming(pts: trackedPts, callbackNs: callbackNs)
    self.completeEncodeSlot()

    switch disposition {
    case .dropped:
        self.recordEncoderFrameDrop(pts: trackedPts)
        if requestKeyframe {
            self.clearRecoveryEncodeGate()
            self.scheduleRecoveryKeyframeRetry(afterNanoseconds: 750_000_000)
        }
        return
    case .failed:
        self.handleEncoderOutputFailure(
            status: status,
            requestedKeyframe: requestKeyframe,
            generation: submittedEncoderGeneration,
            pts: trackedPts
        )
        return
    case .valid:
        break
    }
    guard let encodedSample else { return }
    self.enqueueEncodedSampleForPacketization(
        encodedSample,
        requestedKeyframe: requestKeyframe,
        callbackNs: callbackNs,
        generation: submittedEncoderGeneration
    )
}
```

`scheduleRecoveryKeyframeRetry` dispatches once on `encodeQueue` after the
cooldown, rechecks that the session generation and `networkAwaitingKeyframe`
still require recovery, and only then calls `requestRecoveryKeyframe()`.
The helper implementation must avoid a second `completeEncodeSlot()` in
packetization or error branches. Increment/decrement `packetizationInFlight`
around the packetization queue work under `stateLock` and clamp decrements at
zero.

- [ ] **Step 4: Measure the synchronous EncodeFrame call**

Record `DispatchTime.now().uptimeNanoseconds` immediately before and after
`VTCompressionSessionEncodeFrame`, append microseconds to a bounded rolling
sample, and calculate p50/p95 in the existing stats snapshot pattern. Keep
submit-call time separate from `inputPreparationUs` and callback latency.

- [ ] **Step 5: Make output counters sample-valid**

Increment `encodeOutputCallbacks`, output-rate windows, and startup-confirmed
output only in the `.valid` branch. Increment `encoderFrameDrops` and its
one-second rate only in `.dropped`. Do not increment `encodeSubmitFailures` for
callback-level frame drops.

- [ ] **Step 6: Run Swift tests and compile the dylib**

Run:

```bash
xcrun swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
/tmp/leftcar-encode-policy-tests
xcrun swiftc -O -emit-library native/macos-capture-shim/Sources/CaptureShim.swift -o /tmp/libleftcar_capture.dylib -framework AppKit -framework CoreGraphics -framework CoreMedia -framework CoreVideo -framework Foundation -framework IOSurface -framework Metal -framework VideoToolbox
```

Expected: policy tests exit 0 and `/tmp/libleftcar_capture.dylib` is produced.

- [ ] **Step 7: Record the unstaged checkpoint**

Run `git diff --check` for the two Swift paths and confirm no staging with
`git status --short`.

---

### Task 4: Implement v6 session selection and adaptive Base QP

**Files:**

- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

- Consumes Swift `EncoderExperiment` from Task 2.
- Produces `leftcar_capture_start_v6(..., encoderExperimentName)` and stores requested/applied experiment for the full `CaptureSession` lifetime.
- Produces `adaptiveQp` frame properties on every frame and `encoderPool` input policy.
- Produces `leftcar_capture_encoder_experiments_v1()` as cached JSON capability discovery for Rust Host catalog advertisement.

- [ ] **Step 1: Add failing Base QP pressure/stability assertions**

Add tests for the exact window classifier:

```swift
precondition(adaptiveQpWindowIsPressured(
    encoderDrops: 1,
    validOutputFps: 60,
    submitP95Us: 10_000
))
precondition(adaptiveQpWindowIsPressured(
    encoderDrops: 0,
    validOutputFps: 54,
    submitP95Us: 10_000
))
precondition(adaptiveQpWindowIsPressured(
    encoderDrops: 0,
    validOutputFps: 60,
    submitP95Us: 16_668
))
precondition(!adaptiveQpWindowIsPressured(
    encoderDrops: 0,
    validOutputFps: 60,
    submitP95Us: 16_000
))
```

- [ ] **Step 2: Run the policy test RED**

Run the Task 3 policy compile command.

Expected: the missing window classifier fails compilation.

- [ ] **Step 3: Add and parse `leftcar_capture_start_v6`**

Match v5 arguments and append `encoderExperimentName`. Reject unknown values
with `setLastError("unknown encoder experiment: ...")`. Pass the parsed value
through `startCaptureSession` to the `CaptureSession` initializer. Preserve v2
through v5 behavior by passing `.auto`.

- [ ] **Step 4: Verify explicit profile capability at session setup**

Resolve `auto` to `rateControl`. For `adaptiveQp`, query
`kVTCompressionPropertyKey_SupportsBaseFrameQP` after session creation and fail
startup if it is not true. For `encoderPool`, require successful access to
`VTCompressionSessionGetPixelBufferPool` or fail startup. Do not fall back to
`rateControl` for explicit requests.

Add `leftcar_capture_encoder_experiments_v1() -> UnsafeMutablePointer<CChar>`.
It returns `auto` and `rateControl` whenever RTVC hardware creation succeeds,
adds `adaptiveQp` only when `SupportsBaseFrameQP` is true, and adds
`encoderPool` only when a prepared RTVC session returns a non-null
`VTCompressionSessionGetPixelBufferPool`. Cache this hardware probe once per
process and return JSON through the existing capture-string allocation/free
contract. Do not include either reserved split ID.

- [ ] **Step 5: Supply Base QP to every frame**

Build one mutable frame-properties dictionary for every submission:

```swift
var frameProperties = [String: Any]()
if requestKeyframe {
    frameProperties[kVTEncodeFrameOptionKey_ForceKeyFrame as String] = true
}
if appliedExperiment == .adaptiveQp {
    frameProperties[kVTEncodeFrameOptionKey_BaseFrameQP as String] = currentBaseFrameQp
}
let properties: CFDictionary? = frameProperties.isEmpty
    ? nil
    : frameProperties as CFDictionary
```

Use initial QP 32. Once per one-second submitted-frame window, apply the spec's
pressure predicate and stable-window rule. Manual Host quality selection maps
through `baseFrameQp(forSliderPercent:)`; auto reset returns control to the QP
window controller.

- [ ] **Step 6: Export experiment, QP, drop, and timing stats**

Add every field from Task 1 to the JSON dictionary in `statsJSON()`. Use null
for `baseFrameQp` outside `adaptiveQp`; use `encoderExperimentFallbackReason`
only for `auto` selection explanation, never to hide an explicit-profile
failure.

- [ ] **Step 7: Run Swift policy and dylib compilation GREEN**

Run the three Task 3 commands.

Expected: tests and both compilation operations exit 0.

- [ ] **Step 8: Record the unstaged checkpoint**

Run `git diff --check` for the Swift files and `git status --short`.

---

### Task 5: Carry experiments through Rust Host and v6 FFI

**Files:**

- Modify: `apps/host-desktop/src-tauri/src/backend.rs`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src-tauri/tests/control_e2e.rs`

**Interfaces:**

- Consumes `EncoderExperiment` and stats types from Task 1.
- Extends `CaptureBackend::start(..., encoder_experiment: EncoderExperiment)`.
- FFI calls start v6 first; only `Auto` may fall back to v5.
- Host reads the shim's cached experiment capability JSON and validates requests before capture.

- [ ] **Step 1: Add failing control e2e tests**

Add one start request with `"encoderExperiment":"adaptiveQp"` and assert the
fake backend session status reports requested/applied `adaptiveQp`. Add one
request with `"encoderExperiment":"splitHorizontal"` and assert the response
contains `unsupported encoder experiment`.

Extend the catalog assertion to parse and verify these advertised IDs:

```rust
assert!(line.contains("\"id\":\"auto\""), "{line}");
assert!(line.contains("\"id\":\"rateControl\""), "{line}");
assert!(line.contains("\"id\":\"adaptiveQp\""), "{line}");
assert!(line.contains("\"id\":\"encoderPool\""), "{line}");
assert!(!line.contains("splitHorizontal"), "{line}");
```

- [ ] **Step 2: Run Host Rust tests RED**

Run:

```bash
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib --tests -- --nocapture
```

Expected: backend signatures, fixtures, and experiment fields fail compilation.

- [ ] **Step 3: Extend backend capability and fake fixtures**

Add `encoder_experiment: EncoderExperiment` to `CaptureBackend::start`. The
fake backend records or returns the explicit experiment in its complete
`StatsInfo` fixture. Fill all new numeric fields with zero and profile strings
with `auto`/`rateControl` where the test does not select an explicit profile.
Add `CaptureBackend::encoder_experiments() -> Result<Vec<EncoderExperimentInfo>,
String>`; the fake returns the four Phase A records.

- [ ] **Step 4: Implement v6-first FFI dispatch**

Add a `StartV6` type with the v5 signature plus one C string. Dispatch rules:

```rust
if let Ok(start_v6) = lib.get::<StartV6>(b"leftcar_capture_start_v6") {
    start_v6(/* existing args */, c_encoder_experiment.as_ptr())
} else if encoder_experiment == EncoderExperiment::Auto {
    // existing v5/v4/v3/v2 compatibility chain
} else {
    return Err(format!(
        "capture shim does not support encoder experiment {}",
        encoder_experiment.as_str()
    ));
}
```

Load `leftcar_capture_encoder_experiments_v1` and parse its JSON into
`Vec<EncoderExperimentInfo>`. When the symbol is missing, advertise only
`auto`; this is the only profile compatible with the v5 fallback chain.
Parse all new JSON fields in `parse_stats_json`, preserving absent-field
defaults for old shims.

- [ ] **Step 5: Advertise and validate Host experiments**

Read `backend.encoder_experiments()` for catalog and request validation. Decorate
the returned IDs with canonical Korean label/hint text and
`requires_reconnect=true`. Reject requests not present in that exact list before
AOAP negotiation or capture start. Pass the validated enum to `backend.start`.

- [ ] **Step 6: Map all metrics into SessionView**

Update running, terminal, fake, and test fixtures. Map names one-to-one from
`StatsInfo`; do not derive encoder drops from generic `dropped` or network loss.

- [ ] **Step 7: Run Rust contract and Host tests GREEN**

Run:

```bash
cargo test -p control-contract -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib --tests -- --nocapture
cargo fmt --all -- --check
```

Expected: all tests pass and formatting is clean.

- [ ] **Step 8: Record the unstaged checkpoint**

Run `git diff --check` for the four Rust Host paths plus
`crates/control-contract/src/host.rs`, then `git status --short`.

---

### Task 6: Add the 4K Viewer experiment selector and preserve restarts

**Files:**

- Modify: `apps/viewer-expo/src/control.ts`
- Modify: `apps/viewer-expo/src/stream-profile.ts`
- Modify: `apps/viewer-expo/src/stream-profile.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`
- Create: `apps/viewer-expo/src/encoder-experiment.ts`
- Create: `apps/viewer-expo/src/encoder-experiment.test.ts`

**Interfaces:**

- Consumes `CatalogView.encoderExperiments` from Task 1.
- Produces `EncoderExperimentId = "auto" | "rateControl" | "adaptiveQp" | "encoderPool"`.
- Adds `encoderExperiment` to `StartStreamArgs`, `ActiveStream`, restoration, USB/Wi-Fi switch, and automatic reconnect.

- [ ] **Step 1: Add failing pure selector tests**

Create tests for this pure API:

```ts
export type EncoderExperimentId =
  | "auto"
  | "rateControl"
  | "adaptiveQp"
  | "encoderPool";

export function availableEncoderExperiments(
  advertised: EncoderExperimentInfo[] | undefined,
  width: number,
  height: number,
): EncoderExperimentInfo[];

export function resolveEncoderExperiment(
  selected: EncoderExperimentId,
  advertised: EncoderExperimentInfo[] | undefined,
): EncoderExperimentId;
```

Assertions: sub-4K returns only `auto`; 4K returns advertised profiles; a stale
selection not advertised resolves to `auto`; undefined old-host capability
resolves to `auto`.

- [ ] **Step 2: Add failing launch preservation tests**

Set `encoderExperiment: "adaptiveQp"` in the shared `args` fixture and assert
every `startStream` call contains it for UDP, USB, and address-discovery paths.
Add a catalog restart assertion that the stored active stream passes the same
value after transport auto-switch.

- [ ] **Step 3: Run focused Viewer tests RED**

Run:

```bash
bunx vitest run apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts apps/viewer-expo/src/stream-profile.test.ts
```

Expected: missing module/types and omitted request field fail.

- [ ] **Step 4: Implement pure experiment resolution**

Implement the exact API from Step 1. Treat 3840x2160 or greater as 4K for
selector visibility. Never synthesize experiments the Host did not advertise.

- [ ] **Step 5: Carry selection through every stream lifecycle path**

Add the field to `StartStreamArgs`, `ActiveStream`, and all start/restore objects
in `catalog.tsx`. Initialize selection to `auto`. When catalog capabilities
change, resolve a stale choice to `auto`. Keep it unchanged during automatic
session restore and USB/Wi-Fi transport replacement.

- [ ] **Step 6: Render the advanced 4K selector**

Below the existing quality profiles, render `인코더 실험` only when the selected
resolution is 4K and the Host advertises more than one experiment. Use Uniwind
`className` utilities for the new wrapper and buttons; combine selected/disabled
classes through the existing class helper or `clsx` plus `tailwind-merge`.
Display each Host-provided label/hint and the text `변경 사항은 다음 연결부터
적용됩니다.`. Keep touch targets at least 44 points high and set accessibility
role/selected state.

- [ ] **Step 7: Run focused Viewer tests and typecheck GREEN**

Run:

```bash
bunx vitest run apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts apps/viewer-expo/src/stream-profile.test.ts apps/viewer-expo/src/transport-switch.test.ts
bun run --cwd apps/viewer-expo typecheck
```

Expected: all tests pass and TypeScript exits 0.

- [ ] **Step 8: Record the unstaged checkpoint**

Run `git diff --check` for the Viewer paths and `git status --short`.

---

### Task 7: Show applied experiment and true encoder pressure in Desktop

**Files:**

- Modify: `apps/host-desktop/src/sessionTypes.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.test.ts`
- Modify: `apps/host-desktop/src/SessionInspector.tsx`

**Interfaces:**

- Consumes the new `SessionView` camelCase diagnostics from Task 5.
- Produces a read-only diagnostics model and UI; Desktop does not mutate the session experiment.

- [ ] **Step 1: Add failing diagnostics-view test**

Extend the fixture with requested/applied `adaptiveQp`, QP 34, two QP changes,
three encoder drops, submit p95 17,200us, callback p95 18,100us, and
packetization in-flight 1. Assert the pure view returns:

```ts
expect(encoderDiagnosticsView(session)).toMatchObject({
  experiment: "적응형 QP",
  experimentDetail: "요청 adaptiveQp · 적용 adaptiveQp · QP 34 (2회 조정)",
  pressure: "드롭 3 · 제출 p95 17.2ms · callback p95 18.1ms",
  inFlight: "인코더 0 · 패킷화 1",
});
```

- [ ] **Step 2: Run the focused Desktop test RED**

Run:

```bash
bunx vitest run apps/host-desktop/src/encoderDiagnostics.test.ts
```

Expected: missing SessionRow fields and view properties fail.

- [ ] **Step 3: Extend SessionRow and pure diagnostics mapping**

Add optional camelCase fields matching Task 1. Map known profile IDs to Korean
labels and preserve unknown IDs verbatim for forward compatibility. Show `없음`
for no fallback, `측정 중` for absent timing, and never fold network loss into
encoder drops.

- [ ] **Step 4: Render the new inspector rows**

Add rows adjacent to existing encoder identity/settings:

- 실험 프로필
- 인코더 압력
- 인코더/패킷화 in-flight
- 유효 출력 FPS

Keep the existing quality slider. In `adaptiveQp`, label the slider result as
`Base QP 기반`; in the other profiles keep the current bitrate/quality wording.
Use `cn`, `cva`, and existing Tailwind classes for conditional Desktop styling.

- [ ] **Step 5: Run Desktop test and typecheck GREEN**

Run:

```bash
bunx vitest run apps/host-desktop/src/encoderDiagnostics.test.ts
bun run --cwd apps/host-desktop typecheck
```

Expected: test and typecheck pass.

- [ ] **Step 6: Record the unstaged checkpoint**

Run `git diff --check` for the four Desktop files and `git status --short`.

---

### Task 8: Run repository quality gates and repair regressions

**Files:**

- Modify only source/test files already listed when a gate exposes a defect.

**Interfaces:**

- Consumes all Phase A implementation tasks.
- Produces one source-level verification receipt with no ignored React Doctor findings.

- [ ] **Step 1: Run the complete JavaScript/TypeScript test set**

Run:

```bash
bun run typecheck
bun run test
bun run test:contract
bun run test:architecture
```

Expected: every command exits 0.

- [ ] **Step 2: Run React Doctor from repository root**

Run:

```bash
npx -y react-doctor@latest . --verbose
```

Expected: `100 / 100`. Fix every source finding without ignore rules or score overrides.

- [ ] **Step 3: Rerun typecheck and relevant tests after React Doctor fixes**

Run:

```bash
bun run typecheck
bunx vitest run apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts apps/host-desktop/src/encoderDiagnostics.test.ts
```

Expected: all commands exit 0 after any React Doctor repair.

- [ ] **Step 4: Run Rust and Swift gates**

Run:

```bash
cargo test -p control-contract -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib --tests -- --nocapture
cargo fmt --all -- --check
xcrun swiftc -O -o /tmp/leftcar-encode-policy-tests native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreGraphics -framework IOSurface -framework Security -framework Foundation
/tmp/leftcar-encode-policy-tests
xcrun swiftc -O -emit-library native/macos-capture-shim/Sources/CaptureShim.swift -o /tmp/libleftcar_capture.dylib -framework AppKit -framework CoreGraphics -framework CoreMedia -framework CoreVideo -framework Foundation -framework IOSurface -framework Metal -framework VideoToolbox
```

Expected: all tests and builds exit 0.

- [ ] **Step 5: Run final diff hygiene without staging**

Run:

```bash
git diff --check
git status --short
```

Expected: no diff errors; all implementation changes remain unstaged.

---

### Task 9: Install the Host and execute the real-device 4K matrix

**Files:**

- Modify: `docs/11-low-latency-investigation.md`
- Generated runtime artifacts: `/tmp/leftcar-4k60-*`

**Interfaces:**

- Consumes the source-verified Phase A build.
- Produces separate physical receipts for `rateControl`, `adaptiveQp`, and `encoderPool` plus one 600-second soak of the best candidate.

- [ ] **Step 1: Build and install the exact Host/shim pair**

Run:

```bash
/bin/zsh tools/dev-host-macos.zsh
```

Expected: build succeeds; the tool verifies freshly built, bundled, and installed shim bytes and preserves the existing signing requirement.

- [ ] **Step 2: Verify the real device and application before claiming runtime proof**

Run:

```bash
/Users/loopy/Library/Android/sdk/platform-tools/adb devices -l
/Users/loopy/Library/Android/sdk/platform-tools/adb shell pm path leftcar.ll3.kr
shasum -a 256 "/Applications/Leftcar Host.app/Contents/MacOS/leftcar-host-desktop" "/Applications/Leftcar Host.app/Contents/Resources/libleftcar_capture.dylib"
```

Verify one intended device is `device` and the package path exists. Do not treat
build success as a stream test.

- [ ] **Step 3: Run the 30-second smoke matrix**

For each explicit profile `rateControl`, `adaptiveQp`, and `encoderPool`:

1. Select the 4K60 Viewer profile and the experiment.
2. Confirm Host reports requested/applied profile exactly and encoder dimensions 3840x2160.
3. Keep a static terminal for 30 seconds.
4. Use `Option+0` and keep the high-change desktop moving for 30 seconds.
5. Save Host stats snapshots and Android `LeftcarNative` logcat to separate `/tmp/leftcar-4k60-<profile>-smoke-*` files.

Expected: no silent fallback, no stream termination, and ordinary encoder drops do not increase recovery IDR.

- [ ] **Step 4: Run the 180-second acceptance matrix**

Repeat each profile with 30 seconds static warm-up and 180 seconds high-change
content. Calculate cumulative valid output/render averages, 1-second FPS p5,
encoder drop delta, submit/callback p95, decoder age p95, frame gaps, recovery
IDR delta, and queue oldest age. Record failures without estimating missing
values.

- [ ] **Step 5: Run the 600-second soak for the best valid candidate**

Choose the candidate that satisfies the most spec gates without silent fallback.
Run high-change content for 600 seconds and verify no 0fps stop, session exit,
monotonic age growth, unbounded packetization/network queue, or recovery storm.

- [ ] **Step 6: Update the investigation document with corrected evidence**

Add a dated section that:

- marks the old standalone 64fps result invalid because dropped callbacks were counted;
- reports all three profiles, including failures and residual drops;
- separates source/build proof from physical runtime proof;
- states whether Phase A reached every 4K60 gate;
- starts Phase B planning only when all single-session profiles fail the final gate.

- [ ] **Step 7: Run final documentation and worktree checks**

Run:

```bash
git diff --check
git status --short
```

Expected: no whitespace errors and no staged files.
