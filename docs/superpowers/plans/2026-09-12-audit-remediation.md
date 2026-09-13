# Leftcar Audit Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 승인된 전체 감사의 잔여 결함과 성능 후보를 순서대로 구현하고 실제 근거로 검증한다.

**Architecture:** 기존 native media 경로를 유지한다. Host 권한/세션과 Viewer 연결의 수명을 실제 production policy로 수렴시키고, 복구·자원·표시 선택을 bounded 정책으로 표현한다. 기본 동작의 정확성을 먼저 고친 뒤 선택적 성능 모드를 추가한다.

**Tech Stack:** Rust, Swift/VideoToolbox, Kotlin/MediaCodec, TypeScript/React Native/Tauri, Bun, Gradle.

**Spec:** [승인된 재검증·성능 비교 기준](../specs/2026-09-12-audit-followup.md) — user explicitly approved its recommended order on 2026-09-12. Byte-identical historical source: `/Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/leftcar-followup-2026-09-12.md`.

## Global Constraints

- Work only in /Users/loopy/dev/ll3/leftcar/.worktrees/audit-improvements on codex/audit-improvements; baseline 068b6628df5dc57f264446f8ec51215b37c51b6f.
- Existing user approval covers the report remediation and recommended experiments in order. No repeated routine design approval.
- Keep protocol compatibility and existing persisted settings; new experimental latency/codec modes default to the current behavior until measurements justify promotion.
- Never publish, push, merge, delete user data or change external account settings. Commits require the final concrete commit plan and user confirmation; no intermediate commits or staging.
- Implementation subagents are sequential and never spawn other agents. Root owns independent reviews and integration. Reviews use before/after patch packages while changes are uncommitted.
- Meaningful behavior fixes follow red/green tests against production logic. No source-text tests, mock-only assertions, score ignores, or generated-file edits.
- After React/React Native/TSX/style behavior changes run npx -y react-doctor@latest . --verbose from this worktree root and require 100 / 100, then rerun typecheck and relevant tests.
- Preserve the native video data path; compressed video and high-rate input do not cross JavaScript/Rustra.
- Bun is the default JavaScript/TypeScript package manager and script entry point, as confirmed by the user. Keep packageManager, frozen Bun lockfile, CI, doctor and documented commands aligned. Native Cargo/Gradle/Swift and necessary Expo/React Native Node compatibility remain supported; the prescribed React Doctor command remains unchanged.
- No secrets in reports. Source/build/packaging/device/soak/glass-to-glass evidence remain distinct. A missing device or permission cannot become a passing test.

## Verification baseline

HEAD is unchanged from the fresh re-audit: TS612, workspaceRust390+1ignored, standaloneHost172, typecheck/fmt/clippy/ReactDoctor100 passed. New worktree installed the same frozen lockfile. The original restricted Swift split run failed at the Metal pixel-buffer fixture; a later isolated synthetic CVPixelBuffer probe succeeded outside that restriction. Automatic approval review rejected executing the old full split binary with real capture/network paths outside the sandbox. Task3 supplies a genuinely isolated production policy test target; full native runtime remains a separate evidence boundary. Fresh baseline Android and macOS builds are archived, while a fresh Windows target check exposes E0382 assigned to Task5. Detailed receipts and corrections are in this plan's build-environment.md.

### Task 1: Host 세션 수명과 페어링 영속성

**Scope:** F02, F08 추가 예산, F11, F27 Host policy seam

