# 4K60 Split Flow Control and Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the split-stream recovery feedback loop while preserving exact 3840x2160 capture, sustained dual-AVE 60fps throughput, direct dual-Surface rendering, and bounded low latency.

**Architecture:** Android classifies an IDR following an AU gap as a usable recovery boundary instead of emitting another Loss. macOS reserves one capture-to-send lease before dual encode, keeps admitted encoded pairs ordered, assigns the shared left/right AU ID only at socket-send time, and discards post-encode output only at an explicit paired-IDR dependency boundary.

**Tech Stack:** Swift 6, VideoToolbox, Metal, UDP/FEC, Rust 2021, Android NDK MediaCodec, Kotlin/Gradle, Tauri/Rust, React 19, TypeScript, Vitest, ADB.

**Spec:** `docs/superpowers/specs/2026-08-29-4k60-split-flow-control-recovery-design.md`

## Global Constraints

- Preserve exact 3840x2160 capture and the 60fps product profile; do not silently reduce either value.
- Keep exact dual AVE hardware encoders and two exact-name hardware MediaCodec decoders; no software fallback.
- Keep direct MediaCodec-to-Surface output; do not add OpenGL ES, Vulkan, SurfaceTexture composition, or CPU readback.
- Keep the existing two-port UDP/FEC wire and validated 64Mbps recovery pacing ceiling.
- Do not replace UDP/FEC with TCP, RTP, WebRTC, QUIC, or a wire-v2 protocol.
- Drop stale captures before encode. Ordinary admitted H.264 delta pairs must not be dropped after encode.
- Allocate a shared left/right AU ID only when a pair is selected for an attempted wire transmission.
- A paired IDR may atomically replace unsent older-generation deltas because it is an independent dependency boundary.
- Keep active flow leases and the encoded queue bounded by `splitEncoderInFlightLimit(fps:)`, which is 5 at 60fps.
- Keep `splitPostEncodeDeltaDrops == 0` as a hard acceptance invariant.
- Preserve unrelated dirty work and keep all changes unstaged and uncommitted per user direction.
- Keep newly touched orchestration source files at or below 500 lines by extracting policy/state into focused files.
- After any React, React Native, JSX/TSX, style, or component behavior change, run root `npx -y react-doctor@latest . --verbose` and require exactly `100 / 100`, then rerun typecheck and relevant tests.
- Physical validation must use matched installed Host/APK binaries, the real `TB710FU`, direct Wi-Fi UDP, and the visibly moving `Option+0` workspace.

## File structure

| Path | Responsibility |
|---|---|
| `native/android-viewer/src/renderer/split_session/gap_policy.rs` | Pure split AU-gap decision; no socket or MediaCodec dependency |
| `native/android-viewer/src/renderer/split_session/tile_worker/helpers.rs` | Apply the pure decision to one tile worker |
| `native/android-viewer/src/renderer/stats.rs` | Append split gap-recovery counters to backward-compatible LCF1 feedback |
| `native/macos-capture-shim/Sources/Split/SplitFlowControlState.swift` | Capture-to-send lease accounting and transport recovery generation |
| `native/macos-capture-shim/Sources/Split/SplitPairLifecycleState.swift` | Carry one lease through dual-encoder admission/callback completion |
| `native/macos-capture-shim/Sources/Transport/PendingSplitAccessUnit.swift` | Atomic left/right Annex-B payload and codec config |
| `native/macos-capture-shim/Sources/Transport/SplitWireSequence.swift` | Send-time shared AU-ID allocation and envelope construction |
| `native/macos-capture-shim/Sources/Split/CaptureSession+Split.swift` | Split encode/packetization orchestration only |
| `native/macos-capture-shim/Sources/Transport/CaptureSession+NetworkQueue.swift` | Serial send drain and lease completion only |
| `native/macos-capture-shim/Sources/Encoder/CaptureSession+Recovery.swift` | Start one soft split transport recovery boundary |
| `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift` | Export split flow/recovery metrics |
| `crates/control-contract/src/host.rs` | Backward-compatible typed diagnostics contract |
| `apps/host-desktop/src-tauri/src/ffi.rs` | Parse shim JSON diagnostics |
| `apps/host-desktop/src/sessionTypes.ts` | TypeScript diagnostics shape |
| `apps/host-desktop/src/encoderDiagnostics.ts` | Pure diagnostic strings |
| `apps/host-desktop/src/SessionPipelineDiagnostics.tsx` | Read-only split flow/recovery inspector rows |

