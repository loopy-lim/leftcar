# 4K60 Split Flow Control and Recovery Design

## Status and relationship to the existing design

- Status: approved in chat on 2026-08-29; implementation not yet started.
- Scope: macOS `splitVertical` Host transport and Android split receiver recovery.
- This document corrects the post-encode latest-wins policy in
  `2026-08-29-4k60-dual-surface-split-stream-design.md`.
- The existing dual AVE, two-port UDP/FEC, two exact MediaCodec decoders, and
  direct dual-Surface presentation architecture remain unchanged.
- The earlier rule that allowed network queue overflow to discard an encoded
  delta chain and request another paired IDR is superseded by this document.
- Per user direction, implementation changes remain unstaged and uncommitted.

## Problem statement and evidence

The current physical run separates the failure from the capture, encoder,
socket, and kernel receive paths:

- Host capture, submit, and valid split output sustain `60/60/60fps`.
- Split pair callback timeouts and pair drops are zero after extending the
  callback expiry to the bounded five-pair in-flight window.
- Host UDP send failures are zero and Android `RcvbufErrors` do not increase.
- Android joined rendering remains at `0fps` while left and right frame gaps
  increase.
- Host recovery queue drops increase into the thousands during the same run.

Two policies form a feedback loop.

First, the Host assigns an AU ID while packetizing an encoded pair, before the
pair reaches the socket. Queue overflow and recovery replacement can then
discard that pair. The next transmitted pair keeps its later AU ID, so the
Viewer observes a wire gap even when the network delivered every datagram the
Host actually sent.

Second, the Android split worker emits `Loss` for an AU gap before checking
whether the current AU is an independently decodable IDR. A recovery IDR that
follows intentionally discarded pre-boundary deltas can therefore request a
second recovery. The single-stream renderer already avoids this error by
treating a keyframe as a valid recovery boundary.

The resulting loop is:

```text
post-encode pair discarded
  -> transmitted AU ID jumps
  -> split worker emits Loss
  -> Host requests paired IDR
  -> large IDR is paced while more encoded deltas accumulate
  -> recovery queue discards those deltas with assigned AU IDs
  -> IDR itself arrives after another apparent gap
  -> split worker emits another Loss
```

Increasing queue capacity or IDR pacing does not remove the loop. A larger
queue only delays it, while the previously tested 160Mbps recovery burst
exhausted MediaCodec input slots and reduced rendering to 3-6fps.

## Goals

1. Preserve exact 3840x2160 capture and the 60fps product profile.
2. Keep the existing dual AVE and direct dual-Surface paths without adding an
   app-owned Android GPU composition pass.
3. Drop stale screen captures before encode rather than dropping ordinary
   dependency-bearing H.264 output after encode.
4. Keep every admitted pair bounded from capture admission through socket send.
5. Assign one shared left/right AU ID only when the pair becomes a wire-visible
   transmission.
6. Let one paired IDR terminate one recovery episode even when its AU ID follows
   a real or intentional gap.
7. Keep post-encode ordinary-delta drops at zero and expose any violation as a
   separate metric.
8. Preserve the existing 4K60 throughput, tile-skew, latency, and physical soak
   acceptance criteria.

## Non-goals

- Do not reduce resolution, frame rate, or decoder count.
- Do not replace UDP/FEC with TCP, RTP, WebRTC, QUIC, or a new wire version.
- Do not increase recovery pacing above the validated 64Mbps ceiling.
- Do not add all-intra encoding, software codecs, OpenGL ES, Vulkan, or CPU
  readback.
- Do not change the single-stream transport or the USB/AOAP transport in this
  correction.
- Do not make bitrate or quality changes part of recovery correctness.

## Chosen approach

Use reference-aware, credit-based split flow control in three layers:

1. Android classifies an IDR following a gap as a usable recovery boundary,
   not as a reason to request another IDR.
2. Host admission reserves one bounded lease before split preparation and dual
   encode. If no lease is available, only the pending unencoded capture is
   replaced with the newest capture.
3. Encoded pairs remain ordered and lossless until send, except that an explicit
   recovery IDR may replace unsent pairs from the older dependency generation.
   Wire AU IDs are allocated at send time, after this replacement decision.

This approach is preferred over a receiver-only patch because a receiver-only
patch leaves normal post-encode overflow capable of breaking the H.264
reference chain. It is preferred over a transport replacement because the
current direct LAN UDP path already sustains the required steady-state bitrate
and has no observed kernel or send failures in the failing sample.

## Host architecture

### Split flow lease

Add a pure `SplitFlowControlState` owned by `CaptureSession`. One lease covers a
logical pair from capture admission until one of these terminal events:

- both tiles are sent successfully;
- the pair is released by an explicit recovery boundary;
- packetization or encoder failure starts hard paired recovery;
- the session stops and releases all leases.

The hard capacity equals the existing split encoder in-flight limit. This keeps
the five-pair callback window that was required for dual AVE throughput without
allowing a separate unbounded packetization or network queue. Metrics expose
admitted leases, active leases, admission drops, and leaked-lease recovery.

`drainEncodeQueue` obtains a split lease before dequeuing and submitting a
capture pair. When no lease is available, the existing single pending capture
slot remains latest-wins. No sequence or AU ID is consumed for the replaced
capture.

Encoder completion still releases the existing `encodeInFlight` slot so dual
AVE can accept more work, but it does not release the split flow lease. The
network send terminal event releases that lease and schedules pending capture
work.

### Ordered encoded pair queue

Replace `SplitPacketizedTile` plus the global pending tile-config maps with an
atomic `PendingSplitAccessUnit`:

- pipeline submission sequence;
- recovery generation and requested-keyframe marker;
- left and right Annex-B payloads;
- optional left and right codec configuration;
- capture and encode timestamps;
- queued timestamp and lease identity.

The pair assembler may complete callbacks asynchronously, but the transport
emits admitted pairs in pipeline submission order. A missing or failed pair is
not skipped as an ordinary delta; it starts hard paired recovery so a later
delta is never sent across a broken encoder dependency.

The queue is bounded by the lease capacity. An ordinary delta reaching a full
queue is an invariant violation, not a reason to drop an arbitrary encoded
pair. The implementation records `splitPostEncodeDeltaDrops` and the acceptance
value is exactly zero.

### Send-time wire identity

Packetization converts each `CMSampleBuffer` into Annex-B payload and config but
does not prepend the `G + AU ID + L2` media header. `drainNetwork` allocates the
next 16-bit AU ID only after selecting the pair that will actually be sent. It
adds the same ID to left and right envelopes, then fragments, protects, paces,
and sends both sides.

Consequences:

- pre-encode capture replacement consumes no ID;
- a pre-IDR recovery-boundary discard consumes no ID;
- left and right always expose the same wire ID;
- every ID the Host allocates represents one attempted wire-visible pair;
- a genuine partial send or network loss remains observable as a real gap.

The receiver continues deriving presentation PTS from the expanded wire ID.
Capture-wall and encode-wall timestamps remain in the envelope for latency
measurement, so dropping an unencoded capture does not falsify latency.

### Recovery boundary

Viewer recovery requests remain soft encoder requests: they do not invalidate
dual AVE callbacks already in flight. The Host records a new transport recovery
generation and forces the next admitted pair to be a paired IDR.

While waiting for that IDR:

1. stop admitting ordinary delta pairs;
2. release unsent older-generation deltas and their leases;
3. allow one newest capture to acquire the freed lease and request paired IDR;
4. discard late callbacks from the older transport generation without assigning
   wire IDs;
5. atomically enqueue the paired IDR with both codec configs;
6. resume ordinary admission only after the IDR pair is handed to the sender.

These discards are measured as `splitRecoveryBoundaryDiscards`, not as ordinary
post-encode queue drops. They are decoder-safe because the next transmitted
access unit is an independently decodable paired IDR.

An encoder callback error, pair assembly failure, packetization failure, or
partial send starts hard paired recovery. Every error branch must release or
transfer its lease exactly once.

## Android architecture

### Split gap decision

Extract a pure `SplitFrameGapDecision` helper so receiver behavior is testable
without MediaCodec or a socket. Inputs are previous AU ID, current AU ID,
`keyframe`, and `awaitingKeyframe`. Outputs separately describe telemetry,
decoder admission, and coordinator recovery action.

Rules:

1. No missing ID: feed the frame normally.
2. Missing ID and current frame is IDR: record the missing count, feed the IDR,
   clear tile `awaitingKeyframe`, and emit only `CoordinatorEvent::Idr`.
3. Missing ID and already awaiting IDR: discard the delta and do not emit a
   duplicate Loss.
4. Missing ID and not awaiting IDR: record the gap, enter awaiting state, emit
   one `CoordinatorEvent::Loss`, and discard the delta.
5. MediaCodec rejecting an IDR is a real decoder failure and may emit Loss after
   the IDR feed attempt; it is not suppressed as a gap classification event.

Both tile workers use the same helper. A paired IDR with the same generation
causes `PairedRecoveryGate` to return `ResumePair` even if either IDR followed a
wire gap. Mismatched generations continue waiting for the peer, and the existing
750ms retry remains the fallback for a genuinely missing IDR.

### Presentation behavior

