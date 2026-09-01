# Adaptive Resolution Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the viewer at its source-compatible target, move a congested 4K stream to 1440p60 after two bad windows, and return to 4K after four stable seconds without closing the Android window.

**Architecture:** A deterministic policy consumes host/viewer performance observations and emits `Keep`, `Downshift`, or `Upshift`. The control contract carries the accepted target and quality state; a bounded host reconfigure replaces the capture handle while retaining the logical session and port. Android rebinds the existing Surface/native state on the next launch intent, and a failed bounded retry remains on that Activity with an on-screen retry state.

**Tech Stack:** Rust/Tauri control server, Rust viewer JNI core, Swift VideoToolbox shim, TypeScript/Vitest React Native controller, Kotlin Android Activity, Cargo/XCTest/Gradle/ADB.

**Spec:** docs/superpowers/specs/2026-08-31-adaptive-resolution-recovery-design.md

**Implementation status (2026-08-31):** Tasks 1–7 are implemented; automated gates and a bounded physical Lenovo verification passed. Commit steps were intentionally skipped because this worktree contains overlapping dirty changes from the neighboring encoder/USB work; the adaptive hunks remain available for explicit patch staging later.

## Global Constraints

- Exact source-compatible dimensions; automatic mode never upscales and preserves the requested FPS.
- A 4K downshift requires 2 consecutive congested one-second windows; an upshift requires 4 stable one-second windows and a 5-second rebind cooldown.
- A 1440p-or-smaller source never downshifts below its source dimensions.
- Resolution transitions use the existing viewer port/session ownership and publish only after first valid keyframe; failed upshift stays on fallback and resets stability.
- Local render-stall reason 5 keeps the Activity alive; its one bounded retry also stays on the same Activity/Surface. Host/operator reasons 1–3 keep their current finish behavior.
- Only ADB serial `192.168.0.18:40607` may be used for physical verification; do not install, launch, or reset any other device.
- Existing dirty neighbor files remain untouched except for surgical hunks required by this feature; never run reset, checkout, broad formatting, or destructive cleanup.

---

### Task 1: Freeze the deterministic adaptive policy

**Files:**
- Create: `apps/viewer-expo/src/adaptive-resolution.ts`
- Create: `apps/viewer-expo/src/adaptive-resolution.test.ts`
- Create: `native/macos-capture-shim/Sources/Encoder/AdaptiveResolutionPolicy.swift`
- Create: `native/macos-capture-shim/Tests/AdaptiveResolutionPolicyTests.swift`
- Modify: `tools/build-macos-capture-shim.zsh` (add the isolated policy-test entry)

**Interfaces:**
- Produces TypeScript `AdaptiveTarget`, `AdaptiveQualityState`, `AdaptiveObservation`, `AdaptiveResolutionState`, `createAdaptiveResolutionState`, `observeAdaptiveResolution`, and `recordAdaptiveResolutionResult` for the viewer controller.
- Produces Swift `AdaptiveResolutionTarget`, `AdaptiveResolutionObservation`, `AdaptiveResolutionDecision`, and `AdaptiveResolutionPolicy.observe(nowMs:observation:)` with the same thresholds for shim-side diagnostics.

- [ ] **Step 1: Write the failing TypeScript threshold tests.** Test exact 4K fitting, the second congested window producing `downshift`, a single loss inside recovery grace producing `keep`, no fallback for a 1440p source, the fourth stable window after cooldown producing one `upshift`, and a failed upshift retaining fallback/resetting the stable counter.
- [ ] **Step 2: Run the focused test to verify RED.** Run `bunx vitest run apps/viewer-expo/src/adaptive-resolution.test.ts`; expect failure because the policy module does not exist.
- [ ] **Step 3: Implement the TypeScript reducer.** Use `congestionWindows >= 2`, `stableWindows >= 4`, `cooldownUntilMs = transitionAt + 5000`, `encodedFps/transmittedFps` thresholds of `requestedFps * 0.90` and `requestedFps * 0.95`, queue age comparison, and `recoveryActive || rebindInFlight` guards. Return a new state and action without reading platform APIs.
- [ ] **Step 4: Add the Swift policy and parity tests.** Keep all thresholds as named constants (`downshiftWindows = 2`, `upshiftWindows = 4`, `rebindCooldownMs = 5_000`) and put XCTest assertions for downshift, grace suppression, cooldown, and failed transition reset in the new test file so the neighboring dirty test file is not staged.
- [ ] **Step 5: Run the focused TypeScript and Swift tests to verify GREEN.** Run `bunx vitest run apps/viewer-expo/src/adaptive-resolution.test.ts` and `tools/build-macos-capture-shim.zsh adaptive-policy-test /tmp/leftcar-adaptive-policy-tests && /tmp/leftcar-adaptive-policy-tests`; both must pass.
- [ ] **Step 6: Commit only the four new policy files.** Run `git add apps/viewer-expo/src/adaptive-resolution.ts apps/viewer-expo/src/adaptive-resolution.test.ts native/macos-capture-shim/Sources/Encoder/AdaptiveResolutionPolicy.swift native/macos-capture-shim/Tests/AdaptiveResolutionPolicyTests.swift && git commit -m "feat: add deterministic adaptive resolution policy"`.

