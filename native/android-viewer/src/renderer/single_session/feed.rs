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
    let result = dec.feed_au_status(&frame.au, pts_us, DECODER_FEED_TIMEOUT_US);
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
    // This input produced the output released to the Surface, so record the
    // capture-to-release approximation. Surface/compositor handoff is not the
    // panel presentation instant and must not be reported as glass-to-glass.
    if dec.frames_rendered > rendered_before {
        if let Some(age) = capture_age_ms {
            store_smoothed_latency(&control.capture_to_surface_release_ms, age);
        }
    }

    if dec.frames_rendered > 0 && dec.frames_rendered.is_multiple_of(30) {
        log_info!(
            "Rendered {} frames; outputDrops={} staleInputs={} staleInputDrops={} outputBurst={} fecRecovered={} decoderInputsQueued={} decoderInputDrops={} completedBatch={} liveEdgeBatch={} maxCompletedBatch={} frameGaps={} intentionalLiveEdgeGaps={} recoverySkippedFrames={} feedUs={} maxFeedUs={} captureAgeMs={:?} encodeAgeMs={:?} wireAgeMs={:?}",
            dec.frames_rendered,
            dec.frames_discarded,
            stats.stale_inputs,
            control.stale_input_drops.load(Ordering::Relaxed),
            stats
                .pressure
                .live_edge_discards
                .saturating_add(stats.pressure.decoder_output_discards),
            stats.pressure.fec_recovered_fragments,
            stats.queued,
            stats.input_drops,
            stats.completed_batch,
            stats.live_edge_batch,
            stats.max_completed_batch,
            stats.frame_gaps,
            stats.intentional_live_edge_gaps,
            stats.recovery_skipped_frames,
            feed_us,
            stats.max_feed_us,
            capture_age_ms,
            encode_age_ms,
            wire_age_ms
        );
    }
    let rendered_delta = dec.frames_rendered.saturating_sub(rendered_before);
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
    queued
}
