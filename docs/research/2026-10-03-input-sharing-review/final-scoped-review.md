# Scoped final fix re-review

Scope: `final-fix.diff` only, read with its report, manifest and original `final-review.md`. All 11 current source/test SHA-256 values match the manifest. No source, Git, device or permission changes; no tests rerun; no subagents.

## Original findings

1. **Premature native Start completion — ADDRESSED.** The production worker now owns a ticketed completion; Hello leaves it pending, authenticated State application completes it, and the deadline/disconnect/stop paths reject once. The new real-runtime encrypted-socket tests cover delayed State, readback failure, cancellation/late callback, disconnect and timeout.
2. **Android held-state/capture commitment race — ADDRESSED.** Callback routing and physical state updates use the shared short gate. A revision ticket is checked again after unlocked Binder/grab/readback work; concurrent local input invalidates capture. The owner assignment under the gate is nonblocking. The concurrency/mutation evidence targets the original race.
3. **Cleanup failure reported as successful Stop — ADDRESSED.** Cleanup now returns failure to the RN Promise, publishes persistent `cleanupFailed`, and retains Stop/recovery across remount. Host cleanup still runs on native failure. Retention introduces the new recovery issue below.

## New Critical / Important findings in the delta

**Critical: none.**

**Important: A dead helper Binder becomes a permanent cleanupFailed trap.**

- Changed path: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt:379–400`; interacting existing gates at `:119` and `:142–146`.
- The normal helper-Binder-death callback calls Stop. `releaseAll`, `releaseMouse`, and `close` on that dead proxy throw; cleanup therefore fails and the new retention branch keeps `authority`, `binding`, and `args`. Every explicit Stop retries the same permanently dead proxy, so it fails forever even after Shizuku is restarted and local observation is successfully restored. `prepare` rejects `cleanupFailed`, and an old binding's later connection callback is fenced out by the incremented generation. There is no reachable recovery to OFF/new authority short of destroying the Leftcar process.
- This is introduced by retaining authority on every cleanup failure. The new retry test replaces the private authority field with a healthy proxy, which production cannot do in this state; it consequently does not verify the dead-Binder recovery path. The UI instructs users to retry Stop, so this is a broken supported authority-loss recovery, not merely an untested OEM timing concern.
- Distinguish confirmed helper-process/Binder death from a live helper whose cleanup is still unconfirmed. Confirmed process death closes that process's UHID/evdev FDs; still require successful local accessibility restoration before confirming OFF. Retire/unbind the dead authority so a later explicit prepare can establish a new generation. Retain the current error/retry behavior for live-but-failing cleanup. Add a production-path regression where one proxy remains permanently dead (no test field replacement), local restoration succeeds, and explicit re-prepare becomes possible without automatic rearming. Do not swallow arbitrary Binder errors as proof of process death.

## Verdict

**Original findings: 3 / 3 ADDRESSED. Scoped readiness: WITH FIXES — 1 new Important recovery defect, 0 Critical.**

The supplied 18 native tests, 19 JS tests and typecheck are accepted as reported evidence, not rerun here. Root owns React Doctor 100/100, post-Doctor checks and fresh APK packaging. Host/shared source is unchanged by this delta. No physical/OEM acceptance is inferred; the original runtime, KeyBridge ownership and explicit-consent boundaries remain in force.
