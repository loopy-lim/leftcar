# Task6 bounded startup diagnostic review

**PASS — Critical: 0; Important: 0.** No actionable source blocker found for the proposed single bounded diagnostic attempt. This is source-review approval only; compilation, packaging, device execution, and the nativeHandshake cause remain unproved.

Reviewed the binding `task-6-brief.md` first, then the report, manifest, isolated patch, exact saved preimages, changed source directly, and immediate Runtime/Duplex/Lifecycle/observation-session and native module/service call contracts. Did not re-audit completed Task5, edit product source, run tests/builds, access a device, change permissions, or delegate. Only this review artifact was written.

## Exact source identity

Independently computed SHA-256 for every manifest current/preimage file: all match. Independently checked patch SHA-256 and successfully ran the read-only `git apply --reverse --check` against the isolated patch.

| File under `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/` | Exact preimage SHA-256 | Reviewed SHA-256 |
| --- | --- | --- |
| KmRuntime.kt | `d664a9d0968ac40f22eef1e72c0d85fe4ae30b15a16dac760bfe2fd51228c145` | `86bd350248e84d8641440a7f8bdb8651a0d5e0c10a7e4095fd50a5f4ed7c1672` |
| KmDuplex.kt | `6a4298ff3104abebc61366650e3c3961035cd2fb0daade631110e35dd369158b` | `9e775cf31c2099006fef739b4540d8509558b13b749b87e9342b75d989d7cde7` |
| KmHandshakeProbe.kt | ABSENT | `030d3078be0209ee221cbefb0901f41ffa3c5f635fad01f2f8656e87124bbafa` |

Patch SHA-256: `703606b09a1e274c8ca5f79ded00b48fb3ec1c817815b54fe8f5af2148b80301`.

Also verified unchanged current/preimage identities: Lifecycle `0a4957b5bce8c5febf4e1a8d6af56baeaf0c278b54c091e66a59b3ee31b40794`; Task5 AccessibilityService `21c153b1a28620babd02a0e91c4c0d95183a1c6677c259cc4e5764e071401e58`; Cipher `00d641e333a7b83d6f9b2696e6e9cc06222c0731a8a147b529732ffd1f55190b`.

## Bounds, privacy, and lifetime

- `KmHandshakeProbe.kt:19-37`: all worker/main/reader/writer records share one synchronized counter. Ordinary records stop at 19; terminal can consume slot 20 and closes the probe. Slots are consumed before formatting/Log, so an emitter failure cannot expand the allowance. `record`/`finish` contain their own Throwable-catching boundary; diagnostic failures do not replace product exceptions or cleanup. `close` and the volatile active flag provide a common terminal/replacement boundary.
- The only new Log sink is fixed INFO tag `LeftcarKmHandshakeProbe` with fixed stage/code enums and either `-` or the exception class name, limited to 160 characters and restricted to `[A-Za-z0-9_.$]`. No Throwable object, raw message/cause/stack, payload, input data, cryptographic data, endpoint/session identifier, capability, or inventory is passed to Log. The existing `stopNow(error.message)` remains product status behavior; it is not a new logging path.
- `KmRuntime.kt:143-155` creates a probe only for a current admitted prepare in the existing off state, closes the old probe first, and captures that admission in the freshness closure. Async preparation callbacks and Duplex retain their original probe. Generic task failure reporting captures scheduling admission and the emitter checks its own admission again. A stale/closed attempt cannot log into the replacement object.
- `KmRuntime.kt:489-497` preserves synchronous cancellation before queued cleanup. A record checks the revision without acquiring the observation-session lock, avoiding a probe/session lock inversion. A record already admitted concurrently with cancellation can finish its bounded Log call, but replacement waits for the old probe's synchronized close; no late old-object record can contaminate the replacement lifecycle.
- `KmRuntime.kt:326-334` closes diagnostics after initial authenticated State application/start settlement. First-frame flags suppress subsequent reader/writer success logs; inactive probes suppress later State, heartbeat, flush, and failure records. There are no input-event or ordinary ongoing-session logs.

## Behavior preservation and diagnostic usefulness

- Runtime's original validation, prepared/capture gates, Base64/seed rules, constructor/Hello ordering, secret-zeroing finally, deadlines, admission/commit fences, failure reasons and catches remain. The added stage assignments are diagnostic context only. Native owner transition still executes neutral → ungrab → restore/configure → acknowledgement in the same order; stop cleanup still retains the original authority/death/restoration requirements.
- Duplex preserves queue size/offer semantics, socket settings/connect timeout, frame-size bounds, encryption/decryption order, strict decode, callbacks, thread count, close/zeroing behavior, and caught exception types. Splitting the nested decode expression into locals preserves call order. No cipher/provider/protocol change is present.
- Diagnostic context distinguishes config/precondition/secret setup, transport construction, TCP connect, first Hello encryption/write, first frame authentication/decode, and initial State gate/owner/neutral/ungrab/coexistence/capture/configure/commit/ack failure. Original swallowed Exception objects are supplied to the sanitized class emitter before the same original product failure handling. Runtime startup timeout, heartbeat/flush exceptions, and preparation callback exceptions also have bounded failure signals.
- The ordinary success path uses at most 15 records as documented, leaving room for a failure class and terminal under the shared 20-record cap. Concurrent failures have no guaranteed ordering. `STARTUP/APPLIED` is a startup settlement marker, not live-input acceptance; `STARTUP/STOPPED` precedes or initiates product cleanup, not proof that cleanup succeeded.

The collector must retain the documented hard timeout: synchronous cancellation deliberately suppresses a later queued terminal rather than bypassing freshness to force one. After the first decoded frame, later pre-readiness decode/auth failures have the generic RECEIVE_FRAME context plus sanitized class; this is an explicitly documented diagnostic precision limit, not packet logging or a claim of exact per-frame localization. No logger can guarantee a record if Log itself fails. These limits do not block the intended one-attempt startup localization.

Root may proceed to the separately owned normal full Android internal build and exact artifact/signature verification. This review provides no compile or device success claim and makes no crypto-provider hypothesis. Remove the temporary isolated Task6 diagnostic change after the minimum real cause evidence is captured, as required by the brief.