---

### Task 1: Make split IDR gap handling recovery-safe

**Files:**

- Create: `native/android-viewer/src/renderer/split_session/gap_policy.rs`
- Modify: `native/android-viewer/src/renderer/split_session.rs`
- Modify: `native/android-viewer/src/renderer/split_session/tile_worker/helpers.rs`
- Modify: `native/android-viewer/src/renderer/split_session/tile_worker.rs`
- Modify: `native/android-viewer/src/renderer/stats.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitGapSignal {
    None,
    Loss,
    Idr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SplitFrameGapDecision {
    pub missing: u16,
    pub feed: bool,
    pub awaiting_keyframe_after: bool,
    pub signal: SplitGapSignal,
}

pub(super) fn decide_split_frame_gap(
    previous: Option<u16>,
    current: u16,
    keyframe: bool,
    awaiting_keyframe: bool,
) -> SplitFrameGapDecision;
```

`SplitFeedbackSnapshot` gains cumulative `keyframe_gap_recoveries: u32` and `delta_gap_recoveries: u32` at LCF1 offsets 52 and 56. Existing offsets 0-51 remain unchanged.

- [ ] **Step 1: Add failing pure gap-policy tests**

Create `gap_policy.rs` with tests that refer to the not-yet-implemented decision function:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_gap_requests_one_recovery() {
        assert_eq!(
            decide_split_frame_gap(Some(10), 13, false, false),
            SplitFrameGapDecision {
                missing: 2,
                feed: false,
                awaiting_keyframe_after: true,
                signal: SplitGapSignal::Loss,
            }
        );
    }

    #[test]
    fn another_delta_while_awaiting_does_not_repeat_loss() {
        let decision = decide_split_frame_gap(Some(13), 15, false, true);
        assert_eq!(decision.signal, SplitGapSignal::None);
        assert!(!decision.feed);
    }

    #[test]
    fn idr_after_gap_completes_recovery_without_loss() {
        assert_eq!(
            decide_split_frame_gap(Some(15), 24, true, true),
            SplitFrameGapDecision {
                missing: 8,
                feed: true,
                awaiting_keyframe_after: false,
                signal: SplitGapSignal::Idr,
            }
        );
    }

    #[test]
    fn uint16_wrap_is_contiguous() {
        assert_eq!(
            decide_split_frame_gap(Some(u16::MAX), 0, false, false).missing,
            0
        );
    }
}
```

- [ ] **Step 2: Run the focused tests and prove RED**

Run:

```bash
cargo test -p android-viewer renderer::split_session::gap_policy::tests -- --nocapture
```

Expected: compilation fails because `decide_split_frame_gap`, `SplitGapSignal`, and `SplitFrameGapDecision` are missing.

- [ ] **Step 3: Implement the pure decision**

Implement the exact keyframe/awaiting/gap precedence from the spec. A keyframe always yields `Idr` and is feedable, an awaiting non-keyframe is discarded without Loss, a new delta gap yields one Loss, and a contiguous delta is feedable.

- [ ] **Step 4: Integrate the decision without double-counting Loss**

In `process_frame`, compute the decision before mutating `last_id`, update missing telemetry, emit exactly the selected signal, and return before MediaCodec when `feed == false`. Keep MediaCodec `InputUnavailable`, `InputTooLarge`, and feed errors as real Loss events after a feed attempt. Remove the extra `stats.frame_gaps.fetch_add(1, ...)` from the coordinator's `CoordinatorEvent::Loss` branch; tile workers own missing-AU counts.

- [ ] **Step 5: Extend and test backward-compatible split feedback**

Add two `AtomicU32` values to `RuntimeStats`, append both counters to `SplitFeedbackSnapshot`, reserve 60 bytes, and extend the feedback test:

```rust
assert_eq!(body.len(), 60);
assert_eq!(u32::from_be_bytes(body[52..56].try_into().unwrap()), 7);
assert_eq!(u32::from_be_bytes(body[56..60].try_into().unwrap()), 8);
```

The existing assertions for rendered FPS at offsets 32/34 and pair metrics at 36-51 must remain unchanged.

- [ ] **Step 6: Run Android renderer tests and target check**

```bash
cargo test -p android-viewer renderer:: -- --nocapture
cargo check -p android-viewer --target aarch64-linux-android
```

Expected: all renderer tests pass and the Android target check exits 0.

---

### Task 2: Add capture-to-send split flow leases

**Files:**

- Create: `native/macos-capture-shim/Sources/Split/SplitFlowControlState.swift`
- Modify: `native/macos-capture-shim/Sources/Split/SplitPairLifecycleState.swift`
- Modify: `native/macos-capture-shim/Sources/Split/DualEncoderPipeline.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/VideoToolboxTileEncoder.swift`
- Test: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`

