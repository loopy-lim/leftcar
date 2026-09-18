use super::*;

pub(super) fn feed_and_render(
    dec: &mut viewer_decoder::AndroidDecoder,
    frame: &FramePacket,
    aus: &mut u64,
    fps: u32,
    stats: &mut RendererStats,
    control: &RendererControl,
) -> FeedOutcome {
    *aus += 1;
    let frame_us = 1_000_000u64 / u64::from(fps.max(1));
    let pts_us = aus.saturating_mul(frame_us) as i64;
    let started = std::time::Instant::now();
    let rendered_before = dec.frames_rendered;
    // Do not wait for a hardware codec slot. A missed AU is cheaper than
    // turning a transient decoder backlog into visible interaction latency.
    dec.set_output_target(control.presentation.lock().unwrap().target_now());
    let mut result = dec.feed_au_status(&frame.au, pts_us, DECODER_FEED_TIMEOUT_US);
    if matches!(result, Ok(viewer_decoder::FeedStatus::InputUnavailable)) {
        // Bursty arrivals can exhaust every hardware input slot for a few
        // milliseconds while the codec retires outputs. Treating that as a
        // broken reference chain (flush + epoch reset + IDR request) fed the
        // recovery storm — each cycle's burst manufactured the next slot
        // exhaustion (~1.1 requests/s measured on the XR while the decoder
        // itself released a steady 56fps). One short bounded wait absorbs
        // the transient; a genuinely saturated codec still reports
        // InputUnavailable and takes the established recovery path.
        result = dec.feed_au_status(&frame.au, pts_us, 3_000);
    }
    let feed_us = started.elapsed().as_micros() as u64;
    stats.max_feed_us = stats.max_feed_us.max(feed_us);
    control.last_feed_us.store(feed_us, Ordering::Relaxed);

    let queued = match result {
        Ok(viewer_decoder::FeedStatus::Queued { .. }) => {
            stats.queued += 1;
            FeedOutcome::Queued
        }
        Ok(viewer_decoder::FeedStatus::InputUnavailable) => {
            stats.input_drops += 1;
            stats.pressure.record_decoder_input_drop();
            FeedOutcome::ResyncRequired
        }
        Ok(viewer_decoder::FeedStatus::InputTooLarge { required, capacity }) => {
            stats.input_drops += 1;
            stats.pressure.record_decoder_input_drop();
            log_info!(
                "decoder input AU too large: required={} capacity={}; resyncing",
                required,
                capacity
            );
            FeedOutcome::FatalError
        }
        Err(e) => {
            log_info!("decoder feed failed: {}", e);
            FeedOutcome::FatalError
        }
    };

    stats
        .pressure
        .record_decoder_output_discards(dec.frames_discarded);

    let capture_age_ms = clock_corrected_age_ms(frame.capture_wall_ms, stats.host_clock_offset_ms);
    let encode_age_ms = clock_corrected_age_ms(frame.encode_wall_ms, stats.host_clock_offset_ms);
    let wire_age_ms = clock_corrected_age_ms(frame.send_wall_ms, stats.host_clock_offset_ms);
    if let Some(age) = capture_age_ms {
        store_smoothed_latency(&control.capture_to_decoder_ms, age);
    }
    if let Some(age) = encode_age_ms {
        store_smoothed_latency(&control.encode_to_decoder_ms, age);
    }
    if let Some(age) = wire_age_ms {
        store_smoothed_latency(&control.wire_to_decoder_ms, age);
    }
    record_decoder_output(
        dec,
        rendered_before,
        stats,
        control,
        (queued == FeedOutcome::Queued).then_some((pts_us, frame.capture_wall_ms)),
        [capture_age_ms, encode_age_ms, wire_age_ms],
        feed_us,
    );
    queued
}

/// Poll ready MediaCodec output on every receive-loop tick, including socket
/// timeouts. Waiting for the next compressed frame adds a whole frame of delay
/// and lets ordinary network jitter turn two ready outputs into a burst drop.
pub(super) fn drain_pending_output(
    dec: &mut viewer_decoder::AndroidDecoder,
    stats: &mut RendererStats,
    control: &RendererControl,
) -> Result<(), viewer_decoder::DecoderError> {
    let before = dec.frames_rendered;
    dec.set_output_target(control.presentation.lock().unwrap().target_now());
    dec.pump_latest_output(0)?;
    record_decoder_output(dec, before, stats, control, None, [None; 3], 0);
    Ok(())
}

