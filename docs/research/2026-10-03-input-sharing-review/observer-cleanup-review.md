# Scoped observation cleanup source review

## Verdict

SPEC: PASS on source inspection. QUALITY: PASS on source inspection. Critical findings: none. Important changed-source findings: none. Root normal full integrated Android internal build remains pending; no compilation, device behavior, observation capability, or real input-sharing success is claimed.

Reviewed only observer-cleanup-brief.md, observer-cleanup-report.md, observer-cleanup.diff, observer-cleanup-manifest.json, exact saved before/baseline source, and the resulting two production files. Read the existing JS reason mapping only to verify the fixed reason has the specified presentation code. The new delegation direction and its outstanding design gaps were not reopened or implemented.

## Frozen evidence

| Source | Before SHA-256 | Reviewed after SHA-256 |
| --- | --- | --- |
| KmRuntime.kt | 939f95abd11850e1a6830a7fce96e5c08a52108fa60287975d7fbf1c64dfbae6 | efd416b2c26f63e47b6364e0aac6cdabd43d2c8b60882807d9ec6e08ae3be22a |
| KmAccessibilityService.kt | 1f474744568d8c7af6e3e8894eba51c72dd0441a75626a8ee1b699306f1970fc | b7dfcb068afca7218dd8cd7e9c910b5c0a156ec2f3203f9d2b5a088b5e365cb7 |

Independent hash computation matches all manifest entries. Reconstructed combined unified diff with 16 context lines matches observer-cleanup.diff exactly. Complete source reconstruction proves:

- Runtime is the exact full saved pre-diagnostic source (baseline SHA-256 `70d3893ae85ef872b7624959b1ba151738e2b6525f74d5809d4c2af9d63a4234`) plus only the new typed catch.
- Service is its exact cleanup preimage plus only the one false-verifier replacement and final exception definition.

This is stronger than checking only selected removal hunks. An `rg` search across the native km directory for the temporary tag/probe names, optional supplier/lifetime fields, checkpoint calls, worker/branch/readback diagnostic markers, exception-class log field, and Log calls found no matches (normal no-match exit 1). Scratch evidence is not production instrumentation.

## SPEC and behavior

All temporary Runtime diagnostics are removed: Log import, PrepareProbe, tag/budget/stage/branch mapping, diagnostic task supplier, active lifetime field, preparation/bind/main checkpoints, ready closure, and stop closure. Full baseline equality after undoing only the typed catch establishes that every unrelated Runtime behavior was preserved rather than selectively inferred.

KmAccessibilityService.kt line 45 still invokes `verifyObservation(applied, mask, next.observesMouse())` once at the same place, after serviceInfo readback and before routing/capture/owner commit. False now throws only KmObservationUnavailable. Exceptions thrown by the verifier itself still propagate normally; null-info errors, capture-holds failure, setting calls, mode transitions, and cleanup are unchanged.

The internal class at Service lines 108–112 is final under Kotlin's default final semantics. It has no caller-supplied reason and one fixed constant: `UHID 또는 Android 16 마우스 관찰 기능을 사용할 수 없습니다`. Runtime lines 115–117 catch the exact type before the same existing Unavailable and generic Exception handlers. The new catch passes that constant to existing stopNow, without reading error.message or performing broad text/prefix classification. The existing exact map in apps/viewer-expo/src/km-failure.ts line 22 maps this literal to `unsupported`; no React/JS edit is required.

The same stopNow at Runtime lines 389–421 preserves generation cancellation, input release before local restoration, helper/transport cleanup, KeyBridge lease closure, failed-cleanup retention, stopped status, preparation failure callback, and service shutdown. A cleanup failure can still replace the capability reason with the existing explicit cleanupFailed outcome; this necessary existing priority is preserved.

## QUALITY and scope

The typed exception is a narrow source-defined failure signal. It avoids exposing raw exception text and does not convert a failed capability check into readiness. Baseline reconstruction confirms queue admission/decrement, dispatch threads, authority/helper setup, generation fences, coexistence checks, input handling, FGS ordering, readiness, and deadlines have no residual diagnostic or unrelated changes.

The two-file delta contains no delegated connection interface, hidden-access library/adapter, helper/AIDL change, permission request/grant, monitor, build configuration, or capture substitution. The current ordinary app observing path remains unsupported under the separately recorded OEM evidence. The shared reason does not assert that UHID/helper creation itself failed, and this review does not claim actual sharing was repaired.

## Remaining proof

Root must build these exact reviewed sources through the normal full Android internal flow before any authorized update, including Kotlin compilation and real JS packaging. Device failure presentation and native capability remain separate proof levels; old artifact/runtime evidence cannot establish this new artifact. The new own-service delegation capability remains subject to separate explicit approval and the five previously reported design-contract gaps.

No source edits, tests/suites, Gradle attempts, device/UI/input actions, installs, permissions, commits, or additional agents were performed. Only this review report was written. This Kotlin-only cleanup does not introduce a new React quality-gate trigger.
