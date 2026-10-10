# Own-service delegation addenda — five-gap design re-review

## Verdict and exact inputs

All five previously reported Important gaps are addressed at the binding design-contract level for a concrete, bounded user approval decision. No additional blocking design gap was found in these addenda. Actual framework-member/OEM/in-process feasibility, safe implementation, tests, packaging, and device acceptance remain unproved prerequisites. This review does not supply user approval or authorize implementation/runtime capability use.

Final reviewed proposal SHA-256: `2be190f1eb8e09e038bff1646de60d54425a58c021ccf1d935aff2d1f1873579`.
Preserved original proposal SHA-256: `29495935a1bc8781a9347c3b334a30f630dfe2e84026dd741f19f674cc4d33ab`.

Read only observer-delegation-proposal-reviewed-before.md, the latest proposal, their diff, and observer-delegation-proposal-review.md. The delta appends only the Explicit modes/rollback and Partial-transition/targeted-access contracts; the original scope is unchanged. This re-review covers only those five original gaps and is separate from the already completed cleanup source review/build.

## Gap verdicts

| Original gap | Verdict for approval contract | Concrete closure and remaining implementation proof |
| --- | --- | --- |
| 1. System/local rollback and verification | Addressed | Every uncertain set, readback failure, stale generation, or failed commit requires both system and local OFF recovery. The contract checks actual ordinary/observed sources zero, key filter off, local dispatch zero, routing OFF; null/dead connection or setter return alone cannot prove cleanup. Unconfirmed cleanup stays cleanupFailed and blocks start/forwarding. Rollback always targets OFF, never an active snapshot. Tests and device recovery must prove this behavior after approval. |
| 2. Own-target provenance and lifecycle | Addressed with explicit trust assumption | The raw capability comes only from the current exact human-enabled own service and its freshly obtained process-local connection id. No React/network/user Binder/target parameters or global discovery exist. The helper trusts the existing authorized original app UID for provenance; descriptor/metadata checks are mistake/foreign-target guards, not cryptographic attestation against a malicious authorized app. Own instance/id/generation are checked before mutation and before commit; queued/stale work is invalidated. Own-path and user-aware UID/target checks still require source/tests. |
| 3. Finite modes and authority | Addressed | A closed OFF/OBSERVE/CONSUME table determines every mask/flag; the helper accepts only mode 0/1/2 and rejects unknown modes rather than independent source/flag parameters. Non-OFF needs current app-bound prepared authority/owner, and lease/capture-ticket guards remain before ownership commit. OFF recovery is permitted after input resources expire for the exact own target. Exact readback must match the full table, including flags. |
| 4. Local bookkeeping and partial transitions | Addressed as an explicit fail-stop prerequisite | The local write is an ordered, bounded main-thread completion tied to a live instance/attempt token before worker shell set/readback, with fresh commit checks and invalidation on cancellation. No worker/main mutual-wait dependency or late active-mask restore is allowed. Unexpected input-method-editor requests are rejected. Exact deployed setter/send/onMotionEvent audit must establish that only the allowed local dispatch field is needed; if mInfo/cache handling cannot be substantiated within the permitted path, stop. This proof remains pending, not assumed. |
| 5. Targeted member/library route | Addressed as an explicit fail-stop prerequisite | The pinned 6.1 artifact requires digest/license review and actual in-app compatibility. Required own getter/cache/field/observed getter members and shell getServiceInfo/setServiceInfo are allowlisted. There are no global exemptions, arbitrary target/window discovery, broad mutation, or alternate bypass fallback. Production-adapter seam tests must establish guard/rollback/generation order. Missing/unsupported prerequisites leave explicit unsupported status and stop the route. Actual member access is not claimed to work. |

## Correction to the original review

The original review's illustrative CONSUME tuple prescribed observed sources zero. That prescription should not be followed. The addendum explicitly keeps mouse observation enabled in both OBSERVE and CONSUME; actual physical exclusivity remains the existing helper EVIOCGRAB/ownership operation. This resolves the finite-table gap while preserving the stronger non-consuming observation invariant and avoiding a subscription-based capture workaround. This is a correction to the earlier review suggestion, not a new capability authorization or device proof.

## Approval direction and limits

The user can now review and approve the bounded investigation/direction with a concrete scope: targeted access to only the current own human-enabled accessibility connection and local dispatch field, passing that exact capability to the existing app-bound shell helper, and a closed configure/OFF recovery operation. The explicit provenance trust assumption should remain visible in the resulting specification and code.

That approval would authorize only the stated route and its necessary scoped work, not a guarantee that private members, library compatibility, or OEM behavior will succeed. Before any nonzero connection subscription or safe implementation claim, substantiate exact OEM dispatch/bookkeeping, in-process targeted-member behavior, own-target/lifetime guards, and rollback semantics. Failed prerequisites stop with unsupported/unconfirmed status; they do not authorize broader fields, targets, permissions, global exemptions, monitors, or alternate input capture.

Required later evidence remains separate: meaningful production-adapter tests, independent source/security review, normal integrated Android build and exact package/signature/update verification, then specifically authorized device observation/input/recovery proof. Existing Shizuku approval, design wording, a dependency version, getter equality, and earlier artifacts do not establish those levels. User approval of the added private connection capability is still pending.

No source/device/suite/network/subagent actions occurred. Only this report was written.
