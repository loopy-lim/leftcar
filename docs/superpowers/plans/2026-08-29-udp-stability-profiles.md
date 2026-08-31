# UDP Stability Profiles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add negotiated UDP stability presets and advanced controls, strengthen FEC and adaptive pacing, and verify fewer recovery gaps at exact 4K60.

**Architecture:** Viewer and Host exchange optional capability objects, the Host validates and canonicalizes one applied UDP configuration, and the macOS shim receives only validated burst/FEC/adaptation values. Receiver feedback appends loss-shape counters so a pure Swift burst state machine can move automatic sessions between burst four and burst two without changing resolution, FPS, codec, or transport.

**Tech Stack:** TypeScript, React Native, Vitest, Rust 2021, Swift 6, VideoToolbox, UDP, Reed-Solomon FEC, Android NDK MediaCodec, Kotlin/Gradle, Tauri.

**Spec:** `docs/superpowers/specs/2026-08-29-udp-stability-profiles-design.md`

## Global Constraints

- Preserve exact 3840x2160 at 60fps, dual AVE, and direct dual-Surface rendering.
- Do not silently lower resolution/FPS or switch a selected UDP stream to TCP.
- Manual and custom settings are rejected when unsupported; only `auto` may explicitly downgrade.
- Older peers that omit capabilities retain the existing two-parity/burst-eight behavior.
- Keep existing feedback offsets 0-59 unchanged and append feedback v2 to exactly 120 bytes.
- Keep all queues bounded and `splitPostEncodeDeltaDrops == 0`.
- Keep touched orchestration files at or below 500 lines by extracting focused policy modules.
- Keep changes unstaged and uncommitted.
- After React/React Native changes, require root React Doctor `100 / 100`, then rerun typecheck and relevant tests.

## File structure

| Path | Responsibility |
|---|---|
| `crates/control-contract/src/udp_stability.rs` | Typed capabilities, selection, applied configuration, and validation |
| `crates/control-contract/src/host.rs` | Optional catalog/start/status fields |
| `apps/viewer-expo/src/udp-stability.ts` | Local capability intersection and UI option resolution |
| `apps/viewer-expo/src/udp-stability.test.ts` | Client compatibility and selection tests |
| `apps/viewer-expo/app/catalog.tsx` | Preset/advanced controls and controlled reconnect arguments |
| `apps/host-desktop/src-tauri/src/control.rs` | Server-side normalization and rejection |
| `apps/host-desktop/src-tauri/src/backend.rs` | Applied configuration backend boundary |
| `apps/host-desktop/src-tauri/src/ffi.rs` | v7 shim ABI and status parsing |
| `crates/fec-core/src/lib.rs` | Variable two/four parity encoding and backward-compatible decoding |
| `native/android-viewer/src/renderer/fec_stats.rs` | FEC/UDP receiver counters and feedback v2 snapshot |
| `native/android-viewer/src/renderer/stats.rs` | Exact 120-byte append-only feedback body |
| `native/macos-capture-shim/Sources/Transport/UdpStabilityPolicy.swift` | Preset resolution and adaptive burst state machine |
| `native/macos-capture-shim/Sources/Transport/UdpPacingPolicy.swift` | Burst ranges and selected parity count |
| `native/macos-capture-shim/Sources/CaptureShim+Exports.swift` | v7 start ABI parsing validated values |

---

### Task 1: Add negotiated contract types and server validation

