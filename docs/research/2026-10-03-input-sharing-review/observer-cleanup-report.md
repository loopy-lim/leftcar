# Observation diagnostic cleanup and explicit unavailable result

Status: DONE. Source is frozen for scoped root review and normal full internal Android build. Startup remains unsupported from the current ordinary app observation caller; this is failure presentation repair, not working input sharing or helper-creation failure.

## Changed files and preimages

Worktree: `/private/tmp/leftcar-km-20261003`.
Only production files changed:
- `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`
- `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt`

Starting hashes were verified against the controller brief before editing. Exact two-file preimages were copied with shutil.copy2 to matching relative paths under `.superpowers/sdd/2026-10-03-input-only-km/observer-cleanup-before/`, refusing existing destination snapshots.

| File | Before SHA-256 | Frozen after SHA-256 |
| --- | --- | --- |
| KmRuntime.kt | 939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6 | efd416b2c26f63e47b6364e0aac6cdabd43d2c8b60882807d9ec6e08ae3be22a |
| KmAccessibilityService.kt | 1f474744568d8c7af6e3e8894eba51c72dd0441a75626a8ee1b699306f1970fc | b7dfcb068afca7218dd8cd7e9c910b5c0a156ec2f3203f9d2b5a088b5e365cb7 |

## Complete temporary diagnostic removal

Explicitly removed Runtime's Log import, entire PrepareProbe class/tag, active preparationProbe field, optional task probe supplier and catch checkpoint/branch classifier, preparation/bind checkpoints and diagnostic arguments, ready diagnostic closure, and stop diagnostic closure. Restored the task/prepare/bindAuthority blocks to their pre-diagnostic behavior and structure using the exact saved reference for those blocks. This was not a blind whole-file replacement.

Before adding the typed capability catch, asserted the entire resulting Runtime string equals the saved pre-diagnostic Runtime byte-for-byte: SHA-256 `70d3893ae85ef872b7624959b1ba151738e2b6525f74d5809d4c2af9d63a4234`. This validates every removal and confirms no intervening legitimate behavior was discarded. After adding the catch, reviewed the diff against that reference: it contains only the new typed exception catch plus the unchanged Unavailable catch on the next line. A second assertion that reverses only the new typed catch again reproduces the complete pre-diagnostic source exactly.

The canceled readback increment had already been restored before this task, so there was no active callback/Boolean logger to remove from Service. Checked both final source files for all temporary class/tag/field/checkpoint/branch/readback markers and Log import; none remain. Legacy scratch evidence/preimages are retained as audit artifacts, not active production diagnostics.

## Fixed typed unavailable path

Defined internal KmObservationUnavailable in AccessibilityService.kt. Kotlin classes are final by default; it is not open. Its sole reason constant is the existing literal `UHID 또는 Android 16 마우스 관찰 기능을 사용할 수 없습니다`.

Only the existing false verifyObservation result in service.configure throws this type. The verifier still executes once in the original location after applied serviceInfo is read and before routing/mode/owner commit. A false result remains fail-closed. Verifier exceptions, both missing-serviceInfo branches, capture-holds branch and all other behavior are unchanged.

Runtime.task catches only this exact final type and calls existing stopNow with its fixed constant. It does not inspect error.message, classify prefixes, or expose a raw exception. Existing Unavailable and generic Exception catches remain behaviorally unchanged. Queue admission, pending decrement, worker dispatch, generation/lifetime, helper preparation, coexisting observer and KeyBridge checks, input paths, FGS request/readiness/deadlines and cleanup are preserved.

Read-only verification of apps/viewer-expo/src/km-failure.ts confirms its existing exact literal mapping to unsupported at line 22. No React/JS/AIDL/authority/helper/build/permission/configuration edit is needed or was made.

## Validation and proof boundary

Read observer-cleanup-brief.md first. Read the current diagnostic Runtime and configure source; verified the pre-diagnostic reference and starting hashes. Saved two preimages before editing.

Source checks:
```sh
diff -u .superpowers/sdd/2026-10-03-input-only-km/prepare-checkpoints-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt
diff -u .superpowers/sdd/2026-10-03-input-only-km/observer-cleanup-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt
shasum -a 256 .superpowers/sdd/2026-10-03-input-only-km/observer-cleanup-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt .superpowers/sdd/2026-10-03-input-only-km/observer-cleanup-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmAccessibilityService.kt
```
Reviewed both resulting diffs. Runtime against pre-diagnostic baseline contains only the typed catch. Service contains only the false-verifier typed throw and final exception definition. Source assertions independently confirm Runtime is the complete baseline plus the typed catch, all temporary markers are absent, and Service contains exactly one verifier call and one typed throw.

No compilation, mirror tests, Gradle workarounds or repeated suites were run per controller instructions. No device/UI/input/install/launch/grant/permission action, commits or subagents. Root owns independent scoped review, normal full internal build and any separately authorized update. This report does not claim build/runtime/native input acceptance.

## Remaining boundary

The controller's deployed OEM evidence establishes that the current ordinary app caller cannot preserve observed motion sources at the service update permission gate. This task does not independently re-prove that OEM report, obtain delegation approval, or implement a workaround. No own-service delegation, hidden API, helper/app/monitor or permission change was introduced. The new typed result accurately preserves the current unavailable startup outcome; actual sharing remains unverified and unsupported under this current path. Any shell-to-own-service proposal and implementation require the separate approval boundary owned by root.
