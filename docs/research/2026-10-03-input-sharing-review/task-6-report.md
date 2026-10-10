# Task6 — temporary native startup handshake diagnostics

Source frozen in `/private/tmp/leftcar-km-20261003`. This task adds diagnostics only; the physical handshake cause has **not** yet been established or fixed. No compile, packaging, install, launch, device/UI/log capture, grant, Host, React, helper, protocol, cipher/provider, dependency, timeout, commit or subagent actions were performed.

## Exact artifacts and scope

Read the binding `task-6-brief.md` first, then current `KmRuntime.kt`, `KmDuplex.kt`, `KmLifecycle.kt`, `KmWire.kt`, `KmCipher.kt`, `KmModule.kt`, `KmConnectionService.kt` and the real `KmObservationSession` admission/cancellation implementation in `KmOwnObservation.kt`. Repository AGENTS.md has the React gate; no React source or behavior was changed.

Changed only `KmRuntime.kt`, `KmDuplex.kt`, and new `KmHandshakeProbe.kt`. Existing product preparation/start completion, admission/observation fencing, generation checks, neutral/ungrab/restore/ack ordering and exact failure reasons remain. `KmLifecycle.kt`, the Task5-reviewed `KmAccessibilityService.kt`, and unchanged `KmCipher.kt` were saved and hash-checked unchanged.

Every modified existing file was copied byte-for-byte before any source edit under `task-6-before/apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/km/`. `task-6-before/preimages.json` records those SHA-256 values and explicitly records the new probe's absent preimage. Unchanged critical sources were also copied for reference. `task-6-only.patch` is a unified diff against those exact preimages, including `/dev/null` for the new file. `task-6-hashes.json` is the machine-readable manifest.

| File | Preimage SHA-256 | Frozen current SHA-256 | Status |
| --- | --- | --- | --- |
| KmRuntime.kt | d664a9d0968ac40f22eef1e72c0d85fe4ae30b15a16dac760bfe2fd51228c145 | 86bd350248e84d8641440a7f8bdb8651a0d5e0c10a7e4095fd50a5f4ed7c1672 | changed |
| KmDuplex.kt | 6a4298ff3104abebc61366650e3c3961035cd2fb0daade631110e35dd369158b | 9e775cf31c2099006fef739b4540d8509558b13b749b87e9342b75d989d7cde7 | changed |
| KmHandshakeProbe.kt | ABSENT | 030d3078be0209ee221cbefb0901f41ffa3c5f635fad01f2f8656e87124bbafa | changed |
| KmLifecycle.kt | 0a4957b5bce8c5febf4e1a8d6af56baeaf0c278b54c091e66a59b3ee31b40794 | 0a4957b5bce8c5febf4e1a8d6af56baeaf0c278b54c091e66a59b3ee31b40794 | unchanged |
| KmAccessibilityService.kt | 21c153b1a28620babd02a0e91c4c0d95183a1c6677c259cc4e5764e071401e58 | 21c153b1a28620babd02a0e91c4c0d95183a1c6677c259cc4e5764e071401e58 | unchanged |
| KmCipher.kt | 00d641e333a7b83d6f9b2696e6e9cc06222c0731a8a147b529732ffd1f55190b | 00d641e333a7b83d6f9b2696e6e9cc06222c0731a8a147b529732ffd1f55190b | unchanged |

Task6-only patch SHA-256: `703606b09a1e274c8ca5f79ded00b48fb3ec1c817815b54fe8f5af2148b80301`.

## Exact emitted contract for bounded collector

Android INFO tag: **`LeftcarKmHandshakeProbe`**.

Entire message, anchored, without any additional fields:

```text
stage=<STAGE> code=<CODE> class=<CLASS>
```

`CLASS` is `-` when no exception was supplied. Otherwise it is only `Throwable.javaClass.name`, truncated at 160 characters, with every character outside `[A-Za-z0-9_.$]` replaced by `_`. No Throwable is passed to Log. No messages, causes, stacks, input/payload fields, keys, plaintext, ciphertext, session IDs, network address/port, capabilities or inventory enter the emitter. Exact ordinary platform exception class names remain intact.

