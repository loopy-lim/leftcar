# Scoped temporary preparation branch review

## Verdict

SPEC: PASS on source inspection. QUALITY: PASS on source inspection. Critical findings: none. Important changed-source findings: none. Root normal full integrated Android internal build and device observation remain pending; this report does not claim compilation, cause determination, feature repair, or physical input acceptance.

## Exact reviewed input and evidence

Only the second incremental diagnostic delta in `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt` was reviewed. Requirements: prepare-branch-brief.md. Worker evidence: prepare-branch-report.md. Frozen inputs: prepare-branch.diff, prepare-branch-manifest.json, saved prepare-branch-before source, and current source. Read only configure lines 33–48 in KmAccessibilityService.kt to verify the four exact matching messages; no broad native review.

Verified before SHA-256: `c3805127940777224e6365459ab816b91ae0ffa93a6a2c97eadbb6dd265a6190`.
Verified frozen after SHA-256: `939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6`.
Both match the manifest. Independently reconstructed unified diff with 16 context lines matches the frozen incremental diff byte-for-byte after its explanatory header.

The entire source delta is optional logger argument/formatting at KmRuntime.kt lines 52–61 and closed preparation worker-exception classification at lines 148–157. Locations below refer to the frozen after source.

## SPEC and privacy

The only emitted new values are `service_info_missing`, `service_info_readback_missing`, `observation_readback_rejected`, `capture_holds`, and `unclassified`. The `when (error.message)` at lines 150–155 uses exact equality against the four specified local configure messages. Those match unchanged KmAccessibilityService.kt lines 37, 44, 45, and 47 respectively. Null or any other message follows the fixed fallback. There is no prefix/substring matching.

Raw exception text is used transiently as the expression subject, without a new stored message variable/probe field or a logger argument. The classification result consists only of fixed labels. Log formatting at lines 59–61 appends only that optional label; the existing exception class field is preserved. No stack, inventory, input values, token/generation values, Binder identity, authorization data, or raw message is emitted by the increment.

Only the existing opt-in preparation `worker_exception` catch supplies the branch field. The unavailable catch and all other call sites are unchanged. Safe calls at line 148 short-circuit argument evaluation when the diagnostic supplier or probe is null, so ordinary non-preparation tasks do not evaluate `error.message` or classification. No input-loop or additional catch/log call site was added.

## QUALITY, lifetime, and behavior

The message getter and exact-match expression are enclosed by `runCatching` (lines 149–157). A throwing getter yields `unclassified` through `getOrDefault`; the original generic `stopNow` reason and execution at line 158 remain unchanged. The temporary Result can hold a getter failure internally until fallback, but no such failure is retained in the probe or emitted. Getter access is one bounded diagnostic classification per preparation exception, not added to normal input paths.

The incremental diff leaves the synchronized 20-record guard, stage updates, logging-failure isolation, atomic ready close, stop close, per-attempt allocation/capture, and existing generation fences untouched. The logger still serializes its budget and emission under the same monitor. Main callbacks still use their captured old probe and `advanceStage = false`; they cannot alter a replacement attempt or worker failure stage. Adding the optional final parameter preserves existing positional/named call binding.

No failure reason, callback, queue/thread selection, deadline, authority/capability check, KeyBridge coexistence gate, readiness behavior, or cleanup logic changes appear in the reviewed delta. No service/helper/React/manifest/build configuration change is part of it.

## Pending proof and limits

- Root must complete the normal full internal Android build of this exact reviewed source, including Kotlin compilation and real JS packaging, before installing. No Gradle or redundant native/React suites were run by this reviewer; no temporary log-mirroring tests were requested.
- The next authorized user-operated Start must provide runtime class/stage/branch evidence. Earlier installed-artifact evidence does not validate this new hash. Matching a fixed message supplies diagnostic classification; it does not alone establish a behavioral fix or native capability acceptance.
- The existing close-at-ready FGS observation limit and preparation-only logging overhead are unchanged from the first diagnostic increment. The branch field adds no log records.
- After the exact cause is established, remove this closed mapping/optional field with the existing probes, preserve legitimate subsequent fixes, and recompile. The saved prepare-branch-before file removes only this increment; the earlier full diagnostic preimage remains the broader removal reference.

Only this report was written. No production edits, additional agents, installs, launches, UI/input actions, permission changes, commits, or builds were performed.