### Task 2: Extend the control contract with accepted target state

**Files:**
- Modify: `crates/control-contract/src/host.rs:787-825`
- Modify: `crates/control-contract/tests/contract.rs:289-340`
- Modify: `apps/viewer-expo/src/control.ts:39-124`

**Interfaces:**
- `StartStreamOutput` gains optional-backward-compatible `width`, `height`, `fps`, and `qualityState` fields.
- Adds `ReconfigureStreamInput { session, width, height, fps, qualityState }` and `ReconfigureStreamOutput { session, width, height, fps, qualityState }` using camelCase JSON.
- `SessionView` gains optional `width`, `height`, `fpsTarget` remains authoritative for requested FPS, and `qualityState` defaults to `native` for old hosts.

- [ ] **Step 1: Add contract RED tests.** Decode a legacy `{"session":7}` start response, encode/decode a complete accepted-target response, and reject a reconfigure request with zero dimensions or FPS zero in the contract validation test.
- [ ] **Step 2: Run `cargo test -p control-contract contract` to verify RED.** Expect missing fields/types until the contract is implemented.
- [ ] **Step 3: Add serde-defaulted Rust fields and reconfigure structs.** Use `#[serde(default)]` for response fields so older Hosts remain readable; serialize new Host responses with actual accepted dimensions.
- [ ] **Step 4: Mirror the fields in `control.ts`.** Use `qualityState?: AdaptiveQualityState` for legacy status payloads and import the policy type without introducing a runtime dependency.
- [ ] **Step 5: Run the contract tests to verify GREEN and regenerate checked-in contract artifacts if the repository test requires them.** Run `cargo test -p control-contract contract`.
- [ ] **Step 6: Commit only contract files and generated contract output if changed.** Use `git add` with explicit paths and `git commit -m "feat: expose accepted stream quality state"`.

### Task 3: Add bounded same-session Host reconfigure

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/control.rs` (Session fields, `snapshot`, `startStream`, and dispatch match)
- Modify: `apps/host-desktop/src-tauri/src/backend.rs` (test fake handle bookkeeping only if required by the reconfigure test)
- Modify: `crates/control-contract/src/host.rs` (reconfigure command payload from Task 2)
- Test: `apps/host-desktop/src-tauri/src/control.rs` existing `#[cfg(test)]` module

**Interfaces:**
- Adds `ControlServer::reconfigure_stream(ReconfigureStreamInput) -> Result<ReconfigureStreamOutput, String>` internally and dispatches command `reconfigureStream`.
- Session stores accepted `width`, `height`, `fps`, and `quality_state`; the session ID and viewer port never change.
- The operation validates dimensions, stops the old backend handle only after the request is authorized, starts the replacement with the saved source/transport/backend/experiment settings, waits for `first_send_ms`, then atomically updates the Session. On failure it attempts one old-target restart and returns an error while leaving the old target state authoritative when recovery succeeds.

- [ ] **Step 1: Write failing Rust tests.** Cover start response dimensions/`native`, successful reconfigure preserving session ID and port while returning `fallback`, invalid target rejection without backend calls, and failed replacement restoring the old target.
- [ ] **Step 2: Run the focused host tests to verify RED.** Run `cargo test -p leftcar-host-desktop control::tests`; expect unknown command/fields and missing Session data.
- [ ] **Step 3: Add Session target fields and central validation.** Keep all edits surgical around existing split-experiment hunks; do not reformat `control.rs`.
- [ ] **Step 4: Implement the reconfigure transaction.** Capture a clone of the old session metadata under the mutex, stop/start outside the mutex, wait for first frame, and update the live map only after success. If the replacement fails, start the old dimensions once and retain the prior quality state.
- [ ] **Step 5: Publish dimensions and quality state from `snapshot()` and `startStream`.** Populate the contract fields from Session, not from a requested value that the backend may have rejected.
- [ ] **Step 6: Run focused and workspace Rust tests.** Run `cargo test -p leftcar-host-desktop control::tests` followed by `cargo test --workspace`.
- [ ] **Step 7: Commit only clean contract/backend paths, or leave overlapping Host hunks unstaged.** Because `apps/host-desktop/src-tauri/src/control.rs` is already dirty from the neighboring encoder work, inspect `git diff --cached` and stage only the adaptive hunks with patch mode; never stage the whole file. Commit `feat: reconfigure stream target without changing session` only when the staged diff contains no neighbor changes.

