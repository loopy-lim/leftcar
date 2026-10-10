# Startup attribution fix round 1 review

**SPEC COMPLIANCE — PASS. QUALITY — PASS for the scoped source repair. SD-I1 — ADDRESSED. Open Critical: 0. Open Important: 0.** No newly introduced Critical/Important findings.

Scope: `start-diagnosis-fix1.diff`, its report, saved pre-fix bytes and manifest, necessary current production seams/tests, and the refreshed consolidated task manifest. The bounded F2 single-cleanup-Notice and plain stage/recovery copy changes were explicitly Root-authorized. No unrelated audit item or previous native lifecycle review was reopened. No source edits, suite reruns, subagents, UI/device actions, install/grants, dependency changes, or commits; only this review document was written.

## SD-I1 closure

- `apps/viewer-expo/src/km-session.ts:91–101` preserves normalization before cleanup, waits for native cleanup and any negotiated Host Stop, and closes the peer. `stopFailed` remains first priority. The existing `check()` now runs **after** that cleanup/dominance decision, so both an aborted signal and an obsolete peer become `AbortError` even when preparation/Host/native-start **rejects**, rather than only when it resolves. The original meaningful noncancelled failure is still retained.
- Added real `KmSession` dependency-seam regressions cover all six cancel/stale × prepare/Host-start/native-start rejection combinations. They hold native cleanup pending and assert startup remains unsettled until cleanup completes, then assert `AbortError`, closed peer, no active lease, and negotiated Host Stop where required. Cancelled rejecting `openPeer` is separately covered without inventing an acquired lease. Cancelled/stale rejecting preparation with failed cleanup remains `stopFailed`.
- `apps/viewer-expo/src/use-km-model.ts:24–33` takes the ordinary OFF-reason decision before reconciliation can retire an active lease. An OFF reason with no active lease is removed from the **visible status**, so neither the model error state nor the screen's reason fallback resurrects pending/retired startup failures. Startup promise presentation remains responsible for noncancelled failures. Genuine active automatic-stop reasons still enter the existing error state and survive reasonless cleanup. `cleanupFailed` is unconditional and dominates cancelled/stale ordinary reasons.
- New `use-km-model.test.ts` executes the actual production `useKmModel`, reducer, action/subscription callbacks, and actual `KmSession`, using a minimal hook dispatcher and replaced platform boundaries. It checks both cancellation and stale-selection OFF/rejection events before retirement and late OFF events after retirement, active automatic-stop preservation, failed cancellation cleanup, and late `cleanupFailed` after silent cancellation. It adds no production-only test API. This closes SD-M1's focused model-event evidence gap; it does not certify real React scheduling or a native device lifecycle.

## Bounded F2 presentation

- `app/input-sharing.tsx` maps the native `cleanupFailed` state line to short `cleanupUnconfirmed`, with localized `종료 미확인` / `Shutdown unconfirmed` labels. The existing single `SharingFailure` Notice owns the detailed failure and manual Leftcar-accessibility recovery; KeyBridge remains enabled.
- Production screen/primitives renders in both languages assert detailed shutdown prose occurs exactly once, Stop remains visible, and Start is absent. The real presenter still prioritizes native `cleanupFailed`/`stopFailed` over ordinary startup causes.
- `nativeHandshake` remains an internal closed stage but displays an ordinary input connection check. Affected Korean helper recovery uses `입력 서비스`. These are copy changes within existing tokens/primitives, with no new controls, flow, retry, logging, input activation, or gate change.

## Evidence checked

- Independently verified fix-round **7 after hashes/byte counts, 6 before hashes/byte counts, diff hash**. Verified consolidated task **10 after hashes/byte counts, 8 original before hashes/byte counts, diff hash**.
- Compared archived pre-fix consolidated manifest with the refreshed one: every original file retains its exact original-base hash, byte count, and snapshot path. Each of the six pre-fix base hashes matches the archived prior after hash. Consolidation adds only the new model test to the original nine source/test files.
- Read targeted red logs: session retirement **7 failed / 3 passed / 22 skipped**, model ordering **2 failed / 3 passed**, duplicated presentation **2 failed / 20 skipped**. Failures match the reviewed defects.
- Read final relevant green log: **3 files / 59 tests passed** (session 32, model 5, production presentation 22) and successful repository typecheck log. This is the fix-round test inventory, not the first-round inventory that included connection-flow. No test was rerun by this reviewer.
- Native bytes are unchanged; no native/OEM finding or fix is introduced. Previously passed native lifecycle/connection-flow suites were not duplicated.

## Remaining boundaries

The original SD-M2 remains a documented nonblocking limit: JS cannot reconstruct detailed reasons already replaced by an existing generic native catch, and explicit refresh/focus/remount persistence belongs to separate audit scope. No additional source correction is required by this review.

Root still owns the exact `npx -y react-doctor@latest . --verbose` gate and required post-gate checks for **these final bytes**; an earlier 100/100 result does not validate changed source. Integration/package/device acceptance also remain Root-owned. The user's brief connecting → OFF behavior is not explained conclusively by these source/test results. After verification/integration, observe only the safe displayed phase/reason and state with KeyBridge enabled; do not treat stage attribution, default-control connectivity, or passing fixtures as proven device root cause or physical input acceptance.

**Frozen verdict: SD-I1 addressed; 0 Critical / 0 Important open.**
