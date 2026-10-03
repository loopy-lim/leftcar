# Leftcar safety and UX system

User objective: improve security and runtime safety across the application, improve the entire design and UX, and standardize Tailwind, Uniwind, `cn`, and `cva`.

This continues the existing Host/Viewer work in the current worktree. Existing fixes remain part of the candidate. Publication, commits, installation, device resets, and permission changes are outside this implementation request.

## Scope and product direction

The application consists of the macOS/Windows Tauri Host, Android Expo Viewer, and native streaming/input surfaces. The marketing website is not part of the application. Tailwind applies to DOM, Uniwind to React Native; native Kotlin/Swift surfaces use equivalent platform tokens and accessible native controls rather than CSS.

Keep DESIGN.md's quiet monochrome, system theme, functional hierarchy, readable numbers, and uncluttered layout. A screen presents one main next action. Connection, authorization, selected computer/display, active sharing, and stop must remain understandable. Advanced diagnostics and experiments stay in disclosures/settings; errors and safety decisions are never hidden in those disclosures.

## Requirements

| ID | Requirement | Authoritative completion evidence |
|---|---|---|
| S1 | Stored Host identity must be loaded and validated before network credential transmission; corrupted/unreadable identity storage fails closed. | Session/pairing security regressions plus source review. |
| S2 | Selecting a new Host or leaving a connecting screen cancels old socket, catalog and trust persistence work. | Host/Hub/browser and session cancellation regressions. |
| S3 | Settings read/save failure preserves stored data, blocks writes after failed reads, and offers actionable retry. Host security gates reflect acknowledged native state; Viewer local preferences may apply user intent immediately but must visibly distinguish saving/unsaved intent from persisted state. | Real Host/Viewer settings failure UI and native settings tests. |
| S4 | Control/media operations have bounded resource admission and shutdown; slow peers cannot hold slots forever. | Native bounded-write, shutdown, revocation, admission and proxy regressions. |
| S5 | File sharing preserves receiver path/owner limits, bounds transfer state, and pins every async picker/transfer/reconnect to the initiating Host. | File-name, concurrency/owner and cross-Host transfer tests. |
| S6 | Script execution and network policy remains restricted in the Host WebView. | Actual configured-CSP browser regression and Tauri build. |
| S7 | Input permission, request, revocation, stream stop and disconnect remain explicit and accessible. | Real controls/error/keyboard tests; native accessibility/source and runtime checks where available. |
| D1 | Shared TS tokens own colors, typography, spacing, radius and target sizes; CSS is produced from these tokens and drift is checked. | Token/CSS consistency check, computed appearance in both themes. |
| D2 | Shared utility recipes use `cva`; shared `cn` resolves conflicts without dropping semantic text sizes. | Class composition and real compiled-style verification. |
| D3 | Host controls use real Tailwind utilities and common DOM primitives. Viewer controls use real Uniwind utilities and common RN primitives. Installing dependencies alone is insufficient. | Source inventory, Vite CSS build, Metro native class compilation, rendered controls. |
| D4 | All application screens and user-facing cards use common roles for action, text, field, surface and notice. Runtime geometry/native configuration may use typed dynamic styles. | Screen/component migration inventory with exceptions explained. |
| U1 | First launch guides the user to permission/pairing/connection without invented availability or WAN claims. | Dashboard/Hub/Host-picker first-run and empty/error/pending fixtures. |
| U2 | Viewer catalog prioritizes display selection and active sharing; one settings entry holds advanced quality/UDP/experiments. | Catalog compact/tablet rendered flow and interaction tests. |
| U3 | Every async user action exposes progress, a local failure and usable recovery; no fabricated telemetry, silently reset invalid fields, or invisible modal errors. | Stop/revoke/file/settings/validation failures and missing-data fixtures. |
| U4 | Interactive targets are at least 44 logical pixels, text is at least 12px, normal text meets 4.5:1 contrast on its intended surfaces, keyboard focus is visible, disabled/busy/selected states are conveyed. | Computed target/contrast checks, keyboard/screen-reader semantics and native source/runtime evidence. |
| U5 | Pairing PIN supports numeric entry, paste, focus and validation; camera denial/settings/retry remains actionable. | Pairing input and camera lifecycle regressions. |
| U6 | Responsive layouts remain usable at Host 780×540, compact Viewer width, tablet/landscape, both themes and Korean/English. | Purpose-defined screen fixtures, overflow/layout and interaction checks. |
| V1 | Root React Doctor must be 100/100 without suppressions; after fixes typecheck and relevant tests must pass. | Final command output. |
| V2 | Requirements are audited against exact current sources and test/build/render receipts; automated checks, packages and physical runtime evidence are distinct. | Requirement-by-requirement completion report with explicit remaining gaps. |

## Design implementation

Use existing installed Tailwind 4.3.3, Uniwind 1.11.0, CVA 0.7.1, clsx and tailwind-merge. Retain legacy class recipes during migration only; migrated controls consume shared `actionVariants`, `actionLabelVariants`, `textVariants`, `surfaceVariants`, `inputVariants` and `noticeVariants`. Theme CSS and scales are generated from `packages/ui-tokens/src/tokens.ts`; DOM and RN render the same role colors. `cn` registers the custom typography scale so `text-body text-ink` remains both a size and color.

DOM and RN primitives retain platform semantics: DOM Button forwards native button properties with default `type=button`, `aria-busy` and disabled busy state; RN Action forwards Pressable properties, role/state, minimum target and a visible progress label. Form fields have an explicit accessible name and local help/error. Notices use alert/status semantics and have optional retry actions. Host policy switch value changes after native acknowledgement. Viewer local preference intent keeps the existing optimistic controller contract, with explicit saving/unsaved feedback and persisted-state retry.

The Host keeps one dashboard, clearly ordered permission/pairing setup, sources and active sessions; each operation error lives beside the affected action or inside its active dialog. Viewer Hub connects or resumes, Host picker separates discovered/recent/manual addresses, Pairing explains QR/PIN alternatives, and Catalog selects displays before advanced settings. Active streams and their stop/input controls remain immediately reachable.

## Safety review boundaries

Audit current Rust/Tauri auth, settings, file transfer, AOAP media backpressure, Viewer credential/connection/file ownership, and native Android input/stream lifecycle. Fix reproducible issues with behavior-oriented regression evidence. A green unit suite does not certify arbitrary absence of vulnerabilities, performance, permissions or physical long-duration behavior. Record those limits rather than declaring them complete by implication.

## Execution ownership

Root: design foundation, shared tokens/recipes/primitives, CSS compilation checks, integrated UI fixtures, final requirement audit.

Host worker: Host screen/control migration and Host UX defects. Viewer worker: RN screen/control migration and Viewer UX defects. Safety worker: bounded resource/transfer/proxy/lifecycle fixes. Shared translations are coordinated by explicit key lists to avoid concurrent overwrites.
