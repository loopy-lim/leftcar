# Task 1 review: domain safety, wire contract and crypto

Scope: the supplied `task1.diff` against the initial untracked foundation, the Task 1 plan/shared constraints and implementer report. Source changes, commits and broad test reruns were excluded. Line references below refer to the reviewed worktree. No React changes are in this task.

## Spec verdict

**Not yet compliant: 1 Important finding, 0 Critical findings.** Domain release ordering, capture separation and key derivation meet the requested Task 1 behavior; the generated schema does not represent the wire validator's bounds.

### Important: emitted JSON Schema accepts envelopes and negotiation geometry rejected by the actual decoder

Locations: `crates/control-contract/src/km.rs:133-141`, payload numeric fields at `:35-56`, and `KmStartInput` dimensions at `:169-176`.

Requirement: bounded camelCase envelope/negotiation matching the schema, including validated version/session/epoch/seq and bounded numbers.

The DTOs derive `JsonSchema` without schema constraints for the manual limits enforced by `KmEnvelope::validate` / `KmStartInput::validate`. A focused temporary probe printed the actual `schemars::schema_for!(KmEnvelope)` and `schema_for!(KmStartInput)` from the current compiled contract. It emitted:

- `version`: integer, minimum 0, no const/enum requiring 1.
- `epoch`/`seq`: integer, minimum 0; `sessionId`: unconstrained string.
- `key.code`: uint16 minimum 0, no maximum 255; `move.dx`/`dy`: unconstrained numbers.
- Cursor and start dimensions: uint32 minimum 0, no 1..32768 bound.

Concrete trigger: schema-valid envelopes with `version=2`, `epoch=0`, `key.code=256` or `move.dx=65537` are accepted by those schema fragments but `KmEnvelope::decode` rejects each, confirmed by the same probe. A schema-derived peer validator or generated client therefore advertises a different contract and can produce traffic rejected by the Rust receiver. `tabletWidth=0` has the same mismatch at negotiation.

Recommendation: add schema annotations or a custom schema implementation for constant version, nonzero headers, scalar ranges and dimensions. Represent session UTF8 byte/control restrictions and cursor coordinates relative to dimensions as explicitly documented runtime invariants where Draft 7 cannot directly express them. Add a focused schema/decoder parity regression for representable bounds. Keep the manual receive validator as the authority.

The unit-payload concern was checked and cleared: emitted hello/releaseAll/heartbeat/stop schemas carry `additionalProperties:false`, and the private struct-variant deserializer rejects extra fields consistently with those schemas.

## Code quality verdict

**Pass at the reviewed scope: 0 Critical / Important quality findings.**

- `ReturnToLocal` releases injected holds first, stops Mac suppression or Android key filtering plus consuming capture, restores local input and preserves Armed consent/watchdog (`domain/src/km.rs:645-660`). Active consuming capture is not retained.
- Armed observation is gated by `android_observe_and_pass`; the public consuming subscription flag cannot activate it. Android cursor injection and independent mouse capture are explicit preflight gates, and `mouseCapture` is a distinct required wire boolean.
- `KmEnvelope::decode` enforces the 8192-byte bound and payload validation; `validate_for` checks trusted session, current epoch and authenticated origin. Unknown envelope/payload fields are denied, including unit payloads. HID usages are explicitly documented.
- `km_keys` uses HKDF-SHA256 with sessionId UTF8 salt, `leftcar/km/v1`, 64 bytes split c2s/s2c; the fixture includes both independently derived directional keys and fixed CryptoKit ciphertexts with counter1 nonce `00000000 || u64BE(counter)`.
- The StrictPayload duplication is a justified serde unit-variant workaround, rather than an unnecessary abstraction.

## Evidence and limits

Accepted the supplied fresh Rust verification: domain 50; control-contract 21 unit +20 integration; secure-channel 14 with 2 preexisting ignored fixture printers; scoped clippy exit 0. No suite was rerun. The only additional execution was a named schema/decoder interoperability probe under `/private/tmp`, linked to the current control-contract artifact and its matching schemars/serde_json dependencies. Probe source and output: `/private/tmp/leftcar-km-task1-schema-probe.rs`, `/private/tmp/leftcar-km-task1-schema-probe-output.txt`. The probe did not modify product source.

This review establishes the scoped domain/DTO/crypto behavior and does not establish OS authority, physical input or full runtime acceptance.

## Focused cross-task risks for integration review

1. **Trusted ingress / epoch / sequence:** every transport ingress must call the bounded decoder and `validate_for` using authenticated context, then enforce strict sequence ordering. Derived serde deserialization alone does not call validation; the DTO intentionally delegates replay/state authority to transport.
2. **Release application order:** consumers of `ReturnToLocal` must actually apply/send release under the old owner before restoring local input and announcing the new epoch/state. The ordered effect list alone does not prove remote release timing.
3. **Observer and consuming capture mapping:** `mouseObservation` must reflect verified physical observe-and-pass; `mouseCapture` must reflect separately verified consuming capture. Public accessibility motion observation must never implement Armed edge watch, and native consuming capture must stop on normal return.
4. **Physical provenance / HID adapters:** returning synthetic remote cursor motion must not activate the opposite physical source. Both OS adapters must map keycodes to/from USB HID keyboard-page usage IDs, preserving modifier usages such as 224.

These are named integration obligations, not additional Task 1 findings; wider integration remains with the root reviewer.

## Scoped re-review: schema fix round 1

**Original Important finding: addressed. Revised Spec verdict: pass at the reviewed Task 1 scope. Revised quality verdict: pass. No new Critical / Important findings in the fix.**

Reviewed only `/private/tmp/leftcar-km-task1-schema-fix.diff` and the corresponding implementer evidence. The fix adds actual schemars constraints for version exactly 1, nonzero epoch/seq, sessionId length, every move/wheel axis, button/HID usage limits, cursor scalar/dimension limits and both start dimensions. These cover the previously demonstrated version2/epoch0/key256/move65537/tabletWidth0 mismatch. DTO wire shapes and receiver validation are unchanged.

The remaining UTF8 byte/control rules, relative cursor coordinates, plaintext byte bound and trusted transport context are explicitly documented as runtime invariants. The UTF8 length and sibling-coordinate distinctions are also covered by a focused regression, so the schema is no longer silently advertised as enforcing those nonportable semantics.

Accepted supplied fix evidence: focused parity RED 0 passed /3 failed, then GREEN 3 parity unit tests plus 1 filtered schema integration test passed; full control-contract 25 unit +20 integration passed; scoped clippy exit 0. The tests inspect actual emitted schema fragments and evaluate the limited bound keywords relevant to their fixtures, with that scope stated clearly. No tests or probes were rerun during this re-review.

The initial review's named cross-task obligations remain integration review scope; this fix introduces no new obligations there.
