# Single-encoder default and refresh watchdog design

- Date: 2026-08-30
- Status: approved in chat
- Scope: macOS single-encoder policy, Host and Android stall recovery, and a
  persistent Android display-FPS overlay

## Goal

Leftcar must use one hardware encoder session as the product default on every
Mac, including M1 Max. The dual vertical split path remains available only for
explicit developer diagnostics. This removes dual-session resource contention
from normal use while preserving exact 3840x2160 output and the existing
low-latency latest-frame policy.

The stream must also recover from an encoder callback stall, a decoder/Surface
stall, or a silently unreachable Host without leaving the last frame frozen
indefinitely. Recovery begins after 250ms when there is direct evidence that a
pipeline stage should have progressed. Host reachability uses a separate
multi-probe timeout so ordinary packet jitter cannot close a healthy stream.

Android always shows a very small actual display-FPS value in the bottom-right
corner. The value is derived from frames released to the Surface, not requested
source FPS and not photon-level glass-to-glass measurement.

## Product encoder policy

`auto`, `rateControl`, `adaptiveQp`, and `encoderPool` remain valid experiments,
but each uses exactly one `VTCompressionSession`. `auto` continues to resolve to
the verified single RTVC H.264 rate-control path. Selecting an exact 4K profile
never silently lowers its resolution.

`splitVertical` remains in the wire enum for backward compatibility, but the
Host does not advertise it to normal Viewers and the Viewer does not show it in
the product selector. An explicit split request is accepted only when a
developer diagnostic environment flag is enabled and all existing split
requirements pass. This keeps the dual implementation and its tests available
without allowing model names or a shallow two-session allocation probe to
change production behavior.

The diagnostic flag is exactly `LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC=1`. It never
changes the advertised product capability; it only permits an explicit
developer request that would otherwise fail as unavailable.

The policy is intentionally identical on base M1 and M1 Max. Actual encoded FPS
is reported as measured. This stability change does not claim that a single
encoder reaches 4K60 on either machine.

## Host encoder stall recovery

### Progress evidence

The single encoder records these monotonic values on the existing encode queue:

- latest capture callback time;
- latest valid encoder output callback time;
- every submitted encode slot's generation, submission time, and
  `EncodeSlotCompletionToken`;
- current encoder session generation and restart history.

A pure `SingleEncoderHealth` decision function receives those values and the
current monotonic time. It reports a stall only when capture is still advancing,
at least one encode slot is in flight, no valid output has advanced, and the
oldest uncompleted slot is at least 250ms old. Static content and an idle stream
without an outstanding slot do not trigger recovery.

### Recovery action

The first stall decision for a generation performs one ordered recovery on the
encode queue:

1. claim every outstanding completion token through a watchdog disposition;
2. increment the encoder generation so late callbacks cannot mutate the new
   session;
3. invalidate the stalled `VTCompressionSession` and clear generation-owned PTS
   maps and input resources;
4. release the claimed in-flight slots exactly once;
5. mark the newest pending capture as the next submission and force it to be a
   keyframe;
6. recreate the existing single encoder policy and resume latest-frame
   submission.

The callback path and watchdog path use the same completion token, preventing a
late callback from decrementing an already reclaimed slot. Old-generation
callbacks may record a diagnostic count but cannot packetize output or schedule
another recovery.

One generation may recover only once. Restarts have a one-second cooldown. A
third stall within ten seconds terminates the capture session with an explicit
`encoder_stalled` error instead of looping forever. Normal encoder drops that
produce callbacks remain on the existing drop/recovery path and do not count as
missing-callback stalls.

## Android receiver and Surface recovery

The single-session renderer tracks three independent progress clocks:

- last authenticated `LCP2` control response;
- last completed media access unit;
- last frame released to the Surface.

These clocks have different meanings and must not be collapsed into one media
timeout.

When completed access units continue but no Surface release occurs for 250ms,
the renderer issues one debounced IDR request. If access units continue and no
Surface release occurs for a total of 750ms, it recreates the decoder once and
requests another IDR. If rendering still does not resume within three seconds,
the renderer records a local `render_stalled` termination reason and enters the
existing bounded reconnect flow.

