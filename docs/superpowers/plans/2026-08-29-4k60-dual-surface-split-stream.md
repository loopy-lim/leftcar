# 4K60 Dual Surface Split Stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep a true 3840x2160 source while producing and displaying paired left/right H.264 tiles at sustained 60fps over direct Wi-Fi UDP with at most one frame of tile skew and no app-owned Android GPU composition pass.

**Architecture:** macOS captures one 4K NV12 frame, performs one bounded Metal plane blit into two 1920x2160 IOSurface-backed buffers, and submits them to two exact AVE H.264 hardware encoder sessions. A startup capability probe submits one frame to both AVE sessions concurrently and requires two valid callbacks; dual RTVC remains diagnostic-only because physical concurrent throughput was below 60fps. An encoded-output barrier sends both access units only when the same frame sequence is valid on both encoders; Android receives the existing wire on `viewerPort` and `viewerPort + 1`, decodes on two exact-name hardware MediaCodec workers, and schedules matched outputs to two direct SurfaceViews with the same presentation timestamp.

**Tech Stack:** Swift 6, ScreenCaptureKit, CoreVideo, Metal, VideoToolbox, Rust 2021, Android NDK MediaCodec, Kotlin/Android SurfaceView, Tauri/Rust control plane, React Native 0.86/Expo 57/Uniwind, React 19/Tailwind, Vitest, Cargo tests, Gradle, ADB.

**Spec:** `docs/superpowers/specs/2026-08-29-4k60-dual-surface-split-stream-design.md`

## Global Constraints

- Preserve actual 3840x2160 source and presentation geometry; do not silently reduce resolution or target FPS.
- Use hardware VideoToolbox and hardware MediaCodec paths only; no software fallback.
- Initial product profile is `splitVertical`: two 1920x2160 H.264 tiles.
- Initial split transport is direct Wi-Fi UDP only; existing single-stream USB/TCP/ADB behavior must remain unchanged.
- Reuse the existing `G`, `P`, `CFG`, and `CF2` wire independently on two ports; do not add a tile byte or wire v2.
- `viewerPort` is the left/base port and `viewerPort + 1` is the right port; reject base ports above 65534.
- Pair admission, encoded output, keyframe recovery, decoder lifecycle, and display all operate on a shared frame sequence/generation.
- Normal display pairs matching PTS values. An early tile waits at most one frame period; unmatched output is discarded and the last complete pair stays visible.
- Android must keep MediaCodec-to-Surface direct rendering. Do not add OpenGL ES, Vulkan, SurfaceTexture composition, or CPU frame copies.
- Metal split work is one command buffer with four plane-region blits and no CPU `waitUntilCompleted`.
- All queues and retained frame/sample buffers are bounded; latest screen state wins over backlog.
- Explicit `splitVertical` requests fail closed when any required capability or endpoint is unavailable.
- Do not advertise `splitVertical` through `auto` until every physical gate in this plan passes.
- Preserve unrelated dirty work. Per user direction, do not stage or commit changes while executing this plan.
- Split newly touched oversized Swift/Rust/Kotlin files by responsibility; target `CaptureShim.swift <= 1500`, `jni.rs <= 500`, and `StreamActivity.kt <= 500` lines.
- After any React, React Native, JSX/TSX, style, or component behavior change, run root `npx -y react-doctor@latest . --verbose` and require exactly `100 / 100`; then rerun typecheck and relevant tests.
- Physical high-motion validation uses the real moving screen selected with `Option+0`, not a static desktop or synthetic random-buffer-only result.

## Implementation Status (2026-08-29)

- 구현: Metal NV12 수직 분할, dual AVE capability gate, paired output barrier,
  dual UDP/FEC, Android dual hardware decoder/direct SurfaceView, paired recovery,
  receiver buffer tuning, tail-aligned packet interleave, desktop/Viewer 실험 선택 UI.
- 계측: capture/submit/output, split preparation, encoder callback pair, 좌/우/결합
  FPS, pair-ready p95/max, timeout/unmatched, per-port receiver loss를 분리했다.
- 구조: 변경한 capture/encoder/renderer 핵심 파일을 모두 500줄 이하로
  분리했다. `CaptureShim`은 facade/export/delegate, capture는 socket/control/
  backend/lifecycle, encoder는 setup/property/startup/input/submission/recovery/
  quality/bitrate, Android single renderer는 launcher/worker/network/frame queue/
  presentation 폴더 구조를 사용한다. 최장 변경 파일은 Android worker 494줄과
  encoder policy 483줄이다.
- 리뷰 보강: split network queue는 원자적 pair 1개로 제한하고 overflow 시
  dependency chain을 버린 뒤 paired IDR을 한 번만 요청한다. Android는 hardware,
  `1920x2160@60`, 동시 instance 2개를 확인한 exact codec name만 native에 넘기며
  split decoder의 MIME/software fallback을 금지한다. coordinator는 unmatched
  output을 타일당 1개만 보유하고 양쪽 실제 release ACK 뒤 joined FPS를 집계한다.
- 실기기: 4K moving workspace에서 Host `60/60/60`과 Android 약 60fps를
  확인했다. 160Mbps recovery burst와 과도한 decoder grace는 실패 A/B로
  폐기했으며 최종값은 recovery 64Mbps, decoder grace 4ms다.
- 남은 gate: 재부팅으로 MediaCodec 상태를 초기화한 최종 설치본의 clean
  180초 run을 기록한다. 현재 기기는 재부팅 후 `RUNNING_LOCKED`라 사용자 첫
  잠금 해제 직후 검증을 재개한다.
- 최종 설치 산출물: Host shim
  `2808f40b22deb33e05c8db669cb4c4b9cd9fd82f103aca75155a241a72e069993`,
  Android native
  `0e2c44b22deb33e05c8db669cb4c4b9cd9fd82f103aca75155a241a72e069993`,
  release APK/Downloads 전달본
  `250a21de7a70f05fec66a6b469bb66e9fcf235bb4da8ead422d2a6f5d9b2cc14`.

## Current Physical Baseline

- ADB target: `HA2D6EMP`, model `TB710FU`, Android API 36.
- Device codec catalogue: `c2.qti.avc.decoder.low_latency`, hardware accelerated, `max-concurrent-instances=16`, 3840x2160 performance point 120fps.
- Single-session 4K physical averages: `rateControl=48.66fps`, `adaptiveQp=48.77fps`, `encoderPool=41.53fps`; Android render tracks Host output and the network queue does not accumulate.
- Current branch is dirty and contains the completed Phase A experiments; all changes remain unstaged.

## File Responsibility Map

