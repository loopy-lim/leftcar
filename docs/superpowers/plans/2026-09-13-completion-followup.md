# Leftcar completion follow-up implementation

> Use subagent-driven-development for implementation and task review. Continue through the approved scope without routine approval pauses.

**Goal:** Close the audit's remaining code and release-preparation gaps in a separate checkout. The other active session completed PR #5/CI/merge; this follow-up now uses its merged base and explicit ownership handoff.

**Architecture:** Keep current source grants, input default OFF, authenticated media, native setting generations and display capture architecture. Repair the persistence and AOAP ownership boundaries, restrict diagnostic persistence, and make release scope and evidence executable and explicit.

**Tech stack:** TypeScript/React Native, Expo SecureStore, Rust/Tauri, Android XML/Gradle, Bun/Vitest, macOS Swift capture shim.

**Spec:** `docs/research/2026-09-13-completion-readiness.md`, especially sections 4–8, approved for execution in the user's follow-up.

## Global constraints

- Work only in `.worktrees/completion-followup` on `codex/completion-followup`, initially based on `a62d908`, now fast-forwarded to merged main `5a8dde4`. Do not edit the other session's worktree or main's untracked research documents.
- PR #5 CI repair and merge were completed by thread `01a08f71-c96d-7001-8675-5bd643c77e9a`. It explicitly relinquished workflow ownership; `.github/workflows/ci.yml` may receive the reviewed same-ref SHA pins here. Preserve its `tools/verify.mjs`, `tools/verify.platform.test.mjs`, `file_transfer.rs` and `source_grants.rs` changes. Do not edit the original checkout or original untracked audit reports.
- Preserve current user data and keys. Do not launch installed apps, install/uninstall packages on devices, change network mappings, capture screens/audio, or enable input. The user's existing decision is to run physical-device checks later themselves.
- Do not restore virtual display management, introduce new codecs/relay/XR Full Space, enable experimental defaults, or remove existing optional audio/files/clipboard features.
- Read-only research and local build/test/package preparation are authorized. Public publishing, production credential registration and device acceptance are separate; prepare concrete artifacts and state evidence boundaries.
- After React/component/style changes run `npx -y react-doctor@latest . --verbose` from this worktree root, require `100 / 100`, fix source and rerun typecheck and relevant tests. Use fnm Node. Do not hide findings.
- Use meaningful failing regression tests before behavior changes. Test results, builds, package hashes and physical runtime are distinct evidence levels. Keep implementation changes scoped, reviewable and reversible.
- Implementers do not spawn subagents or alter git history. Leave edits uncommitted for controller snapshot/review unless controller explicitly requests a scoped commit. Do not push.

### Task 1: Viewer preference persistence and recovery

**Files:** `apps/viewer-expo/src/viewer-preferences.ts`, its tests, `use-catalog-model.ts`, relevant catalog model/UI and i18n files; a focused persistence controller/hook and tests may be extracted if needed to exercise the actual integration.

- [x] Reproduce missing versus failed SecureStore reads and silent writes. Treat a missing value as defaults, but a read/parse failure must not trigger saving defaults over existing stored data.
- [x] Preserve the existing profile/FPS/cursor/audio defaults and serialization compatibility. Loading failure must leave the stored value untouched until a successful retry; controls must not silently imply a persistent save.
- [x] Expose loading/saving failure in Korean and English through the existing catalog UI with a reachable retry or explicit rollback. Prevent hydration from overwriting an edit and older asynchronous writes from winning over newer ones.
- [x] Apply the same error/lifecycle discipline to clipboard preference persistence; do not alter clipboard transport or native stream setting generations.
- [x] Regression tests cover failed hydration followed by recovery, missing initial storage, write rejection and retry, rapid edits/out-of-order completion, disposal, and successful reloading of saved values. Prefer testing the actual controller integrated by the hook over source-pattern assertions.
- [x] Run focused Vitest, typecheck and React Doctor (100/100), then relevant tests again after fixes. Report commands and RED/GREEN evidence; physical app restart remains a separate user check.

### Task 2: AOAP media proxy lifecycle

**Files:** `apps/host-desktop/src-tauri/src/aoap_proxy.rs`, focused adjacent lifecycle helper/tests only if necessary. Inspect AOAP channel ownership/call sites before choosing synchronization.

- [x] Reproduce stop followed by immediate start using deterministic worker/channel tests without USB devices.
- [x] Make stop completion and worker ownership explicit. Reap/join the previous worker before permitting replacement, without holding a lock that the worker needs. A stale worker cannot clear a newer active proxy or release its USB channel.
- [x] Handle thread spawn failure and natural completion so the active slot/channel cannot remain stranded. Serialize concurrent starts/stops and keep repeated stop safe. Do not introduce an unbounded wait in an async control path without documenting and resolving its blocking behavior.
- [x] Preserve current transport framing and supported single-stream AOAP behavior. Do not implement split over AOAP.
- [x] Test start/stop/restart, duplicate start, worker exit and spawn/start failure ownership. Run Host focused Rust tests, fmt and Clippy; physical cable disconnect/reconnect remains separate.

### Task 3: Android backup and Host diagnostic privacy

