# 1440p60/4K 인코더 처리량 개선 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with verification checkpoints.

**Goal:** 1440p와 4K에서 VideoToolbox 출력이 60fps 목표를 따라가지 못할 때 캡처/인코더 콜백/packetization이 서로 발목을 잡지 않도록 하고, HEVC·저지연 프리셋·하드웨어 인코더 선택을 실제 로그로 확인 가능하게 만든다.

**Scope:** 기존 `CF2` HEVC wire path와 Android decoder는 유지한다. 이번 변경은 macOS shim의 정책과 callback hot path에 집중한다. 1080p interactive H.264 호환성과 자동 캡처 backend는 변경하지 않는다.

**Evidence:** 1440p 물리 측정에서 Host encoder output 약 39fps, Android render 약 38fps, decoder input drop 0이었다. 4K에서도 VideoToolbox encode/output 시간이 60fps frame budget을 초과했다. 따라서 네트워크보다 Host capture/encode/callback 경계가 우선 조사 대상이다.

## Task 1: Pure encoder policy contracts

**Files:**
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`

- [ ] Add failing assertions for 1440p callback offload, 4K video HEVC-first selection, interactive H.264 selection, low-latency H.264 High profile, no forced CAVLC, and video HighSpeed preset policy.
- [ ] Run the focused Swift policy test and observe the expected red failures.
- [ ] Implement pure policy enums/helpers without hiding unsupported codec fallback.
- [ ] Run the focused Swift policy test green.

## Task 2: Apply policy to VideoToolbox setup

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`

- [ ] Keep hardware + low-latency rate-control requirements and add candidate-specific hardware encoder IDs when available.
- [ ] Use HEVC first for 4K video with H.264 fallback; keep H.264 for interactive and sub-4K video.
- [ ] Apply HighSpeed only to video profiles when the preset is advertised; retain VideoConferencing for interactive profiles.
- [ ] Prefer H.264 High under low-latency mode, remove the unconditional 4K CAVLC override, and log property status/selected candidate.

## Task 3: Remove 1440p callback backpressure

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`

- [ ] Offload encoded-sample packetization for all high-resolution streams (`>=2560x1440`) to `packetizationQueue`, not only 4K.
- [ ] Preserve tracked PTS/AU IDs, recovery gates, queue bounds, and completion of encode slots exactly once.
- [ ] Log the selected codec, encoder ID, hardware flag, and applied preset so H.264/HEVC A/B runs are attributable.

## Task 4: Build and physical matrix

- [ ] Run Swift policy tests and compile the shim/dylib.
- [ ] Run Rust decoder/media tests and relevant Android native tests; no Android code change is expected unless a regression appears.
- [ ] Rebuild/install Host, run 1440p60 and 4K profiles on the connected TB710FU, and collect Host output FPS/encode p95 plus Android render/drop stats.
- [ ] Do not claim 60fps unless fresh physical output/render evidence supports it; classify any remaining gap as encoder, callback/packetization, transport, or render limited.

## Task 5: Final verification

- [ ] Run `cargo fmt --all -- --check`, `cargo test -p viewer-decoder -p android-viewer`, and `git diff --check`.
- [ ] Run any touched Swift policy/build checks and preserve unrelated dirty worktree changes.
- [ ] If React files remain unchanged, document that the React Doctor gate is not re-triggered by this native-only change.
