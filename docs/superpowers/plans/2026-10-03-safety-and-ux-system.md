# Leftcar safety and UX implementation plan

> **For agentic workers:** Use subagent-driven-development for the independent Host, Viewer and safety tasks; root executes the shared foundation and integration. Track source evidence and incomplete requirements here across continuations.

**Goal:** Improve the entire application safety and UX using standardized Tailwind/Uniwind/cn/cva design components.

**Architecture:** Shared TS tokens generate platform theme CSS. Shared utility recipes feed thin DOM and RN primitives; platform screens own domain state and cancellation. Existing native Rust and Android boundaries retain their security semantics while remaining resource/ownership defects are fixed.

**Tech Stack:** Bun, Tailwind 4.3.3, Uniwind 1.11.0, CVA 0.7.1, React 19.2.3, Tauri 2, Expo 57, Rust/Kotlin/Swift.

**Spec:** `docs/superpowers/specs/2026-10-03-safety-and-ux-system-design.md`

## Global constraints

- Preserve existing dirty work and all prior security changes; no commit/push/install/device reset or permission writes.
- Monochrome system theme, one primary action per screen, normal text ≥12px and 4.5:1, all interactive targets ≥44 logical pixels.
- Async actions expose progress, local failure and retry; unknown telemetry renders unknown.
- Root `npx -y react-doctor@latest . --verbose` must score 100/100; rerun typecheck and relevant tests after fixes.
- Do not claim source/build/browser fixture success as physical-device acceptance.

## Task 1: shared design foundation (D1–D3, U4)

Files: `packages/ui-tokens/src/{tokens,cn,recipes,index}.ts`, `tools/generate-ui-theme.ts`, theme CSS, `apps/host-desktop/src/ui/`, `apps/viewer-expo/src/ui/`.

Interfaces: `actionVariants({variant,size,disabled})`, `actionLabelVariants({variant})`, `textVariants({variant,tone})`, `surfaceVariants({variant})`, `inputVariants({invalid})`, `noticeVariants({tone})`. Action variants: primary/secondary/ghost/danger; sizes default/compact/icon all meet 44px. DOM `Button` and RN `Action` accept `busy`, `disabled`, `variant`, `className` and platform event props. RN `Label` and DOM text use `textVariants`. Dynamic dimension styles are the only per-screen geometry exception.

- [x] Add fail-first contrast and `cn` semantic-size regressions; run `bun run test packages/ui-tokens/src/design-system.test.ts`.
- [x] Fix token contrast/drift and merge scale; implement utility recipes, theme generator and platform primitives.
- [x] Compile actual classes through Tailwind and Metro/Uniwind, verify same role colors/type/target dimensions rather than string snapshots.
- [x] Run foundation tests and both app typechecks; independent task review.

## Task 2: Host UX and migration (D3–D4, U1/U3/U4/U6)

Files: all Host TSX components, modals, diagnostics, `index.css`, relevant hooks plus behavior/browser tests.

- [x] Add failing real-UI checks for false initial permissions, hidden stop error, file read/save failure and missing telemetry where confirmed.
- [x] Migrate controls to Button/Text/Field/Notice recipes; replace repeated styling with Tailwind; keep native dialog keyboard behavior.
- [x] Move stop/revoke errors into the affected dialog/row, expose busy state, validate experiment inputs, use accurate LAN/WAN copy and loaded permission state.
- [x] Render desktop Korean/English light/dark at 780×540; check targets, focus, overflow and safety actions; independent review.

## Task 3: Viewer UX and migration (D3–D4, U1–U6)

Files: all Viewer app screens and RN UI cards; shared translations coordinated with root; relevant connection/settings/camera/browser fixtures.

- [x] Preserve cancellation/context/confirmed settings semantics; add behavior regressions for newly found errors before changes.
- [x] Migrate View/Text/Pressable/field/surface/notice styling to real Uniwind classes and shared Action/Label/Field components.
- [x] Give Catalog one settings entry, prioritize display selection and reachable active controls; retain camera/PIN/persistence retry.
- [x] Verify keyboard/PIN paste and native targets/labels, compact/tablet layouts and theme compilation; independent review.

## Task 4: remaining security/runtime findings (S1–S7)