Closed stage enum (the collector can whitelist exactly these values):

```text
PREPARE PREPARE_READY START_CONFIG START_PRECONDITION START_SECRET TRANSPORT_SETUP
SOCKET_CONNECT SOCKET_CONNECTED HELLO_QUEUE HELLO_ENCRYPT HELLO_WRITE HELLO_WRITTEN
RECEIVE_FRAME RECEIVE_AUTH RECEIVE_DECODE RECEIVE_DECODED
STATE_GATE STATE_OWNER_GUARD STATE_NEUTRAL STATE_UNGRAB STATE_CONTEXT
STATE_COEXISTENCE STATE_CAPTURE STATE_CONFIGURE STATE_COMMIT STATE_ACK STATE_APPLY
STATE_WAIT HEARTBEAT FLUSH STARTUP
```

Closed code enum: **`BEGIN OK FAILED APPLIED STOPPED`**.

Suggested message allowlist regex, anchored:

```regex
^stage=(PREPARE|PREPARE_READY|START_CONFIG|START_PRECONDITION|START_SECRET|TRANSPORT_SETUP|SOCKET_CONNECT|SOCKET_CONNECTED|HELLO_QUEUE|HELLO_ENCRYPT|HELLO_WRITE|HELLO_WRITTEN|RECEIVE_FRAME|RECEIVE_AUTH|RECEIVE_DECODE|RECEIVE_DECODED|STATE_GATE|STATE_OWNER_GUARD|STATE_NEUTRAL|STATE_UNGRAB|STATE_CONTEXT|STATE_COEXISTENCE|STATE_CAPTURE|STATE_CONFIGURE|STATE_COMMIT|STATE_ACK|STATE_APPLY|STATE_WAIT|HEARTBEAT|FLUSH|STARTUP) code=(BEGIN|OK|FAILED|APPLIED|STOPPED) class=(-|[A-Za-z0-9_.$]{1,160})$
```

Terminal records are **`stage=STARTUP code=APPLIED class=-`** and **`stage=STARTUP code=STOPPED class=-`**. APPLIED follows the original authenticated State ownership application and exactly-once start settlement. STOPPED closes startup diagnostics at failure cleanup or duplex failure. A terminal record is diagnostic completion only, not proof that OS cleanup or ongoing input sharing succeeded. Concurrent thread records have no prescribed order; the root's collector may stop terminal + 2 seconds, hard timeout 120 seconds, and at most 20 accepted records.

A synchronously cancelled admission intentionally suppresses further records, including a queued terminal. Such a case uses the collector's bounded timeout, rather than bypassing cancellation/stale fencing to force a terminal log. Async duplex errors emit their class and terminal before invoking the original product failure callback (which synchronously cancels admission).

## Boundary meaning

| Stage(s) | Signal |
| --- | --- |
| PREPARE / PREPARE_READY | An admitted preparation begins / authority observation setup committed. Preparation exception catches retain the original product reasons. |
| START_CONFIG | Runtime configuration validation begins; class on FAILED. The React Native adapter's existing earlier `km_invalid_config` validation is unchanged and uninstrumented. |
| START_PRECONDITION / START_SECRET | Failure-only current context for native prepared/capture gates or Base64/32-byte seed validation. Values are never recorded. |
| TRANSPORT_SETUP | Failure-only current context for constructor/property setup, including untouched production key derivation before `KmDuplex.init`. |
| SOCKET_CONNECT / SOCKET_CONNECTED | TCP setup/connect begins / connect returns. Constructor caught class is retained, then rethrown exactly as before. |
| HELLO_QUEUE | Original first Hello encoding/enqueue begins. |
| HELLO_ENCRYPT / HELLO_WRITE / HELLO_WRITTEN | First queued frame encryption begins / framed write+flush begins / returns. No later packet success logs. |
| RECEIVE_FRAME / RECEIVE_AUTH / RECEIVE_DECODE / RECEIVE_DECODED | First frame wait/read, authentication attempt, strict wire decoding attempt, decoding returns. Read exceptions retain the exact caught class and original failure callback reason. After the first decoded frame, later pre-readiness receive failures use RECEIVE_FRAME without per-packet logging. |
| STATE_APPLY | Initial authenticated State begins native ownership application. |
| STATE_GATE / STATE_OWNER_GUARD | Failure-only current context for epoch/sequence admission or initial owner guard/discard/transition entry. |
| STATE_NEUTRAL / STATE_UNGRAB | Failure-only current context for the original releaseAll / releaseMouse steps. |
| STATE_CONTEXT / STATE_COEXISTENCE | Failure-only current context for authority/accessibility availability or original conflicting-observer checks. |
| STATE_CAPTURE / STATE_CONFIGURE | Failure-only current context for original Android physical capture checks/API or accessibility mode/ownership configure. |
| STATE_COMMIT / STATE_ACK | Failure-only current context for observation-session owner/status/start-completion commitment or original transition heartbeat acknowledgement. |
| STATE_WAIT / HEARTBEAT / FLUSH | Existing startup deadline/watchdog/flush failure; exceptions retain class, existing timeout has class=-. |
| STARTUP | Terminal APPLIED or STOPPED; no exception class here. |