| Path | Responsibility |
| --- | --- |
| `tools/build-macos-capture-shim.zsh` | One source enumeration and framework list for library and Swift policy tests |
| `native/macos-capture-shim/Sources/CaptureShim.swift` | C ABI and session registry facade only |
| `native/macos-capture-shim/Sources/Capture/` | capture backend, lifecycle, and CaptureSession orchestration |
| `native/macos-capture-shim/Sources/Encoder/` | encoder policy, QP, callback classification, one RTVC tile session |
| `native/macos-capture-shim/Sources/Split/` | geometry, pair admission/output barrier, Metal NV12 split, dual encoder pipeline |
| `native/macos-capture-shim/Sources/Transport/` | media envelopes, socket transport, input/feedback |
| `native/macos-capture-shim/Sources/Metrics/` | rolling percentiles and stats snapshot serialization |
| `crates/control-contract/src/host.rs` | canonical split capability and per-tile stats contract |
| `apps/host-desktop/src-tauri/src/{backend,ffi,control}.rs` | split validation, FFI forwarding, two-port lifecycle, stats mapping |
| `crates/viewer-decoder/src/android/` | NDK bindings, queue/dequeue, timed output release |
| `native/android-viewer/src/renderer/` | single/split renderer lifecycle, tile workers, pair sync/recovery/stats |
| `native/android-viewer/src/jni.rs` | thin renderer-facing Android ABI facade |
| `native/android-viewer/src/jni_wrappers.rs` | JNI conversion and panic boundary |
| `apps/viewer-expo/android/.../stream/view/` | one- and two-Surface layouts without texture composition |
| `apps/viewer-expo/android/.../stream/StreamActivity.kt` | lifecycle/input orchestration only |
| `apps/viewer-expo/src/encoder-experiment.ts` | Viewer capability filtering and experiment IDs |
| `apps/viewer-expo/src/launch-stream.ts` | two-port prepare/open/rollback behavior |
| `apps/viewer-expo/app/catalog.tsx` | 4K Wi-Fi-only split selector copy |
| `apps/host-desktop/src/{sessionTypes,encoderDiagnostics,SessionInspector}.*` | read-only split metrics UI |
| `docs/11-low-latency-investigation.md` | reproducible static and physical receipts |

---

### Task 1: Establish the multi-file Swift build and behavior-preserving module split

**Files:**

- Create: `tools/build-macos-capture-shim.zsh`
- Modify: `tools/dev-host-macos.zsh`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Create: `native/macos-capture-shim/Sources/Capture/CaptureBackend.swift`
- Create: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Create: `native/macos-capture-shim/Sources/Capture/CaptureSession+Setup.swift`
- Create: `native/macos-capture-shim/Sources/Capture/CaptureSession+Capture.swift`
- Create: `native/macos-capture-shim/Sources/Capture/CaptureSession+Input.swift`
- Create: `native/macos-capture-shim/Sources/Encoder/EncoderPolicy.swift`
- Create: `native/macos-capture-shim/Sources/Encoder/AdaptiveQpController.swift`
- Create: `native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift`
- Create: `native/macos-capture-shim/Sources/Transport/MediaWire.swift`
- Create: `native/macos-capture-shim/Sources/Transport/CaptureSession+Transport.swift`
- Create: `native/macos-capture-shim/Sources/Metrics/RollingMetrics.swift`
- Create: `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift`
- Test: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**

- Consumes the current public C symbols through `leftcar_capture_start_v6` unchanged.
- Produces `tools/build-macos-capture-shim.zsh library <output>` and `policy-test <output>`; all later Swift tasks add focused files under `Sources/` without editing build commands.
- Produces internal `CaptureSession` extensions split by setup, capture, encode, transport/input, and stats while retaining the same C ABI.

- [ ] **Step 1: Add the common Swift build helper**

```zsh
#!/bin/zsh
set -euo pipefail
tool_dir=${0:A:h}
repo_root=${tool_dir:h}
shim_root="$repo_root/native/macos-capture-shim"
mode=${1:?"usage: build-macos-capture-shim.zsh <library|policy-test> <output>"}
output=${2:?"missing output path"}
typeset -a shim_sources frameworks
shim_sources=("$shim_root"/Sources/**/*.swift(N))
frameworks=(AppKit CoreGraphics CoreMedia CoreVideo Foundation IOSurface Metal ScreenCaptureKit Security VideoToolbox)
typeset -a framework_args
for framework in "${frameworks[@]}"; do framework_args+=(-framework "$framework"); done
if [[ "$mode" == library ]]; then
  /usr/bin/xcrun swiftc -O -emit-library "${shim_sources[@]}" -o "$output" "${framework_args[@]}"
elif [[ "$mode" == policy-test ]]; then
  /usr/bin/xcrun swiftc -O "${shim_sources[@]}" "$shim_root/Tests/EncodePolicyTests.swift" -o "$output" "${framework_args[@]}"
else
  print -u2 "unknown build mode: $mode"
  exit 2
fi
```

- [ ] **Step 2: Run the helper before extraction to verify source discovery**

Run:

```bash
zsh tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encode-policy-tests
/tmp/leftcar-encode-policy-tests
zsh tools/build-macos-capture-shim.zsh library /tmp/libleftcar_capture.dylib
```

Expected: both binaries build and the policy test exits 0.

- [ ] **Step 3: Move existing symbols without changing behavior**

Move complete declarations, preserving implementation text:

```text
CaptureBackend.swift:
  CaptureBackendKind, MediaTransportKind, StreamContentMode,
  NativePixelSize/NativePixelModeCandidate and display-selection helpers

EncoderPolicy.swift:
  VideoCodecKind through EncoderConfigurationReport,
  quality/bitrate/FEC/pacing/recovery pure functions

AdaptiveQpController.swift:
  AdaptiveQpController and its pressure/stability helpers

MediaWire.swift:
  PendingEncodedFrame, NetworkQueueSnapshot, packet/config/parity builders

RollingMetrics.swift:
  percentile/sample helpers, PerformanceLogTicker, stats-field builders

CaptureSession.swift:
  state, initializer, stop/deinit, and shared lock helpers

CaptureSession+Setup.swift:
  socket connection, SCK/CGDisplayStream setup, encoder setup/teardown

CaptureSession+Capture.swift:
  CaptureOutputHandler, PendingCaptureFrame, capture callbacks/admission

CaptureSession+Encode.swift:
  encoder input preparation, submit, callback, QP and recovery

CaptureSession+Transport.swift:
  packetization, network queues, UDP/TCP send, feedback parsing

CaptureSession+Input.swift:
  pointer/key parsing, permission and release handling

CaptureSession+Stats.swift:
  rate-window updates, statsJSON and diagnostic logging

CaptureShim.swift:
  globals/session registry and @_cdecl public entry points only
```

Members used across extension files become module-internal by removing only the necessary `private` modifier. Do not rename symbols, reorder lock acquisition, or alter queue labels in this step.

- [ ] **Step 4: Point development builds at the helper**

Replace the direct `swiftc` invocation in `tools/dev-host-macos.zsh` with:

