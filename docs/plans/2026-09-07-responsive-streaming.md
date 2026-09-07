# Response-first streaming implementation plan

> Execute through Loop/Z.AI workers with explicit ownership and dependency gates. User approved implementation and parallel work in this conversation.

> 2026-09-07 handoff: user requested wrapping up and recording all remaining work. See [current handoff](../2026-09-07-streaming-handoff.md). E1 remains incomplete; the final U3 follow-up is stopped and its initial changes are preserved. Do not treat pending checkboxes as work still running in the background.

## Goal and accepted design

Make responsiveness a common requirement; expose response-first (default) and clarity-first preferences. Keep logical desktop geometry separate from transport resolution. Interpret the requested 2K as 2560×1440; offer source-aspect-aware targets including 3840×2160 and 5120×1440 for 32:9. A 5120×1440 image contains fewer pixels than 3840×2160; width alone is not a load budget or a promise of hardware support. Never upscale a smaller source merely to label it 5K.

## Constraints

- Preserve all existing dirty changes and latency telemetry. No commit/push/PR without the existing separate commit confirmation.
- Z.AI primary with no automatic Codex fallback. Root owns physical-device operations; workers must not run ADB, restart Host, create virtual displays or change BetterDisplay.
- Do not edit docs/tablet-cursor-streaming-validation.md.
- React gate: root React Doctor 100/100, then typecheck and relevant tests. Do not suppress findings.
- Wire compatibility and valid codec reference chains are mandatory. Do not drop arbitrary dependent H.264 frames and call that low latency.
- No blanket bypass of exact-4K SplitVertical validation. Wide targets must use a supported path or return an explicit limitation.
- Existing 1080p→4K encoder-mode restoration must be considered in integration; keep Viewer and Host modes consistent.

## Evidence

Read docs/tablet-input-response-validation.md and docs/viewer-display-resize-fix-validation.md. Same-session recovery reached 2.895s under recording, and 0.928s without recording. Decode after IDR was about 9ms: do not regress the already fixed decoder configuration. The remaining recovery bottleneck is not conclusively assigned to network versus Host.

## Work breakdown

### H1 Host recovery transmission

Own native/macos-capture-shim only. Trace PLI→IDR generation→packet/FEC sending. Add deterministic loss/repeated-PLI tests before fixes. Bound recovery work without creating huge repeated IDR bursts or invalid timestamps. Preserve original capture times, strict submission PTS, carrier cleanup and existing protocol. Run affected Swift harnesses. Publish concrete behavior, tests and limits for integration.

### V1 Viewer recovery

Own native/android-viewer and crates/viewer-decoder only. Test repeated loss, unmatched IDR generations, recovery request cadence and quiet-stream recovery. Bound retry/coordination delays using existing PLI protocol without busy-looping or invalid reference-frame skipping. Preserve telemetry semantics and named-decoder fallback rules. Run native unit tests and arm64 build when appropriate. No physical device operations.

### P1 Policy and aspect-aware target model

Own pure viewer model files: stream-profile, stream-resolution, adaptive-resolution, viewer-preferences and their tests; new streaming-policy module/tests. Export StreamingPriority = 'responsive' | 'clarity', persisted streamingPriority default responsive and migrate older preferences without dropping showFps/localCursor/profile intent. Preserve legacy profile identifiers for compatibility. Provide source-aspect-aware resolution and fallback selection: 16:9 2560×1440 baseline, 3840×2160 clarity, 32:9 5120×1440 where source supports it, portrait equivalents, even codec dimensions, bounded pixels, no unintended upscaling. Recovery pressure must not indefinitely suppress all downshifting; distinguish active rebind from recovery observation. Tests must exercise loss/queue pressure, cooldown/hysteresis, old preference migration and wide/portrait targets. Publish exact exports before U1.

### C1 Host control compatibility (after a native worker releases its slot)

Own apps/host-desktop, crates/control-contract and affected Rust contract consumers. Add capability-backed encoder reconfiguration with optional catalog `reconfigureEncoderExperiment`, request `encoderExperiment` and actual accepted response `encoderExperiment`. Validate before stopping the previous backend; preserve rollback. Omitted requests retain legacy behavior. Test bidirectional transitions and invalid inputs. Keep exact-4K SplitVertical restriction.

### U1 Viewer integration (depends on P1; C1 contract agreed before parallel implementation)

Own apps/viewer-expo after P1 returns ownership. Wire two simple Korean priority controls and explicit dimensions/auto choice into saved settings, launches and adaptive controller. Preserve logical source size when changing stream target. Use C1 capability-backed encoder routing on size changes; never prepare split Viewer against single Host. Validate 5120×1440 argument acceptance without claiming hardware acceptance. Add regression tests, run React Doctor 100, typecheck and complete viewer tests. E1 waits for H1, V1, C1 and U1 final results.

### U3 Android surface lifecycle integration (found during E1)

Own StreamActivity, StreamLauncherModule and the required Kotlin lifecycle helpers/tests. Reproduce the same-session 1440p single-to-4K SplitVertical transition: Host accepts the new mode, but the old one-Surface Activity never attaches a split renderer and feedback times out. Rebuild the Surface layout when the mode changes, reconnect valid retained split surfaces, and keep recovery mode-aware. Ignore callbacks from retired holders so they cannot detach a replacement renderer. Split detach stops the receiver rather than suspending it; a retry must prepare both listeners again before attaching. Root owns APK build/install and the live round-trip verification. Independent JNI review identifies receiver consumption on a failed attach as a retry precondition, not a claim that every failure has been reproduced.

### E1 Root integration and physical verification

Validation matrix: response-first and clarity-first each use the same source and animation fixture; preserve separate recorded-input and unrecorded-stream runs. Report command-start-to-recorded-response separately from physical touch-to-photon (not measured). Compare gap-to-IDR wait, IDR-to-output, rendered FPS and the number/duration of freezes; a higher average FPS cannot waive long freezes. Snapshot the accepted stream dimensions and actual encoder mode for each run. Verify both 1440p→4K and 4K→1440p transitions, including rollback/older-Host compatibility in tests. The current catalog exposes only 3840×2160, so 5120×1440 remains a calculation/contract gate until a real wide source exists; do not create or alter virtual displays for this gate.

- [ ] Review every worker result against code and existing changes.
- [ ] Build/install serially, verify artifact provenance.
- [ ] Repeat touch/scroll animation checks and no-record live stream measurements in responsive and clarity modes.
- [ ] Verify 1440p and 4K size round trips. Wide geometry is a contract/test gate until a real wide source is available.
- [ ] Record achieved results, residual stalls and device limitations truthfully; physical mouse remains unavailable.
- [ ] Present updated focused commit plan after verification; do not push.
