# UDP Video, Android Input, and USB/Wi-Fi Switching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task with verification checkpoints.

**Goal:** Make Wi-Fi UDP the default video path, keep video recovery loss-aware, expose Android touch/mouse/keyboard control end to end, and switch an active stream between USB AOAP and Wi-Fi without re-pairing.

**Architecture:** `auto` resolves to USB when an authenticated AOAP accessory is attached and to Wi-Fi UDP otherwise; TCP remains an explicit diagnostic/emergency transport only. Video continues through the existing authenticated UDP/FEC/ABR path, while input uses the existing coalesced pointer plus ordered ACK/retry protocol on the same transport-specific reverse channel. The React Native catalog observes USB state changes and performs a bounded stream restart with the existing session/token, releasing pressed input before each transition and requesting a fresh IDR after it.

**Tech Stack:** Rust (`native/android-viewer`, `apps/host-desktop/src-tauri`, `crates/fec-core`), Swift (`native/macos-capture-shim`), Kotlin/Android USB and stream activities, React Native/TypeScript, Vitest, Cargo tests, Android release APK.

**Spec:** `docs/plans/2026-08-26-recovery-fec-abr-zerocopy-design.md` and `docs/plans/2026-08-26-usb-aoap-transport-design.md`, plus the approved 2026-08-26 chat addendum: UDP is the Wi-Fi media default; Android remote input and USB/Wi-Fi automatic switching are required.

## Global Constraints

- Wi-Fi `auto` and `wifi` use UDP; TCP stays available only for explicit diagnostics or emergency fallback.
- Preserve authenticated peer validation, session tokens, host-side input opt-in, and fail-safe `ReleaseAll` behavior.
- USB AOAP negotiation is required before selecting USB; an ordinary ADB USB device is not treated as AOAP.
- All receiver and switching queues remain bounded; no blocking decoder input wait or unbounded playback queue.
- Every production behavior change begins with a failing automated test.
- Do not modify unrelated dirty worktree changes, reset files, or claim a real-device result from build/unit-test output.
- React/RN/TSX behavior changes require `npx -y react-doctor@latest . --verbose` with `100 / 100` from the repository root.

---

### Task 1: Restore UDP as the Wi-Fi default

**Files:**
- Modify: `apps/viewer-expo/src/usb.ts`
- Modify: `apps/viewer-expo/src/usb.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`

**Interfaces:**
- Consumes: `UsbAccessoryState` from `getUsbState()` and the existing `startPreparedStream` transport argument.
- Produces: `resolveTransport({ attached: false }, "auto") === "udp"`; attached AOAP still resolves to `usb`; explicit `tcp` and `adbTcp` remain unchanged.

- [ ] **Step 1: Write the failing transport-default tests**

Change the existing expectations so the default no-USB path is UDP and the auto launch test expects UDP. Add an explicit test that `resolveTransport({ attached: false }, "tcp")` remains TCP.

- [ ] **Step 2: Run the focused tests and verify the expected failure**

Run:

```bash
npm test -- --run apps/viewer-expo/src/usb.test.ts apps/viewer-expo/src/launch-stream.test.ts
```

Expected: the no-USB auto assertions fail because the current implementation returns TCP.

- [ ] **Step 3: Implement the minimal transport policy**

Update `resolveTransport` so its final no-USB auto result is `"udp"`; retain explicit TCP handling and USB precedence. Restore both catalog start paths to `mediaTransport: "auto"` so the existing USB probe/fallback flow is used for initial launches and health restarts.

- [ ] **Step 4: Run the focused tests and typecheck**

Run:

```bash
npm test -- --run apps/viewer-expo/src/usb.test.ts apps/viewer-expo/src/launch-stream.test.ts
npm run typecheck
```

Expected: focused tests and typecheck pass.

### Task 2: Make USB/Wi-Fi switching a stream-level state machine

**Files:**
- Create: `apps/viewer-expo/src/transport-switch.ts`
- Test: `apps/viewer-expo/src/transport-switch.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`

**Interfaces:**
- Consumes: `UsbAccessoryState`, `resolveTransport`, `startPreparedStream`, `requestWithReconnect`, and `ActiveStream` metadata.
- Produces: `StartedStream.mediaTransport`, `shouldSwitchTransport(current, usbState)`, and a single-flight transport switch that preserves the paired control token.

- [ ] **Step 1: Write failing pure switching tests**

Add tests for:

```ts
expect(shouldSwitchTransport("udp", { attached: false })).toBe(false);
expect(shouldSwitchTransport("udp", { attached: true })).toBe(true);
expect(shouldSwitchTransport("usb", { attached: true })).toBe(false);
expect(shouldSwitchTransport("usb", { attached: false })).toBe(true);
expect(shouldSwitchTransport("tcp", { attached: false })).toBe(false);
```