```zsh
"$repo_root/tools/build-macos-capture-shim.zsh" library "$shim_output"
```

- [ ] **Step 5: Verify behavior and file-size boundaries**

Run:

```bash
zsh tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encode-policy-tests
/tmp/leftcar-encode-policy-tests
zsh tools/build-macos-capture-shim.zsh library /tmp/libleftcar_capture.dylib
nm -gU /tmp/libleftcar_capture.dylib | rg 'leftcar_capture_start_v6|leftcar_capture_stats_v2'
wc -l native/macos-capture-shim/Sources/CaptureShim.swift native/macos-capture-shim/Sources/Capture/*.swift native/macos-capture-shim/Sources/Encoder/*.swift native/macos-capture-shim/Sources/Transport/*.swift native/macos-capture-shim/Sources/Metrics/*.swift
git diff --check -- tools native/macos-capture-shim
```

Expected: policy test exits 0, both exported symbols exist, `CaptureShim.swift <= 1500`, and no newly created file exceeds 1600 lines.

---

### Task 2: Define split geometry, pair admission, and encoded-output barrier with Swift TDD

**Files:**

- Create: `native/macos-capture-shim/Sources/Split/SplitGeometry.swift`
- Create: `native/macos-capture-shim/Sources/Split/EncodedPairAssembler.swift`
- Create: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`
- Modify: `tools/build-macos-capture-shim.zsh`

**Interfaces:**

- Produces `SplitGeometry.vertical4K`, `PairAdmissionState`, `TileSide`, `SplitFrameIdentity`, `EncodedTile<Value>`, and `EncodedPairAssembler<Value>`.
- `EncodedPairAssembler.insert(_:nowNs:)` returns `.emit(left:right:)`, `.wait`, or `.drop(requestPairedKeyframe:)`; it retains at most two sequences per tile so one encoder may lead by one frame without creating an IDR loop.

- [ ] **Step 1: Add failing geometry and pair tests**

```swift
precondition(SplitGeometry.vertical4K.fullWidth == 3840)
precondition(SplitGeometry.vertical4K.left.lumaX == 0)
precondition(SplitGeometry.vertical4K.right.lumaX == 1920)
precondition(SplitGeometry.vertical4K.right.chromaX == 960)
precondition(PairAdmissionState(leftAvailable: true, rightAvailable: false).decision == .dropPair)

var barrier = EncodedPairAssembler<String>(frameBudgetNs: 16_666_667)
precondition(barrier.insert(.init(side: .left, sequence: 9, value: "L", valid: true), nowNs: 100) == .wait)
precondition(barrier.insert(.init(side: .right, sequence: 9, value: "R", valid: true), nowNs: 200) == .emit(left: "L", right: "R"))
precondition(barrier.retainedValueCount == 0)

_ = barrier.insert(.init(side: .left, sequence: 10, value: "L10", valid: true), nowNs: 1_000)
_ = barrier.insert(.init(side: .left, sequence: 11, value: "L11", valid: true), nowNs: 2_000)
precondition(barrier.retainedSequenceCount == 2)
precondition(barrier.expire(nowNs: 16_667_668) == .drop(requestPairedKeyframe: true))
```

- [ ] **Step 2: Run the split test RED**

Run:

```bash
zsh tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
```

Expected: helper rejects `split-test` or compilation fails because split types do not exist.

- [ ] **Step 3: Implement exact geometry and bounded barrier types**

```swift
enum TileSide: Int, Equatable { case left, right }

struct TilePlaneRegion: Equatable {
    let lumaX: Int
    let chromaX: Int
    let width: Int
    let height: Int
}

struct SplitGeometry: Equatable {
    let fullWidth: Int
    let fullHeight: Int
    let left: TilePlaneRegion
    let right: TilePlaneRegion
    static let vertical4K = SplitGeometry(
        fullWidth: 3840,
        fullHeight: 2160,
        left: .init(lumaX: 0, chromaX: 0, width: 1920, height: 2160),
        right: .init(lumaX: 1920, chromaX: 960, width: 1920, height: 2160)
    )
}

enum PairAdmissionDecision: Equatable { case admitPair, dropPair }
struct PairAdmissionState: Equatable {
    let leftAvailable: Bool
    let rightAvailable: Bool
    var decision: PairAdmissionDecision {
        leftAvailable && rightAvailable ? .admitPair : .dropPair
    }
}
```

Implement the generic barrier as a sequence-keyed ordered map with at most two entries per tile, one optional value per side per sequence, a first-ready timestamp, explicit invalid callback handling, and deterministic `expire(nowNs:)`. When a third unmatched sequence arrives, expire the oldest incomplete pair and request a paired keyframe.

- [ ] **Step 4: Extend the helper and run GREEN**

Add `split-test` mode using `Tests/SplitPipelineTests.swift`, then run:

```bash
zsh tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
zsh tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encode-policy-tests
/tmp/leftcar-encode-policy-tests
```

Expected: both executables exit 0.

---

### Task 3: Add Metal NV12 tile preparation and the dual RTVC encoder pipeline

**Files:**

- Create: `native/macos-capture-shim/Sources/Split/MetalNv12Splitter.swift`
- Create: `native/macos-capture-shim/Sources/Encoder/VideoToolboxTileEncoder.swift`
- Create: `native/macos-capture-shim/Sources/Split/DualEncoderPipeline.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession+Capture.swift`
- Modify: `native/macos-capture-shim/Sources/Encoder/CaptureSession+Encode.swift`
- Modify: `native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift`
- Create: `native/macos-capture-shim/Sources/Split/SplitFaultInjection.swift`
- Modify: `native/macos-capture-shim/Tests/SplitPipelineTests.swift`

**Interfaces:**

- Consumes `SplitGeometry.vertical4K` and `EncodedPairAssembler` from Task 2.
- Produces `MetalNv12Splitter.prepare(source:completion:)`, `VideoToolboxTileEncoder.submit(...)`, and `DualEncoderPipeline.submit(captured:)`.
- Produces paired valid outputs carrying the same `frameSequence`, PTS, capture timestamps, and recovery generation.
- Produces an off-by-default one-shot `SplitFaultInjection` used only for deterministic physical recovery proof.

- [ ] **Step 1: Add failing policy tests for split startup and pair recovery**

```swift
precondition(EncoderExperiment.parse("splitVertical") == .splitVertical)
precondition(encoderExperimentStartupDecision(
    requested: .splitVertical,
    width: 3840,
    height: 2160,
    fps: 60,
    mediaTransport: "udp",
    hasEncoderPixelBufferPool: true
) == .success(applied: .splitVertical))
precondition(encoderExperimentStartupDecision(
    requested: .splitVertical,
    width: 2560,
    height: 1440,
    fps: 60,
    mediaTransport: "udp",
    hasEncoderPixelBufferPool: true
) == .failure("splitVertical requires 3840x2160 at 60fps over direct UDP"))
```

- [ ] **Step 2: Run Swift tests RED**

Run the policy and split test modes. Expected: split parsing/startup assertions fail because the reserved profile is still rejected.

- [ ] **Step 3: Implement the bounded Metal splitter**

Use this public shape:

```swift
final class MetalNv12Splitter {
    struct PreparedPair {
        let left: CVPixelBuffer
        let right: CVPixelBuffer
        let preparationUs: UInt64
    }
    enum SplitError: Error { case unsupportedFormat, allocation(OSStatus), texture, command }
    func prepare(
        source: CVPixelBuffer,
        completion: @escaping (Result<PreparedPair, SplitError>) -> Void
    )
}
```

Create two IOSurface/Metal-compatible 1920x2160 `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` pools with `minimumBufferCount = pairedInFlightLimit + 1`. Create Y textures as `.r8Unorm` and CbCr textures as `.rg8Unorm`. Encode four `MTLBlitCommandEncoder.copy` calls in one command buffer and invoke completion from `addCompletedHandler`; never call `waitUntilCompleted`.

- [ ] **Step 4: Implement one tile encoder wrapper and pair barrier**

```swift
struct TileEncodeRequest {
    let side: TileSide
    let frameSequence: UInt64
    let pts: CMTime
    let duration: CMTime
    let captureNs: UInt64
    let captureWallMs: UInt64
    let recoveryGeneration: UInt64
    let forceKeyframe: Bool
    let pixelBuffer: CVPixelBuffer
}

