# Adaptive Resolution Recovery Design

- Date: 2026-08-31
- Status: approved in chat
- Scope: source-sized stream negotiation, 4K/1440p adaptive recovery, and
  same-window Android rebind

## Goal

Leftcar must start each stream at the source-compatible resolution and keep the
requested frame rate whenever the transport can sustain it. An exact 4K source
starts at 3840x2160, a 1440p source starts at 2560x1440, and a smaller source
is never upscaled by the automatic profile. If a 4K stream cannot sustain its
frame rate during sustained high motion, the system must step down to 1440p60
before the encoder rate controller collapses the transmitted frame rate. When
the same stream is healthy again, it must step back up to its original 4K
target without user intervention.

The viewer must not close and reopen its Activity for this quality transition.
The visible window remains in place while the native renderer and Host session
are rebound. A user-visible reconnect error is reserved for a failed bounded
rebind, not for an expected quality change.

## Product behavior

### Initial source resolution

The catalog's `DisplayInfo.width` and `DisplayInfo.height` are the source
authority. `auto` chooses the existing profile recommendation, then
`fitProfileToDisplay` produces even dimensions no larger than the profile cap.
The automatic path never upscales a source. An exact 3840x2160 source may use
3840x2160; a source below that size uses its fitted dimensions. Manual profile
choices retain their current caps and content-mode semantics.

The `startStream` request and the native `openStream` call must use the same
resolved dimensions. The Host response includes the accepted dimensions and
FPS so the Viewer records and reuses the actual stream shape after a restart.
If a backend cannot accept the requested shape, it returns a bounded
resolution error; it must not silently report a different shape.

### Adaptive quality state machine

Each logical stream keeps its original source target and its current active
target separately:

- `sourceTarget`: the resolution selected from the catalog;
- `activeTarget`: the dimensions currently being encoded and decoded;
- `fallbackTarget`: a 2560x1440 fit for a 4K source, or no fallback when the
  source is smaller than 1440p;
- `qualityState`: `native`, `downshifting`, `fallback`, or `upshifting`.

The Host evaluates quality once per existing performance window. A 4K stream
may downshift only after two consecutive windows satisfy all of these:

1. receiver frame gaps, incomplete access units, or decoder input drops
   increased outside the recovery-burst grace period;
2. encoded/transmitted FPS is below 90% of the requested FPS, or the oldest
   queue age exceeds the existing latency budget;
3. the stream is not inside a rebind or recovery cooldown.

The downshift changes dimensions to the fallback target while keeping FPS at
60. It does not attempt to solve a 4K overload by repeatedly lowering the
  bitrate until the transmitted FPS approaches zero. A 1440p or smaller source
  never downshifts below its source dimensions.

An upshift is deliberately slower than a downshift. A fallback stream must
have four consecutive one-second windows with no new receiver loss, encoded
and transmitted FPS at or above 95% of the requested FPS, queue age within the
latency budget, and no active decoder recovery. It then returns to
`sourceTarget` once. After either transition, a five-second rebind cooldown
prevents oscillation while keeping recovery responsive. A failed upshift keeps
the fallback stream running and restarts the stability window; it does not
close the Activity.

The pure policy must be deterministic and receive observations rather than
reading VideoToolbox or Android state directly. It returns one of:
`Keep`, `Downshift(width,height,fps)`, or `Upshift(width,height,fps)`.

### ABR interaction

Adaptive bitrate continues to react to genuine sustained congestion, but its
quality floor may not drive a 4K stream into a low-FPS state while a resolution
fallback is available. Recovery-induced loss during the one-second grace
period is not counted as a new congestion window. The policy records the
reason and target for every transition so Host status can distinguish
`bitrate_changed` from `resolution_changed`.

## Host/Viewers protocol

`startStream` returns:

```json
{
  "session": 42,
  "width": 3840,
  "height": 2160,
  "fps": 60,
  "qualityState": "native"
}
```

The response uses the existing camelCase JSON field names exactly as shown:
`session`, `width`, `height`, `fps`, and `qualityState`. An internal
`reconfigureStream` operation uses the same viewer port and
session ownership, prepares the new dimensions, waits for the first valid
keyframe, and atomically publishes the new target. If preparation fails, the
old target remains authoritative until the bounded retry is exhausted.

