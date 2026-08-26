# 4K HEVC Low-Latency Stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or superpowers:subagent-driven-development when subagents are available) to implement this plan task-by-task with verification checkpoints.

**Goal:** Make the 4K profile use a hardware HEVC low-latency path while preserving H.264 compatibility for existing profiles and older peers.

**Architecture:** The macOS shim selects HEVC first only for 4K video and falls back to H.264 if VideoToolbox cannot prepare HEVC. A new `CF2` configuration packet carries a codec id plus codec-specific parameter sets; `G` media fragmentation, FEC, IDR recovery, and bounded live-edge decoding remain unchanged. Android creates `video/hevc` or `video/avc` directly through the existing NDK decoder boundary.

**Tech Stack:** Swift VideoToolbox/CoreMedia, Rust Android NDK `AMediaCodec`, existing UDP/FEC wire path, Cargo/Vitest/Gradle.

**Spec:** `docs/superpowers/specs/2026-08-26-4k-hevc-low-latency-design.md`

## Global Constraints

- Preserve the legacy H.264 `CFG` packet and all existing `G` fragmentation fields.
- Use `CF2 | codec:u8 | repeated { nal_length:u32 BE | Annex-B NAL }` for negotiated codec configuration.
- Use HEVC only for 3840x2160-or-larger video streams; keep H.264 for interactive/1440p paths.
- Never block the capture or decoder hot path waiting for a codec slot.
- HEVC setup failure must fall back to H.264 in the same Host start attempt.
- React/RN/TSX changes require React Doctor `100 / 100` from the repository root.
- Every production behavior change starts with a failing test.

### Task 1: Add codec-independent configuration parsing tests

**Files:**
- Modify: `crates/viewer-decoder/src/lib.rs`
- Test: `crates/viewer-decoder/src/lib.rs`
- Modify: `native/android-viewer/src/media_datagram.rs`
- Test: `native/android-viewer/src/media_datagram.rs`

**Interfaces:**
- Produces `VideoCodec::{H264, Hevc}`, codec-specific NAL type helpers, and a `parse_codec_config` result containing codec plus VPS/SPS/PPS bytes.

- [x] Write failing tests for parsing `CF2` HEVC config with VPS/SPS/PPS and for legacy `CFG` H.264 SPS/PPS handling.
- [x] Run the focused red test phase and observe the missing codec API before implementation.
- [x] Implement bounded parsing with explicit length checks and reject unknown codecs or malformed NAL lengths.
- [x] Run the focused tests and the existing decoder/media tests.

### Task 2: Add generic Android H.264/HEVC decoder construction

**Files:**
- Modify: `crates/viewer-decoder/src/lib.rs`
- Test: `crates/viewer-decoder/src/lib.rs`

**Interfaces:**
- Consumes `VideoCodec`, codec parameter sets, dimensions, FPS, window, and optional named codec.
- Produces `AndroidDecoder::new_video_named(...)`; retain `new_h264` and `new_h264_named` as compatibility wrappers.

- [x] Add a failing constructor/API test that verifies MIME and parameter-set mapping through a host-testable `VideoCodec::mime()`/`parameter_set_count()` contract.
- [x] Run the focused red test phase and confirm the new API was absent.
- [x] Implement the generic constructor: H.264 uses `video/avc` and two CSD buffers; HEVC uses `video/hevc` and three CSD buffers; preserve realtime, operating-rate, priority, low-latency, and nonblocking feed behavior.
- [x] Run `cargo test -p viewer-decoder --lib` and `cargo clippy -p viewer-decoder --tests -- -D warnings`.

### Task 3: Make the macOS shim select HEVC for 4K and emit CF2

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Test: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`

**Interfaces:**
- Consumes existing `contentMode`, output dimensions, and VideoToolbox samples.
- Produces `preferredVideoCodec(width:height:contentMode:)`, HEVC-first encoder setup with H.264 fallback, and codec-aware `CF2` packets.

- [x] Add failing policy tests: 4K video prefers HEVC, 1080p video and interactive 4K prefer H.264; HEVC config requires three parameter sets.
- [x] Run the Swift red test phase and verify the expected missing-symbol failure.
- [x] Implement the policy, codec-aware VideoToolbox setup, HEVC parameter extraction, and `CF2` emission. Keep packet payload NALs Annex-B and preserve `CFG` for H.264 fallback/legacy mode.
- [x] Run the Swift policy test and compile `/tmp/leftcar_capture-hevc.dylib`.

### Task 4: Wire CF2 and codec selection into the Android receiver

**Files:**
- Modify: `native/android-viewer/src/jni.rs`
- Modify: `native/android-viewer/src/media_datagram.rs`
- Test: existing parser/recovery tests plus Task 1 tests

**Interfaces:**
- Consumes `CF2` codec configs and existing `G` AUs.
- Produces Android logs showing selected codec and the same bounded decoder/recovery behavior for H.264 and HEVC.

- [x] Cover valid `CF2` HEVC selection through the codec parser contract and physical receiver log verification.
- [x] Run the focused red test phase before adding the receiver codec path.
- [x] Store codec parameter sets in a bounded session config, create the generic decoder only after all required sets arrive, and preserve CFG as H.264-only legacy input.
- [x] Update keyframe detection for HEVC (NAL types 19/20/21) without weakening gap/IDR recovery.
- [x] Run `cargo test -p android-viewer --lib`, `cargo clippy -p android-viewer --tests -- -D warnings`, and Android native compilation.

### Task 5: Build, install, and verify the physical 4K/60 path

**Files:**
- Modify: `docs/EVIDENCE.md` only after fresh physical evidence.

- [x] Compile the Swift shim and install the Host with `zsh tools/dev-host-macos.zsh`.
- [x] Build/install the current release APK and record SHA-256 plus the connected-device identity.
- [x] Start the 4K video profile and collect more than 30 seconds of Host UI metrics and `LeftcarNative` logs.
- [ ] Verify `codec=hevc`, `actualCodec=c2.qti.hevc.decoder.low_latency`, `3840x2160`, and sustained 55~60fps without monotonically increasing capture-to-render latency.
- [x] Run `npm test`, `npm run typecheck`, React Doctor, `cargo test --workspace`, `cargo fmt --all -- --check`, and `git diff --check`.
- [x] HEVC remained below 55fps in the physical sample; record the measured Host-side bottleneck and retain no claim of 4K/60 completion.

**Physical result:** HEVC was selected successfully on Lenovo TB710FU (`CF2`, VPS 28B, SPS 40B, PPS 11B; `actualCodec=c2.qti.hevc.decoder.low_latency`) and rendered continuously for the observed long run without termination. The measured Host output was approximately 34–37 FPS with roughly 65–81ms video processing time and rising capture/recovery drops during high-motion content. The HEVC path is active, but the 55–60 FPS acceptance criterion remains open; the remaining bottleneck is the macOS 4K capture/encode path rather than codec negotiation or Android decoder creation.