Media silence alone does not reset the decoder because a future dirty-region
capture path may intentionally suppress unchanged frames. When the control
peer is known, the Viewer sends the existing authenticated latency probe once
per second. Three consecutive missing valid responses record a local
`host_unreachable` reason, stop the native renderer, and dismiss the stale
Surface. The Activity then uses the existing reconnect arguments; it does not
wait indefinitely for an authenticated Host termination packet that can no
longer arrive.

IDR requests, decoder recreation, and session reconnect are separately gated so
one incident cannot create an IDR storm. Successful Surface release resets the
render recovery state immediately.

## Persistent FPS overlay

The Android stream window owns a `PersistentFpsOverlay` alongside the existing
temporary diagnostic HUD.

- Position: bottom-right, inside system-bar and gesture insets.
- Text: `-- FPS` until measurable, then rounded actual values such as `37 FPS`.
- Source: delta of native `rendered_frames` sampled by the existing 250ms HUD
  poll; use the current bounded exponential smoothing.
- Appearance: 9sp monospace text, low-opacity white foreground, a minimal
  translucent dark rounded background, and no shadow or animation.
- Behavior: always visible while the stream Activity is visible; it never fades
  with the detailed debug HUD and never intercepts touch input.
- Accessibility: update the content description with the current actual
  display rate.

The label means Surface-release FPS. Documentation and UI must not call it
source FPS, panel FPS, or glass-to-glass FPS.

## Compatibility and failure behavior

Old control clients may still send `splitVertical`. Production Hosts reject it
as unavailable unless the diagnostic flag is present; they do not silently
substitute another encoder experiment for an explicit request. Omitted or stale
selections continue to resolve to `auto`.

Local Viewer termination reasons are retained by logical instance exactly like
authenticated Host termination reasons so the 250ms Activity poll cannot miss
them between native cleanup and UI handling. A reconnect rebuilds the decoder
and Surface and requests a fresh IDR. It never displays an old Surface as if the
connection were healthy.

Reason code `4` means `host_unreachable` and reason code `5` means
`render_stalled`; existing authenticated Host reason codes `1` through `3`
remain unchanged. `StreamLauncherModule` emits one `leftcarStreamTerminated`
event containing the port and reason. `useStreamController` maps the port to the
active stream and invokes the existing single-flight `restoreStream` mutation
once. A successful restore reopens the document task with `reconnect=true`; a
failed restore leaves the stale Surface closed and exposes the existing
reconnection error instead of retrying indefinitely.

## Verification

Automated verification must include:

1. Swift decision tests at 249ms and 250ms, capture-not-advancing, no-in-flight,
   successful-output, cooldown, restart budget, and late-callback cases.
2. Swift integration tests proving every encode slot completes exactly once and
   the first post-restart output is a valid keyframe from the new generation.
3. Rust tests proving media silence with a live control peer is not a render
   stall, completed-AU/no-Surface progress requests one IDR at 250ms, decoder
   recreation occurs at 750ms, and three missed probes terminate locally.
4. Kotlin tests for local termination mapping and a persistent bottom-right FPS
   label that uses actual rendered-frame deltas.
5. TypeScript tests proving one native termination event starts at most one
   restore for the matching port and a failed restore does not loop.
6. Existing TypeScript, Rust, Swift, architecture, and Android build gates.
   React Doctor `100 / 100` is required if any React, React Native, TSX, style,
   or component behavior is changed.

Physical verification on the connected Viewer uses exact 3840x2160 single
RTVC H.264 and the moving wallpaper reached with AeroSpace workspace `0`:

- run moving content for at least 180 seconds;
- confirm the Host advertises no product `splitVertical` capability and reports
  one encoder mode/session;
- confirm no lasting frozen frame, no repeated recovery loop, and no stale
  Surface after a controlled Host stop;
- compare the persistent FPS label with native rendered-frame deltas over the
  same interval;
- record capture, encoded output, Android Surface-release FPS, 250ms watchdog
  activations, recovery outcomes, loss, and queue age without presenting the
  result as 4K60 proof.

A base-M1 run is a separate physical proof. Until that machine is tested, the
accepted claim is that base M1 receives the same single-session policy and never
enters the production dual path, not that it sustains 4K60.

## Out of scope

- Removing the diagnostic split implementation or its historical tests.
- Automatically reducing a selected 4K resolution.
- Adding a third ROI encoder.
- Claiming photon-level glass-to-glass latency from software timestamps.
- Treating this refresh-recovery work as proof that single-session 4K60 has
  already been achieved.
