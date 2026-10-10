# Scoped preparation diagnostics review

## Verdict and scope

SPEC: PASS on source inspection. QUALITY: PASS on source inspection. Critical findings: none. Important changed-source findings: none. Installation remains gated on the root normal full integrated Android internal build: Kotlin compilation and JS packaging are not yet verified.

Reviewed only the temporary one-file diagnostic delta in `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/KmRuntime.kt`, from the saved preparation preimage. This does not reopen the whole native implementation review or claim a startup fix, helper capability, device acceptance, or real input sharing acceptance. No production edits, subagents, compilation, device operations, or unrelated native/React suites were performed by this reviewer. The requested review report is the only write.

Read inputs: prepare-checkpoints-review-brief.md, prepare-checkpoints-brief.md, prepare-checkpoints-report.md, prepare-checkpoints.diff, the frozen current KmRuntime.kt, and its saved preimage. Used the two-axis code-review approach within the explicit one-file/no-subagents scope.

Verified SHA-256:
- Before: `70d3893ae85ef872b7624959b1ba151738e2b6525f74d5809d4c2af9d63a4234`.
- After: `c3805127940777224e6365459ab816b91ae0ffa93a6a2c97eadbb6dd265a6190`.

An independently reconstructed unified diff with 18 context lines matches the frozen diff byte-for-byte after its explanatory header. Changed-code locations below refer to the frozen after source.

## SPEC assessment

- Privacy and rate: lines 47–68 implement a private probe with synchronized budget, stage, and closed state. The check/increment/log attempt share one monitor; at most 20 log attempts occur per accepted preparation. The sole tag is `LeftcarKmPrepareProbe`. All actual event/stage arguments are fixed literals. The only dynamic logged field is `error.javaClass.name` at lines 143 and 146. There are no exception messages, stacks, token/generation values, Binder identity, inventory, payload, or input values.
- Preparation isolation: only preparation entry (line 154) and its connection task (line 188) opt into catch logging. The unchanged receive/motion/key/raw-mouse task callers use the null default. No sustained input, heartbeat, watchdog, flush, ownership transition, or touchscreen capture diagnostics are introduced.
- Lifetime: preparation allocates one local probe after its original off-state gate (lines 155–159). Async callbacks retain that exact object; none retrieve the active probe. Existing generation checks remain at lease acceptance, connection, and timeout. Ready closes atomically with its final checkpoint (lines 63–66, 227); stop/cancellation closes before existing cleanup (line 455). A stale callback cannot emit against or consume the budget of a replacement attempt.
- Stages and races: main bind completion/failure and FGS failure pass `advanceStage = false` (lines 222, 240, 242). Therefore late main results cannot overwrite the worker stage used by its suppressed preparation exception catch.
- Existing behavior: the diff preserves eligibility, permission/authority checks, KeyBridge lease gate, failure reasons, scheduling queues, callbacks, 4000-ms timeout, cleanup order, and readiness behavior. `api.prepare(lifetime)` remains one call before the same false-result branch (lines 207–209). No additional permission, constructor workaround, input substitution, or transport behavior is added.

## QUALITY assessment

Log formatting and `Log.i` execute inside `runCatching` (lines 57–60), so a log backend/formatting failure does not propagate into the original input failure handling. Diagnostic catch suppliers are the local trivial lambdas, not product callbacks. Nested synchronization in `ready()`/`checkpoint()` uses a reentrant JVM monitor. Worker-owned active-probe state and captured attempt objects avoid an unsynchronized shared lookup from main.

The generic task signature change was checked against all its call sites in this file: unchanged trailing-lambda callers continue to supply the action parameter and do not activate diagnostics. No method-reference use exists. No evidence of changed exception reasons, duplicate callbacks, new input capture, or altered authorization was found in this delta.

## Diagnostic and validation limits

1. **FGS observation is intentionally incomplete.** Lines 220–228 retain the existing main-posted foreground-service request followed immediately by worker readiness/closure. `fgs_failed` emits only if main failure precedes closure; a later failure is silent under this tag while the unchanged `stop(...)` still runs. Similarly, a sufficiently late bind return is omitted after closure. This follows the mandatory close-at-ready lifetime rule and is not a source defect in this task. `prepare_ready` proves only that the worker reached its existing ready settlement, not FGS success or physical input acceptance.
2. **Compilation/integration remains a mandatory pending proof.** The worker report states the Gradle attempts failed in existing signing/bundle/init prerequisites before verified Kotlin compilation. Those attempts are not a pass and do not establish a Kotlin syntax error. Root must build the reviewed source using the normal full integrated internal Android flow, including real JS bundling, before installation. Do not use the scratch native-only init script as release evidence.
3. **Timing and runtime are not established here.** Synchronization and Android logging add preparation-only overhead; a 20-record cap bounds count, not latency. Thread/queue selection and deadlines are unchanged by source inspection. No runtime probe-output, Shizuku startup causality, helper capability, or real-device acceptance was verified by this review.
4. **Removal remains required after the minimum diagnostic evidence.** Use the saved preimage as the exact diagnostic removal reference while preserving any legitimate later changes, then recompile. No mirror-log tests or unrelated suite repeats are requested by this review. This Kotlin-only delta does not trigger a new React Doctor run.
