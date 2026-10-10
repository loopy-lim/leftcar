# Task 1: Domain safety and shared native KM wire

Scope: `crates/domain/src/km.rs`, `crates/control-contract/src/km.rs` and module export, `crates/secure-channel/src/lib.rs`. Preexisting untracked foundation preserved. No commits, install, grants or runtime suppression.

## TDD evidence

All cargo commands run from `/private/tmp/leftcar-km-20261003` with `CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target`.

- RED `cargo test -p domain km --locked`: 31 passed, 4 failed. Normal return remained `Active(MacToAndroid)` rather than `Armed`; public consuming motion subscription started while Armed; missing actual cursor injection authority still armed; missing consuming physical mouse authority still activated AndroidToMac.
- GREEN same command: 35 passed, 0 failed.
- RED `cargo test -p control-contract km --locked`: 1 passed, 5 failed (invalid version/nonzero headers, trusted session/epoch/origin, numeric bounds, unknown unit-payload fields, invalid start geometry).
- RED `cargo test -p secure-channel km_cross --locked`: fixture failed because missing derivation returned zero bytes rather than independently derived keys.

## Shared interfaces

Rust public types live at `control_contract::km` and serialize camelCase JSON with unknown fields denied. Payload uses `kind` discriminator, Hello/ReleaseAll/Heartbeat/Stop have no other fields.

`KmStartInput`:

```json
{"arrangementSide":"right","tabletWidth":2560,"tabletHeight":1600,"viewerConsent":{"macToAndroid":true,"androidToMac":true},"androidCapabilities":{"cursorInjection":true,"mouseObservation":true,"keyboardCapture":true,"mouseCapture":true}}
```

`KmStartResult` includes `sessionId`, `epoch`, `port`, `sessionKey` (32-byte random seed as base64url without padding), `status`. Status fields: `owner` (`local|macToAndroid|androidToMac`), `armed`, `epoch`, `capabilities` (`macCapture`, `macInjection`, `android` capabilities as above), `blockedReason` (null or string). Session key Debug output is redacted; seed travels only over existing sealed authenticated control.

`KmEnvelope::decode` and `encode` enforce plaintext <=8192 bytes, version1, nonempty sessionId <=128 UTF8 bytes without controls, nonzero epoch/seq, finite motion abs<=65536, button0..7, USB HID keyboard usage code0..255, dimensions1..32768, finite cursor within0..width/height. `validate_for(session_id,epoch,origin)` requires caller-supplied authenticated session, current host epoch and peer origin. Native transport owns strict ordered sequence/replay handling.

**Key.code is a USB HID keyboard page usage ID, never CG/Android raw keycodes or Unicode. A=4, leftCtrl=224. Native adapters map OS keycode to HID and reverse; target OS applies keyboard layout.**

`KmAndroidCapabilities.mouseObservation` means verified physical **observe-and-pass**, not merely the public consuming accessibility motion subscription. Domain fields keep separate `android_cursor_injection`, `android_observe_and_pass`, `android_mouse_capture`. Consuming capture starts only in Active(AndroidToMac) via `StartAndroidMouseCapture` and ends via `StopAndroidMouseCapture`. `ReturnToLocal` is an original physical-source return, releases held remote input before local restoration, and preserves Armed consent/watchdog; it never activates the opposite physical origin.

UHID authority corresponds to explicitly approved USB-started Shizuku local Binder helper; product traffic remains direct authenticated Wi-Fi, no wireless ADB.

## Fixed interoperability fixture

Derived independently with Swift CryptoKit HKDF/ChaChaPoly. Python RFC5869 HMAC-SHA256 independently matches directional keys. Swift invocation: `swift -module-cache-path /private/tmp/leftcar-km-modulecache /private/tmp/leftcar-km-vector.swift` (default system module cache was sandbox-denied; temporary cache succeeds).

- Seed hex: `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f`
- Session ID/salt UTF8: `km-vector-v1`
- HKDF SHA256 info: `leftcar/km/v1`, output64, first32 c2s(Android→Host), next32 s2c(Host→Android).
- c2s key: `c1832723e3708ccaba0b6556606315f47bed62a617ea2f5245c70bf03d154e9d`
- s2c key: `43a451ccb7c5fa7ca56d14341c280227f52696f862d3d288a88ad2efcd719346`
- Exact UTF8 plaintext, 104 bytes:

```json
{"version":1,"sessionId":"km-vector-v1","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}
```

First sealed frame uses counter1; AEAD nonce is four zero bytes followed by counter as u64 big-endian, no AAD. Frame layout counter8 + ciphertext104 + tag16 =128 bytes; TCP prefix u32BE `00000080`.

c2s sealed frame:

```text
0000000000000001466bd7edde80cbaa55c15a07434f23b12d7c5221fb465c4eb0977abaf4c145e950919376679c11da9fc5ee46e873b766a8a6bbf6cb81cde9f6a580a134a8875a7e284131812f27006f516f5b3e95fabb9218916b58d8970ee6ec8779f83766fe0c42f4ac631a7aebe8c2c18ec109b297b98e51119ff21759
```