**Interfaces:**

```swift
struct SplitFlowLease: Hashable {
    let id: UInt64
    let generation: UInt64
    let isRecoveryBoundary: Bool
}

struct SplitFlowRecoveryTransition: Equatable {
    let generation: UInt64
    let releasedLeaseCount: Int
}

struct SplitFlowControlState {
    init(capacity: Int)
    mutating func admit() -> SplitFlowLease?
    func accepts(_ lease: SplitFlowLease) -> Bool
    @discardableResult mutating func complete(_ lease: SplitFlowLease) -> Bool
    mutating func beginRecovery() -> SplitFlowRecoveryTransition
    mutating func cancelAll() -> Int
    var activeCount: Int { get }
    var capacity: Int { get }
    var recoveryBoundaryPending: Bool { get }
}
```

`SplitPairAdmission` gains `lease: SplitFlowLease`. `DualEncoderPipeline.submit(captured:lease:)` requires the caller-owned lease. Pipeline events return the lease on encoded, injected-drop, and dropped events so every terminal path can release it exactly once.

- [ ] **Step 1: Add failing lease-state tests**

Append assertions for capacity two, startup boundary lease, saturation, exactly-once completion, recovery generation increment, one admitted recovery boundary, denied admission while that boundary remains active, resumed delta admission after boundary completion, and `cancelAll()` returning active count.

- [ ] **Step 2: Compile the Swift split test and prove RED**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests-red
```

Expected: compilation fails because `SplitFlowControlState` and `SplitFlowLease` do not exist.

- [ ] **Step 3: Implement the pure lease state**

Use a `Set<SplitFlowLease>` for active ownership, a wrapping `nextID`, current `generation`, and a recovery-boundary flag initialized to true. The first lease after initialization or recovery is the boundary; deny additional leases until it completes.

- [ ] **Step 4: Carry lease identity through the encoder lifecycle**

Change lifecycle admission to accept a lease and store `[UInt64: SplitFlowLease]` keyed by pipeline sequence. Make `completePair(sequence:) -> SplitFlowLease?`, `beginPairedRecovery() -> [SplitFlowLease]`, and `cancelAll() -> [SplitFlowLease]`. Carry the lease in `TileEncodeRequest`, `TileEncodedSample`, and these events:

```swift
case encodedPair(lease: SplitFlowLease, left: TileEncodedSample, right: TileEncodedSample)
case injectedRightDrop(lease: SplitFlowLease, left: TileEncodedSample)
case dropped(reason: String, releasedLeases: [SplitFlowLease])
```

A soft external keyframe request preserves in-flight leases; internal callback/encoder failure returns every invalidated lease.

- [ ] **Step 5: Extend lifecycle tests for exact lease return**

Admit one lease, complete by sequence, prove the second completion is nil, admit two leases, hard-recover and assert both identities are returned, then prove soft `requestPairedKeyframe()` returns no lease and preserves in-flight count.

- [ ] **Step 6: Run split and policy tests**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests
/tmp/leftcar-policy-tests
```

Expected: both executables exit 0.

---

### Task 3: Build atomic encoded pairs and send-time wire identity

**Files:**

- Create: `native/macos-capture-shim/Sources/Transport/PendingSplitAccessUnit.swift`
- Create: `native/macos-capture-shim/Sources/Transport/SplitWireSequence.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/MediaWire.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+SplitTransport.swift`
- Test: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`

**Interfaces:**

```swift
struct SplitEncodedPayload {
    let config: Data?
    let annexB: Data
    let captureWallMs: UInt64
    let encodeWallMs: UInt64
}

struct PendingSplitAccessUnit {
    let sequence: UInt64
    let generation: UInt64
    let lease: SplitFlowLease
    let left: SplitEncodedPayload
    let right: SplitEncodedPayload
    let isKeyframe: Bool
    let isRecoveryKeyframe: Bool
    let queuedNs: UInt64
}

struct SplitWireSequence {
    init(next: UInt16 = 0)
    mutating func allocate() -> UInt16
}