**Files:**
- apps/host-desktop/src-tauri/src/control.rs
- apps/host-desktop/src-tauri/src/pairing.rs
- apps/host-desktop/src-tauri/src/aoap_control.rs
- apps/host-desktop/src-tauri/src/session_lifecycle.rs (new if needed)
- apps/host-desktop/src-tauri/tests/control_e2e.rs

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Make revoke and final session registration atomic with respect to the paired-device authorization generation. Do not hold blocking/reentrant locks over backend callbacks. Reject stale replacement success AND rollback after Host force_stop_session, stopStream, revoke, or a newer operation; always stop/disable the uncommitted backend. Factor production lifecycle policy into a focused typed module actually consumed by ControlServer rather than adding another fake core. Preserve existing JSON command signatures.
Credential and paired-device metadata persistence must succeed together before pairing success. Metadata failure must restore prior memory/credentials, including re-pair of an existing device; both PIN and approval paths. Use atomic metadata persistence. Tighten pre-token command line budget to 16KiB, allowing the existing 12MiB ceiling only after valid device authentication; preserve the 64-connection bound.
Bring the three failing real-source probes from evidence into repository tests. Cover revoke after final check/before insertion, operator-stop during replacement success and rollback, metadata failure including restart. Audit lock order and resource cleanup.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked; cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests --locked -- -D warnings`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 2: Viewer 호스트 identity와 비동기 취소

**Scope:** F05, F06, F07, F18

**Files:**
- apps/viewer-expo/src/session.ts
- apps/viewer-expo/src/control.ts (verified handshake/token context if needed)
- apps/viewer-expo/src/connect-flow.ts
- apps/viewer-expo/src/pairing.ts
- apps/viewer-expo/src/pinned-host-keys.ts
- apps/viewer-expo/src/recent-hosts.ts
- apps/viewer-expo/src/catalog-helpers.ts
- apps/viewer-expo/src/clipboard-sync.ts
- apps/viewer-expo/src/use-catalog-model.ts
- apps/viewer-expo/app/index.tsx
- apps/viewer-expo/app/host.tsx
- apps/viewer-expo/app/pairing.tsx
- apps/viewer-expo/src/*test.ts

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Introduce a request context binding client, target and connection generation. Unauthorized handlers remove only the failed target credential and never disconnect/navigate a newer selected host. Automatic reconnect belongs to its original user-selection generation and cannot supersede a new user selection. Host replacement/disconnect invalidates ongoing clipboard rounds at every side-effect boundary. QR and PIN share one cancellation lifetime through persistence, connection and navigation; unmount and a newer attempt cancel old work. Use stable pinned host identity when available for token lookup and endpoint aliases, preserve existing endpoint keys with safe migration (no identity reuse before key verification), and test address change and A/B isolation.
Reuse the current-source probes in evidence as behavioral regressions with I/O boundaries only mocked. Cover old A 401 after B selected, B connect versus A reconnect, target switch during clipboard poll, PIN exit and QR exit during connect.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `bun run test -- apps/viewer-expo/src; bun run typecheck; npx -y react-doctor@latest . --verbose; bun run typecheck; bun run test -- apps/viewer-expo/src`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 3: 미디어 참조 안전성과 NACK 복구 경계

**Scope:** F09, F14

**Files:**
- native/macos-capture-shim/Sources/Split/CaptureSession+Split.swift
- native/macos-capture-shim/Sources/Split/SplitPairLifecycleState.swift
- native/macos-capture-shim/Sources/Split/SplitFlowControlState.swift
- native/macos-capture-shim/Sources/Split/SplitRecoveryPolicy.swift (new production-consumed pure seam if needed)
- native/macos-capture-shim/Sources/Capture/CaptureSession+PendingCapture.swift
- native/macos-capture-shim/Sources/Encoder/CaptureSession+Recovery.swift
- native/macos-capture-shim/Sources/Split/DualEncoderPipeline.swift
- native/macos-capture-shim/Sources/Transport/CaptureSession+SplitTransport.swift
- native/macos-capture-shim/Sources/Transport/CaptureSession+NetworkQueue.swift
- native/macos-capture-shim/Tests/SplitPipelineTests.swift
- native/macos-capture-shim/Tests/SplitPolicyTests.swift (new safe unit entry if needed)
- tools/build-macos-capture-shim.zsh
- native/android-viewer/src/media_datagram.rs
- native/android-viewer/src/renderer/single_session/runtime/worker.rs
- native/android-viewer/src/renderer/single_session/frame_queue.rs
- native/android-viewer/src/renderer/split_session

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
On split soft expiry fence every pre-admitted dependent delta until a confirmed paired IDR; use existing flow recovery generation, queue cleanup and retained carrier seeding rather than a next-admission flag alone. Preserve resource accounting for late callbacks and avoid repeated recovery storms. Idle screen recovery must not wait for a new capture callback.
Arm eligible NACK grace at the completion/hole boundary before a same-RX-batch queue count can irreversibly emit later AUs. Apply shared policy to both single/split receivers. Preserve absolute 8–25ms deadline and 6-completed-AU hard cap. Existing armed grace tests plus partial11/complete12,13,14 in one batch and repaired11 ordering must pass. Include duplicate/reorder/expiry/IDR transition cases.
Provide a focused Swift unit-test binary containing the production recovery/pair-lifecycle policy and synthetic buffers only, excluding real ScreenCaptureKit/CGDisplayStream and TCP/UDP capture/send entry paths. Automatic approval review rejected running the previous full linked split-test binary outside the sandbox; do not bypass that rejection by renaming/rebuilding the same full binary. Extract a production-consumed pure policy seam where needed, retaining tests sensitive to actual production recovery behavior. The separate synthetic CVPixelBuffer probe passes outside the sandbox; the original -6662 was an execution restriction, not absent GPU hardware. Real capture/network acceptance remains a separately scoped Task11 action.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `cargo test -p android-viewer --locked`, `cargo check -p android-viewer --target aarch64-linux-android --locked`, and the new focused Swift policy-only build/run target. The target check covers receiver worker code excluded from macOS-host tests; use the existing Android NDK/toolchain described in build-environment.md. Compile the complete shim, but keep real capture/network execution separate until its approval boundary is resolved. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 4: 설정·모달·터치 UI와 디코더 예약

**Scope:** F10 UI, F15 reservation, F20, F21

**Files:**
- apps/host-desktop/src/Privacy.tsx
- apps/host-desktop/src/Modal.tsx
- apps/host-desktop/src/App.tsx
- apps/host-desktop/src/PairingPanel.tsx
- apps/viewer-expo/src/decoder-budget.ts
- apps/viewer-expo/src/use-catalog-model.ts
- apps/viewer-expo/src/use-stream-controller.ts and launch-stream.ts (reservation lifetime adapters if needed)
- Corresponding production-boundary tests
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt (awaitable close/cleanup only)
- native/android-viewer/src/jni.rs, jni_exports.rs, renderer/single_session.rs (actual renderer-finished acknowledgment, exact native-state renderer ownership and release timeout only; non-Android stub as needed)
- native/android-viewer/src/renderer/single_session/runtime.rs (reject replacement spawn after unfinished predecessor and return/bind exact installed renderer ownership)
- apps/viewer-expo/src/reserved-stream.ts and catalog-model-types.ts (actual reservation/lifetime composition)
- apps/viewer-expo/android/app/build.gradle (Robolectric test setup only; Task5 owns build policy)
- tools/ui-regression (actual production UI browser fixture and runner)
- apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Apply initial privacy values independently per setting, expose pending and actionable errors in actual callers. Prevent child cancel from closing ancestor modal; supply meaningful accessible names and preserve focus restoration. Lay out HUD controls in one measured container or equivalent nonoverlapping measured offsets with minimum48dp targets and spacing.
Reserve decoder slots synchronously before launching/reconfiguring, release exactly once on failure/stop/cancel, count in-flight requests, preserve split two-slot cost. Separate instance budget from pixel-rate budget; do not infer more instances from reducing resolution. Task7 will supply real capability hints; use conservative fallback4 for unavailable capability. Test overlapping launch completion/cancellation and downgrade transitions. Native cleanup acknowledgment must follow actual decoder teardown: bounded wait timeout returns failure without freeing still-used native Surface/state, and retains the device-wide reservation with an actionable retry path. Exact instance incarnation and reservations survive catalog unmount or Host selection. Retain predecessor cleanup obligations through successor publication; old native-state teardown must address its exact installed renderer rather than a later renderer at the same port. Preserve retry after timeout and acknowledge shared reservation cleanup only after all retained obligations finish. If these native completion adapters change, run relevant Rust tests and the Android target compile in addition to the existing gates.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `bun run test -- apps/host-desktop/src apps/viewer-expo/src; bun run typecheck; npx -y react-doctor@latest . --verbose; bun run typecheck; ./gradlew :app:testDebugUnitTest (Android cwd)`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 5: 전체 빌드·검증·릴리스 산출물 재현성

**Scope:** F23, F24, F26 follow-up, F28 tooling

**Files:**
- .github/workflows/ci.yml
- apps/viewer-expo/android/app/build.gradle
- tools/build-macos-capture-shim.zsh
- native/macos-capture-shim/Tests/SplitPipelineTests.swift
- tools/verify.* (new)
- tools/doctor.* (new)
- tools/release-manifest.* (new)
- tools/build.*, tools/benchmark-profile.* and their focused tooling tests (new if needed)
- bun.lock (only required portable tooling dependency updates)
- apps/viewer-expo/app.config.ts and android/app/src/main/AndroidManifest.xml (isolated profile identity/handler wiring only)
- apps/host-desktop/src-tauri/src/state_profile.rs (new explicit isolated development/benchmark profile if needed)
- apps/host-desktop/src-tauri/src/lib.rs, settings.rs, identity.rs, audit.rs, pairing.rs (profile path/credential namespace wiring only)
- tools/dev-host-macos.zsh
- apps/host-desktop/src-tauri/src/windows_backend/mod.rs (baseline compile repair)
- native/macos-capture-shim/Sources/Transport/CaptureSession+UdpPacket.swift (existing var-to-let warning only)
- native/android-viewer/src/jni.rs (verified unused shared_crypto wrapper and nonfunctional owner key/map type aliases only; no warning suppression)
- tools/architecture-check
- package.json
- docs/versioning.md
- README.md

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Install frozen JS dependencies inside Android CI job before Gradle settings resolution; align pinned Rust toolchain and locked builds. Cargo native step must run its own incremental checker for every relevant build so shared-crate/root-config changes cannot be hidden by Gradle UP-TO-DATE. Add one documented doctor/build/verify entry with executable prerequisite diagnostics and actual app scopes. Separate pure split carrier test buffers from GPU-only probes; unsupported hardware is explicit unverified evidence, not a skipped green hardware claim. Verify React Doctor actual100 score in CI.
Keep Bun as the root developer entry point and pin the package-manager version consistently in CI and setup instructions; detect required Node compatibility explicitly instead of replacing Expo/React Native internals blindly.
Repair the fresh baseline Windows E0382 socket move at windows_backend/mod.rs304 before claiming complete target build gates, retaining the real UDP reachability handshake. Existing llvm-rc is at /opt/homebrew/Cellar/llvm@21/21.1.8/bin; Task9 consumes this correction. Make the local Host update command preserve a verified previous bundle until the newly copied/signed bundle is ready and provide rollback on replacement failure; the current remove-before-copy sequence can lose the installed app if copying fails.
Use resolved Cargo metadata for dependency edges including renamed/path/target inheritance and define scope for standalone Host without blindly accepting unknown crates. Release packaging must never silently use a debug key: explicitly named internal build opt-in may use existing debug identity, public release requires configured credentials. Generate manifest with commit+dirty/source hash, APK/Host/native hashes, versions, signing classification and platform; no secrets. Add build/update/rollback instructions and reconcile version/security summaries with implementation. No publication or key enrollment.
Add an explicit internal benchmark profile for later isolated acceptance: a separately identified Viewer package and Host state/credential namespace covering settings, identity, pairing and audit. Normal app identifiers/storage/credentials stay compatible. Invalid or incomplete explicit profile configuration must fail visibly rather than falling back to the user profile; never repurpose HOME. This adds packaging/development isolation only, not authorization bypass or automatic source grants. Preserve the exact profile-only adaptation so baseline068 and candidate can be built with the same isolation changes and independently bound hashes; such artifacts must be labeled adapted baselines, not the original APK hash. Do not install, launch or migrate user app data in this task. Root coordinates actual benchmark runs later.

Preserve existing full-shim policy-test and retransmit-ring-test CI adapter coverage. Local verification compiles those modes but runs only the accepted Foundation-only split-policy target; report full-adapter local runtime unverified. Do not extract the broad policy adapter suite in this task. Task6 owns pure RTX cache extraction and runtime coverage.

For clean worktrees lacking the ignored debug keystore, support explicit `LEFTCAR_INTERNAL_DEBUG_KEYSTORE` with early actionable validation, preserving the ordinary app/debug.keystore default. Local debug/internal verification may reference the existing original checkout identity; bind its actual certificate fingerprint, never create/enroll a new key or expose secrets. Public release remains configured-only.

Root packaging scopes in this task are Android and macOS Host. Preserve pinned/locked Windows CI packaging and the local Windows target compile check; no new Windows-only NSIS driver is required. Windows installer creation/signing and artifact-manifest coverage must remain explicit final gaps wherever unimplemented or unrun; do not infer those from compilation or claim absent manifest wiring exists. Carry the limitation to Task9 and final acceptance.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `bun run verify (new); cargo test -p architecture-check; clean/configured Android Gradle dry-run and actual debug/internal build; release manifest fixture round-trip`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 6: 수신 FEC와 RTX 캐시의 자원 비용

**Scope:** performance A, F17

**Files:**
- native/android-viewer/src/media_datagram.rs
- native/macos-capture-shim/Sources/Capture/CaptureSession+Setup.swift
- native/macos-capture-shim/Sources/Transport/MediaRetransmitRing.swift (production cache/budget extraction if needed)
- tools/build-macos-capture-shim.zsh (policy-only RTX target)
- tools/verify.mjs (run the new safe pure RTX target in the applicable root verification scope)
- native/macos-capture-shim/Tests/RetransmitRingTests.swift
- tools/perf-matrix

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Count valid unique available shards before pack/clone; impossible reconstruction takes zero allocation, then preserve full width and shard validation. Keep FEC vectors byte-identical. Add repeatable allocator benchmark corresponding to original3-of8 fixture.
Replace current-AU unbounded RTX exception with an explicit configurable/documented byte+age budget while preserving whole-AU recovery semantics. Enforce total memory budget across sides/sessions or provide an owning shared budget rather than multiplying an implicit limit. Oversized AU eviction must avoid partial-eviction retransmit traps and expose served/missed/evicted bytes. Maintain linear insertion performance and thread-safety. Benchmark before/after with original source snapshot and candidate in same run.

Initial configurable RTX defaults: shared total `6 * 1024 * 1024` payload bytes, per-AU `2 * 1024 * 1024` payload bytes, age `250ms`, global `256` access units, existing `8` AUs per side. Treat these as initial defaults for later physical assessment. Bound per-session/side high-water state and test UInt16 wrap, late/duplicate fragments after eviction and legitimate ID reuse. Record actual retained/served/evicted Data bytes separately from bookkeeping/RSS; NAK does not contain original payload length, so missed fragment counts and any explicitly named requested-byte upper bound must not be represented as measured missing payload bytes. Derive that upper bound from the actual packetizer/cached-envelope limit.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `cargo test -p android-viewer --locked; cargo test -p fec-core --locked; Swift RTX tests and paired microbenchmark`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 7: 기기별 디코더와 선택적 표시 타이밍

**Scope:** performance C, D, F15 capability

**Files:**
- crates/viewer-decoder/src/android/decoder.rs
- native/android-viewer/src/renderer/presentation_sync.rs
- native/android-viewer/src/renderer/single_session
- native/android-viewer/src/renderer/split_session.rs and split_session
- native/android-viewer/src/renderer/dispatch.rs
- native/android-viewer/src/jni.rs
- native/android-viewer/src/jni_exports.rs
- native/android-viewer/src/jni_wrappers.rs and jni_wrappers/attach.rs
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt
- apps/viewer-expo/src/launch-stream.ts and use-catalog-model.ts
- Actual presentation setting caller and corresponding production-boundary tests
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream
- apps/viewer-expo/src/decoder-budget.ts
- apps/viewer-expo/src/viewer-preferences.ts

- crates/viewer-decoder/src/android FFI boundary and focused production tests
- apps/viewer-expo/app/catalog.tsx, actual ActiveStream type and relevant i18n entries
- Activity-owned Choreographer adapter, module wiring and focused lifecycle tests

- apps/viewer-expo/src/reserved-stream.ts and actual new-method cancellation/compatibility regression tests

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Implement progressive QTI retries (both extensions, low-latency-only, standard) with fresh codec instances and deterministic fallback. Read advertised codec capabilities as hints, conservatively handle absent/failed/lying limits, connect max-instance and pixel-rate hints to Task4 reservation. Add validated recipes only for observed supported devices; other vendor recipes must stay experimental with fallback and no support claims.
Offer a persisted optional balanced presentation mode using actual Choreographer/vsync timing and bounded latest decoded output; immediate remains default. Carry mode through real native start configuration, synchronize split sides on the same display timeline, drop only decoded surplus output, and never use an artificial60Hz epoch. Test 60/90/120Hz timelines, late frames, clock reset and mode changes. No enabled-by-default latency increase. Read task-7-integration-notes.md for actual mode/capability plumbing and keep display-refresh timelines distinct from target stream FPS limits.

Prune acknowledged historical Activity ownership generations so memory and main-thread registration/close work depend on outstanding obligations, not total reconnect history. Keep the current published generation for idempotence and every pending/failed/in-flight lease; existing close snapshots must still settle safely. Add repeated-reconnect plus failed/pending cleanup coverage. This is the nonblocking Task4 FIX2 follow-up in task-4-rereview-2.md.

Conservative capability admission uses one slot for absent/failed hints and caps advertised counts at the prior four-slot ceiling. Test one-slot split rejection, valid two-slot split, retained active reservations when a hint shrinks, and cleanup after actual codec fallback/failure. Instance and pixel-rate budgets remain separate. Activity frame callbacks must be fenced to exact renderer ownership and invalidated on stop, display/generation replacement and reattach.

Keep per-instance VideoCapabilities rate hints separate from aggregate capacity: add a per-instance pixel-rate hint and validate each decoder/tile demand independently. Existing maxPixelRate remains an explicitly provided aggregate policy/measurement limit; unknown aggregate remains unknown. Do not substitute a single-instance rate or multiply by instance count to claim measured concurrent throughput. Test valid two-instance/tile admission separately from true aggregate-budget exhaustion and label unmatched actual codec fallback accurately.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `cargo test -p viewer-decoder -p android-viewer --locked; Android JVM tests/build; React Doctor100 then typecheck/relevant TS tests`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.


### Task 8: 오디오 지연·압축과 idle 비용

**Scope:** performance B, G

**Files:**
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamAudioPlayer.kt
- native/macos-capture-shim/Sources/Transport/CaptureSession+Audio.swift
- native/android-viewer/src/audio_protocol.rs
- apps/viewer-expo/src/clipboard-sync.ts
- apps/viewer-expo/src/use-stream-controller.ts
- apps/viewer-expo/src/viewer-preferences.ts
- apps/host-desktop/src-tauri/src/clipboard.rs
- apps/host-desktop/src-tauri/src/control.rs (clipboard revision fast path only)
- apps/viewer-expo/src/launch-stream.ts
- apps/viewer-expo/src/use-catalog-model.ts and actual audio settings caller
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt
- apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt
- native/android-viewer/src/jni.rs
- native/android-viewer/src/jni_wrappers.rs
- native/android-viewer/src/jni_wrappers/stats.rs
- native/android-viewer/src/jni_exports/session_io.rs
- native/android-viewer/src/renderer/single_session/runtime/worker.rs
- native/android-viewer/src/renderer/split_session/tile_worker.rs
- native/macos-capture-shim/Sources/Transport/CaptureSession+ViewerControl.swift
- native/macos-capture-shim/Sources/Capture/SystemAudioOwnership.swift
- Focused production codec/framing/buffer seams and corresponding tests as needed
- native/android-viewer/src/renderer/single_session/runtime.rs (actual RendererControl initializer)
- native/macos-capture-shim/Sources/Capture/CaptureSession.swift (owned converter/reusable storage)
- native/macos-capture-shim/Sources/Capture/CaptureSession+Lifecycle.swift (audio-queue disposal)
- tools/build-macos-capture-shim.zsh (AudioToolbox linkage and safe test selector integration)
- native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift (lock-protected audio-queue metrics snapshot)

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Remove accidental100ms floor overriding the60ms target; introduce real requested/actual buffer, underrun and written/playback position instrumentation and bounded adaptation. Preserve minimum platform constraints and safe route-change behavior. Reduce PCM temporary allocations with reusable conversion storage; replace avoidable idle polling with signaling where supported, retaining loss recovery and stop responsiveness.
Add explicitly negotiated optional Opus audio (128kbps experimental target) end-to-end with PCM fallback for old peers/unavailable codec; default PCM until measured. Version only the audio subprotocol/capability, never silently reinterpret LCAU PCM bytes. Compare encode/decode delay, bandwidth, underrun and A/V skew. Public upstream implementations are reference only, do not copy licensing-incompatible code.
Use the corrected native-first feasibility report /tmp/leftcar-opus-capability-research.md: Apple AudioConverter successfully encodes standard Opus outside the restricted probe sandbox, so prefer it before adding a vendored codec dependency. Its magic cookie is not OpusHead; construct and test Android CSD from the actual negotiated format/prime delay. Codec absence must be a runtime capability result with PCM fallback, not inferred from a restricted CLI probe.
Avoid reading/encoding/hash of unchanged image clipboard when native revision/change signals exist; fallback bounded poll without payload reread when safe. Idle/hidden/no-stream status polling backs off while active stream recovery remains covered. Persist user-selected modes. Read task-8-integration-notes.md for the actual JNI/command/audio-owner path and Host clipboard cost. Propagate codec preference/fallback through both single/split production subscriptions; cache Host clipboard unchanged results only against a validated native revision while preserving privacy and caller hash behavior.

For isolated video benchmarks, disable Host system-audio capture before the initial SCStream construction as well as after owner changes; Viewer mute/SNDOFF after start is insufficient. Add a restrictive internal profile control with the same minimal, hash-bound baseline/candidate adaptation, preserve normal app behavior, and test the actual configuration decision. See task-8-integration-notes.md.

Preserve actual effective Activity audio/stream configuration across partial control intents and save/recreation while integrating audio modes. Retain nondefault dimensions/FPS/split, audio-off and Task7 presentation state, exact ownership generation and start-versus-toggle semantics. Add actual lifecycle regressions through the existing Activity harness; see task-8-integration-notes.md for the confirmed source boundary.

The additive Opus audio datagram uses LCO1 and repeats actual format/pre-skip metadata with a u64 big-endian encoder-lifetime epoch. Use a process-wide monotonically advancing encoder generation scoped to the authenticated media/session lifetime, with explicit bounded stale-epoch and sequence-wrap acceptance. A delayed retired epoch must not reset the decoder back to old state. Apply pre-skip on actual decoder initialization/reset, not per packet. Cover lost first packet, old epoch after new epoch, owner transfer, duplicate/reorder, sequence wrap and PCM fallback/resume; preserve LCAU bytes unchanged. Record exact final header layout and negotiation/reset semantics in the report.

Audio worker JNI poll/wait/codec-setting operations must target the exact native-state plus instance renderer ownership, so a retired worker cannot drain or downgrade a successor with the same logical instance. Preserve old ABI via additive owned entry points or prove equivalent actual operation fencing. Test old/current owner isolation and stop/wakeup ordering, not only worker-local flags.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `Rust audio parser/round-trip tests; Swift audio conversion/ownership tests; Android JVM tests/build; TS clipboard/poll lifecycle tests; React Doctor100 then typecheck`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.



### Implementer wire decision (root-approved additive audio scope)

Legacy `LCAU` and its six-byte PCM JNI blob remain unchanged. `LCO1` fields: magic[0..4], sequence:u16BE[4..6], rate:u16BE[6..8], channels:u8[8], reserved=0[9], frames:u16BE[10..12], epoch:u64BE[12..20], encoder preSkip:u16BE[20..22], payload bytes:u16BE[22..24], exactly one raw Opus packet[24..]. Require48000Hz,1/2 channels,480frames,nonzero monotonic epoch,payload1..1276. The complete LCO1 packet is the Opus JNI blob. Repeated header recovers first-packet loss; decoder applies preSkip only upon initialization. Host epochs advance process-wide under a lock per encoder construction/reset; no wrap is permitted. Receiver keeps the highest admitted epoch even after drain/PCM transition, rejects lower epochs, and resets that watermark only at the existing authenticated media session/challenge reset (`AudioRing.clear`). Within an epoch u16 sequence uses positive signed wrapping distance, so duplicate/backward and ambiguous half-range packets are rejected. A higher epoch clears incompatible pending state. Media session handshake remains the reset boundary. Existing ownership groups by destination IPv4; Task10 owns stronger authenticated device identity.

Negotiation uses legacy SNDON/SNDOFF plus explicit audio-only `SNDA1O` (Opus128k permitted) or `SNDA1P` (PCM). Both single/split production subscription refreshes repeat the versioned preference for loss recovery; SNDON does not erase a chosen codec. Runtime unavailable/failing codec latches PCM until an explicit user preference retry or new renderer lifetime. Default remains PCM.

FIX1 owner authority: only the current lowest-handle live audio owner can apply SNDA1O/P to the group. Nonowner refresh must not override an owner's PCM failure fallback. Preserve group mute behavior and inherited negotiation during transfer; a successor's current request becomes authoritative in its new owner lifetime. Last-group retirement clears negotiation, and repeated current-owner refresh is idempotent.

### Task 9: Windows 전송 pacing과 batch 경계

**Scope:** performance E

**Files:**
- apps/host-desktop/src-tauri/src/windows_backend/capture.rs
- apps/host-desktop/src-tauri/src/wire.rs
- apps/host-desktop/src-tauri/src/media_pacing.rs (new platform-neutral production policy if needed)
- apps/host-desktop/src-tauri/src/lib.rs (portable policy module wiring)
- apps/host-desktop/src-tauri/src/windows_backend/mod.rs

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Use a platform-neutral tested bounded pacing policy carried across AUs and derived from configured bitrate/transport recovery budget. Bound Windows send bursts and work per iteration, include parity and encrypted bytes in accounting, honor cancellation and AU deadlines. Use supported platform batch submission only with a correct fallback and capability proof; do not claim macOS sendmmsg. Preserve Windows idle encoder pump. Metrics include AU drain, paced delay, batch size, failures. Do not copy Sunshine800Mbps assumption.
Retain Task5's baseline Windows socket-move repair and UDP reachability handshake. Add /opt/homebrew/Cellar/llvm@21/21.1.8/bin to PATH for local MSVC target checks. The original failure receipt is /tmp/leftcar-improvements-baseline-windows-check-with-llvm.log.

Task5 deferred Minor M1 is assigned here: make `bun run verify rust` use platform-neutral Rust prerequisites; Swift/xcrun applies only on macOS. Narrow tools/doctor.mjs, tools/verify.mjs and focused production tooling tests are in scope. Preserve Windows-specific tool prerequisites where actually needed and prove Linux/Windows routing with focused fixtures.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `Host unit tests on portable pacing policy; Windows target compile and CI checks where available; no Windows runtime claim from macOS tests`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.



Task9 controller ruling before implementation: use the existing terminal capture error/stop path for deadline or failed/partial submission, so a partially sent AU cannot count as complete or allow dependent deltas. Initial policy limits are at most8 ordinary datagrams per burst plus an actual-wire byte cap chosen from the valid framing sizes, interruptible pacing waits of at most1ms, and an absolute1s AU-drain ceiling. These are safety/default policy limits, not a1ms end-to-end cancellation guarantee or measured latency target. Existing socket write timeout is20ms and a synchronous send/lock may exceed a pacing slice; report and test the real boundary without claiming hard cancellation during an OS call. Preserve prior active-stop semantics and do not introduce automatic IDR retry loops.

Metrics stay on the long-lived production pacer and are emitted as structured bounded-frequency snapshots (at most once per second during normal drain, plus terminal/final outcome), rather than logging every AU. Include actual session/capture identity, explicit cumulative-counter lifetime and transport; Task11 will bind process incarnation and collector schema. Keep shared StatsInfo unchanged unless integration reveals a concrete necessity. Count submitted bytes/packets separately from complete AUs, failures and unknown partial TCP bytes; no delivery claim. AU drain and burst extrema must be named as maxima, not p95 quantiles. Real transient socket saturation now terminates visibly instead of silently treating a missing packet as successful; disclose the compatibility/UX tradeoff. Verify sustained configured encoder-rate input with actual encrypted/FEC framing and bounded idle credit.

Task9 config compatibility ruling: preserve the existing supported CFG plaintext size contract through secure_channel::MAX_DATAGRAM (65,536 bytes), with actual AEAD24 and optional TCP4 overhead. Keep the normal media burst cap11,424 wire bytes and at most8 ordinary datagrams. An oversized valid CFG may be submitted only as one isolated bounded packet after an explicit full-cost pacing reservation; carry any bounded debt into following media rather than resetting credit or generally enlarging media bursts. No wire fragmentation change or blanket bypass. Reject beyond the existing sealer contract before submission. Test a CFG above the normal burst cap, exact full charge, subsequent media pacing, cancellation/deadline and upper-bound rejection. UDP OS datagram limits remain a distinct possible send error, not a new claim that the existing maximum can be transmitted on every transport.

Task9 final platform-gate scope: fix the two observed Windows --tests strict warnings in state_profile.rs (child variable used only by the Unix symlink body) and control.rs (TeardownBarrier fixture/impl used by Unix-only ADB tests). Use precise platform scoping matching actual callers, no allow/expect or underscore suppression, and preserve portable normal-state and Unix ownership regressions. These two narrow test-only files are explicitly in scope. Retain failed receipt and re-run covering tests and Windows strict gate on final source.

### Task 10: Host source·입력 승인 정책의 제품 연결

**Scope:** F13, F27 policy integration

**Files:**
- apps/host-desktop/src-tauri/src/pairing.rs
- apps/host-desktop/src-tauri/src/control.rs
- apps/host-desktop/src-tauri/src/lib.rs
- apps/host-desktop/src-tauri/src/backend.rs
- apps/host-desktop/src-tauri/src/ffi.rs
- apps/host-desktop/src-tauri/src/windows_backend
- native/macos-capture-shim/Sources/CaptureShim+Exports.swift
- native/macos-capture-shim/Sources/Capture/CaptureBackend.swift
- native/macos-capture-shim/Sources/CaptureShim.swift
- crates/control-contract/src/host.rs
- apps/viewer-expo/src/use-catalog-model.ts
- apps/viewer-expo/src/launch-stream.ts
- apps/host-desktop/src/PairingPanel.tsx
- apps/host-desktop/src/App.tsx
- packages/ui-tokens
- docs/07-security-privacy.md
- README.md

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Add explicit Host-owned per-device source grants consumed by real getCatalog/startStream/reconfigure authorization; stable source identities must not be confused with mutable enumeration indices. The current production capability is display capture; do not introduce window capture from fake host-core examples. Pass the resolved stable identity into the actual Swift/Windows native capture boundary so a second index enumeration cannot select another display. Use an additive versioned native API when needed. Reject missing/ambiguous identity and never substitute a title or first display. Consult /tmp/leftcar-source-grant-research.md for exact production seams and primary API evidence.
Persist grants and revoke active sessions when grants are removed; async start/reconfigure must respect Task1 generations. Invalidate pending capture/output authorization as well as final registration when permission changes. Durable permission removal failure must be visible and fail closed rather than silently restoring access after restart. Provide usable Host UI to approve/view/remove source access, with a clear Viewer denial/retry message. Newly paired devices receive no implicit source grant. Existing-device migration must require visible Host review before expanding/reusing source access, and explain behavior in docs/UI.
Separate viewing from input: source access alone does not grant control; input starts off until the Host explicitly enables it for that session. Reconfiguration preserves only explicitly allowed control and source switch must be re-authorized. Follow existing source/capture capabilities and avoid creating dummy host-core consumers. Finish typed actual-production policy seams and document fake host-core scope accurately. Regression tests cover two devices, source list reorder, permission change during start/reconfigure, persistence failure and input control.

For the isolated benchmark profile, expose/test a restrictive exact-display check at the actual native selector before capture construction. Preserve ordinary behavior and provide the same minimal hash-bound baseline/candidate restriction through the existing benchmark adaptation tooling; do not rely on an earlier collector index lookup. See task-10-integration-notes.md. Narrow tools/benchmark-profile baseline adaptation/exporter files and tests are in scope for this restriction.

Carry the Host-authenticated device/authorization owner identity through the versioned native start boundary for system-audio arbitration. Current Swift audio ownership is keyed only by destination IPv4, which can conflate different devices behind a loopback/bridge or split one device across transports. Do not accept a Viewer-supplied owner string or infer device identity from IP. Preserve Task8 negotiated codec/epoch/owner-transfer semantics; test two authenticated devices sharing an endpoint address and one device using multiple endpoints. Narrow SystemAudioOwnership/CaptureSession owner-key plumbing is in scope; preserve explicitly labeled legacy ABI behavior.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [x] **Step 4:** Run focused then applicable complete verification: `Host authorization/integration tests; TS UI tests; React Doctor100 then typecheck; updated contract regeneration if needed`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.



Approved Task10 durability design (root ruling after concrete proposal): use a profile-local versioned source_grants.json journal containing dirty plus credential-bound per-device reviewed/revision/source selections, and a separate never-unlinked exclusive lifetime .source-grants.lock acquired before profile reads/migrations/control startup. Only one owner may use the profile. Missing/corrupt/unreadable journal and dirty startup require visible Host review; retain trustworthy selections as suggestions, never effective grants. Before any stored grant activates, durably write the complete journal dirty=true using private sibling write/sync and appropriate replacement/durability operations. Any establishment failure means no grants activate and startup fails visibly. Verify selected Windows API contracts; cross compilation does not prove hardware power-loss behavior.

Runtime updates always persist dirty=true. Additions/review publish only after successful persistence. Removal first invalidates effective authorization revision/leases and schedules active/pending cleanup outside pairing/state locks; keep desired denial in memory on persistence failure and return an error, not success. Clean shutdown fences new and in-flight control/admin authorization/mutations, invalidates leases, waits admitted native/backend/output work outside locks and retires sessions, then durably writes the exact latest desired grant snapshot dirty=false as one final replacement. No future mutation can occur after that point. If replacement happened but final sync fails, any possibly clean record still contains the exact current desired denial; never clear uncertainty with old grants. Dashboard hide is not shutdown. Clean successful restart preserves reviewed grants; unclean/uncertain restart requires reapproval. Do not make every normal restart require approval.

Guarantees are scoped to process-crash and documented filesystem operations, not arbitrary storage rollback/corruption or claimed physical power-loss validation. Credentials remain intact; mismatched credential lifetime cannot reuse an old grant. Test actual startup/shutdown ownership integration, dirty-establishment failures, runtime add/removal failures, shutdown failure at relevant stages, subsequent restart, concurrent profile owner exclusion and delayed authorization/admin work. Narrow profile lock/journal/platform replacement modules, StateProfile path isolation for new files, and actual startup/shutdown wiring are in scope. Preserve the same minimal benchmark source/audio restrictions in exact068 adaptation without importing candidate grant policy.



Task10 inherited native transport parser repair (root ruling2026-09-13 KST): actual exact068 FfiBackend always calls the sealed v8 export, whose UDP-only guard rejects canonical TCP/USB/ADB-TCP starts. Current v9 copied that guard. Repair the candidate shared transport/config parser used by its v8 and v9 exports so known non-UDP transports accept either the exact reconfigure placeholder auto/0/0/false or a tuple accepted by the existing strict canonical UDP validator (actual start defaults responsive/8/2/false), then use existing AppliedUdpStability.legacy as the transport-default policy. UDP accepts only the strict canonical validator and never the placeholder. Unknown/malformed tuples and transports remain rejected. Preserve strict UDP-profile validation, invalid transport/config rejection, sealed-key validation and all v9 stable-source/owner/lease checks; do not restore plaintext or legacy authority fallbacks. Determine exact non-UDP values from the actual Host start and reconfigure callers and record them. Demonstrate RED→GREEN against the real production parser/export route with synthetic no-capture fixtures; cover UDP plus TCP/USB/ADB-TCP, invalid inputs and authorization denial. Run the affected pure production tests and full Swift compile after repair; re-run additional gates only if their source is changed. Do not repair exact068 benchmark source: preserve its inherited behavior and disclose unsupported baseline native transport rows rather than silently importing a candidate fix. Narrow production parser/policy/export and meaningful tests/build-runner files are in scope. This is source/compile acceptance, not physical USB delivery proof.



Task10 FIX1 root ruling64: Host-local grant/device views may add non-secret credential-incarnation ID/generation metadata, and single/all revoke commands may return typed outcomes carrying removed IDs and persistence errors. Use these to bind displayed approval to the actual credential and reconcile authoritative returned GrantViews across remount/stale snapshots. Do not expose credential/token secrets or alter remote wire. Scope retained UI state to current profile/device credential lifetimes and prune removed entries; late saves/snapshots cannot resurrect removed devices or approve a re-paired incarnation. The actual parent store/caller contract and tests must be documented. For I1, record denial before native application; skip a native disable only for a proven retired handle in the current replacement lifetime. A failure disabling an actually live native handle must revoke its lease/retire that session and report failure, not leave native input authorized behind logicalfalse. Preserve exact handle/incarnation checks through replacement/rollback.

### Task 11: 동일 조건 성능 검증과 전체 수용 판정

**Scope:** F29, final F28 provenance, all tasks

**Files:**
- tools/perf-matrix
- native/macos-capture-shim/Sources/Metrics/CaptureSession+Stats.swift (measurement identity/fields if needed)
- native/macos-capture-shim/Sources/Encoder/EncoderSessionSelection.swift (measurement identity/fields if needed)
- native/android-viewer/src/renderer (measurement identity/fields if needed)
- docs/EVIDENCE.md
- docs/2026-09-12-remediation-results.md (new)
- docs/templates/benchmark-result.md

- tools/ui-regression and focused actual UI/lifecycle tests for retained F19/F22 acceptance
- Actual affected UI caller only if those final production-boundary regressions expose a concrete remaining defect

**Interfaces:** Preserve existing public entry points unless the task explicitly adds negotiated capabilities. Each subsequent task reads the final production interfaces from earlier tasks; additive options default to existing behavior. Exact affected signatures and test seam choices are recorded in the task report before integration review.

**Requirements:**
Extend existing matrix collector with build manifest/hash binding, capability/transport/mode fields, actual sample boundaries, CPU/RSS/thermal/power/audio/buffer metrics when available, missing-data status and baseline-candidate comparison. Do not clear unrelated logs or manipulate unrelated apps. Connected test tablet TB710FU may be used for the requested testing; prepare a synthetic dedicated benchmark source and preserve user settings/app state.
Replace the current logcat-clear default with explicit collection boundaries. Partition counters by actual session/stream and incarnation before aggregation; current LeftcarPerf/Rendered lines lack a stream identity and the existing analyzer mixes counters across streams. Cover restart/counter reset, mixed sessions, split-pair metrics, missing hardware fields and a delayed first sample. Add production metric identity/fields only where needed; do not fabricate observations or silently attribute old baseline logs to a new schema. See this plan's task-11-collector-notes.md.
Run same-source local microbench baseline vs candidate and available physical1440p/4K single/multiple-window static/scroll/video plus loss/reconnect/resize. Capture short repeat runs then10min and30min stability (user shortened the final soak from60min to30min on2026-09-13 KST) when the environment allows; keep communicating during long runs and do independent review while collection runs. Glass-to-glass requires an actual suitable camera; do not substitute RTT or Surface-release times. Windows, vendor and public-release credentials missing from this environment are explicit external gaps, not done items.
Run final complete gates after final fixes; independent full diff review; map every previous finding and performance candidate to implementation+test+device evidence. Stage/commit only after presenting concrete commit groups and user confirmation; do not push or merge. Keep worktree/result accessible.

Bind each latency field to its clock basis and actual input/output stage. Existing clock correction does not prove exact presentation timing, and current feed-time capture age may describe a different frame from a drained decoder output. Associate output identity through real PTS metadata where available; otherwise expose the limitation/unknown rather than claiming matching-frame or glass-to-glass latency.

Task5 deferred Minor M2 is assigned here: make manifest builder versus target platform/architecture unambiguous, recording Android target platform/ABI/triple instead of inferring it from the builder. Narrow tools/release-manifest.mjs, tools/build.mjs and focused tooling tests are in scope. Use additive/versioned schema handling and preserve historical receipts unchanged.

- [x] **Step 1:** Port the named failing behavior to a production-boundary regression. Prior exact probes: /Users/loopy/.codex/visualizations/2026/09/11/01a08f71-c96d-7001-8675-5bd643c77e9a/evidence-2026-09-12. Derive expected safety outcomes independently; no source-text assertions.
- [x] **Step 2:** Run the focused regression and save the expected failure; for existing implemented paths, use a targeted mutation/old-source comparison to establish sensitivity.
- [x] **Step 3:** Implement the smallest policy/interface change satisfying every requirement above, preserving cleanup, compatibility, and clear ownership.
- [ ] **Step 4:** Run focused then applicable complete verification: `full verify; artifact manifest verification; same-device collectors; final independent review and source/status receipt`. Store command, exit and relevant output in this task report.
- [x] **Step 5:** Self-review and send patch+report to independent task review. Resolve Important/Critical findings and repeat scoped tests. Leave changes uncommitted for final user-confirmed commit grouping.

Task9 deferred Minor M1/M2 must be closed here: strengthen actual oversized-CFG pacing regression with independently derived full-cost delay/remaining credit for UDP and TCP; preserve one bounded terminal preparation/submission cause in actual pacing outcome/metrics. See task-9-review.md. Narrow apps/host-desktop/src-tauri/src/media_pacing.rs and windows_backend/capture.rs changes are in scope; preserve accepted wire/scheduling/terminal policy, no per-packet logging, and run covering production-policy tests plus final Windows strict check.


Final architecture integration (root ruling62): Task10's real `bun run test:architecture` and the identical check in accepted task-10-before both fail with the same7 Kotlin findings; receipts are task-10-evidence/architecture-final.log and architecture-accepted-task9.log. These are an inherited final gate blocker, not a passing architecture result. Reconcile tools/architecture-check/ts.ts and meaningful positive/negative fixture tests with the already approved native Android interfaces: SplitDecoderCapability may query advertised codec/size/rate/instance hints but may not create a decoder; OpusAudioDecoder is the single native Android MediaCodec audio/Opus adapter with required ByteBuffer/ByteOrder CSD/buffer conversion, while Rust owns network authentication, packet admission/reorder/epoch watermark and video decoder policy. Its exact companion JVM fixtures can exercise those platform contracts. Remove the obsolete syntactic requirement for a particular `<2` spelling/hardcoded geometry-only query, not the boundary forbidding decoder construction in the capability query.

No blanket Kotlin/test/file exclusion, import qualification trick, warning suppression or unconditional pass. Keep network/socket creation forbidden even in an allowed audio adapter; keep video decoder construction and unrelated codec adapters forbidden outside their approved Rust boundary. Scope any new imports/API permissions to the exact approved platform modules and relevant test fixtures. Add negative fixtures proving network calls in the allowed audio module, video decoding or codec creation in the query/unrelated Kotlin still fail, alongside valid capability/audio/CSD fixture checks. Architecture-rule tests inspect source fixtures as the checker's real domain inputs, not source-text assertions substituting for application behavior tests. If actual production code crosses these boundaries, repair the smallest concrete violation rather than legalizing it. Run final architecture and relevant JVM/native tests after any production changes. Propose the precise architectural documentation clarification in the report for root reconciliation with concurrent docs. Narrow checker/test files and only concretely needed platform-boundary production corrections are in scope. Do not rewrite the audio codec backend wholesale solely for this inherited checker mismatch.


Final tooling warning (Task10 reviewMinor5/root ruling63): actual `bun run test:contract` loads vitest.contract.config.ts through deprecated Vite CJS API. Root confirmed the config uses ESM imports while root package has no module type; package.json test:contract is its sole current non-doc reference. Make only this config an explicit ESM entry (prefer rename to vitest.contract.config.mts and update the actual script/reference) and verify contract/tooling/typecheck plus final fullJS gate. Do not set package-wide type=module, suppress/deactivate warnings, change baseline068, or upgrade dependencies just for this. No implementation-mirroring new test is needed for a reversible config rename; actual contract execution must stop emitting that warning and retain its cases. Primary reference: https://vite.dev/guide/troubleshooting.html#this-package-is-esm-only (read2026-09-13KST; recommends .mts config for explicitESM). Existing seven native helper warnings remain separately assigned here; preserve historical baselineSwiftwarning and accurately report any external build warnings.

Task10 FIX1 root ruling66: Task10 FIX1 coordinates only cfg(test) profile-lock fixture lifetimes and actual child-process fixture parents with one test mutex, after a retained diagnostic demonstrated transient macOS fork/flock ownership overlap despite FD_CLOEXEC=1. Three real profile-lock/restart owner lifetimes and the parent lifetimes of the four named child-launch routes acquire before owning a profile or launching children; child branches precede acquisition and exec into fresh statics. Production lock behavior, actual subprocess assertions and all other test parallelism stay unchanged. Cost if wrong: these bounded fixture lifetimes serialize and could hide a test-coordination mistake, so preserve diagnostic source/commands/restore hashes and run the normal parallel full Host gate, not only serialized tests. No production retry, backoff, delay or lock relaxation.

Task10 FIX2 root ruling67: Task10 FIX2 separates complete membership snapshot knowledge from credential-specific partial mutation confirmations. A failure without a causally bound server revision keeps uncertainty for that exact credential across snapshots; only a matching-credential successful save issued after the observed failure clears it, or actual membership removal/re-pair prunes it. Retain revocation tombstones only until an authoritative complete membership snapshot acknowledges removal, while preserving the complete-snapshot fence against delayed old responses. Cost if wrong: an unrelated or merely fresh poll cannot remove an uncertainty warning and the user may need one explicit retry; UI state sequencing becomes more detailed. No Host/native/remote return contract or global action serialization change; cover two-device ordering, late pre-failure saves, repeated failures, re-pair/removal and snapshot tombstone acknowledgment.

Ruling: Task11 wires every accepted actual UI regression suite into the existing test:ui/full verify entry point, which currently builds all fixtures but executes only run.cjs. Run suites sequentially with explicit failures so grant/revoke/incarnation/ordering tests remain effective after this session. Cost if wrong: the normal UI gate takes longer and a small runner/script change needs compatibility review; retain isolated fixture lifetimes and meaningful final execution rather than source-string tests or silent suite skipping.

Ruling: Task11 may add a nonsecret metric incarnation owned once by actual RendererControl in native/android-viewer/src/jni.rs and a corresponding capture metric identity in native/macos-capture-shim/Sources/Capture/CaptureSession.swift. Initialize every actual owner constructor, retain identity through its single/split lifetime, and change it on replacement; keep decoder PTS metadata/reset ownership in the renderer when sufficient. Cost if wrong: two narrow owner files widen metrics scope and require native/Swift integration verification, but no remote wire/authority/ABI change or high-rate JS data path is allowed. IDs identify measurement lifetimes, never authorization, and process-global metrics remain separately scoped.

Ruling: Task11 FIX1 closes invalid-run comparability and structured-finalization loss, and unsupported metric schema handling, as one measurement boundary repair. Require explicit valid process/clock/collection status, same selected device and comparable actual/intended durations before comparison; preserve raw provenance and a failed receipt with nonzero exit on interruption, collector/parser/clock failure or process replacement. Bound sampling by the requested deadline so slow device calls cannot silently redefine a30minute run. Cost if wrong: incomplete runs become visibly unavailable and collector orchestration gains bounded asynchronous cancellation; no production capture/UI/native changes or falsified physical acceptance. Keep the disclosed external Gradle deprecation as a traced compatibility gap rather than suppressing it or upgrading dependencies.

- Ruling71: 실제 macOS log stream의 시작 안내 문구를 통계 JSON과 구분한다. 유효한 안내만 건너뛰며 원본과 잘못된 실제 통계의 실패 판정은 보존한다. 마지막 통합 수정·측정 회귀·독립 재검토에 포함한다.


이전 검증 상태 (실기기 후속 보완 전, 2026-09-13): Task11 구현과 FIX1 독립 재검토, 최종 전체 검토 1회·통합 수정 1회·새 독립 재검토를 완료했다. 최종 소스는 `7de7a418ad9ef0ffb0b0a815392127ec105f30b4df520c812ee1e1591baaeb6c`이며 관련 검사와 두 내부 패키지의 바이트 일치를 확인했다. Step4는 실제 기기 미디어 연결·10분·30분 수용이 남아 있어 체크하지 않는다. 화면1 공유의 명시적 승인을 자동 승인 검토가 요구했으며 현재 영상 전송은 없다. 최종 APK 업데이트는 완료했다. [현재 검증 현황](../../2026-09-13-audit-remediation-validation.md)에 결과·제한·판단74개와 근거 명세를 기록한다.

Task11 실기기 준비 후속 보완: 종료된 Host 연결 실패 뒤의 잘못된 연결 표시를 실제 Host·시작 화면에서 수정했다. source `71e5ddcca2b9de75d4bc21d7c3455e1ac95b8064e1696ed923edac8d82975679`, Doctor100/JS559/계약4/UI78·9suite/타입·구조 검사 및 범위 독립 검토 통과. 새 APK 설치 바이트 일치와 실제 태블릿의 수동·빠른 실패 표시 복구를 확인했다. 당시 영상·10분·30분은 화면 공유 승인과 전송 경로 준비를 기다렸다. 이후 결과는 다음 단락을 따른다.

Task11 명시적 승인 후 실행 (2026-09-13 20:41–20:51 KST): 사용자가 전용 Display1 공유와 2개 로컬 커밋을 승인했다. 최종 Host를 실행하고 실제 Host·Viewer UI에서 전용 화면1개만 허용됐음을 확인했다. 직접 LAN 응답 실패가 유지됐으며 ADB over Wi-Fi 제어 연결 뒤 UDP 영상 도달 증명에 실패했다. 영상 시작 실패를 장시간 수용으로 바꾸지 않으며 Step4는 미완료다. Host 종료 확인 뒤 합성 소스를 닫고 작업 전용 UI·제어 매핑만 정리했다. 구현 커밋은 `f91607cc8810258c16fa76d4de3c5dfbb4344102`, 스테이징 후 실제872개 입력 해시는 `cb0ad4e0e7b209f1d119446facc124eb3d81133ad01a69ecd165fd8afbb069ce`이다. 패키지 당시873개 목록과의 차이는 삭제된 파일의 부재 표식 하나뿐이다. 현재 검증 문서에 승인·실행·실패·정리와 판단74개를 반영한다.

후속 통합 요청: 사용자가 simplify 후 push·merge를 요청하고 실기기 검사는 추후 직접 수행하기로 했다. 따라서 Task11 Step4는 실기기 수용 미완료로 유지하면서, 소스 정리와 자동 검증 후 PR 통합을 진행한다. 원격 main `27e33e4`에서 로컬 기준 `068b662`까지 기존20개 커밋도 포함되는 범위를 PR에 명시한다. 새 판단75–76은 현재 검증 문서에 기록한다.

simplify 후 자동 검증 완료: `bun run verify all`,327.189초/exit0,전후 source `6b73499bbbd73234fad35652b56d4485e9b82cd8b47bb5e94ed0902748af6dcd` 동일. Doctor100·JS559·계약4·UI78/9suite·Host225+12·JVM118/19suite 및 Rust/Swift 자동 검사 통과. 실기기 Step4는 사용자가 나중에 직접 검사하는 미완료 항목으로 유지한다.

PR #5 첫 CI 후 이식성 보완: macOS 캡처 리소스 선행 빌드, Android cargo-ndk·x86_64·고정 NDK 환경, Windows 만료 테스트 시각과 교체 후 오류 주입 경계를 수정했다. 소스 `89197fba4b7214531f3f917c121eaf909eae33280dd7d844bad5f71691ca8a82`. Host225+12·엄격 검사와 JS559/57파일 통과, macOS 준비 순서 회귀의 수정 전 실패 확인. 별도 OS의 최종 결과는 PR CI로 확인한다. 실기기 Step4는 사용자 후속 수행으로 유지한다.