**Files:**
- Create: `crates/control-contract/src/udp_stability.rs`
- Modify: `crates/control-contract/src/lib.rs`
- Modify: `crates/control-contract/src/host.rs`
- Modify: `crates/control-contract/tests/contract.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Test: `apps/host-desktop/src-tauri/tests/control_e2e.rs`

**Interfaces:**

```rust
pub enum UdpStabilityProfile { Auto, Responsive, Balanced, Stable, Custom }
pub struct ViewerUdpCapabilities { pub version: u16, pub max_fec_parity_shards: u8, pub split_feedback_bytes: u16 }
pub struct UdpStabilityCapabilities { pub version: u16, pub profiles: Vec<UdpStabilityProfile>, pub burst_datagram_options: Vec<u8>, pub fec_parity_options: Vec<u8>, pub adaptive_pacing: bool, pub requires_reconnect: bool }
pub struct UdpStabilityRequest { pub profile: UdpStabilityProfile, pub burst_datagrams: Option<u8>, pub fec_parity_shards: Option<u8>, pub adaptive_pacing: Option<bool>, pub viewer: Option<ViewerUdpCapabilities> }
pub struct AppliedUdpStability { pub requested: UdpStabilityProfile, pub applied: UdpStabilityProfile, pub burst_datagrams: u8, pub fec_parity_shards: u8, pub adaptive_pacing: bool, pub fallback_reason: Option<String> }
pub fn resolve_udp_stability(request: Option<&UdpStabilityRequest>, host: &UdpStabilityCapabilities) -> Result<AppliedUdpStability, String>;
```

- [ ] **Step 1: Write failing contract tests**

Add literal assertions that omission resolves to legacy burst 8/parity 2, new `auto` resolves to burst 4/parity 2/adaptive, `stable` requires Viewer max parity 4 and resolves to burst 2/parity 4, unsupported custom values fail, and explicit manual values never downgrade.

- [ ] **Step 2: Prove RED**

Run:

```bash
cargo test -p control-contract udp_stability -- --nocapture
```

Expected: compile failure because `udp_stability` types and resolver do not exist.

- [ ] **Step 3: Implement the pure resolver**

Use discrete membership checks. Treat an absent request as the legacy configuration. Allow fallback only for `Auto`; return a Korean-readable error for unsupported manual/custom fields.

- [ ] **Step 4: Extend catalog, start input/output, and session status**

Add optional serde-defaulted capability/request fields and applied-result fields. Preserve old JSON fixtures. Advertise version 1 with profiles `auto/responsive/balanced/stable`, bursts `[2,4,8]`, parity `[2,4]`, adaptive true.

- [ ] **Step 5: Validate before backend start**

Resolve in the control server before allocating a session. Pass `AppliedUdpStability` into `CaptureBackend::start`; include the applied values in `StartStreamOutput` and status.

- [ ] **Step 6: Verify GREEN**

```bash
cargo test -p control-contract udp_stability -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e udp_stability -- --nocapture
```

Expected: all new and backward-compatibility tests pass.

---

### Task 2: Add Viewer capability intersection and launch arguments

**Files:**
- Create: `apps/viewer-expo/src/udp-stability.ts`
- Create: `apps/viewer-expo/src/udp-stability.test.ts`
- Modify: `apps/viewer-expo/src/control.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`

**Interfaces:**

```ts
export type UdpStabilityProfileId = "auto" | "responsive" | "balanced" | "stable" | "custom";
export interface UdpStabilitySelection { profile: UdpStabilityProfileId; burstDatagrams?: 2 | 4 | 8; fecParityShards?: 2 | 4; adaptivePacing?: boolean }
export const VIEWER_UDP_CAPABILITIES = { version: 1, maxFecParityShards: 4, splitFeedbackBytes: 120 } as const;
export function availableUdpStabilityOptions(advertised: unknown): UdpStabilityOptions;
export function resolveUdpStabilitySelection(selection: UdpStabilitySelection, advertised: unknown): UdpStabilitySelection | null;
```

- [ ] **Step 1: Write failing TypeScript tests**

Test absent/malformed Host capability, exact option intersection, stable hidden when parity four is absent, custom values constrained to advertised sets, and legacy Host causing new start fields to be omitted.

- [ ] **Step 2: Prove RED**

```bash
bunx vitest run apps/viewer-expo/src/udp-stability.test.ts apps/viewer-expo/src/launch-stream.test.ts
```

Expected: missing module and missing launch arguments.

- [ ] **Step 3: Implement normalization and intersection**

Reject malformed entries, deduplicate discrete values, and return no selector for an old Host. Keep labels and hints in client code while IDs come from the intersection.

- [ ] **Step 4: Carry selection through prepare/start/restart**

Add optional selection to `StartStreamArgs` and active/restarted stream state. Send the flattened request plus `VIEWER_UDP_CAPABILITIES`; preserve it through transport switching and automatic restart.

- [ ] **Step 5: Verify GREEN**

```bash
bunx vitest run apps/viewer-expo/src/udp-stability.test.ts apps/viewer-expo/src/launch-stream.test.ts
```

Expected: all tests pass with literal request payload assertions.

---

### Task 3: Extend FEC to four parity rows and instrument loss shape

**Files:**
- Modify: `crates/fec-core/src/lib.rs`
- Modify: `native/android-viewer/src/media_datagram.rs`
- Create: `native/android-viewer/src/renderer/fec_stats.rs`
- Modify: `native/android-viewer/src/renderer/mod.rs`
- Modify: `native/android-viewer/src/renderer/stats.rs`
- Modify: `native/android-viewer/src/renderer/split_session.rs`
- Modify: `native/android-viewer/src/renderer/split_session/tile_worker.rs`
- Modify: `native/android-viewer/src/renderer/split_session/tile_worker/helpers.rs`

**Interfaces:**

```rust
pub fn encode_group_with_parity(data: &[Vec<u8>], parity_count: usize) -> Result<EncodedGroup, FecError>;
pub fn decode_group_with_max_parity(received: Vec<Option<Vec<u8>>>, k: usize, width: usize, max_parity: usize) -> Result<Vec<Vec<u8>>, FecError>;
pub struct FecRuntimeStats { /* atomics for feedback v2 fields */ }
pub struct SplitFeedbackSnapshot { /* existing fields plus v2 fields */ }
```

- [ ] **Step 1: Write failing FEC tests**

Add a full eight-shard fixture that recovers four missing data shards with four parity rows. Add an old two-parity fixture decoded by the new four-slot receiver and a five-loss rejection.

- [ ] **Step 2: Prove RED**

```bash
cargo test -p fec-core four_parity -- --nocapture
```

Expected: missing variable-parity APIs.

- [ ] **Step 3: Implement variable parity without changing existing APIs**

Keep `encode_group` and `decode_group` as two-parity compatibility wrappers. Generate Vandermonde rows for indexes 0-3 and validate parity in `1..=4`.

- [ ] **Step 4: Write failing feedback v2 tests**

Assert exact 120-byte length and literal big-endian offsets 60-119 from the design. Assert old 60-byte feedback leaves every appended Host field zero.

- [ ] **Step 5: Prove feedback RED**

```bash
cargo test -p android-viewer renderer::stats -- --nocapture
```

Expected: 60-byte output does not contain the v2 fields.

- [ ] **Step 6: Record receiver loss shape**

Count accepted data/parity datagrams, restored fragments, unrecoverable groups, maximum missing data fragments, one/multi gap events, recovery episodes, duplicate suppressions, and FEC decode failures. Update counters at the real parser/reassembly/recovery branches rather than inferring them from frame gaps.

- [ ] **Step 7: Verify GREEN**

```bash
cargo test -p fec-core -- --nocapture
cargo test -p android-viewer renderer:: media_datagram:: -- --nocapture
cargo check -p android-viewer --target aarch64-linux-android
```

Expected: all tests and Android target check pass.

---

### Task 4: Implement Host burst/FEC policy and v7 ABI

**Files:**
- Create: `native/macos-capture-shim/Sources/Transport/UdpStabilityPolicy.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/UdpPacingPolicy.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+UdpPacket.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+SplitTransport.swift`
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+ViewerControl.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift`
- Modify: `native/macos-capture-shim/Sources/CaptureShim+Exports.swift`
- Modify: `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- Modify: `apps/host-desktop/src-tauri/src/ffi.rs`

**Interfaces:**

```swift
enum UdpStabilityProfile: String { case legacy, auto, responsive, balanced, stable, custom }
struct AppliedUdpStability { let profile: UdpStabilityProfile; let burstDatagrams: Int; let fecParityShards: Int; let adaptivePacing: Bool }
struct UdpBurstPolicyState { mutating func observe(_ input: UdpBurstObservation) -> UdpBurstDecision }
func udpPacingBurstRanges(datagramCount: Int, maxDatagrams: Int) -> [Range<Int>]
func fecParityCount(dataCount: Int, selectedParity: Int, recovery: Bool) -> Int
```

- [ ] **Step 1: Write failing pure-policy tests**

Test auto startup at four, loss transition four to two, no rise before 30 clean seconds, rise after 30 seconds, recovery hold at two for two seconds, stable staying at two, responsive staying at eight, and custom bounds.

- [ ] **Step 2: Prove RED**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests-red
```

