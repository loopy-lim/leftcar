# Residual helper-death recovery fix

Status: focused native fix complete and frozen in `/private/tmp/leftcar-km-20261003`. This is the Root-authorized exception recorded after `final-scoped-review.md`: recover from confirmed helper death without treating arbitrary RPC errors as successful cleanup. No original-root files, React/TypeScript, Host/shared files, generated sources, build configuration, devices, permissions, installs, commits or subagents were changed.

Artifacts beside this report:

- `residual-recovery-before/`: exact prior source/test snapshots, `.before` suffix.
- `residual-recovery.diff`: incremental source/test patch against those snapshots.
- `residual-recovery-manifest.json`: before/after SHA-256 values for guarded integration.

## Changed paths and behavior

`apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt` is the only changed production file.

The worker retains the exact Binder supplied by the successful authority connection. Its death recipient posts to the worker and verifies Binder identity before recording death or stopping the session. It deliberately does not discard a death merely because Stop already advanced the session generation: a retained failed-cleanup helper still owns that same Binder. A stale death for a retired Binder cannot affect a replacement. The existing generation check continues rejecting stale connection callbacks.

Stop still attempts neutral release, mouse ungrab, helper close and local accessibility restoration independently. If RPC cleanup fails, it may nevertheless confirm cleanup only when (a) the captured exact helper Binder's death recipient fired or its `isBinderAlive` is false, and (b) local accessibility restoration succeeded. Confirmed helper process death closes that process's UHID/evdev descriptors. An RPC exception, including inability to query liveness, alone never proves death. A live-but-failing helper and a dead helper whose local restoration fails both remain `cleanupFailed` with retained authority for retry.

Confirmed cleanup clears the captured Binder/death marker and retires/unbinds the authority. State becomes OFF with no automatic binding or rearm. A later explicit prepare creates a new generation and returns only `preparing`/OBSERVE; config, transport and epoch gate remain absent until explicit authenticated Start.

Tests:

- New `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/KmRuntimeRecoveryTest.kt`: actual runtime prepare, binding, death callbacks, Stop, local restore, unbind and explicit re-prepare. The external Shizuku/system accessibility APIs are Robolectric shadows; helper authority/Binder proxies model lifetime and remote errors. No tested recovery replaces a dead proxy with a healthy authority. One post-assertion teardown clears singleton state only for test isolation.
- Updated `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/KmRuntimeCompletionTest.kt`: existing transient-cleanup retry now changes the external failure condition on the same proxy instead of replacing the runtime's authority field to force success.

## Red / green evidence

Working directory: `/private/tmp/leftcar-km-20261003`.

Before production changes:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmRuntimeRecoveryTest --console=plain
```

Result: **3 tests, 2 failures**. Confirmed dead helper remained `cleanupFailed` instead of OFF; a permanently dead proxy still failed Stop after local restoration became available. The live-but-failing helper test passed. Exact log `/private/tmp/leftcar-km-residual-red.log`. An initial missing test import and then singleton fixture isolation were corrected before recording these behavioral failures.

The additional retained-helper test was mutation-checked by temporarily reintroducing the old `generation == token` condition alongside the Binder identity check in the death callback:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests 'dev.leftcar.viewer.km.KmRuntimeRecoveryTest.retained helper death after failed Stop still recovers despite the advanced generation' --console=plain
```

Result: **1 test, 1 expected failure**: death after an earlier failed Stop was ignored and stayed `cleanupFailed`. Log `/private/tmp/leftcar-km-residual-generation-red.log`. The exact safe runtime was restored before final verification; the mutation is absent from the frozen patch.

Final command on frozen source:

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --tests dev.leftcar.viewer.km.KmRuntimeRecoveryTest --tests dev.leftcar.viewer.km.KmRuntimeCompletionTest --console=plain
```

Exit **0**, `BUILD SUCCESSFUL in 8s`, **10 tests, 0 failures, 0 errors**: recovery 4, runtime completion 6. Log `/private/tmp/leftcar-km-residual-green.log`; XML under `apps/viewer-expo/android/app/build/test-results/testDebugUnitTest/`. Existing Gradle 10 deprecation summary remains; no Kotlin compiler warnings/errors were reported.

Coverage includes permanent proxy death, independently failed local restore then explicit retry, retained-authority death after generation advancement, live repeated RPC failures then same-proxy success, explicit re-prepare without either direction armed, stale old death/connection callbacks, and the directly interacting authenticated Start/timeout/disconnect/Stop completion regressions.

`git diff --check`: exit **0**. No unrelated suites or TypeScript/React gates were rerun because no TS/React source changed in this residual patch.

## Frozen hashes

| File | Before SHA-256 | After SHA-256 |
| --- | --- | --- |
| `KmRuntime.kt` | `1754d2455bc3e85ced2dd6c53add42e8a4777f714ab68e15f2edc53e7606bf23` | `70d3893ae85ef872b7624959b1ba151738e2b6525f74d5809d4c2af9d63a4234` |
| `KmRuntimeCompletionTest.kt` | `fa84c48dc9ac25f51f2fe225d910b3155d117505e5832eddb4902db853430328` | `6f557879c2fa669c2fc2b70aa5018093029fa49a7cc2b67a0e11cc1eec584149` |
| `KmRuntimeRecoveryTest.kt` | New file | `bd315d27fe48b419922ebc2deb91866d0adda39c50947cb5f0abb47755cf509e` |

The manifest carries full repository-relative paths. These hashes are the residual patch's final source; the earlier `final-fix-manifest.json` correctly remains a record of the preceding reviewed snapshot.

## Evidence boundary

This proves production lifecycle decisions through controlled JVM external-system seams. It does not prove real OEM Binder delivery, helper process termination/FD release timing, accessibility restoration or physical input recovery. Those remain runtime acceptance checks. Root owns guarded reintegration, any required final review and packaging. The prior React Doctor/post-Doctor gates from the earlier UI fix remain Root-owned, unaffected by this native-only patch.
