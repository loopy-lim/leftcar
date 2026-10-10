# Input-only Wi-Fi keyboard and mouse sharing implementation plan

> **For agentic workers:** Use subagent-driven-development task by task. Preserve the initial dirty foundation. No commits, installation, permission grants, or device resets are part of this execution.

**Goal:** Each OS keeps its own screen; a physical mouse crosses the configured shared edge, keyboard follows the pointer, and a normal return preserves the armed sharing session.

**Architecture:** Reuse pinned pairing and secure control for negotiation. A dedicated native duplex TCP connection with TCP_NODELAY carries bounded encrypted KM messages independently from video and the React runtime. TCP provides ordered reliable transitions; native sender workers aggregate motion and send at most every 8ms. Host owns epochs and state. Android uses an explicitly approved Shizuku UserService over local Binder to create and feed UHID; the helper has no network listener and records no keys.

**Tech Stack:** Rust/serde/secure-channel, Swift CoreGraphics, Kotlin/Shizuku/AIDL/UHID, Expo/React.

**Spec:** `docs/superpowers/specs/2026-09-25-wifi-bidirectional-km-design.md`, `2026-10-03-wifi-km-optional-usb-roadmap.md`, and the user conversation approval on 2026-10-03 of input-only edge sharing and USB-started Shizuku helper.

## Global constraints

- Product traffic is authenticated direct Wi-Fi. Wireless ADB and USB product transport are excluded.
- Shizuku helper startup via USB and app approval were explicitly approved in this conversation; do not auto-enable debugging, accessibility, Shizuku grants, or Mac TCC permissions.
- No fake pointer, touch gesture, text-only, or toggle-only substitution for the original goal.
- OFF until both directional consent and current OS authority; release before restoring local input; watchdog and disconnect fail closed; reconnect does not automatically arm.
- Android source observation consumes events unless a separately verified observed-source capability is present. Never consume in Armed merely to detect edges.
- Incoming synthetic input must never re-enter physical forwarding. Never capture touchscreen.
- React changes require React Doctor 100/100 at repository root, followed by typecheck and relevant tests.

## Shared interfaces

Wire DTOs live in `crates/control-contract/src/km.rs`. Kotlin mirrors their camelCase JSON under `km/KmWire.kt`; encrypted TCP framing is u32 BE frame length followed by existing StreamSealer counter-u64-BE + ciphertext + tag, plaintext <=8192B. KM keys use HKDF-SHA256, salt=sessionId UTF8, info=`leftcar/km/v1`, 64 output bytes; first 32 c2s, next 32 s2c.

Envelope: version=1, sessionId string (<=128B), epoch u64, seq u64, origin `mac` or `android`, payload discriminated by `kind`. Payloads: hello; state {owner:local|macToAndroid|androidToMac}; move {dx:f64,dy:f64}; button {button:u8,down:bool}; wheel {dx:f64,dy:f64}; key {code:u16,down:bool}; cursor {x:f64,y:f64,width:u32,height:u32}; edge {returning:bool}; releaseAll; heartbeat; stop. All numbers finite/bounded; schema deny unknown fields; trusted current epoch mandatory; ordered TCP prevents replay at AEAD layer. Transitions advance epoch and release the old owner before the new state is applied. Normal return keeps local physical suppression pending until an authenticated Heartbeat confirms the new epoch after peer release application; timeout fails closed.

Control negotiation commands: getKmStatus, startKm, stopKm. startKm params include arrangementSide left/right, tabletWidth, tabletHeight, viewerConsent {macToAndroid,androidToMac}, Android capabilities {cursorInjection,mouseObservation,mouseCapture,keyboardCapture}; OS capabilities never inferred from streaming permission. Authenticated remote device only; loopback cannot impersonate a KM peer. Response includes sessionId, epoch, port, sessionKey (base64url), current capability/readiness; secret only travels over existing sealed authenticated control. If host/Android preflight is missing, startKm fails with actionable reason and starts no tap/socket/video. Same authenticated connection ownership governs revoke/disconnect.

Native Mac backend has separate start/stop/preflight/inject callback exports; never instantiate CaptureSession. Android Km service is foreground connectedDevice and independent from StreamActivity; Shizuku service is explicit-bound local Binder only. Observed-motion hidden API on Android16 is a capability candidate, not a default assumption; support/flag/readback checks precede activation.

## Task 1: Domain safety and shared wire

Files: domain km.rs; control-contract km.rs/lib.rs; secure-channel lib.rs. Tests: Rust new normal return, denied Android receive authority, Armed never consumes, stale epochs and crypto cross-language vectors.

- [ ] Write failing normal return and authority/capture regressions. Run cargo test -p domain km --locked and confirm failure.
- [ ] Add ReturnToLocal release-first transition preserving Armed; consuming motion starts only in Active(AndroidToMac), observed Armed capability stays distinct; actual Android cursor authority is a required MacToAndroid gate.
- [ ] Write bounded wire and key-derivation tests red; implement DTOs, validator and km_keys(seed,sessionId) with fixed known fixture.
- [ ] Run cargo test -p domain -p control-contract -p secure-channel --locked and scoped clippy. Supply exact interface and fixture report; no commit.