Expected: missing stability policy types/functions.

- [ ] **Step 3: Implement pure policy and selected parity**

Use cumulative feedback deltas and monotonic timestamps. Adapt at most once per feedback observation. Tail parity is `min(k - 1, selectedParity)`.

- [ ] **Step 4: Integrate sender paths**

Pass the current burst size to both single and split `udpPacingBurstRanges`. Generate the applied number of parity rows and include parity bytes in pacing. Reset pacing debt at recovery generation changes.

- [ ] **Step 5: Parse feedback v2 and drive automatic mode**

Read appended counters only for 120-byte packets. Feed cumulative incomplete/multi-gap values and recovery send events into the state machine. Export active burst and FEC values in shim stats.

- [ ] **Step 6: Add and use v7 start ABI**

Add `leftcar_capture_start_v7` arguments for canonical profile, burst, parity, and adaptive flag. FFI prefers v7 and falls back to v6 only for the exact legacy configuration; unsupported explicit settings fail rather than downgrade.

- [ ] **Step 7: Verify GREEN**

```bash
tools/build-macos-capture-shim.zsh policy-test /tmp/leftcar-policy-tests
/tmp/leftcar-policy-tests
tools/build-macos-capture-shim.zsh split-test /tmp/leftcar-split-tests
/tmp/leftcar-split-tests
tools/build-macos-capture-shim.zsh library /tmp/libleftcar_capture.dylib
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
```

