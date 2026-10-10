# Task 2 — Host and macOS native KM implementation receipt

Implemented in `/private/tmp/leftcar-km-20261003`. This worker did not commit, install, grant permissions, capture a screen, inject OS input, or mutate a connected device. Source/build checks below are independent of physical-device acceptance.

## Scope and interfaces

Owned files:

- `apps/host-desktop/src-tauri/src/km.rs` (new independent coordinator, native backend, duplex encrypted transport and focused tests).
- `apps/host-desktop/src-tauri/src/control.rs` (authenticated negotiation and independent lifetime/revocation integration).
- `apps/host-desktop/src-tauri/src/lib.rs` (module, Tauri APIs and explicit settings mapping).
- `apps/host-desktop/src-tauri/src/pairing.rs` (parent-authorized removal of test-only cfg on existing internal `authorization`; no other pairing change).
- `native/macos-capture-shim/Sources/Km/{KmEventPolicy,KmNativeSession,KmExports}.swift` (new independent input implementation).
- `native/macos-capture-shim/Tests/KmEventPolicyTests.swift` and `tools/build-macos-capture-shim.zsh` (`km-test` entry and writable Swift module-cache path).

Existing authenticated control exposes `getKmStatus`, `startKm`, `stopKm`. Start is accepted only on a sealed, authenticated, non-loopback peer connection. It returns the shared `KmStartResult` containing an ephemeral native TCP port and secret, without creating a capture session, calling CaptureBackend start, or selecting a video source. The dedicated socket proves possession with an encrypted Hello before native arming. It reuses `secure_channel::km_keys` and `StreamSealer` with c2s receive/s2c transmit keys, TCP_NODELAY, u32 big-endian framing and an 8192-byte plaintext limit.

Local Tauri commands are `get_km_status(deviceId)`, `get_km_consent(deviceId)`, and `set_km_consent(deviceId, macToAndroid, androidToMac)`. Consent defaults OFF and binds atomically to the current paired credential incarnation using `with_authorization`. Partial/withdrawn consent stops the native session outside the pairing lock. `open_system_settings` now allowlists `pane: "inputMonitoring"` to `Privacy_ListenEvent`; no permission is automatically granted.

## Implemented behavior

- Independent Swift session tap, ListenEvent + Accessibility preflight, secure-input/lock safety, active-display union bounds and negative desktop coordinates. No Screen Recording permission or CaptureSession lifecycle is involved.
- Physical outgoing keys use USB HID usages; flagsChanged emits modifier transitions. Native Ctrl+Alt+Cmd+Esc releases and stops. Generated CG events carry a private tag and never become physical outgoing input. Injection covers relative move, drag/buttons, accumulated fractional wheel, HID keys/modifiers/repeat and release-all.
- The callback only try-sends into a bounded 256-event native channel. Consumer contention cannot stop healthy input. Each eight-millisecond flush sums consecutive relative deltas, keeps latest absolute position, preserves key/button ordering and physical versus injected origin. Actual overflow fails closed rather than dropping held-state transitions.
- Native heartbeat is 100 ms and Host response timeout is 500 ms. Dedicated frame reading retains partial reads across ticks. Oversize/replayed/malformed frames, obsolete ownership epochs, lost authority, sleep/lock, explicit stop, pairing revocation and native disconnect stop and release held input.
- Before edge activation, local physical held keys/modifiers/buttons defer transfer; Swift snapshots startup state and rechecks it at suppression commit. A native authenticated Hello transfers transport lifetime out of the ordinary JS control lease. Closing/idle-expiring JS control cancels a pending session but cannot kill a healthy native session; current authorization, native heartbeat and explicit revocation continue to govern it.
- Both out-and-back directions use the shared domain geometry. Normal return releases first, increments the epoch, retains the previous native owner, and drops further physical input until a new-epoch Heartbeat confirms peer-applied cleanup. Stale heartbeat neither confirms return nor extends the separate 500 ms pending deadline.
- Android-origin return sends `ReleaseAll(old epoch)`, mapped actual tablet `Cursor(old epoch)`, then `State Local(new epoch)`. The cursor target preserves normalized vertical correspondence and is placed ten pixels inside the configured shared edge. Mac-origin return restores Mac near its shared edge only after the applied-release acknowledgment.

## Verification

Meaningful red tests were observed for missing independent status, native escape policy, premature local return, stale-heartbeat pending timeout, missing mapped Android return target, and callback contention falsely stopping healthy input.

Final focused Rust commands:

```sh
CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml km --locked -- --nocapture
CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib --locked -- -D warnings
```

Results: **14 focused tests passed**, zero failures; **Clippy passed with warnings denied**. Tests cover no CaptureBackend start using a panic capture backend, denied authentication/authority/consent, AEAD replay and frame bounds, real encrypted native TCP Hello/watchdog, token/native lifetime transfer, release-before-revoke, held modifier crossing, applied-release acknowledgment and stale acknowledgment, pending timeout despite stale traffic, mapped return ordering, bounded queue overflow and SUM motion preservation.

Swift checks completed successfully on the final native source:

```sh
zsh tools/build-macos-capture-shim.zsh km-test /private/tmp/leftcar-km-native-test
/private/tmp/leftcar-km-native-test
zsh tools/build-macos-capture-shim.zsh library native/macos-capture-shim/libleftcar_capture.dylib
```

Pure policy executable output: `KmEventPolicyTests PASS`. Library compilation succeeded; `nm -gU` confirms all seven `leftcar_km_*_v1` exports. The ignored dylib is only a build artifact in the isolated checkout and was not installed. `git diff --check` passed. No React source was changed by this worker; parent owns the combined React Doctor/typecheck gates.

## Remaining acceptance and cross-task dependency

The Host implementation is ready for independent review. Android Task 3 must apply the old-epoch return Cursor through actual native feedback-controlled placement while retaining source capture, and send the new-epoch Heartbeat only after placement, neutral input and grab release finish. The parent is coordinating that Task 3 review fix; this receipt does not claim the combined runtime is already accepted.

Actual macOS permission behavior, physical hover/wheel/key output, automatic two-direction edge crossing, background/sleep/revoke recovery, Android EVIOCGRAB/native feedback, and latency remain hardware acceptance work. Pure Swift policy tests and Rust fake-backend/TCP tests do not prove those OS behaviors. Existing root-reported Android/Shizuku device readiness is not a live-input receipt from this worker.

## Review fix round 1

The Critical and Important findings from the first independent review were fixed. The current implementation and verification supersede the original pending-ACK timing/native stop ABI descriptions above. See `docs/superpowers/plans/2026-10-03-input-only-km-task2-fixround1-result.md`; exact fix delta: `.superpowers/sdd/2026-10-03-input-only-km/task-2-fixround1.diff`. Final results: Host KM tests16/16, Clippy warnings-denied, pure Swift lifecycle/policy tests, library compile and diff check all passed. Independent re-review and physical-device acceptance remain outstanding.
