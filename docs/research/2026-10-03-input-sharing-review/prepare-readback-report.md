# Temporary observation readback diagnostics

Status: CANCELED_AND_RESTORED. Root stopped this diagnostic increment after obtaining decisive read-only OEM evidence. Both source files were restored ONLY to their exact saved preimages, preserving the previously reviewed branch/checkpoint diagnostics. No compile/runtime result, exact Boolean values or behavior fix is claimed. The sections below record the canceled increment for audit, not active source.

## Scope, preimages and SHA-256

Isolated checkout: `/private/tmp/leftcar-km-20261003`.
Only production paths changed:
- `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`
- `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt`

Exact preimages were copied with shutil.copy2 before either edit to matching relative paths under `.superpowers/sdd/2026-10-03-input-only-km/prepare-readback-before/`; the copy operation refused to overwrite any existing snapshot.

| File | Before SHA-256 | Frozen after SHA-256 |
| --- | --- | --- |
| KmRuntime.kt | 939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6 | 27526392d49eb1013849b029dea58e043ff73e523a836ac4118c5840e318026d |
| KmAccessibilityService.kt | 1f474744568d8c7af6e3e8894eba51c72dd0441a75626a8ee1b699306f1970fc | cf826b5ce55845a4274f50622b6205a09438b2da1a05c4984e5de166c9ae0705 |

## Increment

configure now accepts an optional prepareReadback callback defaulting to null, before the final commitOwner parameter. The original trailing owner-commit lambda call therefore continues to target commitOwner. Only the Runtime preparation configure call opts in, passing an immediate lambda that captures its existing attempt-local probe.

The existing authority.verifyObservation call executes once, at its original location after the actual serviceInfo readback and before mode/ownership work. Its Boolean result is named verified. A false result plus a non-null diagnostic callback enables readback diagnostics inside runCatching; the original check then throws with the unchanged error text. A verifier exception bypasses the new block and follows the original exception path. The original false-result failure is not replaced by any diagnostic failure.

On that failure path only, independently guarded reads compare the prepared object's motion and observed sources with the existing requested values, and the applied object's motion and observed sources with those requested values. A single guarded applied observedSources read is reused for both equality and zero comparisons; there is no duplicate applied Binder call.

The callback passes exactly five nullable Boolean comparisons to one extra checkpoint: fixed event observation_readback, fixed stage observer_configure. Its fields are prepared_motion_matches, prepared_observed_matches, applied_motion_matches, applied_observed_matches, applied_observed_zero. Each is formatted only as true, false, or unknown. Other existing checkpoint events do not include these fields. No numeric masks or object contents are emitted.

## Source-only self review

- Only preparation opts in. The condition !verified && prepareReadback != null prevents all added reads/callback work on successful verification and on ordinary failed capture/transition calls with a null callback. restoreLocal is untouched.
- Each added motion getter and observedSources Binder read is guarded separately with runCatching. The enclosing runCatching also contains the callback, isolating callback failures from the original throwing check.
- prepared remains the object originally returned by configureObservation. applied remains the serviceInfo readback already used by the verifier. No extra read of serviceInfo is introduced. No object or Throwable is passed to or retained in the diagnostic callback/probe. Existing objects and requested masks remain method locals; the applied observed integer is transiently local solely to compute two Boolean results.
- The callback captures probe, never the replacement global preparationProbe. Existing synchronized cap20, closed state, atomic ready/stop closure, generation fences and main stage protections are unchanged. This adds one record inside the existing cap, with no budget increase.
- No numeric masks, serviceInfo, exception text/stack, tokens, Binder identity, auth, inventory, or input data enter these new diagnostic fields. The prior closed branch/class fields are unchanged.
- Original failure reasons, authorization/coexistence/KeyBridge gates, mode and ownership commits, queues, timeout constants, input flow, and cleanup are unchanged. No new setting write is added. Diagnostics are read-only and failure-only.
- The Runtime trailing commitOwner call and ordinary OBSERVE calls were inspected; none opt in to prepareReadback. No reflection lookup for configure was found in the searched native tests.

## Exact verification and pending proof

Read prepare-readback-brief.md first. Read current Runtime logger/preparation caller, full AccessibilityService, and the source authority implementations for configureObservation, observedSources and verifyObservation. Saved exact preimages before source edits.

Reviewed these incremental diffs:
```sh
diff -u .superpowers/sdd/2026-10-03-input-only-km/prepare-readback-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt
diff -u .superpowers/sdd/2026-10-03-input-only-km/prepare-readback-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt
shasum -a 256 .superpowers/sdd/2026-10-03-input-only-km/prepare-readback-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt .superpowers/sdd/2026-10-03-input-only-km/prepare-readback-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt
```
Diffs showed only the specified callback/readback logic, five optional Boolean logger fields, and preparation-only callback wiring. SHA-256 results are above.

Read-only rg inspected prepareReadback, verifier/observed-source calls, all service.configure call sites and configure reflection lookups in the two production files and native test directory. A supplementary read of a presumed KmCaptureMode.kt standalone file failed because that path does not exist; this did not affect the scoped source comparison or produce a source change.

Per controller instructions, no compilation, Gradle workaround, mirror tests or repeated suites were run. No device/UI/input/grant/permission/install/app/helper launch, commits, subagents, React/AIDL/authority/helper/build/dependency/generated edits were performed. Independent scoped review and the normal full root internal build remain required. Next actual diagnostic values require a separate authorized user-operated Start observation; prior APK evidence must not be promoted to acceptance of these frozen hashes.

## Concerns and removal

This diagnostic adds two read-only observedSources Binder calls and two guarded motion getter reads only after an opt-in failed verifier. Binder calls can delay the original throw/cleanup by their execution time; no timeout, queue/thread, gate or failure behavior was changed. No successful-start or ordinary non-preparation path has that added work. The finite extra readback checkpoint remains subject to the 20-record cap.

No exact values/cause or speculative fix can be inferred from source alone. Root must build the reviewed frozen source and obtain minimum next runtime evidence without expanding input or permission scope.

After minimum evidence, remove prepareReadback from configure and its preparation caller, remove the failure-only diagnostic reads/callback block, restore the inline original verifier check, and remove the five logger arguments/formatting together with all prior temporary preparation diagnostics. Use prepare-readback-before as the exact reference for removing just this increment while preserving later legitimate fixes; use the earlier documented full diagnostic removal reference only when no intervening legitimate edits would be lost. Recompile via the normal root build after removal.

## Controller stop and verified restoration

After implementation/report preparation, root instructed immediate stop: the OEM caller permission gate had already been identified read-only, so this additional Boolean diagnostic/retry was unnecessary. No build or device action occurred.

Before restoration, each current source hash was required to match this task's increment hash and each snapshot hash had to match the expected preimage; restoration would refuse any concurrent source change. Copied only the two exact saved preimages back, then verified SHA-256 equal to the original before values:

- KmRuntime.kt active/restored: `939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6`.
- KmAccessibilityService.kt active/restored: `1f474744568d8c7af6e3e8894eba51c72dd0441a75626a8ee1b699306f1970fc`.

There is no active readback increment remaining. Existing previously reviewed checkpoint/branch diagnostics remain in Runtime. Root's OEM report is outside this task's independent proof; no speculative fix or previous-diagnostic removal was made. Await the focused correction brief.