The helper must treat only `usb` and `udp` as automatic media routes; explicit diagnostic TCP is not silently rewritten.

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
npm test -- --run apps/viewer-expo/src/transport-switch.test.ts
```

Expected: module/function-not-found failure before implementation.

- [ ] **Step 3: Implement the pure switch decision and report the selected route**

Export `shouldSwitchTransport` and update `StartedStream` to include the resolved `mediaTransport`. Return the resolved route from `startPreparedStream` after `resolveTransport`; the caller stores the actual route rather than the requested `auto` label.

- [ ] **Step 4: Run the focused tests and the existing stream tests**

Run:

```bash
npm test -- --run apps/viewer-expo/src/transport-switch.test.ts apps/viewer-expo/src/launch-stream.test.ts
```

Expected: all switch and launch tests pass.

- [ ] **Step 5: Wire one debounced, single-flight USB listener into the catalog**

Subscribe to `subscribeUsbState` while the catalog is mounted. When the stable state resolves to a different automatic route, invoke the existing restore function for each active stream, guarded by a per-session in-flight set and a 1,000ms debounce. The restore path must:

```ts
await requestWithReconnect("stopStream", { session: active.session }).catch(() => undefined);
return startPreparedStream({
  control: controlClient() ?? (await reconnectHost()),
  request: requestWithReconnect,
  launcher,
  host: mediaHost,
  args: { ...active, mediaTransport: "auto" },
});
```

On success replace the session id, actual route, viewer IPs, and start time. On failure leave the existing stream metadata intact and surface a non-terminal error; do not clear pairing. Before stop/restart, enqueue `ReleaseAll` through the native stream shutdown path already used by `StreamActivity.onDestroy`.

- [ ] **Step 6: Run TypeScript checks and React Doctor**

Run:

```bash
npm run typecheck
npx -y react-doctor@latest . --verbose
```

Expected: typecheck passes and React Doctor reports `100 / 100`.

### Task 3: Lock down Android input on both transport paths

**Files:**
- Modify: `native/android-viewer/src/input_protocol.rs`
- Modify: `native/android-viewer/src/usb_bridge.rs`
- Modify: `native/android-viewer/src/jni.rs`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/host-desktop/src-tauri/src/wire.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift`
- Test: existing Rust input protocol/wire tests and a new transport-independent input route test

**Interfaces:**
- Consumes: Android `MotionEvent`/`KeyEvent`, the Host per-session `inputEnabled` flag, UDP `LCI1/LCA1`, and USB framed reverse packets.
- Produces: a tested contract in which pointer motion is coalesced, reliable events are ordered/ACKed, status is visible on Android, and USB and UDP share the same input semantics.

- [ ] **Step 1: Add a failing cross-transport input contract test**

Add a pure test that encodes a pointer move, pointer button, key down/up, and `ReleaseAll`, then passes the resulting payload through the existing Host `InputSequencer`. Assert pointer motion is `Apply`, reliable events are `ApplyAndAck` in sequence, duplicate reliable packets return `AckDuplicate`, and `ReleaseAll` clears the sequence safely.

- [ ] **Step 2: Run the focused input tests and verify the failure**

Run:

```bash
cargo test -p android-viewer --lib input_protocol
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml wire::tests
```

Expected: the new cross-transport test fails before its adapter/fixture is implemented.

- [ ] **Step 3: Implement only the missing adapter behavior**

Keep the current wire format and reliability rules. Ensure the USB bridge forwards reverse control frames to the native input scheduler without putting them behind the bounded media queue; ensure both UDP and USB acknowledge reliable events with the current input-enabled bit. Keep Android touch mapped to primary pointer button, physical/IME-delivered key events mapped through the existing key protocol, and release all input on cancel, focus loss, surface destruction, and stream shutdown. Do not add input commands to the JSON control contract.

- [ ] **Step 4: Run Rust input tests and native checks**

Run:

```bash
cargo test -p android-viewer --lib input_protocol
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml
cargo clippy -p android-viewer --tests -- -D warnings
```

Expected: all pass without warnings.

- [ ] **Step 5: Build the Android APK and run React Doctor after Kotlin/stream behavior changes**

Run:

```bash
GRADLE_USER_HOME=/tmp/leftcar-gradle ./gradlew :app:assembleRelease
npx -y react-doctor@latest . --verbose
```

Expected: release APK succeeds and React Doctor is `100 / 100`.

### Task 4: Verify real UDP, input, and USB transition evidence

**Files:**
- Modify: `docs/EVIDENCE.md` only after fresh runtime evidence exists.

- [ ] **Step 1: Install the fresh APK and record device state**

Run:

```bash
adb devices -l
adb install -r apps/viewer-expo/android/app/build/outputs/apk/release/app-release.apk
adb shell getprop sys.usb.state
```

Record separately whether the device is ordinary ADB, AOAP accessory, or disconnected. An `adb` state alone is not USB media proof.

- [ ] **Step 2: Verify Wi-Fi UDP sustained rendering**

Start a stream with no AOAP accessory and capture at least 30 seconds of Host status plus `LeftcarNative` logcat. Require actual Host `mediaTransport=udp`, rendered FPS near the configured 60fps, bounded frame gaps, and no monotonic latency growth.

- [ ] **Step 3: Verify Android input with Host opt-in**

Enable remote input in Host, tap/drag/scroll on Android, and send a representative keyboard event. Require the Android input badge to report enabled and Host receiver/input counters to advance without decoder FPS degradation. Disable input and verify subsequent events are rejected while `ReleaseAll` is sent.

- [ ] **Step 4: Verify USB transition or record the AOAP blocker**

With the stream active, attach the cable and capture the Android USB state event, Host route, and native log. Then detach and capture the return to UDP. If AOAP cannot be negotiated, record the exact Host error and leave the final build on the verified UDP path; do not label ordinary ADB as USB success.

- [ ] **Step 5: Run the final repository gates**

Run:

```bash
npm test
npm run typecheck
cargo test --workspace
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml
git diff --check
npx -y react-doctor@latest . --verbose
```

Completion requires fresh runtime evidence in addition to all automated gates.
