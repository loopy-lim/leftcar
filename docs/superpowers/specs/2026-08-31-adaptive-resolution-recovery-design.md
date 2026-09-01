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

The viewer must not close and reopen its Activity for this quality transition
or for its bounded retry. The visible window remains in place while the native
renderer and Host session are rebound. A user-visible reconnect error is
reserved for a failed bounded rebind after the on-screen retry state has been
shown, not for an expected quality change.

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

The decoder is configured once with `KEY_MAX_WIDTH` and `KEY_MAX_HEIGHT` at
the source target, so every later transition stays on the same Surface under
`FEATURE_AdaptivePlayback`: a mid-stream picture-size change is absorbed
through `INFO_OUTPUT_FORMAT_CHANGED` without a second `configure()`.

The controller permits one rebind per session at a time. On success it clears
the indicator and publishes the new dimensions. On failure it retries once
after the existing bounded delay; only then does it use the current
Activity-finish/error behavior. Host/operator reasons 1 through 3 continue to
finish the Activity immediately. Host-unreachable reason 4 retains its
existing bounded reconnect behavior until same-window rebind has a verified
control peer.

## Prior art

Surveyed primary sources (product code, protocol specs, one measurement
paper) on 2026-08-31:

- Adaptation order. Parsec, GeForce NOW, and Stadia adapt bitrate/QP first
  and treat resolution as the last knob. The Stadia measurement paper
  (arXiv:2009.09786) shows where that ends: sustained sub-required bandwidth
  produced visible resolution flapping for over 200 seconds. This design's
  resolution fallback with an ABR floor is the deliberate counter-design:
  spend one clean transition instead of holding a collapsing 4K rate
  controller.
- Asymmetric hysteresis. GCC (draft-ietf-rmcat-gcc) decreases once with a
  multiplicative back-off, increases at most 8% per second, and holds
  between the two; hls.js gates up-switches behind a headroom factor
  (`abrBandWidthUpFactor` 0.7) and a minimum switch interval. The
  2-degraded / 4-stable window split plus the five-second cooldown follow
  the same down-fast/up-slow shape.
- Same-window transitions. MediaCodec documents mid-stream picture-size
  changes for H.264/H.265 under `FEATURE_AdaptivePlayback`; Moonlight
  reconfigures its decoder on the same Surface in the field (a graduated
  flush -> restart -> reset ladder; the Activity never closes). On the host
  side, VideoToolbox has no mid-session dimension contract, and libwebrtc,
  OBS, and FFmpeg all recreate the compression session on size change;
  NVENC's reconfigure API formalizes the companion rule of forcing an IDR
  as the first frame of the new generation. VT session recreation plus the
  keyframe-gated publish in this design matches those conventions.
- Quality change is not reconnect. RDP specifies in-stream surface
  create/delete messages for live layout changes and reserves the
  cookie-based auto-reconnect as the user-visible failure path. RustDesk
  applies quality in place (`set_quality`) with a bounded service switch as
  fallback, and its client discards stale frames via a generation counter.
  Both mirror the separation between bounded rebind and user-visible
  reconnect error used here.

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
- If rebind and its one retry fail, the Activity remains visible with a
  non-interactive retry state and a single explicit retry action; it never
  closes itself or starts a second Activity. No infinite automatic restart
  loop is allowed.
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

Physical verification uses only the Lenovo Yoga Tab (`192.168.0.18`; its adb
port rotates, so match by IP):

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

## Physical verification results (2026-08-31/09-01, Lenovo Yoga Tab run only)

Executed against `192.168.0.18` (TB522FU untouched). Evidence:
`/tmp/leftcar-adaptive-host.jsonl` (5,759 one-second getStatus records),
`/tmp/leftcar-adaptive-logcat.log` (325,981 lines),
`/tmp/leftcar-adaptive-pid.log`, summary in `/tmp/leftcar-adaptive-summary.md`.

- Non-4K negative check: **passed.** Session 5 (1080p profile) emitted 113
  records, all `1920x1080` with `qualityState=native`; zero 4K requests.
- 4K organic downshift: **not fired.** Session 4 (3840x2160, 564 samples)
  never met the trigger (receiver gap increase AND encodeOutputFps < 54):
  loss events existed (gaps 0->26, paired IDR episodes 0->38, recovery
  keyframes 0->39, suppressed recovery requests 0->10) but encoder FPS was
  57-60 at every loss sample, and `qualityState` stayed `native` throughout.
- Upshift: not exercised — no downshift ever produced a fallback stream.
- Same-window rebind / PID invariance: not observed — no rebind occurred.
- No reason-5 finish/reopen loop appeared (all restarts were manual).

Two structural findings explain the absence, both measured and code-traced:

1. **The observer and the stream cannot stay awake together.** The adaptive
   loop lives in viewer JS (`use-stream-controller.ts` 1s react-query
   refetch). While native `StreamActivity` is foreground, React Native timers
   suspend — measured zero `:7777` control traffic across 12 lsof samples and
   nettop connection samples. Conversely, bringing MainActivity forward
   detaches the stream Surface immediately (logcat 02:58:36.883), the native
   renderer's LCF1 feedback stops, and the host health check kills the session
   after 5s of feedback silence
   (`CaptureSession+Lifecycle.swift` receiver-health check): measured focus
   at 02:58:41.6, session gone 02:58:46-47 (~5.2s). The earlier session 2
   congestion episode (02:11:26-31, two+ consecutive trigger windows) went
   unobserved for the same reason.
2. **Raw-RPC reconfigure races the viewer's auto-restore.** `reconfigureStream`
   stops the old capture first, which notifies reason=3; the native layer
   finishes the window without telling JS (`StreamActivity.kt` reasons 1-3 are
   local), the JS hostStatus poll then observes a stats-less session as
   unhealthy and its auto-restore issues `stopStream`, deleting the session
   from the live map — so the final reconfigure lookup fails
   (`"session N ended during reconfigure"`, reproduced 02:25:31, session 3).
   The designed JS path (`reconfigurePreparedStream`) has no claim that closes
   this window either.

The adaptive policy itself (Swift state machine, tests) behaved as designed
wherever it ran; the gap is that nothing awake owns the observation loop for a
live stream.

## Direction correction: host-driven transitions

The original design assigned the observation loop to
`use-stream-controller.ts`. The physical results above supersede that
assignment: a JS-timer observer is dormant exactly when the stream is alive,
and forcing it awake kills the stream. Future work should move the trigger to
the layer that already has every congestion signal while the session lives:

- The capture shim (which already hosts `AdaptiveResolutionPolicy` and the
  receiver-loss telemetry) publishes a resolution-transition request over the
  existing authenticated control channel instead of, or ahead of, any JS
  polling.
- Host `control.rs` gains a host-driven reconfigure mode that suppresses the
  reason=3 viewer-facing stop while swapping capture targets, keeping the
  session in the live map throughout.
- Native `StreamActivity`/android-viewer consumes the transition like a
  reason-4/5-style bounded rebind (same Surface, generation-gated publish),
  reporting success/failure so a failed upshift keeps the fallback target.
- Viewer JS keeps only status display (`qualityState`), removing the hot-path
  polling dependency.

This keeps the design's thresholds, cooldowns, and failure semantics
unchanged; only the ownership of observation and initiation moves. The
revision remains out of scope until a follow-up change implements it; the
implemented JS-loop hunks stay as-is in the working tree.