Files determined by read-only audit: Host `file_transfer.rs`, `aoap_media_proxy.rs`; Viewer `file-transfer.ts` and its card/context; Android input/stream UI/lifecycle.

- [x] Confirm each reported flaw using current source and a smallest meaningful regression.
- [x] Bound outgoing file transfer admission; stop blocked USB media work and release proxy slots; pin transfer work to its initiating Host context if confirmed.
- [x] Review native accessible input approval and resource/permission lifetime; implement equivalent native token/control semantics.
- [x] Run scoped native/Rust/JS regressions and independent review; record actual device availability without installing or resetting.

## Task 5: integrated verification and completion audit (V1–V2)

- [x] Run final React Doctor 100/100, root typecheck, JS/contract/architecture tests and all relevant UI suites.
- [x] Run Host native tests/clippy/fmt after native edits, production Host build and Android bundle/native build appropriate to the changes.
- [x] Capture minimum purpose-defined final screen states; verify compiled actual styling, error/loading/empty/action variants, keyboard and target constraints.
- [x] Audit every spec row against current source and receipts; do not close the goal with incomplete/indirect evidence.

## Progress ledger

2026-10-03: Prior turn made security/UI simplification progress. Current source still lacks usable shared RN utility recipes and broad standardized screen styling, so full active goal is incomplete. Read-only Host/Viewer/safety inventory runs in parallel. Worktree remains the existing authorized checkout; no commits/publication. Task 1 is next.

Preflight interface check: Task 1 produces recipes/primitives consumed by Tasks 2/3; workers may not change shared tokens/primitives without root handoff. Tasks 2/3 share translations, coordinated by key ownership. Task 3 file-transfer card overlaps Task 4; safety worker owns transfer semantics and Viewer worker owns visual changes after handoff. Task 5 consumes exact frozen results; it cannot substitute narrow foundation checks for complete-screen evidence.

Ruling: retain the existing optimistic Viewer local-preference controller, with explicit saving/unsaved feedback; only Host security policy gates require native acknowledgement before changing the shown value. This preserves immediate local intent without claiming persistence succeeded. If unsaved feedback proves unclear in rendered UI, revise it before completion rather than replacing the persistence state machine.

Task 1 progress: contrast and semantic `cn` regression observed RED (11 failures) then GREEN (25/25). Shared utility recipes, DOM/RN primitives, generated platform theme CSS, and actual Tailwind-compiled browser fixture styling now exist. Native style compilation and whole-screen migration verification remain outstanding.

Integration progress 2026-10-03: Tasks 1–3 are implemented and independently reviewed. Legacy screen CSS/StyleSheet recipes were removed; generated native Kotlin roles join the shared token source. Actual Uniwind Android compilation confirms both themes, caption/body sizes, 44px targets and focus outlines. Root Doctor is 100/100, typecheck and 913 JS tests pass, production Host/Android bundles pass, and 14 integrated style/browser suites report 102 PASS. Current source/build/physical evidence is recorded in `docs/superpowers/evidence/2026-10-03-safety-and-ux-verification.md`.

Final audit found three remaining concrete gaps after that snapshot: native setting failures hidden behind the Settings sheet, stale authentication failure-IP records without admission bounds, and revoked devices retaining transfer slots/staging. Workers are completing those scoped regressions and fixes. Earlier receipts are not promoted to those new sources; final React/model/UI/bundle and Host native receipts will be refreshed after freeze. Physical USB/video/TalkBack/Windows acceptance remains outside the proven evidence.

Final completion 2026-10-03: all scoped implementation tasks and required gates are complete. Latest root Doctor 100/100 (183 files), root typecheck, 923 JS tests, 4 contract tests, architecture rules, 14 UI/style suites with 103 PASS, Host frontend/native and Android Metro/arm64 builds pass. Final Host Rust 333 tests, workspace Rust 461 (2 existing ignored), Android JVM 158 pass. Independent final review CLEAR, hashes matched. ADB read confirmed one TB710FU; no candidate installation/runtime acceptance performed. Existing unrelated full-workspace fmt/Clippy and Android targeted Clippy failures are recorded in the requirement report. No commits, publication, install or data reset performed.
