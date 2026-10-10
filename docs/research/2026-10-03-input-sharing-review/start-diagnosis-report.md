# Start diagnosis report

DONE — scoped source/presentation repair is ready for Root integration. React Doctor and live device diagnosis remain Root-owned checks; this report does not claim full application acceptance or a confirmed device failure cause.

Worktree: `/private/tmp/leftcar-km-20261003`. HEAD: `72ff99000ab1981f9773a0b90aad3ee7ca42faad`, with the existing uncommitted input-sharing foundation preserved. No Root edits, subagents, native source changes, dependency changes, installation, UI operation, permission grants, or commits.

## Evidence and boundaries

- F2 is reproduced at the actual `KmSession` dependency seam. Failures from `openPeer`, `native.prepare`, Host `startKm`, and `native.start` previously lost their phase; known native messages became `startFailed`. Tests demonstrate attribution after cleanup, including native Stop clearing status.reason.
- A second production defect is reproduced through actual `openKmControl`: no selected request origin threw `AbortError`, indistinguishable from intentional cancellation and suppressed by `useKmModel`. Root explicitly approved the bounded repair. Missing selection now produces `connectionRequired`; explicit signal abort and stale selection remain `AbortError` with fencing intact. No automatic connection, video start, alternate authority, or bypass was added.
- Root-provided device evidence: permissions reported enabled, Shizuku API_V23 granted, an earlier low-rate Shizuku record proves a helper creation attempt, no helper persists after failed Start. This does not establish successful binding/preparation or associate that earlier attempt conclusively with the latest click. The no-selection regression is a proven code defect, **not the confirmed physical-device root cause**.
- No native/OEM defect was proven by a new production-seam native regression. No native fix was expanded.

## Resulting behavior

- `KmError` holds a closed issue and optional closed startup stage. Stage values are `connectionSetup`, `androidPreparation`, `hostApproval`, `nativeHandshake`; unknown causes stay `startFailed`. Phase identifies where startup failed, not its diagnosed root cause.
- The original error is normalized before native/Host cleanup. Structured `code`, `reason`, and `message` are matched against finite exact known strings; arbitrary strings, known prefixes with added secret-shaped data, auth/session fields, and nested payloads are never forwarded. A throwing diagnostic getter cannot prevent cleanup.
- Recognized Android permission, KeyBridge ownership, helper availability, unsupported capability, Host permission/approval/safety, native timeout/configuration, and selected automatic-stop reasons receive actionable KO/EN recovery. No new logs were introduced.
- The model copies only closed issue/stage for presentation. It preserves a received automatic-stop reason in its existing error state while a following reasonless cleanup status updates native state. A received ordinary stop reason does not overwrite an existing startup stage. `cleanupFailed` / `stopFailed` dominate ordinary errors and offer manual **Leftcar** accessibility shutdown plus retry Stop, with KeyBridge kept enabled.
- The existing Notice/Label primitives and Leftcar tokens are used. Unknown failure prose states that the precise cause is unconfirmed and supplies stage-specific recovery. Unsupported and KeyBridge recovery no longer suggest disabling KeyBridge.
- Prepared capabilities, consent, geometry checks, authenticated/pinned paired Wi-Fi requirements, selection fencing, native lifecycle cleanup, and physical key ownership enforcement are unchanged.

## Red / green evidence

Commands were run from the worktree root. Logs contain static test fixtures only; no live credentials or user input were read.

1. Lifecycle/connection RED:
   `bunx vitest run apps/viewer-expo/src/km-session.test.ts apps/viewer-expo/src/connect-flow.test.ts`
   Exit **1**; **10 failed / 27 passed**. Missing stage, known native cause normalization, returned native reason, and null-origin precondition failed as expected. Explicit cancellation and existing fencing tests passed.
   Log: `/private/tmp/leftcar-km-start-red.log`.
2. Presenter RED:
   `bunx vitest run apps/viewer-expo/src/input-sharing-view.test.tsx`
   Exit **1**; **11 failed / 9 passed**. Eight localized stage/recovery cases, two KeyBridge native-reason cases, and cleanup recovery precedence failed as expected. Secret-shaped unknown reason remained generic.
   Log: `/private/tmp/leftcar-km-start-view-red.log`.
3. Safety refinement RED:
   `bunx vitest run apps/viewer-expo/src/km-session.test.ts apps/viewer-expo/src/input-sharing-view.test.tsx`
   Exit **1**; **1 failed / 42 passed**. A throwing diagnostic message getter interrupted cleanup before the guard was added. The regression now verifies generic safe failure and native Stop/peer close.
   Log: `/private/tmp/leftcar-km-start-normalizer-red.log`.
