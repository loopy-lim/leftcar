# Task5 fix1 narrow re-review

Status: **PASS for the requested source re-review**. **F1 ADDRESSED. F2 ADDRESSED.** Spec verdict: **PASS**. Code-quality/security/concurrency verdict: **PASS**. New directly introduced Critical findings: **0**; Important findings: **0**. No remaining blocking concern identified within F1/F2 and fix1's immediate effects.

Scope: `task-5-fix1-only.patch` SHA-256 `6a83e991b235e6588c640446f78f3469d7dc56972afc04531f9e1ce4d8cfa9da`; consolidated Task5 patch SHA-256 `0330b241e361817c05516e6b57cbc348c0731a76c6fccb24bfe6a5e0be5ac4f3`. Read the fix report, exact patch/hash manifest, relevant preserved Service/Runtime preimages, current changed production/test source, and existing red/green logs/XML receipts. Root independently rechecked all preimage/current hashes. This review did not repeat the broad Task5 or Tasks1–4 review, tests, build, dependency resolution, device actions, delegation, or commits. Only this review report was written.

## F1 — ADDRESSED

- `KmRuntime.kt:124-145`, `:175-191`, `:411-429`, and `KmOwnObservation.kt:44-53`: explicit prepare captures a separate session revision before worker admission. Stop advances it synchronously, independently of worker progress and the per-attempt serial. Configure's subsequent `begin()` cannot reopen the cancelled session. Runtime's production configure calls carry this captured admission through freshness and commitment. The cleanup service reference is volatile; cancellation correctness no longer depends on observing that reference at a particular publication moment.
- `KmAccessibilityService.kt:62-71`: after system/local readback, final routing and owner callback execute under session then attempt gates and recheck instance/generation/session and token validity. Cancellation cannot complete between the protected decision and commitment. CONSUME retains its existing routing capture-ticket/hold guard. `KmRuntime.kt:325-333` now commits owner for all three destinations through that same callback and guards subsequent status publication; it no longer performs an unconditional owner assignment after configure.
- `KmRuntime.kt:180-191` and `:273-277` guard prepared readiness and authenticated State completion against revoked admission. Queued prepare readiness delivers failure if its session has since been cancelled. No claim is made that a Stop can undo a success that already linearized before Stop.
- Lock-order inspection found session → attempt → routing for final commitment. Main local-field writes can hold attempt while reading session revision, but `current()` is a volatile read and does not acquire session. Stop releases session cancellation before acquiring attempt cancellation. Production owner callbacks assign local state; publication posts work rather than invoking user callbacks. The new nested gates contain no Binder RPC, member/reflection read, main-thread wait, helper input/watchdog monitor, or network send. No reverse lock dependency was introduced.
- The supplied regressions exercise the actual boundaries: a separate Stop caller returns while Runtime's helper prepare is paused, then asserts zero active local/helper writes and failed readiness; final local readback cancels after freshness and asserts no owner/routing commitment for both OBSERVE and CONSUME; authenticated State additionally asserts failure through actual Runtime completion. These test the reported behavior rather than a duplicate cancellation model.

An already issued remote operation can still finish after cancellation; its response cannot pass final commitment and follows the existing verified OFF recovery. The review does not reinterpret the main dispatch timeout as a bound on remote Binder latency.

## F2 — ADDRESSED

`KmAccessibilityService.kt:46-56` now calls the exact app-side `observationPlatform.observed(info)` on freshly obtained own service info after the current-instance/generation and no-IME checks, before the first active local dispatch write or helper call. Shell getter availability is no longer substituted for this prerequisite. The same reader remains required by public OFF verification (`:103-107`); failure is not treated as confirmed cleanup.

The new fixture independently fails only the app observed getter while connection and local field access succeed. The production service regression asserts the fixed capability failure, zero active local writes, zero helper calls, no owner callback, and OFF routing. Thus the formerly undetected cleanup-only missing-member condition is exercised before activation.

## Verification and limits

Existing `task-5-fix1-evidence/red.log` and red XML record **3 intended behavioral failures** before production corrections. Existing green log and four suite XML receipts record **30 tests, 0 failures/errors/skips**: OwnObservation 14, RuntimeCompletion 7, RuntimeRecovery 6, PhysicalTransition 3; `BUILD SUCCESSFUL in 10s`. Test source and receipts were inspected, not rerun. The existing exact-target, finite mode/flags, rollback, helper-death, capture-hold, and bounded-main contracts were not widened by this fix.

The setup sentence accurately says this route depends on Shizuku shell and restricted non-SDK APIs, with OS/OEM compatibility and actual device behavior requiring separate verification. It does not claim official public-SDK support. No React source changed.

This closes the two source-review findings only. Normal integrated packaging, signer/artifact verification, actual ART/member access, deployed shell application, physical input behavior, KeyBridge coexistence, and hardware recovery remain Root-owned pending evidence. This PASS is not physical/device acceptance.