fn record_decoder_output(
    dec: &mut viewer_decoder::AndroidDecoder,
    rendered_before: u64,
    stats: &mut RendererStats,
    control: &RendererControl,
    queued_input: Option<(i64, Option<u64>)>,
    stage_ages: [Option<u64>; 3],
    feed_us: u64,
) {
    stats
        .pressure
        .record_decoder_output_discards(dec.frames_discarded);
    let [capture_age_ms, encode_age_ms, wire_age_ms] = stage_ages;
    let rendered_delta = dec.frames_rendered.saturating_sub(rendered_before);
    let (pts, capture) = queued_input.unwrap_or((0, None));
    let (output_capture_wall, decoder_epoch) = {
        let mut metadata = control.output_metadata.lock().unwrap();
        let capture = metadata.observe(
            pts,
            capture,
            queued_input.is_some(),
            dec.last_released_pts_us,
            rendered_delta,
        );
        (capture, metadata.epoch())
    };
    let release_capture_age_ms =
        clock_corrected_age_ms(output_capture_wall, stats.host_clock_offset_ms);
    if rendered_delta > 0 {
        if let Some(age) = release_capture_age_ms {
            store_smoothed_latency(&control.capture_to_surface_release_ms, age);
        } else {
            control
                .capture_to_surface_release_ms
                .store(LATENCY_UNKNOWN, Ordering::Relaxed);
        }
    }

    if rendered_delta > 0 && dec.frames_rendered / 30 > rendered_before / 30 {
        // The render thread must never sit inside a logd write: this block
        // runs every 30 frames (=0.5s at 60fps), and logcat contention here
        // shows up on the display as a visible ~half-second cadence hitch.
        // Format on this thread, emit on a throwaway thread.
        let head = format!(
            "LeftcarViewerPerf schema=2 process={} stream={} incarnation={} decoderEpoch={} kind=single released={} releaseCaptureAgeMs={:?} outputPtsUs={:?} outputStage=surface-release clockBasis=estimated-host-wall-offset",
            std::process::id(),
            control.port,
            control.metric_incarnation,
            decoder_epoch,
            control.rendered_frames.load(Ordering::Relaxed).saturating_add(rendered_delta),
            release_capture_age_ms,
            dec.last_released_pts_us
        );
        let input_rtt = control.input_rtt_ms.load(Ordering::Relaxed);
        let body = format!(
            "Rendered {} frames; outputDrops={} staleInputs={} staleInputDrops={} outputBurst={} fecRecovered={} unrecoveredFecGroups={} decoderInputsQueued={} decoderInputDrops={} completedBatch={} liveEdgeBatch={} maxCompletedBatch={} frameGaps={} intentionalLiveEdgeGaps={} recoverySkippedFrames={} nacksSent={} nacksHealed={} feedUs={} maxFeedUs={} captureAgeMs={:?} encodeAgeMs={:?} wireAgeMs={:?} inputRttMs={:?}",
            dec.frames_rendered,
            dec.frames_discarded,
            stats.stale_inputs,
            control.stale_input_drops.load(Ordering::Relaxed),
            stats
                .pressure
                .live_edge_discards
                .saturating_add(stats.pressure.decoder_output_discards),
            stats.pressure.fec_recovered_fragments,
            stats.pressure.unrecoverable_fec_groups,
            stats.queued,
            stats.input_drops,
            stats.completed_batch,
            stats.live_edge_batch,
            stats.max_completed_batch,
            stats.frame_gaps,
            stats.intentional_live_edge_gaps,
            stats.recovery_skipped_frames,
            stats.nacks_sent,
            stats.nacks_healed,
            feed_us,
            stats.max_feed_us,
            capture_age_ms,
            encode_age_ms,
            wire_age_ms,
            (input_rtt != LATENCY_UNKNOWN).then_some(input_rtt)
        );
        let _ = std::thread::Builder::new()
            .name("lc-perflog".into())
            .spawn(move || {
                log_info!("{}", head);
                log_info!("{}", body);
            });
    }
    control
        .rendered_frames
        .fetch_add(rendered_delta, Ordering::Relaxed);
    control.stale_outputs.store(
        dec.frames_discarded
            .saturating_add(stats.stale_inputs)
            .saturating_add(stats.pressure.live_edge_discards),
        Ordering::Relaxed,
    );
    // `stale_inputs` records late-but-valid frames that were still submitted
    // to MediaCodec. They are not loss and must not feed the Host's recovery
    // or bitrate controller as input drops. Actual decoder input misses are
    // tracked separately below.
    control.stale_input_drops.store(0, Ordering::Relaxed);
    control.output_burst_discards.store(
        stats
            .pressure
            .live_edge_discards
            .saturating_add(dec.frames_discarded),
        Ordering::Relaxed,
    );
    control
        .decoder_input_drops
        .store(stats.input_drops, Ordering::Relaxed);
    control
        .frame_gaps
        .store(stats.frame_gaps, Ordering::Relaxed);
}
