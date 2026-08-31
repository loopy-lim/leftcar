# UDP Stability Profiles and Capability Negotiation Design

## Status and relationship to existing work

- Status: approved in chat on 2026-08-29; written review pending.
- Scope: Viewer configuration, Host capability negotiation, UDP pacing/FEC,
  split-stream recovery, and end-to-end diagnostics.
- This extends
  `2026-08-29-4k60-split-flow-control-recovery-design.md`; its send-time AU
  identity, flow leases, dual AVE encoders, and dual-Surface presentation
  remain required.
- Per user direction, implementation changes remain unstaged and uncommitted.

## Evidence and problem statement

The visibly moving `Option+0` desktop was sampled for 24 seconds on TB710FU.
During that interval:

- Android UDP `RcvbufErrors` stayed at `205 -> 205` and `InErrors` stayed at
  `588 -> 588`, so the sample did not lose packets in the Android kernel
  receive queue.
- The kernel accepted about 40,340 UDP datagrams.
- Right incomplete AUs increased `1029 -> 1043` and left incomplete AUs
  increased `1046 -> 1061`.
- Joined presentation advanced by about 1,347 frames, approximately 56.1fps.
- Each incomplete burst was followed by a paired-IDR request and a temporary
  joined-render dip into the 40-50fps range.

The current split sender groups up to eight interleaved datagrams under one
pacing deadline. A full FEC group carries eight data shards and two parity
shards. The evidence rules out receiver socket overflow for this sample, but
does not yet distinguish over-air microburst loss from another pre-kernel loss
source. The next implementation must expose the missing-fragment distribution
before claiming that smaller bursts or stronger FEC fixed the cause.

## Goals

1. Keep exact 3840x2160 at 60fps, dual AVE, and direct dual-Surface rendering.
2. Give users several useful stability choices without exposing unsafe raw
   transport values by default.
3. Let Viewer and Host negotiate the intersection of their supported settings;
   neither side silently accepts an unsupported value.
4. Reduce correlated UDP loss before it becomes an incomplete AU and paired-IDR
   recovery episode.
5. Make automatic mode converge toward the least expensive setting that keeps
   the current session stable.
6. Keep every queue bounded and preserve the low-latency latest-capture policy.
7. Report requested and applied settings, FEC effectiveness, and recovery cost
   on both sides.

## Non-goals

- Do not lower resolution or requested frame rate as an implicit stability
  response.
- Do not switch a selected UDP session to TCP without an explicit transport
  decision.
- Do not add an Android GPU composition pass, CPU readback, or software codec.
- Do not expose arbitrary numeric text fields for burst size, parity count, or
  recovery timing.
- Do not treat Host send success as proof that the Viewer received an AU.

## Considered approaches

### A. Negotiated presets with bounded advanced controls (chosen)

Expose four presets and, only when both peers advertise support, an advanced
section for burst, FEC, and adaptive pacing. The Host revalidates every request
and returns the canonical applied configuration.

This gives repeatable A/B profiles, allows automatic adaptation, and prevents
old or mismatched peers from entering an undefined combination.

### B. Static presets only

This is simpler but cannot react when Wi-Fi conditions change after the stream
starts. A setting that worked at connection time can later produce a recovery
storm without any bounded downgrade path.

### C. Always use stronger FEC or TCP

Always-on strong FEC spends unnecessary bandwidth and packet-processing cost on
clean links. TCP removes packet loss but introduces head-of-line delay and does
not preserve the selected UDP behavior. Neither is a suitable universal
default.

## User-facing settings

The Viewer catalog adds a `전송 안정성` section below the encoder experiment.
Only Host-advertised and locally supported choices are rendered.

### Presets

| ID | Label | Initial burst | Full-group parity | Adaptation |
|---|---|---:|---:|---|
| `auto` | 자동 (권장) | 4 | 2 | May move 4 -> 2 and 2 -> 4 |
| `responsive` | 빠른 반응 | 8 | 2 | Disabled |
| `balanced` | 균형 | 4 | 2 | Disabled |
| `stable` | 안정성 우선 | 2 | 4 | Holds the stable floor |

For exact 4K split, automatic mode never increases to burst 8. It moves to
burst 2 when receiver incomplete-AU or multi-frame-gap counters increase, and
returns to burst 4 only after a clean 30-second window. A paired-IDR recovery
also forces burst 2 for at least two seconds so the recovery boundary and its
first dependency chain do not recreate the same microburst.