struct TileEncodedSample {
    let side: TileSide
    let frameSequence: UInt64
    let pts: CMTime
    let sampleBuffer: CMSampleBuffer
    let isKeyframe: Bool
    let recoveryGeneration: UInt64
}
```

Each `VideoToolboxTileEncoder` creates an independent 1920x2160 H.264 RTVC session with hardware required, real-time enabled, no frame reordering, expected frame rate 60, and half the aggregate bitrate ceiling. Feed callbacks into one `EncodedPairAssembler<TileEncodedSample>` owned by `DualEncoderPipeline`; packetization occurs only on `.emit`.

Extend `PendingEncodedFrame` with `tileSide: TileSide?`; `nil` means the existing single stream. Enqueue an emitted split pair atomically, keep the pair under one network admission decision, and route each datagram through `sendToTile(_:side:)`. Interleave left/right AU fragment groups on the existing serial network queue so one large tile does not fully block its peer, while preserving one aggregate UDP pacing deadline.

- [ ] **Step 5: Route 4K split capture through the new pipeline**

In `CaptureSession.handlePixelBuffer`, preserve the existing single path for every profile except `.splitVertical`. For split, publish only the latest `PendingCaptureFrame`, require both tile in-flight slots, prepare both tile buffers, then submit both requests with one allocated sequence. On Metal error, callback timeout, encoder drop, or one-sided status error, drop the pair and schedule `forcePairedKeyframe` for both encoders.

For bitrate adaptation, compute split pressure from the maximum of left/right receiver loss, oldest age, and decoder pressure. Apply one aggregate scale decision and divide the resulting aggregate ceiling equally between both RTVC sessions in the same encode-queue transaction; never adapt one tile to a different quality step.

- [ ] **Step 6: Add split metrics and build GREEN**

Record `splitPreparationP50Us/P95Us`, pair admission drops, per-tile valid output/drop FPS, encoded pair callback p50/p95, pair timeouts/drops, and per-tile/aggregate bitrate. Add this pure diagnostic policy:

```swift
struct SplitFaultInjection {
    private(set) var remainingPairs: UInt64?
    private(set) var injectedDrops: Int64 = 0

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        remainingPairs = environment["LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER"]
            .flatMap(UInt64.init)
            .flatMap { $0 > 0 ? $0 : nil }
    }

    mutating func shouldDropRightAu() -> Bool {
        guard let remainingPairs else { return false }
        if remainingPairs > 1 {
            self.remainingPairs = remainingPairs - 1
            return false
        }
        self.remainingPairs = nil
        injectedDrops += 1
        return true
    }
}
```

Invoke it only after `EncodedPairAssembler` emits a complete pair and immediately before right-tile packetization. It must never run without the explicit environment value. Add deterministic tests for disabled, countdown, one-shot, and permanent disarm behavior. Then run:

```bash
zsh tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
zsh tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encode-policy-tests
/tmp/leftcar-encode-policy-tests
zsh tools/build-macos-capture-shim.zsh library /tmp/libleftcar_capture.dylib
```

Expected: both tests and dylib build pass.

---

### Task 4: Activate split capability, two-port Host lifecycle, and stats contract

**Files:**

- Modify: `crates/control-contract/src/host.rs`
- Modify: `crates/control-contract/tests/contract.rs`
- Modify: `apps/host-desktop/src-tauri/src/backend.rs`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src-tauri/src/windows_backend/mod.rs`
- Modify: `apps/host-desktop/src-tauri/tests/control_e2e.rs`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+Transport.swift`
- Test: focused Rust unit/contract/e2e tests

**Interfaces:**

- Produces advertised `EncoderExperiment::SplitVertical` only on the macOS shim capability path.
- Keeps `Backend::start(..., port, ..., encoder_experiment)` unchanged; Swift derives right port with checked addition.
- Produces backward-compatible zero/default values for every new split stats field on single streams and Windows.

- [ ] **Step 1: Add failing contract and validation tests**

```rust
#[test]
fn split_vertical_capability_roundtrips() {
    let info = EncoderExperimentInfo {
        id: EncoderExperiment::SplitVertical,
        label: "4K dual encoder".into(),
        hint: "Wi-Fi UDP only".into(),
        requires_reconnect: true,
    };
    assert!(serde_json::to_string(&info).unwrap().contains("splitVertical"));
}

