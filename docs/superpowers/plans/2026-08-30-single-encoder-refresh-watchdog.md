# Single-encoder Refresh Watchdog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make one exact-resolution VideoToolbox session the production default, recover boundedly from Host encoder, Android decoder/Surface, and control-peer stalls, reconnect a terminated Android stream once, and keep a tiny actual Surface-release FPS label visible.

**Architecture:** The product capability path never advertises the dual vertical split experiment, while an explicit developer-only environment flag retains that implementation for diagnostics. Pure Swift and Rust state machines classify progress before orchestration mutates VideoToolbox or MediaCodec. Native Android persists local termination reasons and emits one React Native event; the existing restore mutation remains the only reconnect operation. The existing 250ms HUD polling loop supplies both the temporary diagnostics and a separate persistent FPS overlay.

**Tech Stack:** Swift 6, ScreenCaptureKit, VideoToolbox, Rust 2021, Android NDK MediaCodec, Kotlin/Gradle, React Native 0.86, React 19, TypeScript, Vitest, Tauri, ADB.

**Spec:** `docs/superpowers/specs/2026-08-30-single-encoder-refresh-watchdog-design.md`

## Global Constraints

- Preserve exact selected dimensions, including 3840x2160. Never silently downscale to make an acceptance gate pass.
- Production `auto`, `rateControl`, `adaptiveQp`, and `encoderPool` each use exactly one `VTCompressionSession` on every Mac model.
- Keep `splitVertical` in wire enums and source, but never advertise or display it in normal product capability/UI paths.
- Permit an explicit split start only when `LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC=1` and every existing exact-4K60/direct-UDP/decoder constraint succeeds. Never silently substitute `auto` for a direct diagnostic protocol request at the Host boundary.
- The Host callback watchdog is evidence-gated: capture must advance, an encode slot must remain outstanding, valid output must not advance, and the oldest outstanding submission must be at least 250ms old.
- Claim each encode slot exactly once across callback, synchronous submission failure, watchdog reclaim, and late callback paths.
- Allow at most two encoder restarts in a rolling ten-second window, with a one-second cooldown. The third qualified stall terminates with `encoder_stalled`.
- Android media silence by itself is not a decoder stall. A render recovery decision is evaluated only when completed access units advance while Surface-release count does not.
- Android issues at most one IDR at 250ms, one decoder rebuild at 750ms, and one local `render_stalled` termination at three seconds for an incident. A successful Surface release resets the incident immediately.
- Host reachability is independent of media flow. Send the authenticated probe once per second and terminate locally only after three consecutively missed matching responses.
- Preserve native reason codes 1-3. Add 4 for `host_unreachable` and 5 for `render_stalled`.
- A native termination event may start one restore for the matching port. A failed native-triggered restore removes the dead stream from controller state and does not enter a retry loop.
- The persistent label means actual MediaCodec Surface-release FPS. Do not call it source FPS, display refresh rate, panel FPS, or glass-to-glass FPS.
- Preserve the existing dirty worktree. Before every commit, inspect the complete staged diff and stage only feature-related hunks. Never overwrite or discard unrelated user changes.
- Several scoped files are already modified or untracked. Use hunk staging for tracked files and inspect every entire untracked file before staging it. If unrelated content cannot be separated safely, leave that task uncommitted and report the exact overlap.
- Keep generated logs, APK pulls, screenshots, and performance reports under `/tmp`; do not add them to git.
- After any React, React Native, JSX/TSX, style, or component-behavior change, run `npx -y react-doctor@latest . --verbose` from the repository root and require exactly `100 / 100`. Then rerun typecheck and relevant tests.
- Source tests, cross-builds, APK installation, a live stream, and current-device performance are distinct evidence levels. Report each separately.
- Current physical verification covers the connected Viewer and this M1 Max. It does not prove base-M1 throughput or 4K60 unless measured output sustains 60fps under moving content.

## File Structure

| Path | Responsibility |
|---|---|
| `native/macos-capture-shim/Sources/Encoder/SingleEncoderHealth.swift` | Pure 250ms stall decision, restart budget, and outstanding submission ledger |
| `native/macos-capture-shim/Sources/Encoder/EncoderSubmissionState.swift` | Exactly-once completion token with callback, submit-failure, and watchdog ownership |
| `native/macos-capture-shim/Sources/Capture/CaptureSession.swift` | Session-owned ledger, generation, and watchdog metrics |
| `native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift` | Capture progress observation and one queued watchdog evaluation |
| `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncodeSubmission.swift` | Register and retire each VideoToolbox submission |
| `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderLifecycle.swift` | Ordered session invalidation and restart/termination orchestration |
| `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift` | Watchdog counters in stats and `LeftcarPerf` logs |
| `native/macos-capture-shim/Sources/Encoder/EncoderExperimentPolicy.swift` | Single-session product policy and diagnostic-only explicit split gate |
| `apps/host-desktop/src-tauri/src/control.rs` | Product capability filtering and hidden diagnostic request admission |
| `apps/viewer-expo/src/encoder-experiment.ts` | Defensive removal of split from product selectors and resolution |
| `native/android-viewer/src/renderer/single_session/health.rs` | Pure render-progress and control-probe watchdogs |
| `native/android-viewer/src/renderer/single_session/runtime/worker.rs` | Apply health actions to the single-session receive loop |
| `native/android-viewer/src/renderer/single_session/network.rs` | Feed authenticated matching probe responses to health state |
| `native/android-viewer/src/jni.rs` | Local reason codes and retained renderer termination state |
| `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/PersistentFpsOverlay.kt` | Always-visible, touch-transparent bottom-right FPS label |
| `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt` | Shared 250ms rendered-frame sample and overlay lifecycle |
| `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt` | Emit `leftcarStreamTerminated` to React Native |
| `apps/viewer-expo/src/stream-termination.native.ts` | Typed native event subscription |
| `apps/viewer-expo/src/stream-termination.ts` | Pure matching and single-flight selection policy |
| `apps/viewer-expo/src/use-stream-controller.ts` | Invoke one existing restore mutation for native termination |

