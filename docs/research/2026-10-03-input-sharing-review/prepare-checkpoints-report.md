# Preparation checkpoint implementation report

Status: DONE_WITH_CONCERNS. Source is frozen. Kotlin compilation is NOT verified; root owns the normal full internal build compile gate before installation. No startup failure fix or native input acceptance is claimed.

## Scope and frozen source

Only production source changed by this task:
`apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`

Isolated checkout: `/private/tmp/leftcar-km-20261003`.
Before snapshot: `.superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`.

SHA-256 before: `70d3893ae85ef872b7624959b1ba151738e2b6525f74d5809d4c2af9d63a4234`.
SHA-256 frozen after: `c3805127940777224e6365459ab816b91ae0ffa93a6a2c97eadbb6dd265a6190`.

Auxiliary artifacts: this report, compile attempt logs, and temporary `prepare-checkpoints-native-only.gradle` in this report directory. The temporary script is diagnostic tooling only, is not referenced by production configuration, and must not be used for the root full build. Gradle produced normal build/cache outputs during attempted compilation; no generated files were manually edited. No React/helper/manifest/permissions/settings/data/production build configuration changes, tests, device operations, installs, launches, grants, input, commits, or subagents were performed by this task.

## Implemented diagnostics

Unique Log tag: `LeftcarKmPrepareProbe`.
Each accepted pending preparation allocates its own synchronized probe, with a hard maximum of 20 emitted records. Budget increment and log emission are serialized inside the same monitor. Logging failures are swallowed, so a log backend failure cannot change the product error path.

Event call sites are the fixed required names: prepare_enter, preconditions_passed, lease_accepted, bind_dispatch, bind_call_returned, bind_call_failed, service_connected, death_linked, helper_prepare_enter, helper_prepare_returned, observer_check_enter, observer_check_passed, observer_configure_enter, observer_configured, fgs_requested, fgs_failed, prepare_ready, bind_timeout, unavailable, worker_exception.

All stage strings are fixed source literals: prepare (initial), preconditions, lease, bind, service, helper_prepare, observer_check, observer_configure, foreground_service, ready. Messages contain only event, stage, and, at the preparation-specific suppressed worker catches, error.javaClass.name. No Throwable, message, stack trace, tokens, generation values, Binder identity, package/device inventory, authorization values, or input data enter diagnostics.

Task catch logging is opt-in only at prepare entry and the preparation ServiceConnection task. Other task callers do not obtain a probe. No diagnostics were inserted into heartbeat, flush, inject, motion, keys, raw mouse/capture callbacks, watchdog, transport payloads, or ownership transition paths. checkNoConflictingObserver itself is untouched; checkpoints surround only its preparation caller.

## Lifetime and source-only self review

The preparation entry and service connection tasks supply the exact attempt-local probe to their catches. The worker retains only the active preparation probe for stop closure. Main bind/foreground callbacks capture their original probe, never look up the active one. stopNow closes and clears the active probe before existing cleanup; readiness records prepare_ready and closes atomically under the same lock. A closed old probe cannot emit or consume a new probe budget. Existing numeric generation fences at lease, connection and bind timeout remain unchanged. Main completion checkpoints use advanceStage=false, preventing a delayed bind return from overwriting the worker's current failure stage.

Compared against the saved before bytes using `diff -u`; audited all checkpoint/probe call sites with `rg -n 'LeftcarKmPrepareProbe|checkpoint\(|PrepareProbe|preparationProbe|task\('`.
Original preconditions, KeyBridge fail-closed eligibility/lease gate, all failure strings, callbacks, Handler/queue choices, timeout values, cleanup operations, readiness status, and original asynchronous product calls remain the same. api.prepare(lifetime) still executes once; its Boolean is named solely to put a return checkpoint before the unchanged false-result Unavailable branch.

This is a self review only. Root's scoped read-only reviewer remains the independent privacy/lifetime/no-semantic-change gate. The controller explicitly chose compilation plus scoped review instead of mirror log tests or repeated native/React suites. No tests were added or repeated.

## Compilation attempts and exact outcomes

All commands ran from `/private/tmp/leftcar-km-20261003`. No attempt reached a passing :app:compileReleaseKotlin result. Failed prerequisites must not be reported as a Kotlin syntax error or a passing full build.

