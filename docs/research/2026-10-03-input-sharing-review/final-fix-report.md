# Final integration fix wave — three Important findings

Status: all three reviewed findings addressed in the temporary worktree. Focused native and JS tests and repository typecheck pass. No original-root source edits, commits, subagents, installs, device operations, captures, or permission changes were performed. Original-root React Doctor 100/100, post-Doctor typecheck/tests, packaging and physical acceptance remain Root-owned.

Review input: `final-review.md`, `final-review-brief.md`, existing production paths and `task-4-report.md`. The dispatched `task3-result.md` was absent at the named directory; actual production lifecycle/authority tests and progress rulings supplied its relevant interfaces. No scope was broadened to Host/shared source.

## Reviewable artifacts

- `final-fix-before/`: exact pre-edit snapshots with `.before` suffix (prevents test scanners collecting backup tests).
- `final-fix.diff`: exact incremental unified source/test diff against those snapshots; new files have `/dev/null` old paths.
- `final-fix-manifest.json`: each changed path, original SHA-256 or null for new files, final SHA-256 for guarded integration.

Source changed: viewer Android km `KmRuntime.kt`, `KmAccessibilityService.kt`, `KmModule.kt`, `KmLifecycle.kt`; viewer `src/km-session.ts`, `app/input-sharing.tsx`.

Tests changed: viewer `src/km-session.test.ts`, `src/input-sharing-view.test.tsx`; new native `KmRuntimeCompletionTest.kt`, `KmPhysicalTransitionTest.kt`, `KmStartCompletionTest.kt`.

No Host-bundled code, shared UI tokens, i18n, build configuration or generated source changed. The existing translated `stopFailed` recovery text is reused.

## Fixes and production seams

1. Start is a control command on the native worker and cannot be dropped by the high-rate input queue limit. Sending Hello leaves Start pending. A worker-owned, ticketed completion settles success only after the authenticated TCP receive path applies State and verifies observation/owner setup. A 1000 ms post-Hello State deadline, disconnect, rejected application, explicit stop and cancellation settle failure once. Existing generation fencing ignores late transport callbacks. Connection establishment retains its existing separate 2000 ms socket bound.

2. `KmPhysicalTransition` is used by the actual accessibility key callback, physical pointer modifier/button updates, local-hold reads, and runtime capture commit. Short synchronized operations combine routing with physical held-state updates. Capture obtains a neutral revision ticket, performs evdev acquisition and OS observation readback without holding the gate, then atomically revalidates the ticket/neutral state and commits CONSUME plus the runtime owner. A local down, completed tap, modifier/button change or permission failure during acquisition invalidates commitment. A failed commitment aborts the session through existing cleanup; a locally passed down's up stays local. The existing exact physical eligibility predicate and KeyBridge ACK policy remain in force. No Binder call, UI dispatch or other blocking work runs under the gate. Synthetic cleanup does not falsely claim held physical keys became neutral.

3. Stop returns `Result<Unit>` from the production cleanup path; the React Native module rejects with `km_stop_failed` on failure. Every neutral/ungrab/helper-close/local-readback cleanup remains attempted. Failure retains the native authority and binding for an actual retry and publishes persistent `cleanupFailed`, rather than confirmed OFF. Foreground Stop remains available until confirmed cleanup. Successful retry retires the retained authority and transitions OFF. The screen renders translated unconfirmed-shutdown guidance and Stop even after remount with no JS-local error. Unsolicited failed cleanup retires the Host lease without automatically clearing native recovery. Explicit Stop still closes the Host even if native cleanup rejects.

## Red evidence

All commands ran from `/private/tmp/leftcar-km-20261003` unless specified.

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmRuntimeCompletionTest --console=plain
```

Before production fixes: **2 tests, 2 failures**. Real KmRuntime prematurely completed after Hello; failed native cleanup published `off`. Exact log: `/private/tmp/leftcar-km-final-native-red.log`. The first sandbox attempt could not open Gradle's existing home-cache lock; the authorized escalated rerun produced these actual behavioral failures.

New isolated ticket/gate tests initially had missing-class compile failures (`/private/tmp/leftcar-km-final-seams-red.log`), which are not claimed as behavioral red evidence. The concurrent gate tests were subsequently mutation-checked by temporarily removing only capture revalidation from `commitCapture`, running the following, then restoring the exact saved safe source:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmPhysicalTransitionTest --console=plain
```