Failure-only current-stage assignments replace worker context without logging each step. This distinguishes the initial State release/configuration/ownership chain while keeping the logger small. The stage context is worker-owned; duplex reader/writer maintain separate local first-frame contexts.

## Bounds, threads and stale lifecycle

One probe is created only after a prepare admission is current and existing status is off. The previous probe is closed before replacement. The same probe instance is captured by preparation main callbacks and passed to `KmDuplex`; native worker catches, reader and writer all call its synchronized record/finish methods. There is one shared record count, not per-thread budgets. Nonterminal calls stop at 19; finish may emit record 20, then marks the probe inactive. Count increases before Log, so a diagnostic output failure cannot reopen a slot.

An ordinary successful startup has at most **15** records: prepare+ready (2), runtime config (1), connect begin+success (2), Hello queue (1), first Hello encrypt/write/success (3), first receive read/auth/decode/success (4), State apply begin (1), terminal applied (1). Thus the failure/class and terminal remain within the budget even around the last ownership step. All logging is capped regardless of concurrent faults. No input-event records exist. No ordinary active-session records are emitted: initial State settlement closes the probe, and read/write success calls are guarded by thread-local first-frame flags.

Freshness checks use the admitted `KmObservationSession` ticket. Cancel invalidates it synchronously as before. A closed previous object cannot log after a replacement begins because record/finish/close share the same monitor; replacement closes the old instance first. Async preparation/duplex callbacks hold their original probe rather than looking up a replacement. Generic native task failure logging additionally captures the scheduling admission, so a queued stale task cannot emit into a different admission's current probe. Existing product generation/cancellation/callback gates are unchanged; logging does not widen any gate.

Every diagnostic record and terminal emitter catches Throwable internally via runCatching. Only fixed enums and sanitized class names reach Log. No diagnostic exception replaces a product exception or cleanup reason. Product catches still stop/rethrow/fail with the original reasons, product timeouts and packet format remain unchanged, secret zeroing stays in its original finally, and socket/worker/authority cleanup remains original. The added cost is bounded startup-only Log calls and cheap inactive/context checks. No new thread, scheduling task, retry, wait, socket, queue or timer was added.

## Self-review and evidence boundary

Checked the Task6-only diff directly against saved preimages. `git apply --reverse --check .superpowers/sdd/2026-10-03-input-only-km/task-6-only.patch` passed, proving that the isolated patch matches the frozen current sources (this is not compilation). Manifest generation verified exact equality for unchanged Lifecycle, Task5 Service and Cipher hashes. The only added Log call is the temporary fixed-tag emitter.

**Compile/package/signature proof pending**, owned by root after task-scoped independent review, using the normal full Android internal build. No mirror logger tests or isolated build/test exclusions were added or run. **Device/start/input proof pending**; no new handshake attempt occurred. This task does not claim native start success, physical input acceptance, restored permissions, or a cipher/provider bug. Root should collect the minimum exact-tag cause from one reviewed/verified build and human Start, then remove this temporary Task6-only diagnostic patch immediately once sufficient cause evidence exists.