Expected: policy/split tests, dylib build, and Host tests pass.

---

### Task 5: Add Viewer settings and controlled reconnect

**Files:**
- Create: `apps/viewer-expo/src/UdpStabilityControls.tsx`
- Modify: `apps/viewer-expo/app/catalog.tsx`
- Modify: `apps/viewer-expo/src/udp-stability.test.ts`
- Modify: `apps/viewer-expo/src/launch-stream.test.ts`

**Interfaces:**

```tsx
<UdpStabilityControls options={options} selection={selection} onChange={setSelection} disabled={launching} />
```

- [ ] **Step 1: Add failing behavior tests**

Test preset selection resetting overrides, advanced options producing `custom`, stable hidden when unsupported, and restart preserving display/resolution/FPS/encoder/transport while replacing only UDP stability fields.

- [ ] **Step 2: Prove RED**

```bash
bunx vitest run apps/viewer-expo/src/udp-stability.test.ts apps/viewer-expo/src/launch-stream.test.ts
```

Expected: control-state helpers and restart preservation assertions fail.

- [ ] **Step 3: Implement focused controls**

Use NativeWind classes consistent with the encoder experiment cards. Render four presets, an expandable advanced section, the Host-applied summary, and `적용하고 다시 연결` for an active changed session. Provide accessibility role/state/label for every pressable option.

- [ ] **Step 4: Keep catalog orchestration bounded**

Move all labels, normalization, and selection transitions into `udp-stability.ts`; keep visual controls in the new component. Do not add policy logic to `catalog.tsx`.

- [ ] **Step 5: Verify UI and repository gates**

```bash
bunx vitest run apps/viewer-expo/src/udp-stability.test.ts apps/viewer-expo/src/launch-stream.test.ts
npx -y react-doctor@latest . --verbose
bun run typecheck
bun run test:architecture
```

Expected: tests pass, React Doctor reports exactly `100 / 100`, typecheck and architecture checks exit zero.

---

### Task 6: Build matched binaries and run moving 4K60 acceptance

**Files:**
- Modify: `docs/11-low-latency-investigation.md`
- Runtime artifacts: `/tmp/leftcar-udp-stability-*`

- [ ] **Step 1: Run complete static gates**

```bash
cargo test -p fec-core -p control-contract -p viewer-decoder -p android-viewer -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib -- --nocapture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --test control_e2e -- --nocapture
bun run test
bun run test:contract
bun run test:architecture
npx -y react-doctor@latest . --verbose
bun run typecheck
git diff --check
```

- [ ] **Step 2: Build and install matched Host/APK**

Build the Android release native library and APK, install on `HA2D6EMP`, build/install the signed Host, and compare source/build/installed SHA-256 hashes.

- [ ] **Step 3: Confirm moving source and codec topology**

Use `Option+0`, compare two screenshots at least one second apart, and require two exact AVE encoders plus two `c2.qti.avc.decoder.low_latency` 1920x2160 decoders.

- [ ] **Step 4: Run matched 180-second profile samples**

Run responsive, balanced, stable, and auto with identical moving content. Record applied settings, kernel UDP counters, FEC v2 counters, Host FPS/send metrics, and Android joined FPS.

- [ ] **Step 5: Run injected loss and 600-second auto soak**

Require the existing one-sided loss to produce one paired recovery without lasting corruption. For auto, require joined average at least 59fps, one-second p5 at least 55fps, no 0fps interval, no termination, bounded queues, and no repeated recovery within five seconds.

- [ ] **Step 6: Record evidence and audit completion**

Append accepted and rejected runs with exact timestamps, commands, hashes, and metrics. Stable must reduce incomplete-AU growth by at least 90% versus responsive while Host valid encode remains at least 59fps per side and post-encode drops remain zero.