func splitWireFrame(payload: SplitEncodedPayload, auID: UInt16) -> Data
```

- [ ] **Step 1: Add failing payload and sequence tests**

Test UInt16 wrap, exact `G + AU ID + L2` prefix, unchanged capture/encode timestamps, Annex-B suffix, and byte-identical left/right headers when given the same ID.

- [ ] **Step 2: Compile and prove RED**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests-red
```

Expected: compilation fails for missing payload, pending access-unit, and wire-sequence types.

- [ ] **Step 3: Implement payload-only packetization**

Move AVCC-to-Annex-B conversion out of the old envelope builder. Preserve codec config only for keyframes. Keep timestamps as integer fields until `splitWireFrame` constructs the 21-byte pre-fragment envelope.

- [ ] **Step 4: Implement wrapping send-time sequence**

`allocate()` returns the current value then increments with `&+= 1`. Call it exactly once after selecting a pair for send, then pass the same ID to left and right.

- [ ] **Step 5: Preserve existing fragment/FEC behavior**

Keep fragmenting, Reed-Solomon parity, interleaving, pacing, and destinations unchanged. Change split send to consume `PendingSplitAccessUnit` and return:

```swift
struct SplitPairSendResult {
    let auID: UInt16
    let succeeded: Bool
    let attemptedDatagrams: Int
}
```

No ID is allocated for a pair discarded before this function starts.

- [ ] **Step 6: Run split/policy tests and diff check**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests
/tmp/leftcar-policy-tests
git diff --check
```

Expected: all commands exit 0.

---

### Task 4: Integrate credit backpressure and one paired recovery boundary

**Files:**

- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+EncoderStartup.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+Recovery.swift`
- Modify: `native/macos-capture-shim/Sources/Split/CaptureSession+Split.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+NetworkQueue.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession+Lifecycle.swift`
- Delete after references reach zero: `native/macos-capture-shim/Sources/Transport/SplitNetworkQueuePolicy.swift`
- Test: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`
- Test: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

- `CaptureSession` owns `splitFlowState`, `[PendingSplitAccessUnit]`, and `SplitWireSequence`.
- `acquireSplitFlowLeaseLocked() -> SplitFlowLease?` is called only while holding `captureLock`.
- `finishSplitFlowLease(_:sentRecoveryBoundary:)` releases exactly one lease and schedules the latest pending capture.
- `beginSplitTransportRecovery(reason:)` coalesces repeated requests without hard-invalidating dual AVE callbacks.

- [ ] **Step 1: Replace obsolete queue-policy assertions with flow invariants**

Remove tests for `.dropAndRecover`, `.dropDuringRecovery`, and `.replaceWithKeyframe`. Assert that five admitted leases allow at most five queued/in-flight pairs, a sixth capture is denied before encode, recovery releases old leases, exactly one boundary lease is admitted, and wire sequence is unchanged until send.

- [ ] **Step 2: Run Swift tests and prove RED**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests-red
```

Expected: compile or precondition failure because production still uses `pendingSplitPairs` and queue-overflow recovery.

- [ ] **Step 3: Acquire a lease before split encode submission**

Initialize flow capacity from the clamped session FPS. In `drainEncodeQueue`, acquire before dequeue. If none exists, leave the one-slot latest pending capture, clear `encodeScheduled`, and return. Submit as `encodeSplitFrame(next, lease: lease)`.

- [ ] **Step 4: Enqueue encoded output without ordinary post-encode drop**

On encoded-pair output, complete the encoder slot, convert to `PendingSplitAccessUnit`, and append in submission order. Stale-generation output is counted as `splitRecoveryBoundaryDiscards` and consumes no wire ID. If the queue ever exceeds lease capacity, increment `splitPostEncodeDeltaDrops`, set a fatal invariant error, and stop instead of silently breaking dependencies.

- [ ] **Step 5: Release leases on every terminal path**

Successful send, send failure, packetization failure, encoder failure, callback timeout, injected loss, stale callback, and session stop each release or invalidate every affected lease exactly once. Duplicate completion is ignored and cannot decrement active count.

- [ ] **Step 6: Implement coalesced soft transport recovery**

For split sessions, clear unsent old-generation access units under `networkLock`, then release the lock. Under `captureLock`, call `splitFlowState.beginRecovery()` and schedule the latest pending capture. Finally issue a soft paired-keyframe request on `encodeQueue`. Repeated IDR requests while a current recovery boundary lease is active are suppressed without another generation change.

- [ ] **Step 7: Keep fault injection a real one-sided wire loss**

At pair 300, allocate one ID, send only left, release the lease, increment injection telemetry, and start one recovery. The next attempted pair is a paired IDR using the next contiguous ID.