---

### Task 1: Make one encoder the product policy and hide split diagnostics

**Files:**

- Modify: `native/macos-capture-shim/Sources/Encoder/EncoderExperimentPolicy.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/EncoderExperimentCapability.swift`
- Modify: `native/macos-capture-shim/Sources/Split/SplitEncoderStrategy.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/viewer-expo/src/encoder-experiment.ts`
- Modify: `apps/viewer-expo/src/encoder-experiment.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`

**Interfaces:**

```swift
func splitDiagnosticEnabled(
    environment: [String: String] = ProcessInfo.processInfo.environment
) -> Bool

func encoderExperimentStartupDecision(
    requested: EncoderExperiment,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    mediaTransport: String,
    hasEncoderPixelBufferPool: Bool,
    splitDiagnosticEnabled: Bool
) -> EncoderExperimentStartupDecision
```

```rust
fn encoder_experiment_is_startable(
    advertised: &[EncoderExperimentInfo],
    requested: EncoderExperiment,
    split_diagnostic_enabled: bool,
) -> bool;
```

- [ ] **Step 1: Add failing product-capability tests**

Change Swift policy expectations so `advertisedEncoderExperiments` and JSON capability entries contain only `auto`, `rateControl`, `adaptiveQp`, and `encoderPool` even when a dual AVE probe would have succeeded. Add explicit startup assertions that exact split fails when the diagnostic flag is false and succeeds only when it is true and the existing exact 3840x2160@60 UDP/pool constraints pass.

Add SplitPipeline assertions:

```swift
precondition(!splitDiagnosticEnabled(environment: [:]))
precondition(splitDiagnosticEnabled(environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "1"]))
precondition(!splitDiagnosticEnabled(environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "true"]))
```

Change TypeScript expectations so normalization still understands the wire ID, but `availableEncoderExperiments`, `availableEncoderExperimentsForStreams`, and every resolver hide or reject `splitVertical`, including exact 4K. Change the launch test that previously selected split through product capabilities to expect `auto` and a single receiver port.

Add Rust control tests proving product normalization drops both split IDs and `encoder_experiment_is_startable` permits `splitVertical` only for an explicit diagnostic request.

- [ ] **Step 2: Run focused tests and prove RED**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-single-policy-red
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-single-split-red
bun test apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml control::tests::advertised_encoder_experiments -- --nocapture
```

Expected: Swift/TypeScript/Rust assertions fail because split is still advertised and exact 4K still resolves it through the product path.

- [ ] **Step 3: Implement the diagnostic gate and single-session advertisement**

Implement `splitDiagnosticEnabled` with the exact string value `1`. Remove the dual AVE allocation probe from `encoderExperimentCapabilityJSON`; product capability generation must no longer depend on model name, encoder-pair allocation, or the diagnostic environment.

Make product advertisement functions omit `splitVertical` unconditionally. Keep parsing, labels, wire enums, split implementation, and test helpers intact. Add the diagnostic Boolean to the exact split startup decision before checking dimensions and transport. Production `auto` must continue resolving to the existing single RTVC H.264 rate-control policy.

In Host control, filter split from `getCatalog`/status capability output even if a stale backend returns it. During `startStream`, admit an unadvertised `splitVertical` request only when `LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC=1`; let the existing geometry/UDP checks and shim gate fail closed afterward.

In the Viewer selector, filter split after normalization for every resolution. Do not remove it from the TypeScript union because old hosts and direct diagnostic tooling still use the wire value.

- [ ] **Step 4: Run the complete policy boundary**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-single-policy
/tmp/leftcar-single-policy
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-single-split
/tmp/leftcar-single-split
bun test apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
```

Expected: all commands exit 0; product capability has no split entry; direct split is denied without the exact flag and remains testable with it.

- [ ] **Step 5: Stage only reviewed policy hunks and commit**