The pair presentation coordinator, one-frame-plus-4ms wait, timed dual-Surface
release, and last-complete-pair behavior do not change. Recovery clears pending
unmatched decoder outputs before accepting the new paired IDR generation.

## Metrics and diagnostics

Add and export these split-only values:

- `splitFlowActiveLeases`
- `splitFlowCapacity`
- `splitPreEncodeAdmissionDrops`
- `splitEncodedQueueDepth`
- `splitEncodedQueueOldestUs`
- `splitRecoveryBoundaryDiscards`
- `splitPostEncodeDeltaDrops`
- `splitWirePairsAttempted`
- `splitWirePairSendFailures`
- `splitKeyframeGapRecoveries`
- `splitDeltaGapRecoveries`

The Desktop inspector groups them under split flow/recovery diagnostics. This
is a diagnostics-only UI change; it does not add a user-controlled quality or
transport setting.

Required steady-state invariants:

```text
0 <= active leases <= capacity
encoded queue depth <= capacity
post-encode delta drops = 0
left wire AU ID = right wire AU ID for every pair
at most one paired recovery request per unresolved recovery episode
```

## File boundaries

Keep the new state machines independent from orchestration:

- `Sources/Split/SplitFlowControlState.swift`: lease accounting and recovery
  generation.
- `Sources/Transport/PendingSplitAccessUnit.swift`: atomic encoded payload and
  send-time envelope construction.
- `Sources/Transport/SplitWireSequence.swift`: shared send-time AU ID policy.
- `renderer/split_session/gap_policy.rs`: pure split gap decision.
- Existing orchestration files call these units but do not absorb their policy.

If the implementation makes any touched source exceed 500 lines, split it by
responsibility before completion. Generated and vendored files are excluded.

## TDD and verification

### Swift policy tests

Write failing tests first for:

- lease acquisition up to capacity and denial beyond capacity;
- pending capture replacement consuming no lease or wire ID;
- encode completion retaining the lease until send completion;
- successful send releasing exactly one lease;
- recovery boundary releasing queued and late older-generation pairs;
- paired IDR becoming the next wire-visible pair;
- contiguous send-time IDs across pre-encode and recovery-boundary discards;
- UInt16 wrap preserving identical left/right IDs;
- all failure and stop paths returning active leases to zero;
- ordinary post-encode delta drop remaining zero.

### Rust policy tests

Write failing tests first for:

- delta after a gap emits one Loss and is not fed;
- another delta while awaiting recovery emits no duplicate Loss;
- IDR after a gap is fed and emits Idr without Loss;
- same-generation left/right IDRs resume the pair;
- mismatched IDR generations wait for the peer;
- a real MediaCodec input failure after an IDR starts a new recovery;
- AU ID wrap does not create a false gap.

### Static and build gates

Run the existing Swift split/policy tests, Rust workspace tests, Android target
check, Kotlin debug/release compilation, APK assembly, Tauri/Rust tests,
TypeScript typecheck, Vitest, architecture checks, and `git diff --check`.
Because Desktop React diagnostics change, run
`npx -y react-doctor@latest . --verbose` from repository root and require exactly
`100 / 100`, then rerun typecheck and relevant tests.

### Physical gates

Use matched installed Host and APK binaries on `TB710FU`, direct Wi-Fi UDP, exact
3840x2160 at 60fps, and the known `Option+0` moving workspace.

1. Confirm two exact AVE encoders and two
   `c2.qti.avc.decoder.low_latency` instances.
2. Run a 30-second static sample and a 180-second high-motion sample.
3. Require left and right valid encode average `>=59fps`, joined render average
   `>=59fps`, and rolling one-second p5 `>=55fps` for all three.
4. Require encoder drops zero, post-encode delta drops zero, pair-ready maximum
   `<=16.667ms`, pending decoder output at most one per tile, and no 0fps interval
   or stream termination.
5. Inject exactly one right-tile AU loss after 300 pairs. Require one recovery
   request, matched paired IDR, and resumed joined presentation without lasting
   half-screen corruption.
6. Run the full 600-second moving-screen soak. Require the same FPS bounds,
   capture-to-render p95 `<=50ms` without increasing trend, bounded queues, tile
   skew `<=16.7ms`, and no long freeze or stop.
7. Record timestamps, exact commands, source/installed hashes, device identity,
   codec names, metrics, screenshots, and rejected runs in
   `docs/11-low-latency-investigation.md`.
8. Copy a timestamped verified APK to Downloads only after the repository build,
   installation, hash match, and physical gates all pass.

## Completion boundary

Host 60fps alone is not completion. Completion requires every static gate and
all physical gates above to pass with matched binaries. A run performed on an
older Host or APK, a static source mistaken for a moving source, or a sample
without Android joined-render evidence is rejected rather than reused.