- [ ] **Step 8: Remove obsolete queue policy**

```bash
rg -n "SplitPairQueueAdmission|splitPairQueueAdmission|splitPendingPairLimit|pendingSplitPairs" native/macos-capture-shim
```

Expected before deletion: no production or test references. Remove the obsolete policy file, `pendingSplitPairs`, and `splitNextAuID`; update snapshots for `pendingSplitAccessUnits`.

- [ ] **Step 9: Run Swift tests and compile the shim**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests
/tmp/leftcar-policy-tests
tools/build-macos-capture-shim.zsh library /tmp/libleftcar_capture.dylib
git diff --check
```

Expected: all commands exit 0 and the library exists.

---

### Task 5: Export flow/recovery diagnostics end to end

**Files:**

- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+ViewerControl.swift`
- Modify: `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift`
- Modify: `crates/control-contract/src/host.rs`
- Modify: `crates/control-contract/tests/contract.rs`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src-tauri/src/backend.rs`
- Modify: `apps/host-desktop/src/sessionTypes.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.test.ts`
- Modify: `apps/host-desktop/src/SessionPipelineDiagnostics.tsx`

**Interfaces:**

Add these camelCase JSON/TypeScript fields and matching snake_case Rust fields:

```text
splitFlowActiveLeases: u32
splitFlowCapacity: u32
splitPreEncodeAdmissionDrops: i64
splitEncodedQueueDepth: u32
splitEncodedQueueOldestUs: u64
splitRecoveryBoundaryDiscards: i64
splitPostEncodeDeltaDrops: i64
splitWirePairsAttempted: i64
splitWirePairSendFailures: i64
splitKeyframeGapRecoveries: i64
splitDeltaGapRecoveries: i64
```

- [ ] **Step 1: Add failing contract and TypeScript assertions**

Deserialize an old Rust stats payload and prove all new fields default to zero. Deserialize a full payload and prove values survive into `StatsInfo` and `StatusView`. In TypeScript, assert:

```ts
expect(encoderDiagnosticsView({
  ...splitSession,
  splitFlowActiveLeases: 3,
  splitFlowCapacity: 5,
  splitEncodedQueueDepth: 1,
  splitEncodedQueueOldestUs: 12_300,
  splitPreEncodeAdmissionDrops: 8,
  splitRecoveryBoundaryDiscards: 4,
  splitPostEncodeDeltaDrops: 0,
  splitWirePairsAttempted: 600,
  splitWirePairSendFailures: 0,
  splitKeyframeGapRecoveries: 1,
  splitDeltaGapRecoveries: 1,
})).toMatchObject({
  splitFlow: "lease 3/5 · queue 1 · oldest 12.3ms",
  splitRecovery: "capture 8 · boundary 4 · post-encode 0 · wire 600/0 · gap IDR 1 / delta 1",
});
```

- [ ] **Step 2: Run focused tests and prove RED**

```bash
cargo test -p control-contract split_flow -- --nocapture
bunx vitest run apps/host-desktop/src/encoderDiagnostics.test.ts
```

Expected: fields and diagnostic strings are missing.

- [ ] **Step 3: Parse appended Android feedback counters**

When `message.count >= 60`, read keyframe-gap recoveries at offset 52 and delta-gap recoveries at offset 56. Use max across tile feedback because both carry shared cumulative counters. A 52-byte older packet leaves both zero.

- [ ] **Step 4: Export shim JSON and Rust contracts**

Snapshot flow state under `captureLock` and queue depth/oldest under `networkLock` before taking `stateLock`; never nest them. Add every field to shim JSON, FFI, `StatsInfo`, `StatusView`, and platform backends with serde defaults.

- [ ] **Step 5: Render read-only Desktop diagnostics**

Add `splitFlow` and `splitRecovery` strings and two split-only rows. Use warning tone when post-encode drops or wire send failures exceed zero. Do not add settings.

- [ ] **Step 6: Run focused contract/UI gates**

```bash
cargo test -p control-contract -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
bunx vitest run apps/host-desktop/src/encoderDiagnostics.test.ts
npx -y react-doctor@latest . --verbose
bun run typecheck
```

Expected: all pass, React Doctor is exactly `100 / 100`, and typecheck exits 0.

---

### Task 6: Run complete static verification and install matched binaries

**Files:**

- Modify only when a gate exposes a source defect already in scope.
- Keep generated artifacts outside git.

**Interfaces:**

- Produces one installed signed Host whose resource shim hash matches source.
- Produces one installed Viewer APK whose pulled installed APK matches the build.

- [ ] **Step 1: Run complete repository gates**

```bash
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests
/tmp/leftcar-policy-tests
cargo test -p control-contract -p viewer-decoder -p android-viewer -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
bun run test
bun run test:contract
bun run test:architecture
npx -y react-doctor@latest . --verbose
bun run typecheck
git diff --check
```

Expected: all exit 0 and React Doctor is exactly `100 / 100`.

- [ ] **Step 2: Build Android native library and APK**

```bash
cargo build -p android-viewer --target aarch64-linux-android --release
cd apps/viewer-expo/android
./gradlew :app:testDebugUnitTest :app:compileDebugKotlin :app:compileReleaseKotlin :app:assembleDebug
```

Expected: release `libleftcar_viewer.so` and debug `app-debug.apk` exist.

- [ ] **Step 3: Build and install Host**

```bash
/bin/zsh tools/dev-host-macos.zsh
codesign --verify --deep --strict --verbose=2 "/Applications/Leftcar Host.app"
shasum -a 256 native/macos-capture-shim/libleftcar_capture.dylib \
  "/Applications/Leftcar Host.app/Contents/Resources/libleftcar_capture.dylib"