4. Final JS GREEN:
   `bunx vitest run apps/viewer-expo/src/km-session.test.ts apps/viewer-expo/src/connect-flow.test.ts apps/viewer-expo/src/input-sharing-view.test.tsx`
   Exit **0**; **3 files / 59 tests passed**. Includes original lifecycle and authenticated connection coverage, four phase seams, safe structured Host/native messages, secret-shaped inputs, real screen/primitives rendered KO/EN recovery, and cleanup precedence.
   Log: `/private/tmp/leftcar-km-start-final-green.log`.
5. Final repository typecheck:
   `bun run typecheck`
   Exit **0**, UI-theme check, exact React-runtime compatibility check, workspace `tsc -b`, Viewer typecheck, and Host typecheck all passed.
   Log: `/private/tmp/leftcar-km-start-typecheck-final.log`.
6. Fresh scoped native regression run:
   `JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' apps/viewer-expo/android/gradlew -p apps/viewer-expo/android :app:testDebugUnitTest --rerun --tests dev.leftcar.viewer.km.KmRuntimeRecoveryTest --tests dev.leftcar.viewer.km.KmRuntimeCompletionTest --console=plain`
   Exit **0**, `BUILD SUCCESSFUL in 9s`. XML confirms runtime completion **6 tests** and recovery **4 tests**, **0 failures / 0 errors**, fresh timestamps `2026-10-03T14:38:01.447Z` and `2026-10-03T14:38:05.265Z`. Task `:app:testDebugUnitTest` executed rather than using an inherited UP-TO-DATE result. Existing Gradle 10 deprecation summary remains. Initial sandbox cache-lock denial was resolved by the authorized scoped tool escalation; no device operations occurred.
   Log: `/private/tmp/leftcar-km-start-native-fresh.log`.
7. `git apply --reverse --check .superpowers/sdd/2026-10-03-input-only-km/start-diagnosis.diff` and `git diff --check`: both exit **0**.

React Doctor was deliberately delegated to Root under the task brief: exact `npx -y react-doctor@latest . --verbose` after integration, followed by typecheck/relevant tests if it requires source fixes. No React Doctor pass is claimed here.

## Scope and integration artifacts

Nine source/test files only:

- `apps/viewer-expo/src/km-failure.ts` (new finite safe normalization seam)
- `apps/viewer-expo/src/km-session.ts`
- `apps/viewer-expo/src/km-session.test.ts`
- `apps/viewer-expo/src/session.ts`
- `apps/viewer-expo/src/connect-flow.test.ts`
- `apps/viewer-expo/src/use-km-model.ts`
- `apps/viewer-expo/app/input-sharing.tsx`
- `apps/viewer-expo/src/input-sharing-view.test.tsx`
- `packages/ui-tokens/src/i18n.ts`

Exact byte snapshots of all eight pre-existing files were saved **before editing** under `start-diagnosis-before/<source path>.before`; `.before` suffix prevents archived tests becoming duplicate runnable Vitest sources. `base-sha256.json` records their original hashes. `km-failure.ts` is newly added and has a null base hash.

- `start-diagnosis.diff`: only this task's delta against the saved uncommitted base, including the new file; not a diff against clean HEAD.
- `start-diagnosis-manifest.json`: source paths, snapshot paths, base/after SHA256 and byte counts, HEAD, and diff hash. The original archive hashes were re-verified during generation.

Simplify/self-review: moved the closed error/mapping contract to one small shared seam, consolidated screen recovery into one presenter, merged duplicate imports, retained one stage variable at dependency boundaries, and removed an unused issue. Reviewed for raw cause retention/rendering, unsafe substring matches, cleanup precedence, intentional abort, stale fencing, native feature/ownership bypasses, and unrelated UX changes. No findings were hidden with ignore rules or generated edits.

## Remaining concerns and next live step

- Root must integrate only the nine manifest files, check base hashes before overwrite, run the exact React Doctor gate, package/install within the original authorization, and attempt Start with KeyBridge enabled. Record only the displayed localized stage/reason and status. If it says Android preparation/helper unsupported, that narrows diagnosis but does not prove an OEM API defect; obtain a red-capable native seam before a broader native repair.
- Some native `task` exceptions intentionally become a generic runtime reason, so unavailable details cannot be reconstructed by the JS mapper. The physical-key English guard is safely mapped if supplied, but the current native generic catch may not supply it. Unknown reasons stay generic; this task does not claim all native root causes are now identified.
- Automatic-stop display is improved within the existing mounted model and rendered presentation; no new real React Native hook/device lifecycle acceptance is claimed. Explicit Refresh/focus/remount may still clear model-only causes. F3 persistence/retry-remote-stop semantics and F4 entry discoverability remain separate audit work.
- F1 and F3–F7 were not implemented. Existing shared busy semantics can still describe non-start operations as start work (F5), and Host normal-OFF warnings remain F1.