```bash
git diff -- native/macos-capture-shim/Sources/Encoder/EncoderExperimentPolicy.swift native/macos-capture-shim/Sources/Encoder/EncoderExperimentCapability.swift native/macos-capture-shim/Sources/Split/SplitEncoderStrategy.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift native/macos-capture-shim/Tests/SplitPipelineTests.swift apps/host-desktop/src-tauri/src/control.rs apps/viewer-expo/src/encoder-experiment.ts apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts
git add -N native/macos-capture-shim/Sources/Encoder/EncoderExperimentPolicy.swift native/macos-capture-shim/Sources/Encoder/EncoderExperimentCapability.swift native/macos-capture-shim/Sources/Split/SplitEncoderStrategy.swift native/macos-capture-shim/Tests/SplitPipelineTests.swift apps/viewer-expo/src/encoder-experiment.ts apps/viewer-expo/src/encoder-experiment.test.ts
git add -p native/macos-capture-shim/Sources/Encoder/EncoderExperimentPolicy.swift native/macos-capture-shim/Sources/Encoder/EncoderExperimentCapability.swift native/macos-capture-shim/Sources/Split/SplitEncoderStrategy.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift native/macos-capture-shim/Tests/SplitPipelineTests.swift apps/host-desktop/src-tauri/src/control.rs apps/viewer-expo/src/encoder-experiment.ts apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts
git diff --cached --check
git diff --cached
git commit -m "feat(stream): 단일 인코더를 제품 기본값으로 고정"
```

Expected: the staged diff contains only this task. If an untracked file contains inseparable prior work, do not commit that file and report it before continuing.

---

### Task 2: Add the 250ms VideoToolbox missing-callback watchdog

**Files:**

- Create: `native/macos-capture-shim/Sources/Encoder/SingleEncoderHealth.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/EncoderSubmissionState.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncodeSubmission.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderStartup.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderLifecycle.swift`
- Modify: `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/EncoderSessionSelection.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

```swift
enum SingleEncoderHealthDecision: Equatable {
    case healthy
    case restart(generation: UInt64)
    case terminate
}

struct SingleEncoderHealthState {
    static let stallBudgetNs: UInt64 = 250_000_000
    static let restartCooldownNs: UInt64 = 1_000_000_000
    static let restartWindowNs: UInt64 = 10_000_000_000
    static let maximumRestarts = 2

    mutating func evaluate(
        nowNs: UInt64,
        captureCallbackNs: UInt64?,
        lastValidOutputNs: UInt64?,
        encodeInFlight: Int,
        oldestSubmissionNs: UInt64?,
        generation: UInt64
    ) -> SingleEncoderHealthDecision
    mutating func recordRestart(at nowNs: UInt64, generation: UInt64)
    mutating func recordValidOutput(at nowNs: UInt64)
}

struct SingleEncodeSubmission {
    let id: UInt64
    let generation: UInt64
    let pts: Int64
    let submittedNs: UInt64
    let token: EncodeSlotCompletionToken
}

struct SingleEncodeSubmissionLedger {
    mutating func register(_ submission: SingleEncodeSubmission)
    mutating func retire(id: UInt64) -> SingleEncodeSubmission?
    mutating func reclaim(generation: UInt64) -> [SingleEncodeSubmission]
    func oldestSubmissionNs(generation: UInt64) -> UInt64?
}
```

`EncodeSlotCompletionSource` gains `.watchdog`. `leftcarPerfLogLine` gains `encoderWatchdogRestarts`, `encoderWatchdogTerminations`, `encoderLateCallbacks`, and `encoderWatchdogOldestUs` tokens.

- [ ] **Step 1: Add failing pure health and ledger tests**

In `EncodePolicyTests.swift`, add cases for:

- an oldest slot at 249,999,999ns is healthy and 250,000,000ns requests restart;
- no capture callback, unchanged capture callback, no in-flight slot, or no oldest slot is healthy;
- valid output newer than the oldest outstanding submission prevents a false stall;
- one generation produces one restart decision;
- a second generation inside the one-second cooldown is suppressed;
- two restarts inside ten seconds are allowed and the third qualified stall terminates;
- restart records older than ten seconds are pruned;
- callback and watchdog compete for one token and `completionCount` remains one;
- reclaim returns only the requested generation and retiring a reclaimed/late ID is nil;
- the first post-restart frame plan forces a keyframe.

- [ ] **Step 2: Compile and prove RED**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encoder-watchdog-red
```

Expected: compilation fails because the new health state, ledger, and `.watchdog` completion source are absent.

- [ ] **Step 3: Implement the pure state and exactly-once ledger**

Use monotonic nanoseconds only. `evaluate` must remember the last observed capture timestamp so repeated evaluation without a newer capture is healthy. It must also remember the recovered generation so one stalled generation cannot schedule two restarts.

The ledger owns tokens until callback, synchronous submit failure, or watchdog reclaim retires the submission. Register immediately before `VTCompressionSessionEncodeFrame`, after the PTS maps are populated. Both callback and synchronous return paths retire by submission ID before claiming their token. A late callback from an invalidated generation increments diagnostics but cannot release a second slot or packetize output.

- [ ] **Step 4: Queue one health evaluation from capture progress**

Add `singleEncoderHealthCheckScheduled` to `CaptureSession`. At the end of a valid single-session capture callback, snapshot whether any outstanding submission can be old enough and queue at most one evaluation on `encodeQueue`. Do not run this path for `splitVertical`.

Never nest `stateLock` and `captureLock`. Read each protected snapshot separately, then call the pure decision. This preserves the existing callback-to-completion lock order.

- [ ] **Step 5: Implement ordered restart and bounded termination**

For `.restart(generation)` on `encodeQueue`:

1. Detach that generation's ledger records under `stateLock`.
2. Claim each token with `.watchdog` and count only successful claims.
3. Call the existing encoder invalidation path, which increments `encoderSessionGeneration` and invalidates VideoToolbox/input-transfer resources.
4. Clear old-generation PTS/AU maps, release exactly the successfully claimed slots, and clear the recovery gate.
5. Set `forceKeyframe = true`, record the restart, and schedule the newest pending capture.

For `.terminate`, increment the termination metric and call `markStopped("encoder_stalled")` from `encodeQueue`. The queue-specific invalidation path must avoid a self-deadlock. Ordinary callbacks with dropped/failed dispositions stay on the existing recovery path and do not count as missing callbacks.

- [ ] **Step 6: Export watchdog evidence**

Add cumulative counters and current oldest outstanding age to `statsJSON`. Extend `leftcarPerfLogLine` and its exact-string policy test with:

```text
encoderWatchdogRestarts=0 encoderWatchdogTerminations=0 encoderLateCallbacks=0 encoderWatchdogOldestUs=0
```

The performance parser tolerates additional key/value tokens; add parser assertions so the new integer tokens are accepted without changing existing acceptance math.

- [ ] **Step 7: Run Swift and performance analyzer tests**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encoder-watchdog
/tmp/leftcar-encoder-watchdog
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-encoder-watchdog-split
/tmp/leftcar-encoder-watchdog-split
bun test tools/perf-matrix/analyze-performance.test.ts
```

Expected: all exit 0; slot completion is exactly once; 249ms/250ms and restart-budget boundaries are explicit.

- [ ] **Step 8: Stage reviewed watchdog hunks and commit**

```bash
git diff -- native/macos-capture-shim/Sources/Encoder/SingleEncoderHealth.swift native/macos-capture-shim/Sources/Encoder/EncoderSubmissionState.swift native/macos-capture-shim/Sources/Capture/CaptureSession.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncodeSubmission.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderStartup.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderLifecycle.swift native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift native/macos-capture-shim/Sources/Encoder/EncoderSessionSelection.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift tools/perf-matrix/analyze-performance.ts tools/perf-matrix/analyze-performance.test.ts
git add -N native/macos-capture-shim/Sources/Encoder/SingleEncoderHealth.swift native/macos-capture-shim/Sources/Encoder/EncoderSubmissionState.swift native/macos-capture-shim/Sources/Capture/CaptureSession.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncodeSubmission.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderStartup.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderLifecycle.swift native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift native/macos-capture-shim/Sources/Encoder/EncoderSessionSelection.swift
git add -p native/macos-capture-shim/Sources/Encoder/SingleEncoderHealth.swift native/macos-capture-shim/Sources/Encoder/EncoderSubmissionState.swift native/macos-capture-shim/Sources/Capture/CaptureSession.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncodeSubmission.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderStartup.swift native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderLifecycle.swift native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift native/macos-capture-shim/Sources/Encoder/EncoderSessionSelection.swift native/macos-capture-shim/Tests/EncodePolicyTests.swift tools/perf-matrix/analyze-performance.ts tools/perf-matrix/analyze-performance.test.ts
git diff --cached --check
git diff --cached
git commit -m "fix(stream): 250ms 인코더 정지 복구 추가"
```

---

### Task 3: Add Android control and Surface-progress watchdogs

**Files:**

- Create: `native/android-viewer/src/renderer/single_session/health.rs`
- Modify: `native/android-viewer/src/renderer/single_session.rs`
- Modify: `native/android-viewer/src/renderer/single_session/runtime/worker.rs`
- Modify: `native/android-viewer/src/renderer/single_session/presentation.rs`
- Modify: `native/android-viewer/src/renderer/single_session/network.rs`
- Modify: `native/android-viewer/src/renderer/single_session/frame_queue.rs`
- Modify: `native/android-viewer/src/jni.rs`
- Modify: `native/android-viewer/src/jni_exports/session_io.rs`

**Interfaces:**

```rust
pub(super) const RENDER_IDR_DEADLINE: Duration = Duration::from_millis(250);
pub(super) const RENDER_REBUILD_DEADLINE: Duration = Duration::from_millis(750);
pub(super) const RENDER_TERMINATE_DEADLINE: Duration = Duration::from_secs(3);
pub(super) const MISSED_CONTROL_PROBE_LIMIT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderHealthAction {
    None,
    RequestIdr,
    RebuildDecoder,
    TerminateRenderStalled,
}

