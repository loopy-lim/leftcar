# Parsec-like Low-Latency Stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with verification checkpoints.

**Goal:** Keep the Android viewer at the live edge by bounding completed access units before MediaCodec, while preserving FEC recovery and authenticated IDR resynchronization.

**Architecture:** The existing UDP/FEC/control protocol remains in place. The receiver will explicitly treat completed access units as a live-edge queue: a short reordering window is allowed, but a batch that is already behind must collapse to the newest complete access unit and trigger the existing gap/IDR path instead of feeding every completed frame into MediaCodec. JNI telemetry will expose completed-batch pressure separately from cumulative decoder counters.

**Tech Stack:** Rust (`native/android-viewer`), Android NDK `AMediaCodec`/`ANativeWindow`, existing authenticated UDP and Reed-Solomon FEC.

**Spec:** `docs/plans/2026-08-26-recovery-fec-abr-zerocopy-design.md`

## Global Constraints

- Preserve the existing authenticated UDP wire format and FEC ordering; do not replace the transport with WebRTC or QUIC.
- Keep all receiver queues bounded; no unbounded playback queue or blocking decoder input wait.
- Do not weaken peer validation, token authentication, or IDR request gating.
- Preserve the existing FEC-before-reassembly behavior and the 250ms recovery request cooldown.
- Do not modify unrelated dirty worktree changes.
- Production behavior changes require a failing automated test before implementation code.

---

### Task 1: Add a tested live-edge completed-frame policy

**Files:**
- Modify: `native/android-viewer/src/media_datagram.rs`
- Test: `native/android-viewer/src/media_datagram.rs` unit tests

**Interfaces:**
- Consumes: owned values emitted by `CompletedFrameSequencer` or collected by the JNI receive loop.
- Produces: a small generic helper that selects the live-edge frame batch and reports whether older completed frames were discarded.

- [ ] **Step 1: Write the failing tests**

Add tests for a generic helper named `select_live_edge_frames` with this behavior:

```rust
#[test]
fn live_edge_selection_keeps_all_frames_when_batch_is_small() {
    let frames = vec![10u16, 11u16];
    let selection = select_live_edge_frames(frames);
    assert_eq!(selection.frames, [10, 11]);
    assert_eq!(selection.discarded, 0);
}

#[test]
fn live_edge_selection_collapses_a_large_batch_to_the_newest_frame() {
    let frames = (10..20).collect::<Vec<_>>();
    let selection = select_live_edge_frames(frames);
    assert_eq!(selection.frames, [19]);
    assert_eq!(selection.discarded, 9);
}
```

The helper must use an explicit constant `MAX_LIVE_EDGE_BATCH: usize = 3`. A batch at or below that limit stays ordered; a larger batch returns only its newest frame and counts every discarded frame. Make the result generic over the owned frame type so the JNI path can apply it to `FramePacket` without converting back from `ReassembledFrame`.

- [ ] **Step 2: Run the focused test and verify it fails for the intended reason**

Run:

```bash
cargo test -p android-viewer --lib live_edge_selection
```

Expected: compilation failure because `select_live_edge_frames` and its result type do not yet exist.

- [ ] **Step 3: Implement the minimal pure policy**

Add a result type with owned frames and a discard count, then implement the exact policy:

```rust
pub const MAX_LIVE_EDGE_BATCH: usize = 3;

pub struct LiveEdgeSelection<T> {
    pub frames: Vec<T>,
    pub discarded: usize,
}

pub fn select_live_edge_frames<T>(mut frames: Vec<T>) -> LiveEdgeSelection<T> {
    if frames.len() <= MAX_LIVE_EDGE_BATCH {
        return LiveEdgeSelection { frames, discarded: 0 };
    }
    let newest = frames.pop().expect("len checked above");
    LiveEdgeSelection {
        discarded: frames.len(),
        frames: vec![newest],
    }
}
```

- [ ] **Step 4: Run the focused test and the media datagram tests**

Run:

```bash
cargo test -p android-viewer --lib live_edge_selection
cargo test -p android-viewer --lib media_datagram
```

Expected: all focused tests pass.

### Task 2: Apply the policy at the receiver-to-decoder boundary

**Files:**
- Modify: `native/android-viewer/src/jni.rs:364-384,1290-1530`
- Test: `native/android-viewer/src/media_datagram.rs` live-edge policy tests from Task 1

**Interfaces:**
- Consumes: the existing stack-backed `completed_frames` batch and `select_live_edge_frames`.
- Produces: at most three completed AUs per receive iteration, or the newest AU when the batch is already behind.

- [ ] **Step 1: Reuse the failing boundary policy test**

The Task 1 tests are the regression test for this boundary policy. They fail before the live-edge selector exists and pass only when a batch larger than three is reduced to its newest element.

- [ ] **Step 2: Confirm the policy test is green before wiring it**

Run the single new test with:

```bash
cargo test -p android-viewer --lib live_edge_selection
```

- [ ] **Step 3: Apply live-edge selection before feeding MediaCodec**

Collect the completed `FramePacket`s from the stack-backed batch in receive order. If there are more than `MAX_LIVE_EDGE_BATCH`, retain only the newest packet, increment `RendererStats.output_burst_discards`, set `last_frame_id` to the selected frame only through the existing frame-gap path, and request an authenticated IDR when the selected frame is a delta. Do not clear FEC state for a normal batch collapse; clear it only through the existing peer reset or decoder recovery path.

The resulting loop must not feed all burst frames into `feed_and_render`. It must preserve the current handling of CFG, keyframes, stale hysteresis, frame-gap detection, decoder input errors, and recovery cooldown.

- [ ] **Step 4: Add explicit queue-pressure telemetry**

Extend the periodic native log with:

- `completedBatch`: number of completed AUs before live-edge selection;
- `liveEdgeBatch`: number retained for decoding;
- `outputBurst`: cumulative frames discarded by live-edge selection plus decoder output burst discards;
- `decoderInputDrops`: cumulative `AMediaCodec_dequeueInputBuffer` misses.

Keep `queued` named as a cumulative count or rename it to `decoderInputsQueued` so it cannot be mistaken for queue depth. The log must also include the largest pre-selection batch observed during the session.

- [ ] **Step 5: Run focused Rust tests and compile the native viewer**

Run:

```bash
cargo test -p android-viewer --lib
cargo clippy -p android-viewer --tests -- -D warnings
```

Expected: all tests pass and Clippy reports no warnings.

### Task 3: Retry FEC recovery after either shard arrival

**Files:**
- Modify: `native/android-viewer/src/media_datagram.rs`
- Modify: `native/android-viewer/src/jni.rs:1426-1478,1542-1578`
- Test: `native/android-viewer/src/media_datagram.rs` parity arrival-order tests

**Interfaces:**
- Consumes: parity and data shards that may arrive in either UDP order.
- Produces: a recovered fragment whenever a bounded FEC group reaches enough shards, regardless of which shard completed the threshold.

- [ ] **Step 1: Write the failing parity-first test**

Feed parity shards into a group before feeding all but one data shard. The final late data shard must trigger recovery of the missing fragment. The test must call the new arrival-and-retry methods so it fails before those methods exist.

- [ ] **Step 2: Run the focused test and verify the expected failure**

Run:

```bash
cargo test -p android-viewer --lib parity_first_arrival
```

Expected: compilation failure because the arrival-and-retry methods do not yet exist.

- [ ] **Step 3: Implement retry-on-arrival methods**

Implement `FecGroup::push_data_and_restore` and `FecGroup::push_parity_and_restore` by storing the shard and immediately calling `try_restore`. Use both methods in the JNI UDP loop, and send every restored fragment through the existing reassembler/sequencer path.

- [ ] **Step 4: Run the FEC and full viewer tests**

Run:

```bash
cargo test -p android-viewer --lib parity_first_arrival
cargo test -p android-viewer --lib
cargo clippy -p android-viewer --tests -- -D warnings
```

### Task 4: Make receiver pressure telemetry unambiguous without blocking

**Files:**
- Modify: `native/android-viewer/src/jni.rs:386-402,794-898`
- Modify: `native/android-viewer/src/media_datagram.rs`
- Test: `native/android-viewer/src/media_datagram.rs` receiver-pressure tests

**Interfaces:**
- Consumes: existing non-blocking `feed_au_status`, `dec.frames_discarded`, and live-edge selection count.
- Produces: telemetry distinguishing completed-batch collapse, decoder input-slot misses, and decoder output-burst discards; no new blocking wait.

- [ ] **Step 1: Add the failing test for the telemetry contract**

Add a platform-independent `ReceiverPressure` test that records one live-edge discard, two decoder input misses, and three decoder output discards, then asserts the three fields are exactly `1`, `2`, and `3`. The test must fail because the helper does not yet exist.

- [ ] **Step 2: Run the focused decoder test and verify it fails**

Run:

```bash
cargo test -p android-viewer --lib receiver_pressure
```

Expected: failure because the helper does not exist.

- [ ] **Step 3: Implement the minimal counter helper and wire it to existing counters**

Do not change `AMediaCodec_dequeueInputBuffer` timeout behavior. Keep `timeout_us == 0` in the live path and expose the existing `frames_discarded` counter as decoder output-burst telemetry. Rename the log field from ambiguous `queued` to `decoderInputsQueued` while preserving the cumulative value for compatibility with existing log parsers.

- [ ] **Step 4: Run the receiver tests and checks for the touched crate**

Run:

```bash
cargo test -p android-viewer --lib receiver_pressure
cargo clippy -p android-viewer --tests -- -D warnings
```

### Task 5: Validate the live-edge behavior on the Android target

**Files:**
- Modify: `docs/EVIDENCE.md` only after fresh device evidence exists

- [ ] **Step 1: Build the Android native/app release artifact**

Run `cargo build -p android-viewer --target aarch64-linux-android --release` with the repository NDK linker on `PATH`, then run `./gradlew :app:assembleRelease` from `apps/viewer-expo/android`. Record the generated APK path, size, and SHA-256. Do not treat the build as stream proof.

- [ ] **Step 2: Confirm ADB device identity and install the freshly built APK**

Run:

```bash
adb devices -l
adb install -r apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk
```

Expected: the physical TB710FU device is listed and installation returns `Success`.

- [ ] **Step 3: Reproduce a sustained stream and capture native evidence**

Collect at least 30 seconds of `LeftcarNative` logs and the Host status snapshot. Verify:

- `completedBatch` does not grow monotonically;
- `liveEdgeBatch` is at most three and collapses bursts to one;
- capture-to-render latency does not increase monotonically;
- decoder input misses and output-burst drops are separately visible;
- stream does not reach 0 FPS or terminate under the ordinary test scenario.

- [ ] **Step 4: Run the final verification suite**

Run fresh commands:

```bash
cargo test -p android-viewer --lib
cargo clippy -p android-viewer --tests -- -D warnings
PATH=/Users/loopy/Library/Android/sdk/ndk/27.1.12297006/toolchains/llvm/prebuilt/darwin-x86_64/bin:$PATH cargo build -p android-viewer --target aarch64-linux-android --release
```

If React/React Native/TSX behavior is changed, also run from the repository root:

```bash
npx -y react-doctor@latest . --verbose
```

Completion requires the real device evidence in addition to build and test output.
