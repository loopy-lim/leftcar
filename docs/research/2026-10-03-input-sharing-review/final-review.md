# Final integration review — input-only KM

Review target: immutable `final.diff`, baseline HEAD `72ff990` plus preserved initial foundation. Read the final brief, progress rulings, task receipts and bounded sections of the supplied diff; focused source reads supplied line references. No source, Git, device, permission, or runtime changes; no suites repeated. Existing verification receipts are evidence supplied by the implementers, not independently rerun results.

## Strengths

- Dedicated authenticated native transport is independent of media/JS event traffic. Directional HKDF keys, bounded frames and explicit session/origin/epoch validation agree across Rust and Kotlin.
- Host consent is keyed by credential incarnation; delayed revoke/control teardown cannot target a replacement credential. Completed native sessions intentionally survive control-idle teardown.
- Return ordering includes old-epoch release and Android placement before new-epoch local acknowledgement. The separate bounded placement deadline avoids weakening the normal heartbeat watchdog.
- Swift callback fencing and explicit stop-v2 completion retain Rust context/dylib ownership when retirement is incomplete. Emergency callbacks enqueue cleanup rather than wait behind injection.
- Android authority is narrow, app-UID checked, Binder/heartbeat scoped, and separates observe-and-pass from selected physical evdev capture. KeyBridge remains enabled and unleased eligible hotplug fails closed.

## Findings

### Critical

None identified in this review.

### Important

1. **Native Start completes before authenticated State, but JS treats that as failure.**
   - `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt:195–197`; `apps/viewer-expo/src/km-session.ts:82–90`.
   - Native sends Hello and immediately posts a successful callback using current status. Until the TCP reader receives and the worker applies Host State, that status is `preparing`. JS accepts only `armed`, `macToAndroid`, or `androidToMac`, so a main-thread callback that runs before the network response throws `startFailed` and stops both peers. This makes a valid normal-latency start timing-dependent; the native-start mock returning `armed` does not cover the actual boundary.
   - Complete native Start only after the initial authenticated State has been successfully applied (with bounded timeout, cancellation/failure settlement, and exactly-once completion), or provide an explicit bounded waiting contract consumed by JS. Add a delayed-State regression through the production start/completion seam, including stop-before-State and late-State cases.

2. **Android held-state check and capture-mode commit race with physical key callbacks.**
   - `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt:85–103` and `:37–48`; `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt:271–275`.
   - Main-thread `onKeyEvent` snapshots mode before recording a locally passed down. The KM worker separately checks `hasLocalHolds()`, acquires the physical mouse and commits CONSUME. A down can snapshot OBSERVE, pause before adding to `locallyHeld`, allow the worker to see neutral state and commit CONSUME, then resume and pass down locally. Its later up takes CONSUME and is forwarded/suppressed. Android's local application is left with an unmatched down, violating the source-held-input deferral contract. Volatile mode and a concurrent set do not make this check/commit atomic. This path is reachable with supported Android physical sources and either an acknowledged lease or no competing KeyBridge filter; the current tablet's lack of a source device only postpones exposure.
   - Serialize the whole callback routing/held-state decision with neutral-check/ownership commitment, or perform both on one owner thread with a safe synchronous callback decision. Include modifier/button state in the same boundary. Add the deterministic paused-down/concurrent-commit interleaving used for the Mac regression and verify the up remains local until a later neutral transfer.

3. **Android cleanup failure is converted into successful Stop and loses recovery UI.**
   - `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt:360–381`; `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmModule.kt:63`; `apps/viewer-expo/src/km-session.ts:109–119`; `apps/viewer-expo/app/input-sharing.tsx:30–31,73–74`.
   - `stopNow` detects failed neutral/ungrab/helper-close/local-readback cleanup and records a specific failure reason, but `stop` unconditionally invokes a no-result completion and the RN module unconditionally resolves. JS therefore cannot set `stopFailed`. Native publishes state OFF; the view substitutes the generic “Input sharing stopped” string for the reason and shows Start rather than the explicit failed-stop retry path. Users are told shutdown completed even though native cleanup was not confirmed. The helper watchdog is a separate fallback, not confirmation that this stop succeeded.
   - Propagate a structured cleanup result/error through native Stop and the RN Promise, retain actionable recovery status for unsolicited native cleanup failures, and make the UI distinguish unconfirmed cleanup from confirmed OFF. Test a failing production cleanup seam through the native bridge/session result and rendered recovery state. Preserve attempts to perform every cleanup action even after one fails.

### Minor

None raised; no unrelated cleanup requested.

## Assessment

**Spec compliance: With fixes. Quality/readiness: With fixes. Ready to merge/install as the completed feature: No.**

The architecture and supplied verification establish substantial coverage, but three cross-layer lifecycle/ownership defects remain. Address these focused findings and review their exact deltas; do not rerun unrelated passed suites. React changes, if any, still require the repository's exact React Doctor 100/100 gate followed by typecheck and relevant tests.

## Evidence boundaries

The supplied Rust/Swift/Kotlin/JS receipts, original-root React Doctor 100/100, typecheck/94 JS tests and signed Android APK build are acknowledged; Host final packaging was pending in the brief. None proves physical acceptance. Actual updated-Host TCC preflight, OEM observation setter/readback, shell UHID and EVIOCGRAB authority, actual cursor/hover/wheel/key delivery, both edge crossings/placement timing, hotplug/coexistence, Binder death, background lifetime and emergency/revoke recovery still require their specified runtime checks. KeyBridge with actual Android key devices remains blocked without its real ownership ACK; leaving it enabled with no eligible devices does not establish general coexistence. No physical test was attempted by this reviewer.