pub(super) struct RenderHealthState {
    pub(super) fn observe(
        &mut self,
        now: Instant,
        completed_access_units: u64,
        rendered_frames: u64,
    ) -> RenderHealthAction;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlHealthAction {
    None,
    TerminateHostUnreachable,
}

pub(super) struct ControlHealthState {
    pub(super) fn probe_sent(&mut self, sequence: u32) -> ControlHealthAction;
    pub(super) fn probe_acknowledged(&mut self, sequence: u32) -> bool;
}
```

- [ ] **Step 1: Add failing render-health tests**

Add Rust unit tests proving:

- media silence with no completed-AU change produces no action at every deadline;
- completed AUs advancing with a fixed rendered count produces no action at 249ms and one IDR at 250ms;
- repeated observations do not emit another IDR;
- continued AU progress produces one rebuild at 750ms;
- continued AU progress terminates at three seconds;
- one Surface-release increment resets the incident and permits a future independent incident;
- an AU that advanced once and then remains unchanged does not trigger an action on timer-only loop iterations.

- [ ] **Step 2: Add failing probe-health and termination tests**

Test the first sent probe as outstanding but not missed, matching acknowledgement as reset, an old/wrong sequence as no reset, and the third consecutively superseded unacknowledged probe as `TerminateHostUnreachable`. Extend termination-cache tests to preserve local codes 4 and 5 after renderer cleanup.

- [ ] **Step 3: Run focused tests and prove RED**

```bash
cargo test -p android-viewer renderer::single_session::health::tests -- --nocapture
```

Expected: compilation fails because `health.rs` and its state machines do not exist.

- [ ] **Step 4: Implement pure health state**

`RenderHealthState` starts an incident only on a call where completed access units increased and rendered frames did not. Timer-only calls with unchanged AU count return `None`. On every subsequent AU-progress call, compare total incident age and choose the highest not-yet-emitted threshold. Any rendered-frame increment clears start time and action flags.

`ControlHealthState` stores one outstanding sequence and a consecutive-miss count. Sending a new probe while the previous sequence is outstanding increments misses. A matching authenticated response clears both. Three misses return termination once.

- [ ] **Step 5: Integrate completed-AU and Surface progress**

Add `completed_access_units` beside the existing decoder PTS counter; increment it by `completed_count` before live-edge selection. After every presentation pass and receive timeout, call `RenderHealthState::observe` with native `rendered_frames`.

Apply actions as follows:

- `RequestIdr`: use the existing debounced request gate.
- `RebuildDecoder`: call `reset_decoder`, clear frame identity/reassembly state required by the old reference chain, and request a fresh configuration/IDR.
- `TerminateRenderStalled`: store reason 5, set `send_bye=false`, and stop the renderer.

Do not reset MediaCodec for media silence or static content. Do not apply this single-Surface watchdog to split renderer sessions.

- [ ] **Step 6: Integrate matching probe acknowledgement and local termination**

On each actual authenticated probe send, call `probe_sent`. When `consume_viewer_response` parses a valid LCP2 response, pass its sequence to `probe_acknowledged`; only the outstanding matching sequence resets health. On `TerminateHostUnreachable`, store reason 4, set `send_bye=false`, and stop.

Update JNI comments and cached-reason tests for codes 4 and 5. Keep the existing retention order: remove the active renderer, cache any nonnegative reason, then publish `finished`.

- [ ] **Step 7: Run renderer and workspace Rust gates**

```bash
cargo test -p android-viewer renderer::single_session -- --nocapture
cargo test -p android-viewer --lib -- --nocapture
cargo test -p viewer-decoder -- --nocapture
cargo clippy -p android-viewer --tests -- -D warnings
```

Expected: all exit 0; timer-only media silence is safe; exact 250ms/750ms/3s and three-missed-probe boundaries pass.

- [ ] **Step 8: Stage reviewed receiver hunks and commit**

```bash
git diff -- native/android-viewer/src/renderer/single_session/health.rs native/android-viewer/src/renderer/single_session.rs native/android-viewer/src/renderer/single_session/runtime/worker.rs native/android-viewer/src/renderer/single_session/presentation.rs native/android-viewer/src/renderer/single_session/network.rs native/android-viewer/src/renderer/single_session/frame_queue.rs native/android-viewer/src/jni.rs native/android-viewer/src/jni_exports/session_io.rs
git add -N native/android-viewer/src/renderer/single_session/health.rs native/android-viewer/src/renderer/single_session.rs native/android-viewer/src/renderer/single_session/runtime/worker.rs native/android-viewer/src/renderer/single_session/presentation.rs native/android-viewer/src/renderer/single_session/network.rs native/android-viewer/src/renderer/single_session/frame_queue.rs native/android-viewer/src/jni_exports/session_io.rs
git add -p native/android-viewer/src/renderer/single_session/health.rs native/android-viewer/src/renderer/single_session.rs native/android-viewer/src/renderer/single_session/runtime/worker.rs native/android-viewer/src/renderer/single_session/presentation.rs native/android-viewer/src/renderer/single_session/network.rs native/android-viewer/src/renderer/single_session/frame_queue.rs native/android-viewer/src/jni.rs native/android-viewer/src/jni_exports/session_io.rs
git diff --cached --check
git diff --cached
git commit -m "fix(viewer): 수신 및 화면 정지 복구 추가"
```

---

### Task 4: Add the persistent actual-FPS overlay and native termination event

**Files:**

- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/PersistentFpsOverlay.kt`
- Create: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/PersistentFpsOverlayTest.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt`
- Modify: `apps/viewer-expo/android/app/build.gradle`

**Interfaces:**

```kotlin
internal data class RenderedFpsSample(
    val frames: Long,
    val sampledAtMs: Long,
    val displayedFps: Double?,
)

internal fun nextRenderedFpsSample(
    previous: RenderedFpsSample?,
    renderedFrames: Long,
    nowMs: Long,
): RenderedFpsSample

internal class PersistentFpsOverlay(private val activity: Activity) {
    fun show()
    fun update(displayedFps: Double?)
    fun stop()
}
```

Native event payload:

```json
{ "port": 5000, "reason": 4 }
```

- [ ] **Step 1: Add failing Kotlin sampler and layout-policy tests**

Add `testImplementation("junit:junit:4.13.2")`. Test initial `-- FPS`, a 10-frame/250ms sample as 40 FPS, counter reset handling, the existing 0.65/0.35 bounded smoothing, 9sp size, bottom-end gravity, touch/focus disabled policy, and content-description text derived from the rounded actual rate.

- [ ] **Step 2: Run Kotlin tests and prove RED**

```bash
cd apps/viewer-expo/android
GRADLE_USER_HOME=/tmp/leftcar-gradle ./gradlew :app:testDebugUnitTest --tests dev.leftcar.viewer.stream.PersistentFpsOverlayTest
```

Expected: test compilation fails because the sampler and overlay policy are absent.

- [ ] **Step 3: Implement one shared sample and persistent overlay**

Extract rendered-frame delta and smoothing from `StreamHudController.updateStats` into `nextRenderedFpsSample`. The existing 250ms poll reads native stats once, updates the detailed temporary HUD, and updates `PersistentFpsOverlay` with the same value.

Use a 9sp monospace `TextView`, low-alpha white text, a minimal rounded translucent dark background, no animation/elevation, and `PopupWindow` with touch/focus/outside-touch disabled. Show it at `Gravity.BOTTOM or Gravity.END` with 10dp margins and current system-bar/gesture insets. `show()` is idempotent; `stop()` removes callbacks/references and dismisses the popup. The overlay never participates in the detailed HUD fade.

- [ ] **Step 4: Emit local native termination exactly once**

Give `StreamLauncherModule` a weak reference to the current `ReactApplicationContext`, plus React Native `addListener` and `removeListeners` methods. Add a companion `emitTermination(port, reason)` that emits `leftcarStreamTerminated` through `RCTDeviceEventEmitter` on the native modules queue.

Rename Activity handling to represent both Host and local termination. Keep reasons 1-3 as terminal Host notices. For reason 4 or 5, emit `{port, reason}` before `finish()` so React retains the arguments while the stale Surface closes. Use messages that say reconnecting, not that sharing permanently ended.

- [ ] **Step 5: Run Kotlin tests and Android Kotlin compilation**

```bash
cd apps/viewer-expo/android
GRADLE_USER_HOME=/tmp/leftcar-gradle ./gradlew :app:testDebugUnitTest :app:compileDebugKotlin :app:compileReleaseKotlin
```

Expected: all tasks succeed; the overlay tests prove actual rendered deltas and bottom-right persistent policy.

- [ ] **Step 6: Stage reviewed Android UI/event hunks and commit**

```bash
git diff -- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/PersistentFpsOverlay.kt apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/PersistentFpsOverlayTest.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt apps/viewer-expo/android/app/build.gradle
git add -N apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/PersistentFpsOverlay.kt apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/PersistentFpsOverlayTest.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt
git add -p apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/PersistentFpsOverlay.kt apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/PersistentFpsOverlayTest.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt apps/viewer-expo/android/app/build.gradle
git diff --cached --check
git diff --cached
git commit -m "feat(viewer): 상시 실제 FPS 표시 추가"
```

---

### Task 5: Reconnect one matching stream once

**Files:**

- Create: `apps/viewer-expo/src/stream-termination.ts`
- Create: `apps/viewer-expo/src/stream-termination.native.ts`
- Create: `apps/viewer-expo/src/stream-termination.test.ts`
- Modify: `apps/viewer-expo/src/use-stream-controller.ts`

**Interfaces:**

```typescript
export type LocalStreamTerminationReason = 4 | 5;

export interface StreamTerminationEvent {
  port: number;
  reason: LocalStreamTerminationReason;
}

export interface StreamTerminationSubscription {
  remove(): void;
}

export function subscribeStreamTermination(
  listener: (event: StreamTerminationEvent) => void,
): StreamTerminationSubscription;

export function selectRecoverableStream(
  streams: readonly ActiveStream[],
  event: unknown,
  inFlightSessions: ReadonlySet<number>,
): ActiveStream | null;
```

The restore mutation input becomes:

```typescript
interface RestartRequest {
  active: ActiveStream;
  trigger: "hostStatus" | "nativeTermination";
}
```

- [ ] **Step 1: Add failing pure selection tests**

Test malformed payloads, reasons outside 4/5, no matching port, exactly one matching stream, duplicate event while session is in flight, and two different ports. Add reducer assertions that native-triggered restore failure removes only the dead matching session while host-status failure retains current behavior.

- [ ] **Step 2: Run focused TypeScript tests and prove RED**

```bash
bun test apps/viewer-expo/src/stream-termination.test.ts
```

Expected: compilation fails because the typed event and selection policy do not exist.

- [ ] **Step 3: Implement typed native subscription and single-flight selection**

Use `DeviceEventEmitter.addListener("leftcarStreamTerminated", listener)` in the `.native.ts` adapter, matching the existing USB event style. Validate finite integer port and exact reason 4/5 before selecting.

Keep `streamsRef` current. Store `restartStream` in a ref so the native subscription is installed once without stale closures. On a valid event, find the active stream by port, reject it when `heartbeatInFlight` already contains the session, add the session, and invoke the existing restore mutation with trigger `nativeTermination`.

- [ ] **Step 4: Make failed native restore terminal, not looping**

Change the mutation to accept `RestartRequest`. On success, replace the old session exactly as today and reopen through existing `reconnect=true` launcher behavior. On native-triggered error, remove the old active stream and expose the existing reconnection error. On settlement, clear the single-flight set. Do not schedule another native retry and do not let the two-second Host-status effect rediscover a removed dead session.

Keep automatic Host-status recovery and transport switching behavior unchanged. Remove any duplicate condition introduced while editing the current untracked controller file.

- [ ] **Step 5: Run React quality gates in required order**

```bash
bun test apps/viewer-expo/src/stream-termination.test.ts apps/viewer-expo/src/launch-stream.test.ts
npx -y react-doctor@latest . --verbose
bun run --cwd apps/viewer-expo typecheck
bun run typecheck
```

Expected: tests/typechecks exit 0 and React Doctor reports exactly `100 / 100`.

- [ ] **Step 6: Stage reviewed reconnect hunks and commit**

```bash
git diff -- apps/viewer-expo/src/stream-termination.ts apps/viewer-expo/src/stream-termination.native.ts apps/viewer-expo/src/stream-termination.test.ts apps/viewer-expo/src/use-stream-controller.ts
git add -N apps/viewer-expo/src/stream-termination.ts apps/viewer-expo/src/stream-termination.native.ts apps/viewer-expo/src/stream-termination.test.ts apps/viewer-expo/src/use-stream-controller.ts
git add -p apps/viewer-expo/src/stream-termination.ts apps/viewer-expo/src/stream-termination.native.ts apps/viewer-expo/src/stream-termination.test.ts apps/viewer-expo/src/use-stream-controller.ts
git diff --cached --check
git diff --cached
git commit -m "feat(viewer): 정지 스트림 단일 재연결 추가"
```

---

### Task 6: Run complete static, build, install, and physical verification

**Files:**

- Modify only if new evidence is recorded: `docs/11-low-latency-investigation.md`
- Create runtime artifacts only under: `/tmp/leftcar-single-encoder-20260830/`

- [ ] **Step 1: Prove the final source gates from a clean index**

First inspect the remaining dirty tree and ensure no task left staged files:

```bash
git status --short
git diff --cached --check
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-final-policy
/tmp/leftcar-final-policy
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-final-split
/tmp/leftcar-final-split
cargo test -p control-contract -p viewer-decoder -p android-viewer -- --nocapture
cargo clippy -p android-viewer --tests -- -D warnings
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
bun run test
bun run test:contract
bun run test:architecture
npx -y react-doctor@latest . --verbose
bun run typecheck
git diff --check
```

Expected: all commands exit 0 and React Doctor is exactly `100 / 100`. Record any baseline failure separately and fix only failures caused by or located in scoped files; do not conceal unrelated findings.

- [ ] **Step 2: Cross-build the current Android native library and APK**

```bash
mkdir -p /tmp/leftcar-single-encoder-20260830
PATH=/Users/loopy/Library/Android/sdk/ndk/27.1.12297006/toolchains/llvm/prebuilt/darwin-x86_64/bin:$PATH cargo build -p android-viewer --target aarch64-linux-android --release
cd apps/viewer-expo/android
ANDROID_HOME=/Users/loopy/Library/Android/sdk ANDROID_SDK_ROOT=/Users/loopy/Library/Android/sdk GRADLE_USER_HOME=/tmp/leftcar-gradle CCACHE_DIR=/tmp/leftcar-ccache CCACHE_TEMPDIR=/tmp/leftcar-ccache-tmp ./gradlew :app:testDebugUnitTest :app:assembleRelease
```

Expected: current `target/aarch64-linux-android/release/libleftcar_viewer.so` is copied into a signed release APK at `apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk`.

- [ ] **Step 3: Build and install matched Host and Viewer binaries**

Return to the repository root, then run:

```bash
/bin/zsh tools/dev-host-macos.zsh
/usr/bin/codesign --verify --deep --strict --verbose=2 "/Applications/Leftcar Host.app"
/usr/bin/shasum -a 256 native/macos-capture-shim/libleftcar_capture.dylib "/Applications/Leftcar Host.app/Contents/Resources/libleftcar_capture.dylib" apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk
adb devices -l
adb install -r apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk
adb shell pm path leftcar.ll3.kr
```

Expected: Host signing passes, source and installed shim hashes match, exactly one authorized target Viewer is selected, APK install reports `Success`, and package path exists. Installation/build success is not yet live-stream proof.

- [ ] **Step 4: Prove production capability and one encoder session**

Launch the installed Host and Viewer without `LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC`. Establish exact 3840x2160@60 using the `clarity`/interactive profile and direct Wi-Fi UDP. Inspect catalog/status and logs before accepting the run:

- product capability contains `auto`, `rateControl`, `adaptiveQp`, and `encoderPool`, but no `splitVertical`;
- requested/applied product experiment uses one RTVC H.264 session;
- Host logs report hardware acceleration and exact 3840x2160 input/output;
- Android creates one low-latency H.264 MediaCodec decoder and one Surface;
- the bottom-right label is visible, tiny, touch-transparent, and does not fade.

Save catalog/status receipts and initial logs under `/tmp/leftcar-single-encoder-20260830/`.

- [ ] **Step 5: Run the 180-second moving-wallpaper performance sample**

Move the Mac to the visibly moving desktop with:

```bash
/opt/homebrew/bin/aerospace workspace 0
```

Confirm pixels are actually changing before collection. Then run:

```bash
tools/perf-matrix/collect-1440-4k.sh --profile clarity --duration 180 --output /tmp/leftcar-single-encoder-20260830/clarity-moving
adb shell screencap -p /sdcard/leftcar-single-encoder-fps.png
adb pull /sdcard/leftcar-single-encoder-fps.png /tmp/leftcar-single-encoder-20260830/leftcar-single-encoder-fps.png
```

Expected runtime conditions:

- no lasting frozen frame and no repeated recovery loop;
- `encoderMode=rtvc` and not split;
- capture FPS, valid encode-output FPS, Surface-release FPS, queue age, loss/drop counts, and watchdog counters are present;
- healthy-run watchdog restarts/terminations remain zero unless a real stall occurred;
- the screenshot label approximately agrees with rendered-frame deltas over the same interval, allowing smoothing/sample-boundary variance.

Run the analyzer and retain its JSON:

```bash
bun tools/perf-matrix/analyze-performance.ts --host /tmp/leftcar-single-encoder-20260830/clarity-moving.host.ndjson --android /tmp/leftcar-single-encoder-20260830/clarity-moving.android.log --duration 180 --output /tmp/leftcar-single-encoder-20260830/clarity-moving.recheck.json
```

Do not call the run 4K60 unless capture, encode output, and Android Surface-release evidence all sustain 60fps under this moving workload.

- [ ] **Step 6: Verify local stale-Surface closure and bounded reconnect**

With a live stream, suspend only the exact installed Host process so it cannot send an authenticated termination packet, then resume the same PID after the Viewer crosses its three-missed-probe boundary:

```bash
leftcar_host_pid=$(/usr/bin/pgrep -x leftcar-host-desktop)
test -n "$leftcar_host_pid"
test "$(printf '%s\n' "$leftcar_host_pid" | /usr/bin/wc -l | /usr/bin/tr -d ' ')" = "1"
/bin/kill -STOP "$leftcar_host_pid"
/bin/sleep 5
/bin/kill -CONT "$leftcar_host_pid"
```

Do not use a broad process pattern. Observe Android throughout the five-second suspension and subsequent resume.

Expected:

- native reason 4 is retained and delivered once when no authenticated termination packet arrives;
- the stale Activity/Surface closes instead of showing a frozen last frame;
- one `leftcarStreamTerminated` event targets the matching port;
- one restore attempt occurs; if Host remains unavailable it fails once and the dead stream disappears without an automatic loop.

After resuming the Host, establish a fresh stream and verify normal operation resumes. The three-second render-stall path is accepted from deterministic Rust/Kotlin/TypeScript tests unless a safe real MediaCodec stall is observed naturally; do not claim a physical injected Surface stall without an explicit reproducible fault.

- [ ] **Step 7: Record evidence and limitations**

If updating `docs/11-low-latency-investigation.md`, record:

- commit IDs and source/installed binary hashes;
- Mac model and connected Viewer identity;
- exact profile, transport, encoder mode/ID, dimensions, and duration;
- capture, encode-output, and Surface-release FPS separately;
- 250ms/750ms/3s automated boundary results;
- control reason-4 physical result and whether restore succeeded or failed once;
- losses, drops, queue age, watchdog counts, and report paths;
- explicit statement that M1 Max evidence is not base-M1 evidence and measured non-60 output is not 4K60 proof.

Run final documentation and index checks:

```bash
git diff --check
git status --short
git diff --cached --check
```

If the evidence document is the only intended final change, stage only its reviewed hunk and commit:

```bash
git add -p docs/11-low-latency-investigation.md
git diff --cached --check
git diff --cached
git commit -m "docs(stream): 단일 인코더 실기기 검증 기록"
```

---

## Final Acceptance Checklist

- [ ] Product capability and UI never expose `splitVertical`.
- [ ] Exact diagnostic flag plus exact existing constraints still permit direct split testing.
- [ ] Every product experiment creates exactly one VideoToolbox session.
- [ ] Swift tests prove 249ms/250ms, cooldown, rolling budget, late callback, and exactly-once slot completion.
- [ ] Rust tests prove static/media-silent safety, 250ms IDR, 750ms rebuild, 3s termination, and three missed probes.
- [ ] Kotlin tests prove rendered-frame FPS calculation and persistent bottom-right overlay policy.
- [ ] TypeScript tests prove port matching, one in-flight restore, and no retry loop after native-triggered failure.
- [ ] React Doctor is exactly `100 / 100` after the final React Native change.
- [ ] Swift, Rust, Host Rust, TypeScript, contract, architecture, Kotlin, and release APK gates all pass.
- [ ] Installed Host shim and built shim hashes match; APK installs on one authorized device.
- [ ] A 180-second exact-4K moving-wallpaper sample has no lasting stale Surface or unbounded recovery loop.
- [ ] Persistent overlay agrees with native Surface-release deltas within sampling variance.
- [ ] Current-device results are reported without converting M1 Max evidence into base-M1 or unmeasured 4K60 claims.