**Files:** Android backup/data-extraction XML, `app.config.ts` and manifest if required; `apps/host-desktop/src-tauri/src/audit.rs` plus a focused helper if needed; `docs/platform-permissions-audit.md`, security docs as appropriate. Do not modify the other session's active files.

- [x] Exclude `SecureStore` shared preferences from full backup, cloud backup and device transfer using current Expo guidance; preserve relevant legacy exclusion. Ensure Expo config cannot silently regenerate contradictory rules. Verify the merged release manifest/resources, not only a string in source.
- [x] Inventory overlay/legacy storage permissions against actual app and dependency use. Remove unused release permissions with manifest merger removal directives where dependencies re-add them; keep debug needs in debug manifest if justified. Do not break SAF/file sharing.
- [x] Restrict persisted audit events to an allowlist and safe typed fields; arbitrary strings/objects must not write keys, tokens, IP, source titles or filenames. Keep enough event/session/reason/count information for debugging. If identifiers are needed, use per-process keyed pseudonyms with an existing cryptographic dependency.
- [x] Preserve 5 MiB/one-backup rotation and 0600 creation. Make concurrent append/rotation safe, secure the rotated legacy file, and cap serialized data. State actual retention as size-bounded local storage; do not invent a time-based purge.
- [x] Regression tests cover malicious/unexpected fields, reserved event/timestamp keys, stable within-run pseudonyms (if used), rotation, permissions and concurrent records. Run focused Host tests/fmt/Clippy and Android configuration verification.
- [x] Keep Host identity's existing file storage for this milestone, with an explicit local-account threat model, current regeneration behavior, backup/re-pair guidance and distinction from OS-protected pairing tokens. Do not migrate user keys or add an unreviewed new key-store architecture.
- [x] Document current recents/screenshot visibility and a dedicated XR/Home Space validation gate for FLAG_SECURE; do not switch it blindly without the requested compatibility evidence.

Task 3 package follow-through: merged release/debug manifests passed; final APK embedded resources are verified during Task 5, and physical backup/restore remains user-run.

### Task 4: Isolated test inventory and release preflight

**Files:** root Vitest configuration, `package.json`, new focused tools/tests, existing release manifest/build tools only as necessary, Android Gradle wrapper properties. CI workflow ownership was explicitly handed off after PR #5 completed; only same-ref SHA pins are in scope.

- [x] Make the default Vitest inventory exclude nested `.worktrees` while retaining the default exclusions and all current project tests/contract behavior. Demonstrate a nested worktree cannot inflate the test result.
- [x] Add a deterministic local release preflight that checks current component versions/ABI/shim requirements and manifests, verifies artifact hashes, and separates internal/debug signatures from distribution readiness. Build on current manifest tools; do not create a second incompatible receipt contract.
- [x] Produce release dependency inventories from lockfiles/current resolved dependencies, record license information where available and unknowns explicitly. Run current official vulnerability tools where available; report actual unresolved findings rather than ignore/advisory suppressions or broad untested upgrades.
- [x] Verify/pin the Gradle distribution checksum from its official source if missing. After the PR5 owner explicitly relinquished workflow ownership, pin the same 22 Actions references to freshly verified full commit SHAs; preserve job behavior and record provenance.
- [x] Add tests for fail-closed invalid/missing release records, mismatched component requirements and signature classification. Keep secrets out of inputs/logs and do not require production credentials for internal packaging.
- [x] Keep the current 1800-second collector maximum: the user explicitly changed the physical stability gate to 30 minutes in the active PR5 session. Require referenced Host/Android/source manifests to identify the same source snapshot using one shared validator; reject mixed, missing or altered source evidence before collection.

### Task 5: Scope, support, device handoff and local release candidate

**Files:** README, docs index, PRD, security/versioning/risk/EVIDENCE documents, one current completion/support/acceptance document and JSON receipt. Historical evidence stays historical.

- [x] Reconcile input default OFF/source grants and duplicate FR-021 identifiers; state implemented capture is display-only. Classify implemented optional features and experimental defaults truthfully.
- [x] Create an OS/device/transport/codec/profile/window-count support table with source/build/historical/current physical evidence separated. Preserve 4K/XR/latency goals and apply the user's explicit 60→30-minute stability decision consistently. Historical 60-minute targets remain labeled historical; no new physical results are claimed.
- [x] Update release links against actual latest GitHub assets; document Host/Viewer/shim/protocol version roles and compatibility, installation/manual updates, debug-to-release migration, data/pairing retention, rollback and issue reporting.
- [x] Provide a concrete user device checklist: exact source/package hashes, LAN control then media routing, three short runs then 10/30 minutes, display/input permissions, lifecycle/XR/Windows matrices and no unsupported result claims. Reuse existing collectors.
- [x] Reconcile the other session's merged CI changes without duplicating its work. Run final appropriate verification, React Doctor and full verify once on the final source after all edits; compare before/after input digests.
- [x] Build exact internal macOS arm64 Host and Android arm64 Viewer candidates with existing build tools and verify manifests/embedded JS/native/signature classifications. Do not install, run capture or publish. If a toolchain/credential gate prevents an artifact, preserve logs and complete all unaffected tasks.
- [x] Record completed code/review/test/build results and every remaining human/device/signing/publication dependency in a durable handoff. Keep implementation integration separate from public release acceptance.