```

Expected: install/codesign pass and shim hashes match.

- [ ] **Step 4: Install APK and prove identity**

```bash
adb devices -l
adb install -r apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk
adb shell pm path leftcar.ll3.kr
```

Pull the reported base APK, compare SHA-256 with the built APK, and record native-library package hashes. Require device `HA2D6EMP`, `Success`, and matching installed/build APK hashes.

---

### Task 7: Prove 4K60, recovery, latency, and soak on TB710FU

**Files:**

- Modify: `docs/11-low-latency-investigation.md`
- Runtime artifacts: `/tmp/leftcar-split-flow-*`
- Deliver after all gates: `/Users/loopy/Downloads/Leftcar-Viewer-0.1.1-20260829-4k60-flow-control.apk`

**Interfaces:**

- Consumes matched Host/APK from Task 6.
- Produces 30-second static, 180-second moving, injected-loss, and 600-second soak receipts.

- [ ] **Step 1: Establish a valid moving-source run**

Select exact 3840x2160@60, `선명한 화면 4K 60fps`, and `4K 듀얼 인코더`. Bring the known moving source forward with `Option+0`. Capture two source screenshots at least one second apart and reject unless hashes differ and visual inspection confirms motion.

- [ ] **Step 2: Prove hardware codec topology**

After at least 120 joined frames, require both Host encoders to be exact AVE hardware and Android to name two `c2.qti.avc.decoder.low_latency` instances at 1920x2160@60.

- [ ] **Step 3: Run 30-second static and 180-second moving samples**

Save one-second Host stats, Android logs, `/proc/net/snmp` before/after, and screenshots. Pass only when:

```text
left valid encode average >= 59fps
right valid encode average >= 59fps
joined render average >= 59fps
rolling one-second p5 >= 55fps for all three
both encoder drops = 0
split post-encode delta drops = 0
pair-ready max <= 16.667ms
pending decoder output <= 1 per tile
no 0fps interval, long freeze, or termination
moving screenshots change
```

- [ ] **Step 4: Inject exactly one right-tile loss**

Use `LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER=300`. Require one suppressed right AU, one recovery request, matched paired IDR, resumed joined presentation within two joined output frames after both IDRs, and no lasting half-screen corruption. Clear the environment afterward.

- [ ] **Step 5: Run the 600-second moving soak**

Keep the visibly moving `Option+0` workspace active. Require the same FPS/p5/drop/skew limits, capture-to-render p95 <=50ms without an increasing trend, bounded leases/queue, no new `RcvbufErrors`, no freeze, and no stop.

- [ ] **Step 6: Record authoritative receipts**

Append timestamps, commands, source/installed hashes, device identity, codec names, all metrics, screenshot hashes, and rejected runs to `docs/11-low-latency-investigation.md`. Distinguish source, build, install, and physical proof.

- [ ] **Step 7: Deliver verified APK**

After all gates pass, copy the matched APK to the declared Downloads path and report SHA-256.

- [ ] **Step 8: Final completion audit**

Re-read the correction spec, this plan, and the original dual-surface plan. Point every constraint and pass requirement to current direct evidence. Missing or indirect evidence remains incomplete.

