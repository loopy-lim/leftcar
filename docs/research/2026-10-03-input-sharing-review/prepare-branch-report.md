# Temporary preparation branch classification

Status: DONE. Source is frozen for root's scoped review and normal full internal Android build. No compilation, runtime verification, cause determination, or feature fix is claimed by this task.

## Paths and hashes

Only changed production file:
`apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`

Isolated checkout: `/private/tmp/leftcar-km-20261003`.
Before snapshot: `.superpowers/sdd/2026-10-03-input-only-km/prepare-branch-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`.
Report: `.superpowers/sdd/2026-10-03-input-only-km/prepare-branch-report.md`.

SHA-256 before: `c3805127940777224e6365459ab816b91ae0ffa93a6a2c97eadbb6dd265a6190`.
SHA-256 frozen after: `939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6`.

## Changes and source-only self review

Added an optional branch field to the existing bounded preparation logger. Only the opt-in preparation worker_exception catch supplies it; unavailable and all other checkpoint calls remain unchanged.

The catch reads error.message inside runCatching and matches exactly against the four specified existing source-local configure messages. No prefix or substring matching. The source references were checked read-only in KmAccessibilityService.kt at lines 37, 44, 45, and 47, respectively. The only resulting values are service_info_missing, service_info_readback_missing, observation_readback_rejected, capture_holds, and unclassified. Unmatched/null messages and throwing message getters return unclassified. The class field remains error.javaClass.name.

Raw exception messages are neither logged nor stored in a variable, probe field, report, or other diagnostic artifact. The direct when expression transiently reads the getter solely to choose a closed fixed label; the runCatching result holds a label only. No Throwable, stack, input data, inventory, tokens, generation, Binder identity, or auth values were added to emitted records.

The existing per-attempt 20-record cap, monitor synchronization, worker stage retention, main callback stage protection, atomic ready-close, stop close, and captured stale-probe fences are untouched. The generic task catch's optional-probe safe calls short-circuit before evaluating branch classification for ordinary non-preparation task callers. No new log call sites, catch handlers, or input-loop instrumentation were added.

Failure reasons, callbacks, queues/threads, deadlines, authorization/capability/KeyBridge coexistence gates, and cleanup are unchanged. The new getter access is guarded by runCatching, so a getter exception cannot replace the existing generic failure handling. No other production file was edited.

## Validation and proof boundaries

Read requirements first from prepare-branch-brief.md, then read the current runtime logger/catch and exact local configure branch source. Reviewed the complete incremental source diff against the before snapshot and computed before/after SHA-256.

Commands used for source verification:
```sh
diff -u .superpowers/sdd/2026-10-03-input-only-km/prepare-branch-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt
shasum -a 256 .superpowers/sdd/2026-10-03-input-only-km/prepare-branch-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt
```
Diff contains only optional logger field formatting and the generic preparation-specific catch's closed branch matching. Hash results are above.

Per explicit controller instructions, no Gradle attempts, mirror tests, or native/React suites were run. Root owns independent scoped source review and the normal full Android internal build after review. No build/dependency/generated/config edits, installs, app launches, UI actions, system input, permission/grant changes, commits, or subagents were performed.

## Concerns

- Compilation and runtime observation of this exact new hash remain pending with root. Earlier runtime evidence does not establish the current exact branch; the new classification still needs the next authorized user-operated Start observation.
- Classification is temporary diagnostic evidence only. No permission, capability, constructor, or coexistence workaround is justified by this change.
- Existing bounded logging overhead and the asynchronous FGS close-at-ready observation limit remain as documented in prepare-checkpoints-report.md; this increment adds no additional records.

## Removal plan

After the exact cause is known, remove the optional branch argument/formatting and the closed mapping expression from the suppressed preparation worker catch together with all existing temporary preparation probes according to prepare-checkpoints-report.md. Preserve any subsequent legitimate behavioral fixes. prepare-branch-before is the exact reference for removing only this increment; the earlier prepare-checkpoints-before snapshot is the full diagnostic removal reference if no intervening legitimate KmRuntime changes need preservation. Recompile through the root normal build after removal.
