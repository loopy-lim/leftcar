use super::super::dispatch::{decide_idr_send, IdrRequestOutcome, IdrSendDecision};
use super::super::presentation_sync::{ReadyFrame, TileSide};
use super::super::split_gap_policy::{
    decide_split_frame_gap, decide_split_input_pressure, split_receive_batch_limit, SplitGapSignal,
};
use super::super::split_latency::TileLatencyTelemetry;
use super::super::stats::{split_feedback_body, SplitFeedbackSnapshot};
use super::{CoordinatorEvent, RuntimeStats, TileCommand};
use crate::input_protocol::{
    encode_input, parse_ack, parse_input_status, parse_termination, TerminationReason,
};
use crate::jni::RendererControl;
use crate::log_info;
use crate::media_datagram::{
    discard_fec_groups, parse_fragment, parse_parity, CompletedFecGroups, CompletedFrameSequencer,
    FecGroup, FrameFragment, FrameReassembler, ReassembledFrame, RestoredFragment, PARITY_MARKER,
};
use crate::net_guard::peer_allowed;
use crate::prepared_udp::PreparedUdpReceiver;
use crate::socket_tuning::{configure_split_media_socket, split_media_receive_buffer_bytes};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

extern "C" {
    fn ANativeWindow_release(window: *mut c_void);
}

const MEDIA_BUFFER_BYTES: usize = 2_048;
const COMMAND_IDR: &[u8] = b"IDR";

pub(super) struct TileWorkerLaunch {
    pub(super) side: TileSide,
    pub(super) expected_host: String,
    pub(super) fps: u32,
    pub(super) window: usize,
    pub(super) prepared: PreparedUdpReceiver,
    pub(super) decoder_name: String,
    pub(super) events: mpsc::Sender<CoordinatorEvent>,
    pub(super) commands: mpsc::Receiver<TileCommand>,
    pub(super) stats: Arc<RuntimeStats>,
    pub(super) control: Arc<RendererControl>,
    pub(super) release_window_on_exit: bool,
}

pub(super) fn spawn_tile_worker(launch: TileWorkerLaunch) -> std::thread::JoinHandle<()> {
    let name = match launch.side {
        TileSide::Left => "leftcar-split-left",
        TileSide::Right => "leftcar-split-right",
    };
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || tile_worker(launch))
        .expect("split tile worker")
}

