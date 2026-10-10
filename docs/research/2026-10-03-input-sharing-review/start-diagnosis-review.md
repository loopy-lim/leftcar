# Scoped startup attribution review

Verdict: **SPEC COMPLIANCE — CHANGES REQUIRED; QUALITY — CHANGES REQUIRED.** Open findings: **0 Critical, 1 Important** (the same finding applies to both axes). Two minor observations do not independently block the scoped repair.

Reviewed only `start-diagnosis.diff` against `start-diagnosis-brief.md`, the review brief, `start-diagnosis-report.md`, manifest, saved base snapshots, and the affected production seams. HEAD `72ff99000ab1981f9773a0b90aad3ee7ca42faad`; exact uncommitted base, not clean HEAD. No source edits, installations, grants, UI/device actions, subagents, commits, or duplicate suites. This file is the only review write.

## Important finding

### SD-I1 — Cancelled/stale startup whose dependency rejects is presented as a current failure

**Requirement:** review brief requires “true cancellation and stale host selection remain silent/fenced”; the implementation brief requires selection fencing without automated retries or activation.

**Production path/source:** `apps/viewer-expo/src/km-session.ts:68–100`, especially `:91–100`, only calls `check()` after an awaited dependency **resolves**. The catch normalizes rejected causes and, after cleanup, recognizes cancellation only by the original error's `name`. It does not check `signal.aborted` or `peer.isCurrent() === false`. `apps/viewer-expo/src/use-km-model.ts:40–42` suppresses only `AbortError`, so the resulting `KmError` becomes visible in `app/input-sharing.tsx:24–35`.

**Exact read-only production-seam reproduction:** From the worktree root, execute `bun --eval` importing the actual `KmSession` from `./apps/viewer-expo/src/km-session.ts`. Give `native.prepare()` a deferred rejecting promise, resolve `openPeer()` with a closeable fixture peer, and call `start({ arrangementSide: "right", macToAndroid: true, androidToMac: true })`. Wait until preparation has entered; call `session.cancelPending()`; reject preparation with `new Error("Shizuku 입력 서비스 연결 시간이 초과되었습니다")`. Observed:

```json
{"result":{"name":"KmError","issue":"helperUnavailable","stage":"androidPreparation"},"effects":["nativeStop","close"]}
```

Repeat with `peer.isCurrent()` initially true, set it false while preparation is pending, and reject preparation with the same static fixture error without aborting the signal. The result/effects are identical. Thus both variants complete cleanup and leave no active peer but produce a visible error attributed to a cancelled/obsolete startup. The source has the same gap for rejecting Host/native-start stages. No real device or OEM cause is inferred from these fixtures.

**Required correction:** Preserve cleanup-failure precedence and all native/Host cleanup, then retire a cancelled or stale attempt as cancellation even when its dependency rejected. Ensure the model does not retain an ordinary startup reason from that retired attempt through its status subscription. Add narrowly scoped production-seam regressions for both rejecting-after-cancel and rejecting-after-peer-selection-change, plus failed-cleanup dominance; exercise the real model/status-event ordering if needed to prove visible silence. Do not bypass gates, add retries, activate input, or change native ownership behavior.

**Provenance:** The saved base snapshot's catch had the same missing rejection fence; this is an inherited task acceptance gap, not a claim that this patch newly introduced cancellation behavior. It belongs to this review because the task changes the catch's failure presentation and explicitly requires cancellation/stale silence. Existing cancellation tests resolve late dependencies; they do not reject them.

## Spec compliance otherwise

- Four phase boundaries are represented by closed `connectionSetup`, `androidPreparation`, `hostApproval`, and `nativeHandshake` values. Normalized cause is captured before cleanup, and a non-armed native result preserves its reason.
- `km-failure.ts` exactly matches finite static `code`/`reason`/`message` values. Unknown strings, embellished known strings, nested/auth payloads, and secret-shaped strings do not enter displayed prose. Throwing diagnostic getters are guarded so cleanup still runs. No raw diagnostic logging was added.
- Null selected context is actionable `connectionRequired`; explicit pre-aborted signals and resolved stale control leases remain `AbortError`. Pinned/authenticated Wi-Fi checks remain fail-closed.
- Cleanup errors dominate startup errors; the presenter independently prioritizes native `cleanupFailed`. Recovery specifically identifies **Leftcar** accessibility and keeps **KeyBridge enabled** in both languages.
- Native capability, consent, display geometry, authority, capture, transport, and physical-key ownership gates are unchanged. The patch does not automate connection, streaming, retry, or input activation. No native source change or newly proven native defect exists in this task.
- KO/EN unknown-cause copy distinguishes phase from a confirmed root cause and gives phase-specific recovery. Recognized native/Host reasons use targeted localized recovery.

## Quality and minor observations

**SD-M1 — Model event behavior has weaker evidence than presentation.** `input-sharing-view.test.tsx` renders the real production `InputSharingScreen` and real UI primitives/tokens, with platform renderers replaced; this meets the production-component fixture requirement. It mocks `useKmModel`, so it does not independently prove the new subscription retains causes across cleanup status events. Include only the focused model/cancellation event case needed for SD-I1; a broad hook/lifecycle suite is not requested.

**SD-M2 — Known mapping cannot recover detail native discarded.** Some native `task` exceptions become the existing generic runtime reason before JS sees them. The report explicitly acknowledges this, and unknown causes remain safely generic. This is an evidence limit, not authorization to alter OEM/native behavior. Explicit Refresh/focus/remount clearing is likewise documented and belongs to the separate persistence audit.

## Evidence checked and remaining acceptance

- Independently verified all **9 current source/test SHA256 hashes and byte counts**, **8 saved-base hashes and byte counts**, and `start-diagnosis.diff` hash against the manifest. No mismatch.
- Read actual red logs: lifecycle/connection **10 failed / 27 passed**, presenter **11 failed / 9 passed**, throwing-getter refinement **1 failed / 42 passed**. Read final green log: **3 files, 59 tests passed**, and repository typecheck log. Did not rerun those suites.
- Read the fresh scoped native log: runtime completion/recovery invocation executed `:app:testDebugUnitTest`, `BUILD SUCCESSFUL in 9s`. No native suite was rerun for this review; previous lifecycle review is not reopened.
- Reviewer ran only the two narrowly targeted read-only `KmSession` rejection reproductions above after identifying SD-I1.
- Root reports React Doctor **100 / 100** and post-gate checks for the current bytes. This reviewer did not run React Doctor; necessary fix bytes require Root's gate/post-check verification again.
- Stage attribution is **not** a confirmed real-device root cause. The user's Start briefly connecting then returning OFF remains a live diagnosis issue. After the bounded fix and exact-byte integration/verification, the remaining live step is to attempt Start with KeyBridge enabled and record only the safe displayed phase/reason and state. Source/tests/render fixtures do not prove device startup, cursor crossing, or bidirectional physical input acceptance.

**Frozen review:** 0 Critical / 1 Important open; SD-I1 is the only required source correction.