#[test]
fn split_vertical_requires_exact_4k60_udp_and_safe_base_port() {
    assert!(validate_split_start(3840, 2160, 60, "udp", 5002).is_ok());
    assert!(validate_split_start(3840, 2160, 60, "tcp", 5002).is_err());
    assert!(validate_split_start(3840, 2160, 60, "udp", 65535).is_err());
}
```

- [ ] **Step 2: Run control tests RED**

Run:

```bash
cargo test -p control-contract split_vertical -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml split_vertical -- --nocapture
```

Expected: capability/validation functions or split stats fields are missing.

- [ ] **Step 3: Add canonical split stats fields**

Add serde-defaulted fields to both `StatsInfo` and `SessionView`:

```rust
pub split_direction: Option<String>,
pub split_preparation_p50_us: u64,
pub split_preparation_p95_us: u64,
pub split_pair_admission_drops: i64,
pub encoded_pair_callback_p50_us: u64,
pub encoded_pair_callback_p95_us: u64,
pub encoded_pair_timeouts: i64,
pub encoded_pair_drops: i64,
pub left_valid_encode_output_fps: u32,
pub right_valid_encode_output_fps: u32,
pub left_encoder_frame_drops: i64,
pub right_encoder_frame_drops: i64,
pub left_bitrate_bps: u64,
pub right_bitrate_bps: u64,
pub aggregate_bitrate_bps: u64,
pub left_receiver_loss: u64,
pub right_receiver_loss: u64,
pub left_rendered_fps: u32,
pub right_rendered_fps: u32,
pub joined_rendered_fps: u32,
pub pair_ready_delta_p95_us: u64,
pub pair_ready_delta_max_us: u64,
pub pair_sync_timeouts: i64,
pub unmatched_output_drops: i64,
pub paired_recovery_requests: i64,
pub paired_recovery_keyframes: i64,
pub split_test_injected_drops: i64,
```

- [ ] **Step 4: Validate split before transport fallback**

Implement:

```rust
fn validate_split_start(input: &StartStreamInput, concrete_transport: &str) -> Result<(), String> {
    if input.encoder_experiment != EncoderExperiment::SplitVertical { return Ok(()); }
    if (input.width, input.height, input.fps) != (3840, 2160, 60)
        || concrete_transport != "udp"
        || input.viewer_port == u16::MAX
    {
        return Err("splitVertical requires 3840x2160 at 60fps over direct UDP and two consecutive viewer ports".into());
    }
    Ok(())
}
```

For split requests build only UDP attempts; do not negotiate AOAP or fall back to TCP/ADB. Cleanup both base and base+1 prepared endpoint state when startup fails or the session stops.

In Swift, derive `rightTargetAddr` by copying `targetAddr` and replacing `sin_port` with `(targetPort + 1).bigEndian`. Keep one unconnected UDP socket, send the same unpredictable `LCH1` nonce to both tile addresses, and require an echo from both source ports before `connectSocket()` succeeds. Add `sendToTile(_:side:)`; the old `sendToViewer` delegates to `.left`. Classify `LCF1` feedback by `recvfrom` source port and update left/right receiver counters separately, while input/key events remain accepted only from the base port.

- [ ] **Step 5: Parse/forward shim capability and stats**

Allow `splitVertical` through `advertised_encoder_experiments`, Swift capability JSON, FFI start validation, and stats JSON parsing. Keep `splitHorizontal` reserved and unadvertised. Default all fixtures/Windows stats to zero/`None`.

- [ ] **Step 6: Run the Host/control suite GREEN**

```bash
cargo test -p control-contract -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
```

Expected: all tests pass and old JSON requests still default to `auto`.

---

### Task 5: Split the decoder crate and expose timed MediaCodec output control

**Files:**

- Modify: `crates/viewer-decoder/src/lib.rs`
- Create: `crates/viewer-decoder/src/annex_b.rs`
- Create: `crates/viewer-decoder/src/android/mod.rs`
- Create: `crates/viewer-decoder/src/android/ffi.rs`
- Create: `crates/viewer-decoder/src/android/decoder.rs`
- Create: `crates/viewer-decoder/src/android/output.rs`
- Test: unit tests in `crates/viewer-decoder/src/android/output.rs`

**Interfaces:**

- Preserves `AndroidDecoder::feed_au_status` for the existing single renderer.
- Produces `queue_access_unit`, `dequeue_ready_output`, `release_output_at`, and `discard_output` for split workers.
- `ReadyOutput` owns only an output index and PTS; the decoder object remains on one worker thread.

- [ ] **Step 1: Add failing pure output policy tests**

```rust
#[test]
fn output_burst_keeps_only_newest_bounded_entries() {
    let decision = select_ready_outputs(&[1, 2, 3, 4], 1);
    assert_eq!(decision.discard, vec![1, 2, 3]);
    assert_eq!(decision.keep, vec![4]);
}

