# Start diagnosis fix round 1

DONE — SD-I1 is repaired in source with targeted red/green evidence. Two additional Root-authorized F2 presentation corrections are included. No device startup/root cause, physical cursor behavior, or post-fix React Doctor result is claimed.

Read frozen `start-diagnosis-review.md` first. Work remained entirely in `/private/tmp/leftcar-km-20261003`; HEAD `72ff99000ab1981f9773a0b90aad3ee7ca42faad` plus preserved uncommitted foundation. No Root edits, subagents, dependency changes, native changes, native reruns, broad suites, installation, UI actions, grants, or commits.

## Correction and evidence boundaries

- SD-I1: the startup catch now invokes the existing cancellation/current-peer check **after all cleanup and after cleanup-failure precedence**. A dependency rejection after explicit Cancel or obsolete peer selection becomes `AbortError`; native/remote cleanup and peer close still complete first. Failed cleanup remains `stopFailed` even if the attempt was cancelled/stale. No gates, retries, input activation, or authority behavior changed.
- Model subscription: ordinary OFF reasons are presented only if an active `KmSession` lease exists **before** reconciliation. Pending/retired startup OFF reasons are cleared in the visible status and left to the startup promise's bounded failure/cancellation presentation. This prevents a reason arriving before rejection or late after retirement from persisting as an error or resurfacing through the presenter's native-reason fallback. Active automatic-stop causes remain in existing error state through reasonless cleanup. `cleanupFailed` remains unconditional and visible even after silent cancellation.
- Model evidence runs the actual `useKmModel`, actual production reducer/action/subscription callbacks, and actual `KmSession`. Only platform boundaries and a minimal hook dispatcher are fixtures. It proves the callback/status/action ordering and observable returned state; it does **not** claim React reconciliation, real device lifecycle, visual layout, or accessibility acceptance. No model-only methods were added to production for testing.
- Root's controlled production-component render exposed repeated full shutdown prose. `cleanupFailed` now has a short KO/EN state label (`종료 미확인` / `Shutdown unconfirmed`), while the single Notice contains the detailed failure and manual Leftcar-accessibility recovery. Targeted real screen/primitives renders prove the full prose appears once and Stop remains while Start is absent.
- Root-authorized copy cleanup replaces literal helper with `입력 서비스` in affected Korean KM recovery and presents `nativeHandshake` as an ordinary input connection check. Internal closed codes remain unchanged. No new flow, visual token, or control was added.
- Latest Root-provided Host AX evidence shows TB522FU default-control connected, both approvals ON, Mac permissions allowed, sharing OFF. This proves default-control connectivity, **not dedicated KM Wi-Fi, native preparation, or a root cause**. No native defect or OEM fix was inferred.

## Red / green

Run from the isolated worktree root:

1. SD-I1 RED:
   `bunx vitest run apps/viewer-expo/src/km-session.test.ts -t 'retires|keeps failed cleanup dominant'`
   Exit **1**; **7 failed / 3 passed / 22 skipped**. Six cancellation/stale rejection cases at preparation, Host start, and native start plus cancelled rejecting openPeer failed as expected. Existing cleanup-failure dominance passed. Tests hold cleanup pending and assert the startup promise has not settled before releasing it.
   Log: `/private/tmp/leftcar-km-start-fix1-session-red.log`.
2. Model ordering RED:
   `bunx vitest run apps/viewer-expo/src/use-km-model.test.ts`
   Exit **1**; **2 failed / 3 passed**. Cancelled and stale startup status/rejection ordering exposed retained `helperUnavailable` and startup stage; active automatic-stop and cleanupFailed cases passed.
   Log: `/private/tmp/leftcar-km-start-fix1-model-red.log`.
3. Single Notice RED:
   `bunx vitest run apps/viewer-expo/src/input-sharing-view.test.tsx -t 'presents one detailed'`
   Exit **1**; **2 failed / 20 skipped**. Both languages rendered the detailed shutdown prose twice.
   Log: `/private/tmp/leftcar-km-start-fix1-presenter-red.log`.
4. Final relevant JS GREEN:
   `bunx vitest run apps/viewer-expo/src/km-session.test.ts apps/viewer-expo/src/use-km-model.test.ts apps/viewer-expo/src/input-sharing-view.test.tsx`
   Exit **0**; **3 files / 59 tests passed**: lifecycle **32**, focused model **5**, production screen/primitives **22**. These are the three suites relevant to the changed bytes; the already-passed connection-flow/native/broad suites were not rerun. This 59-test inventory differs from the first round's 59-test inventory, which included connection-flow instead of the new model suite.
   Log: `/private/tmp/leftcar-km-start-fix1-final-green.log`.
5. `bun run typecheck`: exit **0**, theme/runtime checks, workspace build typecheck, Viewer typecheck, Host typecheck all passed.
   Log: `/private/tmp/leftcar-km-start-fix1-typecheck.log`.
6. `git apply --reverse --check` for both `start-diagnosis-fix1.diff` and refreshed `start-diagnosis.diff`; `git diff --check`: exit **0**.

Native source was unchanged and native suites were not rerun. The first-round fresh native 10-test receipt remains historical evidence for unchanged native bytes. Root owns the exact React Doctor and post-fix checks after integration; the previous 100/100 result does not certify these changed bytes.

## Artifacts and scoped files

Exact pre-fix bytes of all six existing changed files are saved under `start-diagnosis-fix1-before/<source path>.before`; `base-sha256.json` records their SHA256. The new model test has a null pre-fix base. Archived files use `.before` to avoid accidental Vitest discovery.

Fix1 changes **7 files**:

- `apps/viewer-expo/src/km-session.ts`
- `apps/viewer-expo/src/km-session.test.ts`
- `apps/viewer-expo/src/use-km-model.ts`
- `apps/viewer-expo/src/use-km-model.test.ts` (new)
- `apps/viewer-expo/app/input-sharing.tsx`
- `apps/viewer-expo/src/input-sharing-view.test.tsx`
- `packages/ui-tokens/src/i18n.ts`

`start-diagnosis-fix1.diff` and `start-diagnosis-fix1-manifest.json` are the exact fix-round delta and base/after hashes/byte counts. The pre-fix consolidated diff and manifest are preserved in `start-diagnosis-fix1-before/artifacts/`.

The consolidated `start-diagnosis.diff` and `start-diagnosis-manifest.json` are refreshed against **original pre-task bytes**, so Root can integrate the complete final task using **10 source/test files** (original 9 plus the new model test). Original snapshots were rehashed and checked before regeneration. The first report/review describe the frozen first-round bytes; this report supplements them with final corrected behavior and final receipts.

Simplify/self-review: reused the existing `check()` rather than duplicating fencing logic; derived one visible status before the existing subscription patch/reconcile path; kept cleanupFailed handling independent of active/pending identity; corrected an unrelated fixture assertion to inspect the actual pending session. Reviewed cleanup ordering, late reason suppression, active automatic-stop preservation, fail-closed gates, safe reason presentation, and absence of unrelated F1/F3–F7 changes. No finding was hidden with ignores or generated edits.

## Remaining work / concerns

Root should independently review the corrected bytes, verify integration hashes, rerun the exact React Doctor gate and required post checks, then use the authorized device build to observe only the safe displayed stage/reason and state with KeyBridge enabled. The live cause remains unconfirmed. Native reasons already collapsed by the existing native catch remain generic. Refresh/focus/remount persistence, remote Stop retry semantics, Host normal-OFF warnings, and the other F1/F3–F7 audit items remain outside scope.
