# 60 FPS 계측·1440p 기준선 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or superpowers:subagent-driven-development when available) to implement this plan task-by-task with review checkpoints.

**Goal:** 사용자가 화면 선택을 강제당하지 않는 자동 캡처 UX를 유지하면서, 캡처·인코더·전송·렌더 FPS를 분리 계측하고 1440p60을 안정적인 기준선으로 만든다.

**Architecture:** 기본 backend는 현재처럼 자동 `CgDisplayStream`을 유지한다. ScreenCaptureKit은 persistent entitlement가 있는 선택형 최적화 경로로만 남긴다. Host는 submit FPS와 output callback FPS를 분리해 보고하고, 1440p UDP recovery burst가 bitrate/pacing 때문에 자기 자신을 막지 않도록 bounded recovery pacing을 적용한다. 1440p와 4K는 같은 계측 수집 포맷으로 비교한다.

**Tech Stack:** Swift VideoToolbox/CoreGraphics, Rust control contract, TypeScript Host UI, Vitest/Cargo/Swift policy tests, Android physical device logs.

**Spec:** `docs/superpowers/specs/2026-08-26-4k-hevc-low-latency-design.md`

## Global Constraints

- 사용자가 선택하지 않아도 되는 기본 자동 캡처 흐름을 유지한다.
- ScreenCaptureKit을 기본 backend로 강제하지 않는다.
- 60fps는 16.67ms/frame 기준으로 판단하고 Host submit FPS를 rendered FPS로 표현하지 않는다.
- Android `Rendered` 로그와 Host output callback을 구분한다.
- UDP send failure 0인 표본을 네트워크 정상의 근거로 삼되, recovery burst/drop은 별도로 표시한다.
- 모든 production behavior 변경은 먼저 failing test를 추가한다.
- React/RN/TSX 변경 후 repository root에서 React Doctor `100 / 100`을 확인한다.

### Task 1: Stage-separated metrics contract

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Modify: `crates/control-contract/src/host.rs`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src/App.tsx`
- Modify: `apps/viewer-expo/src/control.ts`
- Test: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Test: existing TypeScript control tests

**Interfaces:**
- Produces `captureFps`, `encodeSubmitFps`, `encodeOutputFps`, `renderedFps` when viewer feedback is available, `encodeOutputP95Us`, `captureIntervalP95Us`, and current encoder in-flight count.
- Keeps existing `fps` and serialized fields backward-compatible while making the UI labels explicit about each stage.

- [ ] Add failing Swift/TypeScript assertions for submit-vs-output metrics and the 16.67ms frame budget.
- [ ] Run the focused tests and observe missing metrics.
- [ ] Count capture callbacks, successful VideoToolbox submissions, output callbacks, and submit failures in separate 1-second windows; record output callback interval p95 and in-flight count.
- [ ] Thread the fields through the Rust contract/FFI and show them separately in Host diagnostics.
- [ ] Run focused tests, Host tests, `npm test`, `npm run typecheck`, and React Doctor.

### Task 2: Prevent 1440p recovery pacing self-throttling

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Test: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**
- Produces a bounded recovery pacing decision that can flush a recovery IDR within the LAN recovery budget without turning ordinary delta frames into a burst.
- Keeps normal delta pacing and FEC behavior unchanged unless the receiver is in a recovery boundary.

- [ ] Add a failing policy test for recovery burst budget versus 60fps frame budget and for avoiding ordinary-delta overpacing.
- [ ] Run the focused Swift test and verify the expected missing policy symbol/failure.
- [ ] Implement a bounded recovery pacing policy based on frame size, target FPS, and a capped LAN recovery rate; preserve the 750ms recovery request gate and newest-frame policy.
- [ ] Run Swift policy tests and the Host unit tests.

### Task 3: Make the 1440p fallback profile measurable and usable

**Files:**
- Modify: `apps/viewer-expo/src/stream-profile.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`
- Test: `apps/viewer-expo/src/stream-profile.test.ts`
- Test: `apps/viewer-expo/src/stream-resolution.test.ts`

**Interfaces:**
- Provides an explicit 1440p60 fallback profile whose content mode and codec policy are visible in the start request.
- Does not change the automatic capture backend selection or silently upscale/downscale a selected profile.

- [ ] Add failing profile assertions for exact 2560x1440@60 resolution and the fallback content mode.
- [ ] Run the focused Vitest tests and verify the profile contract is missing.
- [ ] Add the profile only where it improves the 4K fallback decision; preserve the existing 1080p, video, and clarity profiles.
- [ ] Run the profile/control tests and TypeScript checks.

### Task 4: Run the reproducible performance matrix

**Files:**
- Create: `tools/perf-matrix/README.md`
- Create: `tools/perf-matrix/collect-1440-4k.sh`
- Modify: `docs/EVIDENCE.md`

**Interfaces:**
- Defines one collection procedure for 1440p H.264, 1440p HEVC when supported, and 4K HEVC using the same Host stats and Android log fields.
- Records backend, codec, dimensions, target FPS, submit/output/render FPS, p95 stage latency, drops, recovery counts, FEC, pacing, and UDP failures.

- [ ] Write the collection contract and expected log fields before adding the script.
- [ ] Add a bounded shell collector that never claims a passing result from build success alone.
- [ ] Run the collector against the physical device for 1440p and 4K, stop each session cleanly, and record results in `docs/EVIDENCE.md`.
- [ ] Classify each matrix row as encoder-limited, recovery/pacing-limited, transport-limited, or Android-render-limited.

### Task 5: Final verification

- [ ] Run `cargo fmt --all -- --check` and `git diff --check`.
- [ ] Run `cargo test --workspace`, Host tests, Clippy for touched Rust crates, Swift policy tests, Android arm64 build, release APK build, and install.
- [ ] Run `npm test`, `npm run typecheck`, and React Doctor with score `100 / 100`.
- [ ] Verify the Host is idle after each physical test and report any remaining 4K60 gap without redefining it away.