#[test]
fn timed_release_uses_exact_coordinator_timestamp() {
    assert_eq!(release_timestamp_ns(7_000_000), 7_000_000);
}
```

- [ ] **Step 2: Move current declarations into focused modules**

Move NDK extern declarations and constants to `android/ffi.rs`, Annex-B parsing to `annex_b.rs`, decoder create/configure/input methods to `android/decoder.rs`, and output drain/release methods to `android/output.rs`. Re-export the current public types from `lib.rs` so downstream imports continue to compile.

- [ ] **Step 3: Add NDK timed-release binding and output methods**

```rust
extern "C" {
    fn AMediaCodec_releaseOutputBufferAtTime(
        codec: *mut AMediaCodec,
        idx: usize,
        timestamp_ns: i64,
    ) -> media_status_t;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyOutput { pub index: usize, pub pts_us: i64 }

impl AndroidDecoder {
    pub fn queue_access_unit(&mut self, au: &[u8], pts_us: i64, timeout_us: i64) -> Result<FeedStatus, DecoderError>;
    pub fn dequeue_ready_output(&mut self, timeout_us: i64) -> Result<Option<ReadyOutput>, DecoderError>;
    pub fn release_output_at(&mut self, output: ReadyOutput, timestamp_ns: i64) -> Result<(), DecoderError>;
    pub fn discard_output(&mut self, output: ReadyOutput) -> Result<(), DecoderError>;
}
```

Do not call `pump_latest_output` from `queue_access_unit`; keep the existing combined method as a compatibility wrapper for single streams.

- [ ] **Step 4: Run decoder tests and Android cross-check GREEN**

```bash
cargo test -p viewer-decoder -- --nocapture
cargo check -p android-viewer --target aarch64-linux-android
wc -l crates/viewer-decoder/src/lib.rs crates/viewer-decoder/src/android/*.rs
```

Expected: tests/check pass, `lib.rs` is a facade, and no new decoder file exceeds 800 lines.

---

### Task 6: Build the host-testable Android pair presentation and recovery state machines

**Files:**

- Create: `native/android-viewer/src/renderer/mod.rs`
- Create: `native/android-viewer/src/renderer/presentation_sync.rs`
- Create: `native/android-viewer/src/renderer/recovery.rs`
- Create: `native/android-viewer/src/renderer/stats.rs`
- Modify: `native/android-viewer/src/lib.rs`
- Test: inline unit tests in the new renderer modules

**Interfaces:**

- Produces `PairPresentationCoordinator::push_ready(side, ReadyFrame, now_ns)` and `expire(now_ns)`.
- Returns worker commands with identical `target_present_ns` for complete pairs.
- Produces `PairedRecoveryGate` that coalesces either tile's loss into one Host request and clears only after both IDRs of the same generation.

- [ ] **Step 1: Add failing presentation tests**

```rust
#[test]
fn matching_pts_release_both_at_the_same_time() {
    let mut sync = PairPresentationCoordinator::new(60);
    assert_eq!(sync.push_ready(TileSide::Left, ready(7), 1_000), SyncDecision::Wait);
    let decision = sync.push_ready(TileSide::Right, ready(7), 1_200);
    let SyncDecision::Present { left, right, target_present_ns } = decision else { panic!() };
    assert_eq!(left.pts_us, right.pts_us);
    assert!(target_present_ns > 1_200);
}

#[test]
fn unmatched_frame_expires_after_one_period() {
    let mut sync = PairPresentationCoordinator::new(60);
    sync.push_ready(TileSide::Left, ready(8), 1_000);
    assert!(matches!(sync.expire(16_667_668), SyncDecision::Discard { .. }));
}
```

- [ ] **Step 2: Implement the bounded coordinator**

Use `TileSide`, `ReadyFrame { output: ReadyOutput, pts_us }`, `WorkerCommand::{PresentAt,Discard}`, and `SyncDecision::{Wait,Present,Discard}`. Keep one pending output per side, discard lower PTS when sides diverge, and derive `target_present_ns` by rounding `now + 1ms` up to the next frame-period boundary.

- [ ] **Step 3: Add failing paired recovery tests and implementation**

```rust
#[test]
fn either_tile_loss_requests_one_paired_idr() {
    let mut gate = PairedRecoveryGate::default();
    assert_eq!(gate.on_loss(TileSide::Right, 1_000), RecoveryAction::RequestPair);
    assert_eq!(gate.on_loss(TileSide::Left, 1_100), RecoveryAction::Suppress);
    assert_eq!(gate.on_idr(TileSide::Left, 3), RecoveryAction::WaitForPeer);
    assert_eq!(gate.on_idr(TileSide::Right, 3), RecoveryAction::ResumePair);
}
```

Implement a 750ms request cooldown and generation-matched two-IDR clear.

- [ ] **Step 4: Run host tests GREEN**

```bash
cargo test -p android-viewer renderer:: -- --nocapture
cargo fmt --all -- --check
```

Expected: deterministic sync/recovery tests pass on macOS without Android hardware.

---

### Task 7: Modularize Android JNI and implement two-port/two-decoder workers

**Files:**

- Modify: `native/android-viewer/src/jni.rs`
- Modify: `native/android-viewer/src/jni_wrappers.rs`
- Create: `native/android-viewer/src/renderer/session.rs`
- Create: `native/android-viewer/src/renderer/network.rs`
- Create: `native/android-viewer/src/renderer/tile_worker.rs`
- Modify: `native/android-viewer/src/prepared_udp.rs`
- Modify: `native/android-viewer/src/media_datagram.rs`
- Modify: `native/android-viewer/src/input_protocol.rs`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt`
- Test: Rust unit tests and Android cross-build

**Interfaces:**

- Produces JNI `prepareSplitStream(basePort, host, mediaTransport)` and `attachSplitSurfaces(state, instanceId, leftSurface, rightSurface, basePort, host, width, height, fps)`.
- Produces one logical `SplitRendererSession` with two tile workers and one presentation coordinator.
- Keeps every existing JNI symbol and single renderer behavior unchanged.

- [ ] **Step 1: Add failing two-port preparation tests**

```rust
#[test]
fn split_ports_are_consecutive_and_bounded() {
    assert_eq!(split_ports(5002), Ok((5002, 5003)));
    assert!(split_ports(u16::MAX).is_err());
}

#[test]
fn rollback_releases_both_prepared_receivers() {
    let mut prepared = FakePrepared::new();
    assert!(prepare_split(&mut prepared, 5002).is_ok());
    rollback_split(&mut prepared, 5002);
    assert_eq!(prepared.bound_ports(), Vec::<u16>::new());
}
```

- [ ] **Step 2: Extract the existing live renderer without behavior changes**

Move `RendererControl`, frame queue/stats, socket batching, live loop, stop/suspend/reclaim, and feedback helpers from `jni.rs` into `renderer/session.rs`, `network.rs`, `tile_worker.rs`, and `stats.rs`. Expose only `pub(super)` functions needed by `jni.rs`; keep `jni.rs` as input validation plus calls into `renderer`.

- [ ] **Step 3: Implement split preparation and JNI wrappers**

Kotlin declarations:

```kotlin
external fun prepareSplitStream(basePort: Int, host: String, mediaTransport: String): Int
external fun attachSplitSurfaces(
    state: Long,
    instanceId: String,
    leftSurface: Surface,
    rightSurface: Surface,
    basePort: Int,
    host: String,
    width: Int,
    height: Int,
    fps: Int,
): Int
```

The JNI wrapper acquires both `ANativeWindow` values before spawning, releases both on every error path, and passes ownership to one logical session only after both decoders configure successfully.

- [ ] **Step 4: Implement tile workers and coordinator commands**

Each worker owns one prepared UDP receiver, `FrameReassembler`, `CompletedFrameSequencer`, hardware `AndroidDecoder`, and command channel. Queue input PTS from the expanded AU sequence, not a local count. Send `ReadyFrame` to the coordinator; process `PresentAt` and `Discard` commands on the same worker thread that owns MediaCodec.

- [ ] **Step 5: Couple loss and lifecycle across both tiles**

Right-tile loss sends one pair IDR request through the base control endpoint. Stop, Surface destruction, resize suspension, fatal decoder error, Host termination, and reconnect operate on both workers. One worker may not remain active after its peer exits.

Extend the authenticated `LCF1` body after the existing 34-byte core with:

```rust
pub joined_rendered_fps: u16,
pub pair_ready_delta_p95_us: u32,
pub pair_ready_delta_max_us: u32,
pub pair_sync_timeouts: u32,
pub unmatched_output_drops: u32,
```

Base-port `rendered_fps` is left tile FPS, right-port `rendered_fps` is right tile FPS, and both reports may carry the same joined coordinator fields. Older Host parsing remains valid because the existing core offsets do not move. Add byte-offset/authentication tests for both the 34-byte legacy body and the extended body. Task 4's Swift parser records source-port-specific tile FPS and accepts joined fields only from the base endpoint.

- [ ] **Step 6: Build and enforce module size**

```bash
cargo test -p android-viewer -- --nocapture
cargo check -p android-viewer --target aarch64-linux-android
wc -l native/android-viewer/src/jni.rs native/android-viewer/src/renderer/*.rs
```

Expected: tests/check pass, `jni.rs <= 500`, and no renderer module exceeds 800 lines.

---

### Task 8: Split Kotlin stream UI and attach two direct SurfaceViews

**Files:**

- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt`
- Modify: `apps/viewer-expo/android/app/build.gradle`
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamIntent.kt`
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/view/AspectRatioSurfaceView.kt`
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/view/SplitStreamLayout.kt`
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/view/StreamHud.kt`
- Create: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/view/SplitGeometryTest.kt`
- Test: JVM/Kotlin compile and existing TypeScript launcher tests

**Interfaces:**

- Consumes `encoderExperiment` Activity extra.
- Produces `SplitStreamLayout` with exact integer left/right content rectangles and one callback after both Surfaces are valid.
- Produces `StreamLauncher.getSplitDecoderCapability()` from Android `MediaCodecList`, including hardware AVC max instances and 1920x2160@60 support.
- Single streams still use one `AspectRatioSurfaceView` and the existing native attach method.

- [ ] **Step 1: Add pure layout tests or deterministic assertions**

```kotlin
@Test
fun exactSplitUsesOneSharedContentRect() {
    assertEquals(
        SplitRects(IntRect(0, 80, 1280, 1520), IntRect(1280, 80, 2560, 1520)),
        splitContentRect(2560, 1600, 3840, 2160),
    )
    assertEquals(1920, mapPointerToSourceX(1280f, IntRect(0, 80, 2560, 1520), 3840))
}
```

Add `testImplementation("junit:junit:4.13.2")` and keep the geometry helpers Android-class-free (`IntRect`) so the local JVM test does not require Robolectric.

- [ ] **Step 2: Extract existing UI helpers**

Move `AspectRatioSurfaceView`, intent parsing, and HUD construction out of `StreamActivity.kt` without changing the single-stream layout or input status behavior.

- [ ] **Step 3: Implement exact split layout**

`SplitStreamLayout.onLayout` computes one aspect-fit content rectangle for 3840x2160, uses `mid = content.left + content.width()/2`, lays out left `[left,mid)` and right `[mid,right)`, sets both holders opaque/from-layout, and never scales each tile independently.

- [ ] **Step 4: Attach only when both Surfaces stabilize**

For `encoderExperiment == "splitVertical"`, debounce geometry once, verify both holders are valid, then call `attachSplitSurfaces`. A destroy callback cancels pending attach and detaches the logical pair. For every other experiment retain `attachSurfacePort`.

- [ ] **Step 5: Forward the selected experiment into the Activity**

Add `encoderExperiment: String` to `StreamLauncherModule.openStream`, put it in the Intent, preserve it across `savedInstanceState` and `onNewIntent`, and validate only the known values. Split preparation calls `prepareSplitStream`; rollback cancels both base ports.

Add this React Native capability method in `StreamLauncherModule`:

```kotlin
@ReactMethod
fun getSplitDecoderCapability(promise: Promise) {
    val candidates = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
        .filter { !it.isEncoder && it.isHardwareAccelerated &&
            it.supportedTypes.any { type -> type.equals("video/avc", ignoreCase = true) } }
    val supported = candidates.mapNotNull { info ->
        runCatching {
            val caps = info.getCapabilitiesForType("video/avc")
            val video = caps.videoCapabilities
            Triple(info.name, caps.maxSupportedInstances, video.areSizeAndRateSupported(1920, 2160, 60.0))
        }.getOrNull()
    }.filter { it.third }
    val best = supported.maxByOrNull { it.second }
    promise.resolve(Arguments.createMap().apply {
        putBoolean("supported", best != null && best.second >= 2)
        putInt("maxConcurrentDecodersHint", best?.second ?: 0)
        putString("codecName", best?.first)
    })
}
```

- [ ] **Step 6: Compile and enforce file size**

```bash
cd apps/viewer-expo/android && ./gradlew :app:testDebugUnitTest :app:compileDebugKotlin
wc -l app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt app/src/main/java/dev/leftcar/viewer/stream/view/*.kt
```

Expected: Kotlin compiles, `StreamActivity.kt <= 500`, and no new view file exceeds 500 lines.

---

### Task 9: Expose the split selector and diagnostics end to end

**Files:**

- Modify: `apps/viewer-expo/src/encoder-experiment.ts`
- Modify: `apps/viewer-expo/src/encoder-experiment.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`
- Modify: `apps/host-desktop/src/sessionTypes.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.ts`
- Modify: `apps/host-desktop/src/encoderDiagnostics.test.ts`
- Modify: `apps/host-desktop/src/SessionInspector.tsx`

**Interfaces:**

- Produces TypeScript `EncoderExperimentId` including `splitVertical` but not `splitHorizontal`.
- Produces `SplitDecoderCapability { supported, maxConcurrentDecodersHint, codecName }` and hides/rejects split when the local hardware hint is below two.
- Split prepare/open/rollback always covers base and base+1 before Host capture starts.
- Desktop shows per-tile encoder FPS, joined FPS, split preparation p95, callback skew, sync timeout/drop, and paired recovery counters.

- [ ] **Step 1: Add failing Viewer normalization and launcher tests**

```ts
expect(normalizeEncoderExperiments([{ id: "splitVertical", label: "Dual", hint: "Wi-Fi", requiresReconnect: true }]))
  .toEqual([{ id: "splitVertical", label: "Dual", hint: "Wi-Fi", requiresReconnect: true }]);

await startPreparedStream({ ...input, args: { ...args, encoderExperiment: "splitVertical", mediaTransport: "udp" } });
expect(launcher.prepareStream).toHaveBeenNthCalledWith(1, 5002, host, "udp");
expect(launcher.prepareStream).toHaveBeenNthCalledWith(2, 5003, host, "udp");
expect(launcher.openStream).toHaveBeenCalledWith(5002, host, 3840, 2160, 60, "splitVertical");
```

Add a rejection test where `getSplitDecoderCapability()` resolves to `{ supported:false, maxConcurrentDecodersHint:1, codecName:null }`; assert no port is prepared and the Host request is never sent.

- [ ] **Step 2: Run focused tests RED**

```bash
bunx vitest run apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts
bunx vitest run apps/host-desktop/src/encoderDiagnostics.test.ts
```

Expected: the split ID is filtered and launcher/diagnostics shapes are incomplete.

- [ ] **Step 3: Implement two-port rollback-safe launch**

Extend `StreamLauncher` with `getSplitDecoderCapability` and `openStream(..., encoderExperiment)`. Query capability before a split prepare and fail with `이 기기는 4K 이중 하드웨어 디코더를 지원하지 않습니다.` when unsupported. For supported split, prepare base then base+1; if second prepare, Host start, or Activity open fails, cancel every prepared port and stop any created Host session. For single profiles, preserve exactly one prepare/cancel call.

- [ ] **Step 4: Add 4K Wi-Fi-only selector copy**

Load `getSplitDecoderCapability()` once while the catalog is active. Show `splitVertical` only when it reports `supported=true`, actual target is 3840x2160, and selected transport resolves to direct UDP. Label it `4K 이중 인코더` with hint `Wi-Fi 전용 · GPU 추가 합성 없음 · 재연결 필요`. Preserve the selection across normal reconnect; if transport changes away from UDP, resolve to `auto` and explain why before connection.

- [ ] **Step 5: Add Desktop pure diagnostics and UI rows**

Extend `sessionTypes.ts`, map each split stat in `encoderDiagnostics.ts`, and render compact rows in `SessionInspector.tsx`. Use existing Tailwind/`clsx`/`tailwind-merge`; do not create an interactive Host-side setting.

- [ ] **Step 6: Run React and TypeScript quality gates**

```bash
bunx vitest run apps/viewer-expo/src/encoder-experiment.test.ts apps/viewer-expo/src/launch-stream.test.ts apps/host-desktop/src/encoderDiagnostics.test.ts
npx -y react-doctor@latest . --verbose
bun run typecheck
```

Expected: all tests pass, React Doctor reports exactly `100 / 100`, and repository typecheck exits 0.

---

### Task 10: Run complete static verification and install matched Host/Viewer builds

**Files:**

- Modify only if verification exposes defects in files owned by Tasks 1-9.
- Update: `docs/11-low-latency-investigation.md` with build hashes and device capability receipt.

**Interfaces:**

- Produces a signed Host with the freshly built multi-file shim and an APK containing the freshly cross-compiled `libleftcar_viewer.so`.
- Produces current hashes so runtime evidence cannot accidentally come from stale binaries.

- [ ] **Step 1: Run all repository/static gates**

```bash
zsh tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-encode-policy-tests
/tmp/leftcar-encode-policy-tests
zsh tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
cargo test -p control-contract -p viewer-decoder -p android-viewer -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
cargo fmt --all -- --check
bun run test
npx -y react-doctor@latest . --verbose
bun run typecheck
git diff --check
```

Expected: every command exits 0 and React Doctor is `100 / 100`.

- [ ] **Step 2: Cross-build the Android native library and APK**

```bash
PATH=/Users/loopy/Library/Android/sdk/ndk/27.1.12297006/toolchains/llvm/prebuilt/darwin-x86_64/bin:$PATH \
  cargo build -p android-viewer --target aarch64-linux-android --release
cd apps/viewer-expo/android && ./gradlew :app:assembleDebug
```

Expected: `target/aarch64-linux-android/release/libleftcar_viewer.so` and `apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk` exist.

- [ ] **Step 3: Build/install the signed Host and install Viewer over ADB**

```bash
bun run dev:host:macos
adb install -r apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am force-stop leftcar.ll3.kr
adb shell monkey -p leftcar.ll3.kr -c android.intent.category.LAUNCHER 1
```

Expected: Host launches from `/Applications/Leftcar Host.app`, APK installation reports `Success`, and Viewer starts on `HA2D6EMP`.

- [ ] **Step 4: Verify binary identity and hardware codec inventory**

```bash
shasum -a 256 native/macos-capture-shim/libleftcar_capture.dylib \
  "/Applications/Leftcar Host.app/Contents/Resources/libleftcar_capture.dylib" \
  target/aarch64-linux-android/release/libleftcar_viewer.so \
  apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk
adb shell dumpsys media.player | sed -n '790,885p'
```

Expected: source and installed Host shim hashes match; device output identifies the hardware low-latency AVC decoder and at least two concurrent instances.

---

### Task 11: Complete physical 4K60 throughput, sync, recovery, and soak gates

**Files:**

- Modify: `docs/11-low-latency-investigation.md`
- Create as runtime artifacts under `/tmp`: timestamped Host logs, Android logcat, stats snapshots, and screenshots only; do not add generated logs to git.

**Interfaces:**

- Consumes installed matched binaries from Task 10 and the real `TB710FU` device.
- Produces authoritative physical receipts for capability, 180-second high motion, loss/recovery, latency/resource, and 600-second soak gates.

- [ ] **Step 1: Prove two hardware encoders and two hardware decoders are active**

Start explicit `splitVertical` at 3840x2160/60 over UDP, then collect:

```bash
adb logcat -c
adb logcat -d -v threadtime LeftcarNative:I LeftcarStream:I '*:S'
```

Run the second command after the stream has produced at least 120 joined frames. Verify Host stats name two RTVC hardware sessions at 1920x2160 and Android logs name two `c2.qti.avc.decoder.low_latency` instances. If either side creates only one or software codec, stop and fix before throughput testing.

- [ ] **Step 2: Run static 30-second and high-motion 180-second capture**

Confirm the stream HUD/source reports 3840x2160, then press `Option+0` to move the Mac to the known moving-screen workspace. Record cumulative deltas, not one instantaneous HUD sample.

Pass requirements:

```text
leftValidEncodeOutputFps average >= 59
rightValidEncodeOutputFps average >= 59
joinedRenderedFps average >= 59
rolling one-second p5 >= 55 on both encoder outputs and joined rendering
encoder drops = 0 in both tiles
splitPreparationP95Us <= 2500
pairReadyDeltaMaxUs <= 16667
pending output <= 1 per tile
no 0fps interval or stream termination
```

- [ ] **Step 3: Verify seam and one-frame presentation bound visually and numerically**

Move high-contrast windows and video across the 1920px source boundary. Capture Android screenshots and SurfaceFlinger/frame timeline samples. Verify no black center line, overlap, independent aspect fit, left/right reversal, or persistent half-screen tear. Correlate screenshots with `pairReadyDeltaP95Us`, `pairReadyDeltaMaxUs`, `pairSyncTimeouts`, and `unmatchedOutputDrops`.

- [ ] **Step 4: Exercise paired recovery**

Task 3 adds an off-by-default diagnostic policy `LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER=<positive frame count>`. Set it to `300` in the Host launch environment for one run; after 300 complete encoded pairs the transport suppresses exactly one right-tile AU, increments `splitTestInjectedDrops`, then permanently disarms for that session. Verify exactly one paired recovery request, both encoders emit the same-generation IDR, both decoders resume within two joined output frames, and neither half remains corrupt. Clear the environment value and restart the Host before all soak/final runs.

- [ ] **Step 5: Run the 600-second high-motion soak**

Keep the `Option+0` moving workspace active for 600 seconds. Pass only if encoder/render averages and p5 remain within the Task 11 Step 2 limits, capture-to-render p95 stays <=50ms without an increasing trend, queues remain bounded, tile skew remains <=16.7ms, and no stream stop or long freeze occurs.

- [ ] **Step 6: Record final receipts and produce a reusable APK artifact**

Append exact commands, timestamps, source/installed hashes, device serial/model/API, codec names, per-gate metrics, and any rejected attempt to `docs/11-low-latency-investigation.md`. Copy the verified debug APK to a timestamped filename in the user's Downloads directory only after asking for filesystem approval if required; report its SHA-256.

- [ ] **Step 7: Final completion audit**

Re-read the design spec and this plan. For every global constraint and Task 11 pass requirement, point to a test output, runtime metric, log, screenshot, hash, or source inspection. Treat missing evidence as incomplete and continue fixing/testing. Do not claim 4K60 completion from Host FPS alone.