### Task 4: Carry source/active/fallback targets through viewer start and restore

**Files:**
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`
- Modify: `apps/viewer-expo/src/catalog-model-types.ts`
- Modify: `apps/viewer-expo/src/use-catalog-model.ts`
- Modify: `apps/viewer-expo/src/control.ts`

**Interfaces:**
- `StartedStream` returns accepted `width`, `height`, `fps`, and `qualityState`.
- `ActiveStream` stores `sourceTarget`, `activeTarget`, `fallbackTarget`, and `qualityState`; legacy callers are normalized from existing width/height/fps.
- Adds `reconfigurePreparedStream(control, launcher, active, target, qualityState)` which prepares the existing port, calls `reconfigureStream`, opens the accepted dimensions, and cancels preparation on failure without stopping the logical session.

- [ ] **Step 1: Add failing Vitest cases.** Assert `startPreparedStream` opens and returns Host-accepted dimensions, restore uses `activeTarget`, reconfigure uses the same session/port, and a rejected reconfigure cancels preparation while leaving the active object unchanged.
- [ ] **Step 2: Run `bunx vitest run apps/viewer-expo/src/launch-stream.test.ts` to verify RED.** Existing tests should fail only on the new receipt/command assertions.
- [ ] **Step 3: Implement accepted-target parsing and exact open dimensions.** Preserve the neighboring encoder-experiment and USB changes; replace only the response typing and dimension source.
- [ ] **Step 4: Implement target normalization in `use-catalog-model.ts`.** Use `fitProfileToDisplay` for `sourceTarget`, derive the 4K fallback with `resolveStreamResolution({width: 3840, height: 2160}, balanced profile)` only for an exact 4K source, and never upscale a smaller display.
- [ ] **Step 5: Implement `reconfigurePreparedStream` and wire restore to active target.** A source/profile change creates a fresh `native` state; a transient recovery retains active dimensions.
- [ ] **Step 6: Run focused tests and TypeScript typecheck.** Run `bunx vitest run apps/viewer-expo/src/launch-stream.test.ts apps/viewer-expo/src/adaptive-resolution.test.ts` and `bun run --cwd apps/viewer-expo typecheck`.
- [ ] **Step 7: Commit only isolated viewer hunks if patch staging excludes the pre-existing encoder/USB changes.** Otherwise leave the feature edits in the working tree and report the exact dirty boundary; never stage an entire already-dirty viewer file.

### Task 5: Make controller transitions single-flight and fast to recover

**Files:**
- Modify: `apps/viewer-expo/src/use-stream-controller.ts`
- Create: `apps/viewer-expo/src/use-stream-controller.test.ts`

**Interfaces:**
- `useStreamController(setError, restoreStream, reconfigureStream?)` consumes `StatusView.sessions` once per one-second poll and the Task 1 reducer.
- The controller keeps one policy state per session, claims one rebind at a time, invokes `reconfigurePreparedStream` for `Downshift/Upshift`, records success/failure, and falls back to the existing restore path only after one bounded retry.

- [ ] **Step 1: Write failing controller tests.** Simulate two bad windows, assert exactly one 4K→1440p reconfigure; simulate four healthy windows after a 5-second cooldown, assert one 1440p→4K reconfigure; assert concurrent polls do not issue a second command; assert failed upshift does not close/remove the stream.
- [ ] **Step 2: Run the focused controller test to verify RED.** Run `bunx vitest run apps/viewer-expo/src/use-stream-controller.test.ts`.
- [ ] **Step 3: Add one-second status polling and observation extraction.** Use receiver-loss deltas, encoded/transmitted FPS, pending-frame oldest age, and recovery counters from `SessionView`; ignore observations during the recovery grace period.
- [ ] **Step 4: Wire action execution and result recording.** Mark `downshifting`/`upshifting` before the command, update `ActiveStream` only after accepted Host response, and reset stability on errors without an infinite loop.
- [ ] **Step 5: Keep reasons 1–3 on the current finish path while allowing reason 5 to request the existing-port rebind.** Maintain the current single-flight restore set for host status and native termination; after one automatic retry fails, retain the stream entry and error state instead of removing it or finishing the Activity.
- [ ] **Step 6: Run Vitest, typecheck, and React Doctor.** Run the focused tests, `bun run --cwd apps/viewer-expo typecheck`, and `npx -y react-doctor@latest . --verbose`; the Doctor score must be `100 / 100`.
- [ ] **Step 7: Commit controller files and tests only through patch staging when it excludes the pre-existing neighboring controller hunk; otherwise leave it unstaged and report the boundary.** Never stage unrelated dirty files.

### Task 6: Rebind Android native renderer without Activity recreation

**Files:**
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt`
- Modify: `native/android-viewer/src/jni_wrappers/attach.rs`
- Modify: `native/android-viewer/src/jni_exports.rs`
- Modify: `native/android-viewer/src/renderer/single_session/runtime/worker.rs` or its focused helper module for generation admission
- Test: `native/android-viewer/src/lib.rs` and renderer unit tests