### Advanced controls

The expandable `세부 설정` section is available only when the negotiated
capability advertises more than one value:

- `UDP 묶음 전송`: `자동`, `2`, `4`, or `8` datagrams per pacing deadline.
- `손실 복구 강도`: `표준` (up to two parity shards) or `강함` (up to four).
- `상태에 맞춰 자동 조절`: enabled or disabled.

Selecting a preset resets advanced overrides to that preset. Changing an
advanced option marks the effective selection as `custom`. Custom values must
come from the advertised discrete option lists; the UI never constructs an
unadvertised number.

Settings are retained in the active stream model. A change while streaming
shows `적용하고 다시 연결`, uses the existing controlled stop/start path, and
preserves display, resolution, FPS, encoder experiment, and transport.

## Capability and contract model

### Host advertisement

`CatalogView` gains an optional `udpStabilityCapabilities` object:

```text
version: 1
profiles: [auto, responsive, balanced, stable]
burstDatagramOptions: [2, 4, 8]
fecParityOptions: [2, 4]
adaptivePacing: true
requiresReconnect: true
```

An older Host omits the object. The Viewer then omits all new start fields and
uses the existing behavior.

### Viewer request

`StartStreamInput` gains optional fields:

```text
udpStabilityProfile: auto | responsive | balanced | stable | custom
udpBurstDatagrams: 2 | 4 | 8 | null
udpFecParityShards: 2 | 4 | null
udpAdaptivePacing: boolean | null
viewerUdpCapabilities:
  version: 1
  maxFecParityShards: 4
  splitFeedbackBytes: 120
```

An older Viewer omits them. A new Host maps omission to the legacy standard
configuration rather than silently enabling four-parity FEC that an old
receiver may ignore.

### Server validation and applied result

The control server computes the intersection of Host and Viewer capabilities,
then resolves the preset and advanced overrides. Unsupported enum values,
numbers outside the advertised discrete sets, or a request requiring a newer
feedback shape are rejected before capture starts with a user-readable error.

`StartStreamOutput` and session status return:

```text
udpStabilityRequested
udpStabilityApplied
udpBurstDatagramsApplied
udpFecParityShardsApplied
udpAdaptivePacingApplied
udpStabilityFallbackReason
```

The fallback reason is present only for a backward-compatible downgrade that
the request explicitly allowed through `auto`. Explicit manual and custom
selections are rejected rather than silently changed.

The macOS C ABI receives only the validated applied values. The shim never
parses arbitrary Viewer input.

## Sender pacing and adaptation

`UdpBurstPolicy` is a pure state machine with these inputs:

- applied profile and allowed burst options;
- cumulative receiver incomplete AUs;
- cumulative receiver frame gaps;
- current and previous feedback timestamps;
- whether a recovery boundary was just sent;
- clean-window duration.

Its output is the current burst size and a reason. Adaptation is monotonic
toward stability inside a loss window: one observed incomplete AU or
multi-frame gap changes `4 -> 2` immediately. Recovery to `4` requires 30 clean
seconds. It never oscillates more than once in a feedback interval and never
changes FEC parity at runtime; parity changes require reconnect so receiver
capability and memory bounds remain explicit.

Pacing keeps the existing serial network queue and bitrate accounting. A burst
deadline accounts for all bytes in that burst, including parity. No individual
AU may retain pacing debt from an older recovery generation.

## Stronger FEC without a media-wire version change

The parity datagram already carries data-shard count and parity index. Extend
the common FEC implementation from a maximum of two to a maximum of four parity
rows. A new receiver allocates four optional parity slots and can decode from
any available `k` rows.

Compatibility behavior:

- New Host + new Viewer may use parity indexes 0-3.
- New Host + old Viewer defaults to standard parity because the Viewer did not
  advertise version 1 capability.
- Old Host + new Viewer sends only indexes 0-1; the new decoder treats indexes
  2-3 as absent and retains two-loss recovery.
- Data fragments and existing parity indexes are byte-compatible.

Tail groups use at most `min(k - 1, selectedParity)` parity shards. The sender
interleaves each group's data and parity before starting the next group.

## Receiver diagnostics and recovery

Each tile records these cumulative values:

- UDP media datagrams received;
- data and parity datagrams received;
- FEC-restored fragments;
- FEC groups that became unrecoverable;
- maximum missing data fragments in an evicted group;
- incomplete AUs;
- one-frame and multi-frame wire gaps;
- paired-IDR episodes and suppressed duplicate requests.

The feedback packet is append-only. Existing offsets remain unchanged; new
fields are read only when the advertised length is present. Host status takes
the maximum of shared counters and preserves per-tile values where asymmetry is
diagnostically useful.

Feedback v2 is exactly 120 bytes. It preserves v1 offsets 0-59 and appends:

```text
60..67   UDP media datagrams received (u64)
68..75   data datagrams received (u64)
76..83   parity datagrams received (u64)
84..91   FEC-restored fragments (u64)
92..95   unrecoverable FEC groups (u32)
96..97   maximum missing data fragments in one group (u16)
98..99   reserved zero
100..103 one-frame wire-gap events (u32)
104..107 multi-frame wire-gap events (u32)
108..111 paired-IDR episodes (u32)
112..115 suppressed duplicate recovery requests (u32)
116..119 FEC decode failures (u32)
```

Recovery remains one coalesced episode:

1. A multi-frame delta gap requests one paired IDR while continuing decoder
   concealment.
2. Further gap events are telemetry only until the paired IDR arrives or the
   bounded retry timeout expires.
3. An IDR following a gap completes the episode and never requests another IDR
   solely because of its AU ID.
4. A real decoder input failure may start a new episode.

## File boundaries

- `crates/control-contract`: capability, request, and applied-result types.
- `apps/viewer-expo/src/udp-stability.ts`: normalization, intersection, preset
  resolution, and labels.
- `apps/viewer-expo/app/catalog.tsx`: selector, advanced controls, and controlled
  reconnect action.
- `apps/host-desktop/src-tauri`: request validation and applied status.
- `Sources/Transport/UdpBurstPolicy.swift`: pure adaptive burst state.
- `Sources/Transport/UdpPacingPolicy.swift`: discrete burst and FEC policy.
- `crates/fec-core`: four-row encode/decode support.
- `native/android-viewer/.../fec_stats.rs`: receiver FEC diagnostics.

If a touched orchestration file exceeds 500 lines, extract the new behavior by
responsibility rather than extending that file.

## TDD and verification

Write and observe failing tests before production changes for:

- old/new Host and Viewer capability intersections;
- unsupported manual/custom values being rejected;
- `auto` fallback being explicit and manual profiles never silently changing;
- burst `4 -> 2` on loss and `2 -> 4` only after 30 clean seconds;
- recovery forcing burst 2 for two seconds;
- four-parity recovery of four missing shards;
- new receiver decoding an old two-parity group;
- old feedback lengths preserving zero defaults for appended diagnostics;
- selector visibility, custom reset, and reconnect argument preservation.

Run Rust, Swift, TypeScript, Kotlin, Android target, APK, and Host build gates.
Because React Native UI changes, require root React Doctor `100 / 100`, then
rerun typecheck and relevant tests.

## Physical acceptance

Use matched installed Host/APK binaries, exact 3840x2160@60 split, direct Wi-Fi
UDP, and a visually confirmed moving `Option+0` desktop.

1. Capture 180-second runs for responsive, balanced, stable, and auto.
2. Record kernel UDP counters, all new FEC counters, applied settings, Host
   encode/send metrics, and Android joined-render metrics.
3. Stable and auto must average at least 59 joined fps with rolling one-second
   p5 at least 55fps, no 0fps interval, and no stream termination.
4. Stable must reduce incomplete-AU growth by at least 90% against the matched
   responsive run and must not create a repeated recovery episode within five
   seconds of a paired IDR.
5. Host valid split encode remains at least 59fps per side, post-encode delta
   drops remain zero, and queue/lease limits remain bounded.
6. Run the existing one-sided loss injection and 600-second moving soak using
   auto, then record accepted and rejected runs in
   `docs/11-low-latency-investigation.md`.

## Completion boundary

UI presence, passing unit tests, or lower gap counters in one short run are not
completion. Completion requires negotiated settings to be truthful on both
peers, backward compatibility tests to pass, matched binaries to run on the
physical device, and the moving-screen acceptance criteria above to hold.