1. Default sandbox:
```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:compileReleaseKotlin --console=plain > .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-compile.log 2>&1
```
Exit 1: `java.io.FileNotFoundException: /Users/loopy/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1-bin.zip.lck (Operation not permitted)`. Initial log was superseded by the next redirected attempt; the exact error is recorded here.

2. Same exact command with authorized execution escalation:
Exit 1; `BUILD FAILED in 29s`; `114 actionable tasks: 77 executed, 37 up-to-date`.
Failed at `:app:validateReleaseIdentity`: public release requires signing variables; internal builds must explicitly use `-PleftcarInternalBuild=true`.
Full output: `prepare-checkpoints-compile-release-identity-blocked.log`.

3. Internal compile:
```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:compileReleaseKotlin -PleftcarInternalBuild=true --console=plain > .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-compile.log 2>&1
```
Exit 1; `BUILD FAILED in 6s`; `153 actionable tasks: 42 executed, 111 up-to-date`.
Failed at `:app:createBundleReleaseJsAndAssets`: Metro cannot resolve `./apps/viewer-expo/node_modules/expo-router/entry` from the isolated checkout. No dependency or React repair attempted.
Full output: `prepare-checkpoints-compile-metro-blocked.log`.

4. Native-only exclusion accepted by root:
```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:compileReleaseKotlin -PleftcarInternalBuild=true -x :app:createBundleReleaseJsAndAssets --console=plain > .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-compile.log 2>&1
```
Exit 1; `BUILD FAILED in 3s`; `159 actionable tasks: 10 executed, 149 up-to-date`.
Failed at `:app:mapReleaseSourceSetPaths`: querying the bundle task's mapped output before the excluded task completes is unsupported.
Full output: `prepare-checkpoints-compile-bundle-provider-blocked.log`.

5. Scratch init-script attempt:
```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:compileReleaseKotlin -PleftcarInternalBuild=true -I .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-native-only.gradle --console=plain > .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-compile.log 2>&1
```
Exit 1; `BUILD FAILED in 516ms`. Gradle resolves the relative init path under apps/viewer-expo/android, where the script does not exist.
Full output: `prepare-checkpoints-compile-init-path-blocked.log`.

6. Absolute init-script path:
```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:compileReleaseKotlin -PleftcarInternalBuild=true -I /private/tmp/leftcar-km-20261003/.superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-native-only.gradle --console=plain > .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-compile.log 2>&1
```
Exit 1; `BUILD FAILED in 718ms`. Init script also applies to the included React Gradle plugin build; `:app` is absent in `:gradle-plugin`, causing settings-plugin resolution failure before Kotlin.
Full output: `prepare-checkpoints-compile.log`.

Root then instructed stop further Gradle workarounds, freeze source, report DONE_WITH_CONCERNS, and transfer the compile gate to the normal full integrated internal build. No further compilation/configuration/dependency attempts made.

## Concerns and proof boundaries

- Kotlin compilation is pending. Root must successfully compile the reviewed source in the normal internal build before any install. This report does not claim compilation, packaging, runtime, or native input acceptance.
- Existing FGS request is asynchronously posted to main and preparation settles ready on worker immediately afterwards. fgs_requested records the scheduling point; fgs_failed can emit only if failure arrives while the attempt remains open. A failure arriving after ready is intentionally suppressed by the mandated close-at-ready fence; its original stop call still runs. No readiness/callback reordering was introduced to make diagnostics observe it.
- Android logging and a short synchronized critical section add bounded overhead during preparation. Maximum 20 records; no sustained-loop instrumentation. Zero timing impact cannot be proved from source review, but product scheduling/thread choices/deadlines are unchanged.
- Previous Shizuku manager interaction coincided with a server PID change. These checkpoints diagnose the current suppressed preparation error and do not establish single-variable causality for earlier provider-null behavior.

## Removal plan

After root gathers the minimum preparation evidence, remove Log import, PrepareProbe class and preparationProbe field, task's optional diagnostic supplier/catch checkpoint calls, all preparation/bind checkpoints and added diagnostic arguments, ready closure, and stopNow's diagnostic close. Inline api.prepare(lifetime) back into its original false-result check. Restore the original prepare/task/bindAuthority signatures and structure without changing unrelated ongoing work. The saved before bytes are the exact removal reference; restore that file only if no legitimate later KmRuntime edits need preservation. Remove this scratch init script from future compile commands; root full build must include the real JS bundle. Recompile after removing diagnostics.