## Task 2: Native Mac backend and independent host session

Files: host km.rs/control.rs/lib.rs; native macos shim Sources/Km and exports; tools/build-macos-capture-shim.zsh. Tests: no-video authenticated fake backend integration, native fake event policy, decrypt/replay/heartbeat.

- [ ] Write red tests proving no CaptureBackend start, rejected unapproved peer, normal edge return and disconnect release-first.
- [ ] Implement new bounded native session worker and separate Swift event tap/injector. Exclude tagged synthesized events; native emergency Ctrl+Alt+Cmd+Esc; desktop bounds supports negative origins.
- [ ] Require Mac preflight and independent KM consent before suppressing; expose status/negotiation behind existing authentication and current connection revocation.
- [ ] Run focused host and Swift tests plus native library build. Do not exercise actual system suppression or grant TCC automatically.

## Task 3: Android helper, capture and background transport

Files: Android km package/AIDL/manifest/build.gradle. Tests: JNI-free helper policy, HID report encoding, wire interoperability, epoch and lifecycle ordering.

- [ ] Write red tests for HID keyboard/mouse reports, wire vector, source observation vs consuming capture, return/disconnect key release, and unsupported capability OFF.
- [ ] Add Shizuku UserService local AIDL: prepare/release, move/button/wheel/key; no sockets/logged keys in helper. Open only UHID and approved physical input adapters; close releases held state.
- [ ] Add explicit human-enabled accessibility service and independent foreground native socket worker; normal Armed observation uses only verified observe-and-pass API. Gate KeyBridge ownership acquire acknowledgement to prevent duplicate remapping.
- [ ] Compile and run relevant JVM tests; build APK through existing signing configuration preserving data. Device install/permission/hardware tests require a connected device and separate human grants.

## Task 4: UI wiring and final acceptance

Files: Expo connect/session UI and bridge; Host peer-facing consent/status. Tests: direct Wi-Fi chosen despite attached USB, independent consent/revoke/blocked helper, no streaming start.

- [ ] Write behavioral tests for independent input-only start/stop and actionable permission state. Wire existing peer connection without opening stream Activity.
- [ ] Keep UI to sharing on/off, left/right arrangement, permission state and recovery. Native threads carry high-frequency events.
- [ ] React Doctor 100/100, typecheck/relevant JS tests, affected Rust/JVM/Swift checks and APK build.
- [ ] Record exact current device/runtime evidence; do not claim full goal if actual native pointer/hover/both edge crossings remain untested.


## Task 5: Own-service shell observation delegation

The user explicitly approved this added helper capability after concrete design and scoped five-gap contract review. Binding extension specification: [own-service proposal](../../research/2026-10-03-input-sharing-review/observer-delegation-proposal.md), SHA2be190f1eb8e09e038bff1646de60d54425a58c021ccf1d935aff2d1f1873579. Exact deployed OEM evidence and existing fixed unsupported outcome are archived in the review record; all temporary diagnostics have been removed and same-signer f90061 package installed with settings/services preserved. This is a necessary approved addition, not a reopened earlier source-review task.

Implement only current own-service connection/dispatch delegation through the existing app-bound shell helper, closed mode table and confirmed system+local OFF rollback; targeted6.1 member access without global exemptions. Preserve KeyBridge, held-state/ownership/generation/death/cleanup gates, all input/native boundaries and current Host artifact. No other apps, permission grants/global policy/monitor/UiAutomation/stream capture or substitute cursor.

Execution: exact OEM bookkeeping prerequisite; production-adapter red/green tests; task-scoped source/security review with fix loop; guarded Root integration and normal full Android build/signer/source/pull proof; current-device own-member/configuration preparation, physical edge/native hover-click-wheel-key and recovery proof. All source/package/device/input evidence remains distinct. Unsupported prerequisite must stop within approved route rather than widening permissions or targets. No commit/push/merge is authorized.

## Task 6: Bounded native handshake diagnosis

Current exact Task5 artifact passes preparation and reaches Host approval; after both directions are allowed it fails at nativeHandshake with a generic reason. Actual-device unchanged KmCipher dummy probe passed, so no cipher/provider change is justified. Add a temporary native-only max20 closed-stage and exception-class probe covering start validation, socket, first encrypted hello, first receive/decode and initial State application. Preserve all protocol, timeouts, gates, cleanup and data paths. Independent exact-delta source review precedes Root normal full packaging/same-signer update, then one human attempt with exact-tag bounded120s capture. Remove temporary diagnostics immediately after minimum cause evidence, retaining any reviewed root-cause repair. No Host/React/permission/KeyBridge change and no input or raw error contents in logs. Binding execution brief is plan scratch task-6-brief.md.
