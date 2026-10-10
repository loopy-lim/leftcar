# Task 2 review fix round 1

Read the complete `.superpowers/sdd/2026-10-03-input-only-km/task-2-review.md` and addressed its Critical and Important findings in the isolated checkout. No commit, OS injection/capture, installation, device action, React change, Android change or shared Rust contract change was performed.

## Finding-to-fix map

1. **Notification callback can use a freed Rust callback context.** All raw-context emissions use `KmCallbackFence`. Complete notification producer invocations also own a counted fence lease. Shutdown closes the fence before retirement and drains active producers within a deadline. A paused notification before emission cannot call the retired Rust context. `leftcar_km_stop_v2() -> i32` returns zero only after tap and callback retirement; on timeout Rust retains its Box. Failed start uses the same safe retirement path. The Swift dylib stays process-loaded because registered Swift/Objective-C classes and retained notification closures can outlive the Rust backend.
2. **Valid return placement can hit Host timeout before its permitted ACK.** Explicit owner/placement transitions have a separately bounded 1500 ms deadline: Android's 1000 ms/64-step placement bound plus 500 ms delivery margin. The normal Host heartbeat deadline remains 500 ms outside that phase. Current-epoch Heartbeat ends the phase; obsolete traffic cannot extend its deadline or acknowledge cleanup. The encrypted TCP regression accepts a valid 600 ms return; an unacknowledged transition still expires despite old-epoch heartbeats.
3. **Local held-state check races with suppression commit.** `KmOwnershipGate` serializes the complete event policy/physical-state decision with owner commitment. A callback can no longer pass a local down using a stale owner snapshot and suppress its up under a new owner. Neutral tracked state and the CG source snapshot are checked inside this boundary before any owner change. Captured modifier bookkeeping resets at the same committed boundary. The deterministic interleaving test pauses a local down while a concurrent owner request waits, then verifies rejection and an ordinary local up before a later neutral commit.
4. **Consent and revocation do not bind the exact credential incarnation.** KM approvals and sessions now use `(deviceId, Authorization generation)`. Pairing revoke records the exact retired Authorization identities in an internal, serialization-skipped outcome field. KM revocation and control connection notifications target those retired generations; old teardown cannot inherit consent or close a newly paired pending/native KM session. Operator disconnect still intentionally stops the currently paired KM generation. The real pairing regression revokes, re-pairs the same ID, verifies default-OFF consent, approves a replacement and starts a pending session, then applies delayed old retirement and drops the old control lease. Only the old connection is notified; replacement consent, control connection and KM epoch survive.
5. **Emergency tap callback blocks on the injection queue.** Native escape, tap failure and lock/sleep callbacks enqueue one latched emergency onto the serial native work queue and return without waiting behind injection. Further physical input is dropped while release is pending. Suppression is restored only after neutral input is actually posted. Native injection/release/shutdown waits are bounded at 250 ms per operation, with explicit shutdown completion status; a timeout retains context/session ownership rather than freeing a live pointer. A stuck native shutdown blocks another native start until retirement succeeds; app restart is the final recovery for a permanently stuck OS callback.

No media/capture path was added. Retired-generation connection filtering is the parent-authorized control teardown adjustment needed to protect pending KM from an old revoke; media target selection and capture behavior were preserved.

## Verification

Red evidence: the new encrypted 600 ms placement regression failed under the original 500 ms deadline; initial lifecycle tests could not compile before the callback/ownership seams existed. Final source checks:

```sh
CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml km --locked -- --nocapture
CARGO_TARGET_DIR=/Users/loopy/dev/ll3/leftcar/target cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib --locked -- -D warnings
zsh tools/build-macos-capture-shim.zsh km-test /private/tmp/leftcar-km-native-test
/private/tmp/leftcar-km-native-test
zsh tools/build-macos-capture-shim.zsh library native/macos-capture-shim/libleftcar_capture.dylib
```

**Final results:** 16 focused Host tests passed (zero failures; 1.63 s); Clippy passed with warnings denied; the final pure Swift executable passed callback-fence, paused-observer, held-state-interleaving, bounded-emergency-queue, policy and HID checks; the final Swift library build succeeded; `git diff --check` passed. `nm -gU` confirms `leftcar_km_stop_v2` and the six remaining KM v1 exports. Frozen source hashes were rechecked after verification. Prior Task 2 receipts' seven-v1-export/500 ms pending-return descriptions are superseded by this round: shutdown now requires `leftcar_km_stop_v2`; a v1-only shim fails readiness with a rebuild reason.

## Review artifacts

- Pre-fix source snapshot: `.superpowers/sdd/2026-10-03-input-only-km/task-2-fixround1-before/`.
- Exact fix delta: `.superpowers/sdd/2026-10-03-input-only-km/task-2-fixround1.diff`.
- Scoped before/after source hashes: `.superpowers/sdd/2026-10-03-input-only-km/task-2-fixround1-files.json`.
- Receipt: `docs/superpowers/plans/2026-10-03-input-only-km-task2-fixround1-result.md`.

The fix delta contains seven source files: `km.rs`, `control.rs`, `pairing.rs`, `KmExports.swift`, `KmNativeSession.swift`, new `KmLifecycle.swift`, and `KmEventPolicyTests.swift`. Original Task 2's other files are unchanged in this round. The independent review and physical-device acceptance remain separate gates; these tests do not prove OS event delivery or actual out-and-back crossing.

Build artifact SHA-256: `0ac530967637c2784e6440fe571195670cbbc2152bdcf4a0bc2a4cc3418fadd4`. This ignored dylib is an isolated-checkout build artifact; it was not installed.