The Viewer stores `sourceTarget`, `activeTarget`, and `qualityState` in
`ActiveStream`. The restore path uses the active target for a retry and the
source target for an eventual upshift. A source or profile change explicitly
resets the state to `native`; a transient render recovery does not.

## Android same-window rebind

For local reason 5 (`render_stalled`), the Activity remains alive. Kotlin
shows a small non-interactive quality/reconnect indicator, emits the existing
termination event with the logical stream identity, and asks the controller
for a rebind. The native renderer releases the old decoder, retains the
Surface, and accepts the new generation only after a valid config/keyframe.
Old-generation packets are discarded.

The controller permits one rebind per session at a time. On success it clears
the indicator and publishes the new dimensions. On failure it retries once
after the existing bounded delay; only then does it use the current
Activity-finish/error behavior. Host/operator reasons 1 through 3 continue to
finish the Activity immediately. Host-unreachable reason 4 retains its
existing bounded reconnect behavior until same-window rebind has a verified
control peer.

## Files and ownership

- `apps/viewer-expo/src/stream-resolution.ts`: source/profile fitting and
  adaptive target helpers, with pure TypeScript tests.
- `apps/viewer-expo/src/launch-stream.ts` and
  `apps/viewer-expo/src/catalog-model-types.ts`: start/restart receipts and
  active source/target state.
- `apps/host-desktop/src-tauri/src/control.rs`: start/reconfigure response,
  target validation, and status publication.
- `native/macos-capture-shim/Sources/Encoder/CaptureSession+AdaptiveBitrate.swift`:
  congestion interaction and resolution-transition request.
- `native/macos-capture-shim/Sources/Encoder/AdaptiveResolutionPolicy.swift`:
  deterministic downshift/upshift state machine and Swift tests.
- `native/android-viewer/src/renderer/single_session/*`: generation-safe
  decoder rebind and render-health integration.
- `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`:
  same-window indicator and rebind lifecycle.
- `apps/viewer-expo/src/use-stream-controller.ts` and related tests:
  single-flight rebind and fallback/upshift state transitions.

Existing modified files from the neighboring encoder/split work are preserved.
Changes will be applied as surgical hunks, and no reset, checkout, or broad
formatting operation is permitted.

## Failure behavior

- If catalog dimensions are unavailable, automatic start fails closed with a
  readable catalog error; it does not guess 4K.
- If 4K preparation succeeds but the first keyframe does not arrive, the
  current stream remains visible until the bounded timeout, then falls back to
  1440p if the source supports it.
- If an upshift fails, the fallback stream remains active and the stability
  counter restarts.
- If rebind and its one retry fail, the old Activity behavior reports the
  error; no infinite restart loop is allowed.
- Explicit split requests and the neighboring split experiment remain outside
  this policy and are not changed by automatic resolution control.

## Verification

Automated tests must prove:

1. exact 4K, 1440p, and smaller source fitting without upscaling;
2. two-window 4K downshift at the exact threshold, no downshift for a smaller
   source, and no transition during the recovery grace period;
3. four stable windows plus the five-second cooldown before one upshift;
4. failed upshift/rebind keeps the fallback target and never loops;
5. Host start/reconfigure responses preserve accepted dimensions and FPS;
6. Android old-generation packets cannot publish after a same-window rebind;
7. existing Rust, Swift, TypeScript, Kotlin, architecture, and Android build
   gates; React Doctor `100 / 100` after any React/React Native change.

Physical verification uses only Lenovo Yoga Tab `192.168.0.18:40607`:

- an exact 4K moving source starts at 3840x2160, then steps to 2560x1440 if
  sustained congestion is observed;
- a stable 1440p interval returns to 3840x2160 without Activity closure;
- a non-4K source never requests 4K;
- no reason-5 finish/reopen loop occurs during the test;
- capture FPS, encoded/transmitted FPS, Surface-release FPS, target
  resolution, queue age, frame gaps, and rebind counts are recorded.

This evidence does not claim universal 4K60 or glass-to-glass latency; it
proves the adaptive policy and the connected device's observed behavior.

## Out of scope

- Replacing the existing codec with a software encoder.
- Making the diagnostic split path the production default.
- Upscaling a source in automatic mode.
- Claiming panel or photon-level FPS from software counters.
- Resetting or overwriting unrelated dirty worktree changes.