**Interfaces:**
- Adds `ViewerNative.rebindSurfacePort(state, instanceId, surface, port, host, width, height, fps): Int`.
- The JNI rebind stops and joins the old renderer, detaches/releases the old Surface handle, claims the prepared receiver for the new generation, and spawns the replacement while retaining the same `ProcessState`/Activity Surface.
- `StreamActivity.onNewIntent` updates target fields and calls rebind; it does not call `recreate()` for a quality/reconnect intent. `StreamHudController` exposes non-interactive `showRebindIndicator`, `clearRebindIndicator`, and resets termination polling after a successful attach.

- [ ] **Step 1: Add failing Rust lifecycle/generation tests.** Assert a rebind requires a new generation token, an old token cannot publish packets, and a failed attach leaves no duplicate Surface ownership.
- [ ] **Step 2: Run focused native tests to verify RED.** Run `cargo test -p leftcar-android-viewer`; expect missing JNI symbol/generation guard failures.
- [ ] **Step 3: Implement `leftcar_jni_rebind_port`.** Stop/join the old renderer, detach/release the current Surface handle exactly once, call the existing prepared-receiver path, and install the new renderer only after all validation succeeds.
- [ ] **Step 4: Add the Kotlin external method and wire `onNewIntent`.** Preserve the existing `FLAG_ACTIVITY_NEW_DOCUMENT`/`intoExisting` behavior, update window refresh rate and Surface frame rate, and keep Activity alive on reason 5.
- [ ] **Step 5: Add the small HUD indicator and bounded failure callback.** Reason 5 shows the indicator and emits the existing logical termination event; successful rebind clears it; a failed automatic retry keeps the same Activity/Surface visible, shows the retry state, and permits exactly one explicit retry callback without launching a second Activity.
- [ ] **Step 6: Run native Rust tests and Android compile.** Run `cargo test -p leftcar-android-viewer`, then the repository’s debug Gradle task for `apps/viewer-expo/android`.
- [ ] **Step 7: Commit only JNI/Kotlin/native renderer files.** Commit `fix: rebind stalled viewer without closing activity` with explicit paths.

### Task 7: Full verification and exact-device evidence

**Files:**
- No source files unless a failing gate identifies a scoped defect.
- Evidence: `/tmp/leftcar-adaptive-*.log` and `/tmp/leftcar-adaptive-*.json` only.

- [ ] **Step 1: Run all automated gates.** Run `cargo test --workspace`, `swift test --package-path native/macos-capture-shim`, viewer Vitest/typecheck, Android Gradle build, architecture checks, and `npx -y react-doctor@latest . --verbose`; record exit codes and the Doctor `100 / 100` result.
- [ ] **Step 2: Build the debug APK and verify the artifact hash.** Use the repository’s documented Android build command, record APK path/size/SHA-256, and do not copy or install until the build is green.
- [ ] **Step 3: Verify only the target ADB serial.** Run `adb devices -l`, select `192.168.0.18:40607` explicitly, install the newly built APK with `adb -s 192.168.0.18:40607 install -r`, and capture package/version output. Do not issue commands to `192.168.0.7:46557`, emulator serials, or wildcard ADB targets.
- [ ] **Step 4: Run the physical adaptive scenario.** Start an exact 4K moving source, record Host accepted target/FPS and Android PID, drive motion until two congestion windows trigger 1440p60, hold a stable interval for four one-second windows plus the 5-second cooldown, and confirm one return to 4K without Activity PID/task closure.
- [ ] **Step 5: Run non-4K and failure checks.** Start the connected source at 1440p or smaller, confirm no 4K request, force one failed upshift/rebind if reproducible, and confirm fallback remains visible with no loop.
- [ ] **Step 6: Collect bounded evidence.** Save `adb logcat`, Host status snapshots, target/FPS transitions, frame gaps, queue age, render FPS, and rebind counts with timestamps; state explicitly that the receipt proves this Lenovo Yoga Tab run only, not universal 4K60.
- [ ] **Step 7: Inspect the final diff and dirty-file boundary.** Run `git status --short` and `git diff --check`; verify every changed path belongs to this plan or was a pre-existing neighbor file, then report build/test/device results. Do not push or publish.