s2c sealed frame:

```text
000000000000000118a4701245f7c3794984e563b412bbbfd6207a91e46dfdc9161dfb1f6e9139b5ddcc3d9e83a1deabae2b4eabff01aa06a45768b5b5aca8de9a8bcbc0e10d91c1506386bc21c3ab9fe0b45d8b4744b298158aecf2fb0a4d48c71ee2ebc623a8b6062fbe629de18960bd990d1babb3cdef6df4e7c0a1e9ea5b
```

Rust regression checks derivation, both sealed directions, opening fixed fixtures, replay rejection, session salt separation and media-key domain separation.

## Final Rust verification

- `cargo test -p domain -p control-contract -p secure-channel --locked`: exit0; domain50 passed, control-contract20 unit +20 integration passed, secure-channel14 passed +2 preexisting fixture-printer ignored; doc tests pass.
- `cargo clippy -p domain -p control-contract -p secure-channel --all-targets --locked -- -D warnings`: first run flagged collapsible match in new numeric validator. Replaced nested if with match guard, no lint suppression. Second run exit0.
- After clippy change, `cargo test -p control-contract km --locked`: 6 passed, exit0.
- `rustfmt --edition 2021` applied only to owned Rust source files.

All Task1 code checks pass. No React changes in this task; no React Doctor invocation needed for Task1. This establishes domain/contract/crypto behavior only, not OS permission, physical device cursor/capture, native edge crossing or runtime acceptance.

## Explicit physical mouse capture coordination

Root required explicit `KmAndroidCapabilities.mouse_capture: bool` (wire `mouseCapture`) so keyboardCapture or observe-and-pass never implies consuming physical mouse authority. startKm and status capabilities both carry this fourth field. Regression deserializes mouseCapture=false while keyboardCapture=true and mouseObservation=true, preserving all independently. RED `cargo test -p control-contract mouse_capture_capability --locked`: failed on unknown mouseCapture field before DTO extension. Hello/HKDF fixture unchanged.

After explicit mouseCapture field extension:

- `cargo test -p control-contract --locked`: exit0; 21 unit +20 integration passed, doc tests pass.
- `cargo clippy -p domain -p control-contract -p secure-channel --all-targets --locked -- -D warnings`: exit0.

## Task 1 review fix round 1: emitted schema scalar bounds

Reviewed `2026-10-03-input-only-km-task1-review.md` Important finding against the actual derived `JsonSchema`: the decoder was bounded but emitted scalar fragments omitted version/nonzero/range/geometry constraints.

Changed only `crates/control-contract/src/km.rs` plus this report. Added schemars bounds for version exactly1, epoch/seq>=1, sessionId codepoint length1..128, move/wheel dx/dy±65536, button0..7, HID code0..255, cursor x/y0..32768 and width/height1..32768, start tablet dimensions1..32768. Public DTO shape and native validator behavior are unchanged; no dependency changes.

Module and field schema descriptions explain the remaining runtime invariants: sessionId<=128 **UTF8 bytes**, Unicode controls forbidden, cursor x<=this message's width and y<=its height, bounded plaintext bytes, authenticated session/current epoch/origin. Draft7 length counts Unicode codepoints rather than UTF8 bytes; it has no sibling-value comparison for relative cursor bounds. The native receiver remains authoritative for these checks. The focused regression explicitly exercises the byte/control/relative-coordinate rules in addition to scalar schema parity.

Preserved pre-fix source `/private/tmp/leftcar-km-task1-before-schema-fix.rs` and exact scoped unified diff `/private/tmp/leftcar-km-task1-schema-fix.diff`. `diff -u` exited1 as expected for changed files, producing the diff successfully.

Commands from `/private/tmp/leftcar-km-20261003`, all cargo commands with `CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target`:

- RED `cargo test -p control-contract schema_ --locked`: exit101, 0 passed/3 failed. Failures reproduced emitted-schema acceptance while decoder rejected: `schema version=0`, `schema move.dx=-65537`, `schema tabletWidth=0`.
- GREEN same focused command: exit0, 3 parity unit tests +1 existing filtered schema integration test passed. Candidate tables cover both valid boundaries and invalid headers, session lengths, every move/wheel axis, button/code bounds, cursor scalar/dimension fields, and both start dimensions. The regression consumes actual `schemars::schema_for!` output and evaluates only the Draft7 scalar minimum/maximum/minLength/maxLength/type keywords present in these numeric/string fixtures; it does not pretend to be a general JSON Schema implementation.
- Covering `cargo test -p control-contract --locked`: exit0, 25 unit +20 integration passed, doc tests passed. Includes the runtime byte/control/relative-coordinate check.
- Scoped `cargo clippy -p control-contract --all-targets --locked -- -D warnings`: exit0, `Finished dev profile` in4.53s.
- `rustfmt --edition 2021 crates/control-contract/src/km.rs` completed.

No broader Rust suites were repeated, no other Task1 code changed, no commits or device/runtime work performed. Review finding addressed at the representable scalar/schema scope; root can re-review the preserved scoped diff.