Result: **3 tests, 2 expected failures**: concurrent local down incorrectly permitted capture, and a completed local tap failed to invalidate acquisition. Log: `/private/tmp/leftcar-km-final-held-red.log`. The mutation is absent from final source; the safe implementation was restored before final verification.

```sh
bun run test apps/viewer-expo/src/input-sharing-view.test.tsx
```

Before UI fix: **1 failed, 7 passed**, persistent native cleanup failure rendered generic stopped text and Start. Log: `/private/tmp/leftcar-km-final-ui-red.log` (clean rerun after giving backup snapshots non-test suffixes).

```sh
bun run test apps/viewer-expo/src/km-session.test.ts
```

Before failed-cleanup reconciliation fix: **1 failed, 10 passed**, unsolicited native cleanup failure did not reject/release the Host lease. Log: `/private/tmp/leftcar-km-final-session-red.log`.

## Final green verification

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmRuntimeCompletionTest --tests dev.leftcar.viewer.km.KmPhysicalTransitionTest --tests dev.leftcar.viewer.km.KmStartCompletionTest --tests dev.leftcar.viewer.km.KmLifecycleTest --console=plain
```

Exit 0, `BUILD SUCCESSFUL in 6s`, **18 tests / 0 failures / 0 errors**: runtime completion 6, physical transition 3, start ticket 2, existing directly affected lifecycle 7. Exact log: `/private/tmp/leftcar-km-final-native-green.log`; XML results under `apps/viewer-expo/android/app/build/test-results/testDebugUnitTest/`. Gradle printed its existing Gradle 10 deprecation summary; no Kotlin compiler warnings/errors were reported.

The actual runtime tests use real KmRuntime, KmAccessibilityService, encrypted loopback TCP, KmWire authentication/decode and worker/main completion paths. External Android Binder authority is a proxy; the system accessibility connection's serviceInfo getter/setter is a test-only Robolectric shadow because Robolectric supplies no live system accessibility connection. Both successful delayed State and failed observation readback are tested. Stop-before-State invokes the retained real transport callback after cancellation to prove generation fencing, and real socket disconnect/deadline reject once. These are JVM integration tests, not physical OS authority proof.

```sh
bun run test apps/viewer-expo/src/input-sharing-view.test.tsx apps/viewer-expo/src/km-session.test.ts
```

Exit 0, **2 files / 19 tests passed**. Log: `/private/tmp/leftcar-km-final-ui-green.log`. Tests cover rendered persistent native recovery, native-stop rejection with Host cleanup, explicit native retry, and unsolicited failure reconciliation; existing visible selections and session cancellation tests remain green.

```sh
bun run typecheck
git diff --check
```

Both exit 0. Typecheck includes generated theme drift check, exact React runtime compatibility (19.2.3), root TypeScript, viewer and Host typechecks. Log: `/private/tmp/leftcar-km-final-typecheck.log`.

No unrelated broader suite or temporary-worktree React Doctor was run. Root must perform its exact mandated `npx -y react-doctor@latest . --verbose` after guarded integration and achieve 100/100, then rerun typecheck/relevant tests; this worker does not claim that gate is complete.

## Preserved boundaries

Observe-and-pass source policy, selected physical evdev grab, KeyBridge enabled/no-external-keyboard narrow route and real ACK requirement for eligible sources, native background lifetime, explicit start, independent native encrypted input and all video separation remain intact. No key values or session seeds are logged, persisted or displayed.

OEM observation/authority, physical keyboard/button timing, actual UHID/cursor placement, evdev grab/release, Binder death/watchdog, background/emergency recovery, current-device APK and signed Host acceptance still need the existing physical plan. Neither these tests nor prior packaging receipts establish those behaviors. Root's separate Host signing issue and rebuild are outside this diff.
