use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn present_completed_frames(
    completed_frames: [Option<(std::net::SocketAddr, FramePacket)>; MEDIA_BATCH_SIZE],
    completed_count: usize,
    control_socket: &std::net::UdpSocket,
    viewer_control_token: &[u8],
    fps: u32,
    control: &RendererControl,
    codec_config: &mut Option<viewer_decoder::CodecConfig>,
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    aus: &mut u64,
    completed_access_units: &mut u64,
    renderer_stats: &mut RendererStats,
    last_frame_id: &mut Option<u16>,
    awaiting_keyframe: &mut bool,
    recovery_gate: &mut RecoveryRequestGate,
) {
    let completed_frames = completed_frames
        .into_iter()
        .take(completed_count)
        .flatten()
        .collect::<Vec<_>>();
    let completed_batch = completed_frames.len();
    *completed_access_units = completed_access_units.saturating_add(completed_batch as u64);
    let selection = select_live_edge_frames(completed_frames);
    renderer_stats.completed_batch = completed_batch;
    renderer_stats.live_edge_batch = selection.frames.len();
    renderer_stats.max_completed_batch = renderer_stats.max_completed_batch.max(completed_batch);
    renderer_stats
        .pressure
        .record_live_edge_discards(selection.discarded);
    let mut intentional_live_edge_discard = selection.discarded > 0;

    for (peer, frame) in selection.frames {
        let frame_was_intentionally_discarded =
            std::mem::replace(&mut intentional_live_edge_discard, false);
        let codec = codec_config
            .as_ref()
            .map(|config| config.codec)
            .unwrap_or(viewer_decoder::VideoCodec::H264);
        let keyframe = crate::media_datagram::is_keyframe(&frame.au, codec);
        let capture_age_ms =
            clock_corrected_age_ms(frame.capture_wall_ms, renderer_stats.host_clock_offset_ms);
        let rtt_ms = control.network_rtt_ms.load(Ordering::Relaxed);
        let stale_budget_ms = stale_frame_budget_ms((rtt_ms != LATENCY_UNKNOWN).then_some(rtt_ms));
        // A single tail-latency delta should render late instead of
        // invalidating the whole reference chain. Enter recovery only
        // after three consecutive over-budget deltas.
        let over_budget = !keyframe && capture_age_ms.is_some_and(|age| age > stale_budget_ms);
        let (stale_streak, should_resync) =
            stale_streak_advance(renderer_stats.consecutive_stale, keyframe, over_budget);
        renderer_stats.consecutive_stale = stale_streak;
        if over_budget {
            renderer_stats.stale_inputs = renderer_stats.stale_inputs.saturating_add(1);
        }
        if should_resync {
            // The next independently decodable keyframe starts a new
            // frame-id observation epoch. Counting its jump from the
            // discarded stale frame would be another false loss.
            *last_frame_id = None;
            resync_decoder_after_frame_gap(decoder, awaiting_keyframe);
            request_idr_debounced(
                control_socket,
                peer,
                viewer_control_token,
                recovery_gate,
                control,
            );
            continue;
        }

        {
            let gap_reason = classify_frame_gap(
                *last_frame_id,
                frame.id,
                frame_was_intentionally_discarded,
                *awaiting_keyframe,
            );
            *last_frame_id = Some(frame.id);

            if keyframe {
                log_info!(
                    "Received IDR access unit id={} gapReason={:?} awaitingKeyframe={}",
                    frame.id,
                    gap_reason,
                    *awaiting_keyframe
                );
            }

            match gap_reason {
                FrameGapReason::None => {}
                FrameGapReason::NetworkLoss { missing } => {
                    renderer_stats.frame_gaps += 1;
                    control
                        .frame_gaps
                        .store(renderer_stats.frame_gaps, Ordering::Relaxed);
                    let hard_recovery = should_resync_after_network_loss(missing, keyframe);
                    log_info!(
                                "UDP access-unit gap detected at id={} reason=networkLoss missing={} hardRecovery={}",
                                frame.id,
                                missing,
                                hard_recovery
                            );
                    if keyframe {
                        *awaiting_keyframe = false;
                    } else if hard_recovery {
                        // Any missing delta invalidates the low-latency
                        // reference chain. Keep the last good Surface
                        // image while a fresh IDR is requested.
                        *last_frame_id = None;
                        resync_decoder_after_frame_gap(decoder, awaiting_keyframe);
                        request_idr_debounced(
                            control_socket,
                            peer,
                            viewer_control_token,
                            recovery_gate,
                            control,
                        );
                    }
                }
                FrameGapReason::LiveEdgeDiscard { missing } => {
                    renderer_stats.intentional_live_edge_gaps += 1;
                    log_info!(
                                "UDP access-unit gap detected at id={} reason=liveEdgeDiscard missing={} awaitingKeyframe={}",
                                frame.id,
                                missing,
                                *awaiting_keyframe
                            );
                    if keyframe {
                        *awaiting_keyframe = false;
                    } else if !*awaiting_keyframe {
                        // The selected delta may depend on one of the
                        // AUs intentionally discarded by the
                        // live-edge policy. Start one coalesced
                        // recovery boundary; the outer gate prevents
                        // another IDR request for this same episode.
                        *last_frame_id = None;
                        resync_decoder_after_frame_gap(decoder, awaiting_keyframe);
                        request_idr_debounced(
                            control_socket,
                            peer,
                            viewer_control_token,
                            recovery_gate,
                            control,
                        );
                    }
                }
                FrameGapReason::RecoverySkip { missing } => {
                    renderer_stats.recovery_skipped_frames += 1;
                    log_info!(
                                "UDP access-unit gap skipped at id={} reason=recoverySkip missing={} keyframe={}",
                                frame.id,
                                missing,
                                keyframe
                            );
                    if keyframe {
                        *awaiting_keyframe = false;
                    }
                }
            }

            if should_feed_frame(gap_reason, keyframe) {
                let outcome = if !*awaiting_keyframe || keyframe {
                    decoder.as_mut().map(|decoder| {
                        feed_and_render(decoder, &frame, aus, fps, renderer_stats, control)
                    })
                } else {
                    None
                };
                match outcome {
                    Some(FeedOutcome::Queued) => {
                        *awaiting_keyframe = false;
                        if keyframe {
                            recovery_gate.recovered();
                        }
                    }
                    Some(FeedOutcome::ResyncRequired) => {
                        *last_frame_id = None;
                        resync_decoder_after_frame_gap(decoder, awaiting_keyframe);
                        request_idr_debounced(
                            control_socket,
                            peer,
                            viewer_control_token,
                            recovery_gate,
                            control,
                        );
                    }
                    Some(FeedOutcome::FatalError) => {
                        reset_decoder(decoder, codec_config, awaiting_keyframe);
                        request_idr_debounced(
                            control_socket,
                            peer,
                            viewer_control_token,
                            recovery_gate,
                            control,
                        );
                    }
                    None => {}
                }
            }
        }
    }
}