fn tile_worker(launch: TileWorkerLaunch) {
    let TileWorkerLaunch {
        side,
        expected_host,
        fps,
        window,
        prepared,
        decoder_name,
        events,
        commands,
        stats,
        control,
        release_window_on_exit,
    } = launch;
    let (socket, mut token, mut peer) = match prepared.into_socket_and_token() {
        Ok(value) => value,
        Err(_) => {
            let _ = events.send(CoordinatorEvent::Fatal);
            if release_window_on_exit {
                release_window(window);
            }
            return;
        }
    };
    match configure_split_media_socket(&socket) {
        Ok(actual) => log_info!(
            "split {:?} media SO_RCVBUF requested={} actual={}",
            side,
            split_media_receive_buffer_bytes(),
            actual
        ),
        Err(error) => log_info!("split {:?} media SO_RCVBUF tuning failed: {}", side, error),
    }
    if socket.set_nonblocking(true).is_err() {
        let _ = events.send(CoordinatorEvent::Fatal);
        if release_window_on_exit {
            release_window(window);
        }
        return;
    }
    let mut buffer = [0u8; MEDIA_BUFFER_BYTES];
    let mut reassembler = FrameReassembler::default();
    let mut sequencer = CompletedFrameSequencer::default();
    let mut fec_groups: HashMap<(u16, u16), FecGroup> = HashMap::new();
    let mut fec_group_order = VecDeque::new();
    let mut completed_fec_groups = CompletedFecGroups::default();
    let mut config: Option<viewer_decoder::CodecConfig> = None;
    let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
    let mut awaiting_keyframe = true;
    let mut last_id: Option<u16> = None;
    let mut expanded_sequence = 0u64;
    let mut last_raw_sequence: Option<u16> = None;
    let mut frame_gaps = 0u32;
    let mut input_drops = 0u32;
    let mut input_pressure_started_ns: Option<u64> = None;
    let mut rendered = 0u64;
    let mut last_feedback_rendered = 0u64;
    let mut last_feedback_joined = 0u64;
    let mut last_feedback = Instant::now();
    // Local monotonic latency telemetry (receive -> feed -> ready -> release
    // plus gap -> IDR -> first output). All CLOCK_MONOTONIC: the split path
    // has no host clock offset, so host wall-clock timestamps stay unused.
    let mut telemetry = TileLatencyTelemetry::default();
    let fec_stats = stats.fec(side);

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                TileCommand::PresentAt(output, target_ns, generation) => {
                    if let Some(decoder) = decoder.as_mut() {
                        if let Err(error) = decoder.release_output_at(output, target_ns) {
                            log_info!(
                                "split {:?} timed output release failed at pts={}: {}",
                                side,
                                output.pts_us,
                                error
                            );
                            telemetry.note_discarded(output.pts_us);
                            let _ = events.send(CoordinatorEvent::Presented {
                                side,
                                pts_us: output.pts_us,
                                generation,
                                succeeded: false,
                            });
                            let _ = events.send(CoordinatorEvent::Fatal);
                        } else {
                            rendered = rendered.saturating_add(1);
                            // Release submitted to MediaCodec: closes the local
                            // ready -> release (pair wait + dispatch) and
                            // receive -> release segments for this pts.
                            telemetry.note_released(output.pts_us, monotonic_ns());
                            match side {
                                TileSide::Left => {
                                    stats.left_rendered.store(rendered, Ordering::Relaxed)
                                }
                                TileSide::Right => {
                                    stats.right_rendered.store(rendered, Ordering::Relaxed)
                                }
                            }
                            let _ = events.send(CoordinatorEvent::Presented {
                                side,
                                pts_us: output.pts_us,
                                generation,
                                succeeded: true,
                            });
                        }
                    }
                }
                TileCommand::Discard(output) => {
                    if let Some(decoder) = decoder.as_mut() {
                        if let Err(error) = decoder.discard_output(output) {
                            log_info!(
                                "split {:?} output discard failed at pts={}: {}",
                                side,
                                output.pts_us,
                                error
                            );
                        }
                    }
                    telemetry.note_discarded(output.pts_us);
                }
                TileCommand::EnterRecovery => {
                    log_info!("split {:?} decoder entering recovery", side);
                    if telemetry.reset_for_recovery() {
                        log_info!(
                            "split {:?} gap recovery aborted by decoder flush without resumed output",
                            side
                        );
                    }
                    awaiting_keyframe = true;
                    if let Some(decoder) = decoder.as_mut() {
                        let _ = decoder.flush();
                    }
                    reassembler.clear();
                    sequencer.clear();
                    discard_fec_groups(&mut fec_groups, &mut fec_group_order);
                    completed_fec_groups.clear();
                    last_id = None;
                    input_pressure_started_ns = None;
                }
                TileCommand::RequestIdr { episode, request } => {
                    // V3 dispatch: episode AND unique request-id ownership are
                    // re-checked HERE, right before any wire access, so a
                    // request queued before its pair resumed — or an old queued
                    // wire copy of a superseded same-episode command — is
                    // cancelled instead of becoming a late pending PLI (the
                    // Host only coalesces PLIs while its recovery boundary is
                    // pending; a late one would start a new Host generation).
                    // An unready path is explicitly reported unsent to the
                    // coordinator for retention — logging-and-dropping is not
                    // allowed. A UDP send is reported as a transmit attempt,
                    // never as delivery.
                    let current_episode = stats.recovery_episode.load(Ordering::Relaxed);
                    let current_request = stats.recovery_request.load(Ordering::Relaxed);
                    match decide_idr_send(
                        current_episode,
                        episode,
                        current_request,
                        request,
                        peer.is_some(),
                        !token.is_empty(),
                    ) {
                        IdrSendDecision::CancelStale => {
                            log_info!(
                                "split {:?} paired IDR request cancelled before wire: episode={} stale (current={}) or request={} superseded (current={})",
                                side,
                                episode,
                                current_episode,
                                request,
                                current_request
                            );
                            let _ = events.send(CoordinatorEvent::IdrRequestOutcome {
                                side,
                                episode,
                                request,
                                outcome: IdrRequestOutcome::CancelledStale,
                            });
                        }
                        IdrSendDecision::ReportUnsentNoPeer => {
                            log_info!(
                                "split {:?} paired IDR request NOT sent: peer not learned yet (episode={}, request={}); reported for retention",
                                side,
                                episode,
                                request
                            );
                            let _ = events.send(CoordinatorEvent::IdrRequestOutcome {
                                side,
                                episode,
                                request,
                                outcome: IdrRequestOutcome::UnsentNoPeer,
                            });
                        }
                        IdrSendDecision::ReportUnsentNoToken => {
                            log_info!(
                                "split {:?} paired IDR request NOT sent to {}: no session token yet (episode={}, request={})",
                                side,
                                peer.map(|peer| peer.to_string())
                                    .unwrap_or_else(|| "unknown peer".into()),
                                episode,
                                request
                            );
                            let _ = events.send(CoordinatorEvent::IdrRequestOutcome {
                                side,
                                episode,
                                request,
                                outcome: IdrRequestOutcome::UnsentNoToken,
                            });
                        }
                        IdrSendDecision::Transmit => {
                            let Some(peer) = peer else {
                                continue;
                            };
                            stats.idr_transmit_attempts.fetch_add(1, Ordering::Relaxed);
                            log_info!(
                                "split {:?} paired IDR request transmit attempt to {} (episode={}, request={}; send accepted != delivered)",
                                side,
                                peer,
                                episode,
                                request
                            );
                            let outcome = match send_authenticated_checked(
                                &socket,
                                peer,
                                COMMAND_IDR,
                                &token,
                            ) {
                                Ok(()) => IdrRequestOutcome::Transmitted,
                                Err(error) => {
                                    log_info!(
                                        "split {:?} paired IDR request send failed: {}",
                                        side,
                                        error
                                    );
                                    IdrRequestOutcome::SendFailed
                                }
                            };
                            let _ = events.send(CoordinatorEvent::IdrRequestOutcome {
                                side,
                                episode,
                                request,
                                outcome,
                            });
                        }
                    }
                }
                TileCommand::Stop { send_bye } => {
                    if send_bye {
                        if let Some(peer) = peer {
                            send_authenticated(&socket, peer, b"BYE", &token);
                            log_info!(
                                "split {:?} sent stream close signal peer={} authenticated={}",
                                side,
                                peer,
                                !token.is_empty()
                            );
                        } else {
                            log_info!(
                                "split {:?} could not send stream close signal: no peer",
                                side
                            );
                        }
                    }
                    break;
                }
            }
        }

        // The coordinator sets the shared stop flag before enqueueing the
        // final Stop command. Check it only after draining commands so the
        // worker can still authenticate and send the one final BYE.
        if control.stop_requested() {
            break;
        }

        if wait_for_socket(&socket, 2) {
            for _ in 0..split_receive_batch_limit() {
                match socket.recv_from(&mut buffer) {
                    Ok((size, source)) if peer_allowed(Some(source), &expected_host) => {
                        peer = Some(source);
                        let packet = &buffer[..size];
                        if packet.starts_with(b"LCH1") && packet.len() > 4 {
                            token.clear();
                            token.extend_from_slice(&packet[4..]);
                            if side == TileSide::Left {
                                control.input.lock().unwrap().reset_session();
                                control.audio.lock().unwrap().clear();
                            }
                            let _ = socket.send_to(packet, source);
                        } else if side == TileSide::Left {
                            if crate::audio_protocol::accept_audio_packet(
                                packet,
                                &mut control.audio.lock().unwrap(),
                            ) {
                                continue;
                            }
                            if let Some(ack) = parse_ack(packet, &token) {
                                control.input.lock().unwrap().acknowledge(ack.sequence);
                                if let Some(enabled) = ack.enabled {
                                    control
                                        .input_enabled
                                        .store(i8::from(enabled), Ordering::SeqCst);
                                }
                                continue;
                            }
                            if let Some(enabled) = parse_input_status(packet, &token) {
                                control
                                    .input_enabled
                                    .store(i8::from(enabled), Ordering::SeqCst);
                                continue;
                            }
                            if let Some(reason) = parse_termination(packet, &token) {
                                let code = match reason {
                                    TerminationReason::HealthCheck => 1,
                                    TerminationReason::HostForced => 2,
                                    TerminationReason::HostStopped => 3,
                                };
                                control.termination_reason.store(code, Ordering::SeqCst);
                                let _ = events.send(CoordinatorEvent::Fatal);
                                continue;
                            }
                        }

                        if packet.first() == Some(&PARITY_MARKER) {
                            let Some(parity) = parse_parity(packet) else {
                                continue;
                            };
                            fec_stats.record_parity_datagram();
                            let key = (parity.id, parity.base);
                            if completed_fec_groups.contains(&key) {
                                continue;
                            }
                            ensure_fec_group(
                                &mut fec_groups,
                                &mut fec_group_order,
                                key,
                                parity.id,
                                parity.k,
                                parity.base,
                                parity.total,
                                fec_stats,
                            );
                            let mut restored = None;
                            let mut complete = false;
                            if let Some(group) = fec_groups.get_mut(&key) {
                                group.push_parity(parity);
                                match group.try_restore_result() {
                                    Ok(value) => restored = value,
                                    Err(_) => fec_stats.record_decode_failure(),
                                }
                                complete = group.is_complete();
                            }
                            if let Some(restored) = restored.as_ref() {
                                fec_stats.record_restored_fragments(restored.len());
                            }
                            process_restored(
                                restored,
                                side,
                                fps,
                                window,
                                &mut reassembler,
                                &mut sequencer,
                                &mut config,
                                &mut decoder,
                                &mut awaiting_keyframe,
                                &mut last_id,
                                &mut expanded_sequence,
                                &mut last_raw_sequence,
                                &mut frame_gaps,
                                &mut input_drops,
                                &mut input_pressure_started_ns,
                                &decoder_name,
                                &events,
                                &stats,
                                &control,
                                &mut telemetry,
                            );
                            if complete {
                                fec_groups.remove(&key);
                                fec_group_order.retain(|queued| *queued != key);
                                completed_fec_groups.remember(key);
                            }
                        } else if packet.starts_with(b"CFG") || packet.starts_with(b"CF2") {
                            if let Some(next_config) = viewer_decoder::parse_codec_config(packet) {
                                sequencer.set_codec(next_config.codec);
                                if next_config.requires_decoder_reset(config.as_ref()) {
                                    decoder = None;
                                    awaiting_keyframe = true;
                                    // Pending latency traces cannot complete
                                    // across a decoder reset; an open gap
                                    // episode is aborted for the same reason.
                                    telemetry.reset_for_recovery();
                                }
                                config = Some(next_config);
                            }
                        } else if let Some(fragment) = parse_fragment(packet) {
                            fec_stats.record_data_datagram();
                            telemetry.note_fragment(fragment.id, monotonic_ns());
                            let group_base = (fragment.index / 8) * 8;
                            let group_k = (fragment.count - group_base).min(8);
                            let key = (fragment.id, group_base);
                            let group_already_complete = completed_fec_groups.contains(&key);
                            if group_k > 1 && !group_already_complete {
                                ensure_fec_group(
                                    &mut fec_groups,
                                    &mut fec_group_order,
                                    key,
                                    fragment.id,
                                    group_k as u8,
                                    group_base,
                                    fragment.count,
                                    fec_stats,
                                );
                            }
                            let mut restored = None;
                            let mut complete = false;
                            if !group_already_complete {
                                if let Some(group) = fec_groups.get_mut(&key) {
                                    group.push_data(fragment);
                                    match group.try_restore_result() {
                                        Ok(value) => restored = value,
                                        Err(_) => fec_stats.record_decode_failure(),
                                    }
                                    complete = group.is_complete();
                                }
                            }
                            if let Some(restored) = restored.as_ref() {
                                fec_stats.record_restored_fragments(restored.len());
                            }
                            process_restored(
                                restored,
                                side,
                                fps,
                                window,
                                &mut reassembler,
                                &mut sequencer,
                                &mut config,
                                &mut decoder,
                                &mut awaiting_keyframe,
                                &mut last_id,
                                &mut expanded_sequence,
                                &mut last_raw_sequence,
                                &mut frame_gaps,
                                &mut input_drops,
                                &mut input_pressure_started_ns,
                                &decoder_name,
                                &events,
                                &stats,
                                &control,
                                &mut telemetry,
                            );
                            if complete {
                                fec_groups.remove(&key);
                                fec_group_order.retain(|queued| *queued != key);
                                completed_fec_groups.remember(key);
                            }
                            if let Some(frame) = reassembler.push(fragment) {
                                for frame in sequencer.push(frame) {
                                    process_frame(
                                        side,
                                        frame,
                                        fps,
                                        window,
                                        &mut config,
                                        &mut decoder,
                                        &mut awaiting_keyframe,
                                        &mut last_id,
                                        &mut expanded_sequence,
                                        &mut last_raw_sequence,
                                        &mut frame_gaps,
                                        &mut input_drops,
                                        &mut input_pressure_started_ns,
                                        &decoder_name,
                                        &events,
                                        &stats,
                                        &control,
                                        &mut telemetry,
                                    );
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        break;
                    }
                    Err(_) => {
                        let _ = events.send(CoordinatorEvent::Fatal);
                        break;
                    }
                }
            }
        }

        for frame in sequencer.drain_expired() {
            process_frame(
                side,
                frame,
                fps,
                window,
                &mut config,
                &mut decoder,
                &mut awaiting_keyframe,
                &mut last_id,
                &mut expanded_sequence,
                &mut last_raw_sequence,
                &mut frame_gaps,
                &mut input_drops,
                &mut input_pressure_started_ns,
                &decoder_name,
                &events,
                &stats,
                &control,
                &mut telemetry,
            );
        }

        if let Some(decoder) = decoder.as_mut() {
            loop {
                match decoder.dequeue_ready_output(0) {
                    Ok(Some(output)) => {
                        let ready_ns = monotonic_ns();
                        if let Some(durations) =
                            telemetry.note_output_ready(output.pts_us, ready_ns)
                        {
                            log_info!(
                                "split {:?} gap recovery resumed output pts={} gapToIdrUs={:?} idrToFirstOutputUs={:?} gapToFirstOutputUs={}",
                                side,
                                output.pts_us,
                                durations.gap_to_idr_us,
                                durations.idr_to_first_output_us,
                                durations.total_us
                            );
                        }
                        let _ = events.send(CoordinatorEvent::Ready {
                            side,
                            frame: ReadyFrame {
                                pts_us: output.pts_us,
                                output,
                            },
                            ready_ns,
                        });
                    }
                    Ok(None) => break,
                    Err(_) => {
                        log_info!("split {:?} decoder output dequeue failed", side);
                        let _ = events.send(CoordinatorEvent::Fatal);
                        break;
                    }
                }
            }
        }

        if side == TileSide::Left {
            if let Some(peer) = peer {
                flush_input(&socket, peer, &token, &control);
            }
        }

        if last_feedback.elapsed() >= Duration::from_secs(1) {
            if let Some(peer) = peer {
                let elapsed_ms = last_feedback.elapsed().as_millis().max(1) as u64;
                let joined = stats.joined_rendered.load(Ordering::Relaxed);
                let tile_fps = rate(rendered, last_feedback_rendered, elapsed_ms);
                let joined_fps = rate(joined, last_feedback_joined, elapsed_ms);
                let fec = fec_stats.snapshot();
                let snapshot = SplitFeedbackSnapshot {
                    frame_gaps,
                    input_drops,
                    incomplete_aus: reassembler.incomplete_evictions().min(u64::from(u32::MAX))
                        as u32,
                    // The wire stale-frames field (LCF1 bytes 16..20) feeds
                    // the Host's ABR loss signal; the split path has no
                    // capture-age metric, so it must keep sending 0 rather
                    // than repurpose the field and silently change Host
                    // bitrate policy. The honest per-tile freeze count is
                    // logged below instead (frozenInputs).
                    stale_frames: 0,
                    rendered_fps: tile_fps,
                    joined_rendered_fps: joined_fps,
                    pair_ready_delta_p95_us: stats.ready_delta_p95(),
                    pair_ready_delta_max_us: stats.ready_delta_max_us.load(Ordering::Relaxed),
                    pair_sync_timeouts: stats.pair_sync_timeouts.load(Ordering::Relaxed),
                    unmatched_output_drops: stats.unmatched_output_drops.load(Ordering::Relaxed),
                    keyframe_gap_recoveries: stats.keyframe_gap_recoveries.load(Ordering::Relaxed),
                    delta_gap_recoveries: stats.delta_gap_recoveries.load(Ordering::Relaxed),
                    media_datagrams_received: fec.media_datagrams_received,
                    data_datagrams_received: fec.data_datagrams_received,
                    parity_datagrams_received: fec.parity_datagrams_received,
                    fec_restored_fragments: fec.fec_restored_fragments,
                    unrecoverable_fec_groups: fec.unrecoverable_fec_groups,
                    max_missing_data_fragments: fec.max_missing_data_fragments,
                    one_frame_gap_events: fec.one_frame_gap_events,
                    multi_frame_gap_events: fec.multi_frame_gap_events,
                    paired_idr_episodes: fec.paired_idr_episodes,
                    suppressed_duplicate_recovery_requests: fec
                        .suppressed_duplicate_recovery_requests,
                    fec_decode_failures: fec.fec_decode_failures,
                };
                log_info!(
                    "split {:?} stats renderedFps={} joinedFps={} rendered={} joined={} gaps={} inputDrops={} incomplete={} pairP95Us={} pairMaxUs={} pairTimeouts={} unmatched={} pairedResumes={} idrTransmitAttempts={} idrUnsent={} idrStaleCancelled={}",
                    side,
                    tile_fps,
                    joined_fps,
                    rendered,
                    joined,
                    frame_gaps,
                    input_drops,
                    snapshot.incomplete_aus,
                    snapshot.pair_ready_delta_p95_us,
                    snapshot.pair_ready_delta_max_us,
                    snapshot.pair_sync_timeouts,
                    snapshot.unmatched_output_drops,
                    stats.paired_idr_resumes.load(Ordering::Relaxed),
                    stats.idr_transmit_attempts.load(Ordering::Relaxed),
                    stats.idr_requests_unsent.load(Ordering::Relaxed),
                    stats.idr_requests_cancelled_stale.load(Ordering::Relaxed)
                );
                let fmt = |series: &super::super::split_latency::LatencySeries| {
                    let snapshot = series.snapshot();
                    format!(
                        "p95={}us max={}us n={}",
                        snapshot.p95_us, snapshot.max_us, snapshot.count
                    )
                };
                // Local CLOCK_MONOTONIC latency chain. All segments are
                // viewer-internal; they never measure network wire time and
                // must not be read as end-to-end latency. n=0 means the
                // segment was never observed, never that it measured zero.
                log_info!(
                    "split {:?} latency recvToFeed({}) feedToReady({}) readyToRelease({}) recvToRelease({}) frozenInputs={} gapEpisodes={} gapCompleted={} gapAborted={} gapToIdr({}) idrToFirstOutput({}) gapToFirstOutput({})",
                    side,
                    fmt(&telemetry.recv_to_feed_us),
                    fmt(&telemetry.feed_to_ready_us),
                    fmt(&telemetry.ready_to_release_us),
                    fmt(&telemetry.recv_to_release_us),
                    telemetry.frozen_inputs,
                    telemetry.gap_episodes_started,
                    telemetry.gap_episodes_completed,
                    telemetry.gap_episodes_aborted,
                    fmt(&telemetry.gap_to_idr_us),
                    fmt(&telemetry.idr_to_first_output_us),
                    fmt(&telemetry.gap_to_first_output_us)
                );
                let body = split_feedback_body(snapshot);
                send_authenticated(&socket, peer, &body, &token);
                last_feedback_rendered = rendered;
                last_feedback_joined = joined;
                last_feedback = Instant::now();
                // The system-audio opt-in rides this 1s cadence on the left
                // tile: SNDON/SNDOFF are idempotent, so the periodic
                // re-assert heals a dropped command datagram without an ACK
                // plane, exactly like the single-session refresh loop. The
                // host gates the plane per viewer, so one carrier suffices.
                send_authenticated(
                    &socket,
                    peer,
                    crate::audio_protocol::audio_stream_command(
                        control.audio_requested.load(Ordering::SeqCst),
                    ),
                    &token,
                );
            }
        }
    }
    discard_fec_groups(&mut fec_groups, &mut fec_group_order);
    completed_fec_groups.clear();
    decoder.take();
    if release_window_on_exit {
        release_window(window);
    }
}

mod helpers;
pub(super) use helpers::monotonic_ns;
use helpers::{
    ensure_fec_group, flush_input, process_frame, process_restored, rate, release_window,
    send_authenticated, send_authenticated_checked, wait_for_socket,
};
