use super::super::dispatch::{decide_idr_send, IdrRequestOutcome, IdrSendDecision};
use super::super::presentation_sync::{ReadyFrame, TileSide};
use super::super::split_gap_policy::{
    decide_split_frame_gap, decide_split_input_pressure, SplitGapSignal,
};
use super::super::split_latency::TileLatencyTelemetry;
use super::super::stats::{split_feedback_body, SplitFeedbackSnapshot};
use super::{CoordinatorEvent, RuntimeStats, TileCommand};
use crate::input_protocol::{
    encode_input, encode_latency_probe, estimate_latency, parse_ack, parse_input_status,
    parse_latency_probe_response, parse_termination,
};
use crate::jni::RendererControl;
use crate::log_info;
use crate::media_crypto::SharedMediaCrypto;
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

/// Split clock-sync probe cadence, matching the single-session
/// LATENCY_PROBE_INTERVAL so both paths converge at the same rate.
const LATENCY_PROBE_INTERVAL: Duration = Duration::from_secs(1);
/// Minimum spacing for the first-loss immediate LCF1 send; the 1s tick
/// continues unchanged on top of this.
const IMMEDIATE_FEEDBACK_MIN_INTERVAL: Duration = Duration::from_millis(100);

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

/// One stamped IDR request ("paired" or "per-tile" scope) at pre-wire time.
/// Episode AND unique request-id ownership are re-checked HERE, right before
/// any wire access, so a request queued before its episode resumed — or an
/// old queued wire copy of a superseded same-episode command — is cancelled
/// instead of becoming a late pending PLI (the Host only coalesces paired
/// PLIs while its recovery boundary is pending; a late one would start a new
/// Host generation). An unready path is explicitly reported unsent to the
/// coordinator for retention — logging-and-dropping is not allowed. A UDP
/// send is reported as a transmit attempt, never as delivery. Both scopes
/// send the same sealed `COMMAND_IDR`; in the per-tile scope this tile's own
/// source port is what tells the Host which tile to refresh.
#[allow(clippy::too_many_arguments)]
fn handle_idr_request(
    side: TileSide,
    scope: &str,
    episode: u64,
    request: u64,
    current_episode: u64,
    current_request: u64,
    peer: Option<SocketAddr>,
    socket: &UdpSocket,
    crypto: &SharedMediaCrypto,
    events: &mpsc::Sender<CoordinatorEvent>,
    stats: &RuntimeStats,
) {
    match decide_idr_send(
        current_episode,
        episode,
        current_request,
        request,
        peer.is_some(),
        crypto.is_established(),
    ) {
        IdrSendDecision::CancelStale => {
            log_info!(
                "split {:?} {} IDR request cancelled before wire: episode={} stale (current={}) or request={} superseded (current={})",
                side,
                scope,
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
                "split {:?} {} IDR request NOT sent: peer not learned yet (episode={}, request={}); reported for retention",
                side,
                scope,
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
                "split {:?} {} IDR request NOT sent to {}: sealed handshake not established yet (episode={}, request={})",
                side,
                peer.map(|peer| peer.to_string()).unwrap_or_else(|| "unknown peer".into()),
                scope,
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
                return;
            };
            stats.idr_transmit_attempts.fetch_add(1, Ordering::Relaxed);
            log_info!(
                "split {:?} {} IDR request transmit attempt to {} (episode={}, request={}; send accepted != delivered)",
                side,
                scope,
                peer,
                episode,
                request
            );
            let outcome = match send_authenticated_checked(
                socket,
                peer,
                crate::media_datagram::COMMAND_IDR,
                crypto,
            ) {
                Ok(()) => IdrRequestOutcome::Transmitted,
                Err(error) => {
                    log_info!(
                        "split {:?} {} IDR request send failed: {}",
                        side,
                        scope,
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
    let (socket, crypto, mut peer) = match prepared.into_socket_and_media_crypto() {
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
    let mut batch_buffers = [[0u8; crate::media_datagram::MEDIA_BUFFER_BYTES]; SPLIT_MEDIA_BATCH];
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
    // plus gap -> IDR -> first output). These series stay CLOCK_MONOTONIC —
    // they measure the viewer-internal pipeline only. The separate
    // clock-corrected end-to-end ages (capture/wire -> decoder) come from the
    // LCP1/LCP2 host clock offset and live in the shared RendererControl.
    let mut telemetry = TileLatencyTelemetry::default();
    // Split clock sync (telemetry only): the left tile media socket sends the
    // same sealed LCP1 probe the single-session control socket uses, and
    // whichever tile socket sees the LCP2 response publishes the NTP-style
    // host clock offset into the shared RuntimeStats for both workers.
    let mut latency_probe_sequence = 0u32;
    let mut last_latency_probe = Instant::now();
    // First-loss fast feedback: a new loss signal (the coordinator-wide
    // delta-gap recovery counter moved) requests one immediate LCF1 send at
    // the top of the socket loop, min-interval gated against storms. The 1s
    // tick continues unchanged.
    let mut feedback_soon = false;
    let mut last_seen_delta_gap_recoveries = stats.delta_gap_recoveries.load(Ordering::Relaxed);
    let fec_stats = stats.fec(side);
    // Selective retransmission (NACK/RTX): one request slot for this tile's
    // blocked resequencer hole plus the 1Hz-log counters. The NACK rides the
    // tile media socket; the host identifies the tile by the source port.
    let mut nack_requester = crate::media_datagram::NackRequester::default();
    let mut nacks_sent = 0u64;
    let mut nacks_healed = 0u64;

    loop {
        // First-loss feedback MUST reach the Host before the IDR request:
        // process_frame marks feedback_soon the moment a gap is classified,
        // and this block sits ahead of the command drain, so the fast LCF1
        // (carrying the loss increase the Host's side-aware per-tile IDR
        // decision reads) is on the wire before the IDR datagram leaves for
        // the same gap. `periodic` keeps the verbose logs, the rendered-fps
        // window reset, and the SNDON re-assert on their unchanged 1s
        // cadence.
        let delta_gap_recoveries_now = stats.delta_gap_recoveries.load(Ordering::Relaxed);
        if delta_gap_recoveries_now != last_seen_delta_gap_recoveries {
            last_seen_delta_gap_recoveries = delta_gap_recoveries_now;
            feedback_soon = true;
        }
        let periodic = last_feedback.elapsed() >= Duration::from_secs(1);
        let immediate = feedback_soon && last_feedback.elapsed() >= IMMEDIATE_FEEDBACK_MIN_INTERVAL;
        if periodic || immediate {
            feedback_soon = false;
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
                    paired_idr_resumes: stats.paired_idr_resumes.load(Ordering::Relaxed),
                    wire_to_decoder_ms: crate::renderer::latency_feedback_value_u16(
                        control.wire_to_decoder_ms.load(Ordering::Relaxed),
                    ),
                    capture_age_ms: crate::renderer::latency_feedback_value_u16(
                        control.capture_to_decoder_ms.load(Ordering::Relaxed),
                    ),
                    input_rtt_ms: crate::renderer::latency_feedback_value_u16(
                        control.input_rtt_ms.load(Ordering::Relaxed),
                    ),
                    per_tile_idr_resumes: stats.per_tile_idr_resumes.load(Ordering::Relaxed),
                };
                if periodic {
                    if side == TileSide::Left {
                        log_info!("LeftcarViewerPerf schema=2 process={} stream={} incarnation={} kind=split released={} leftReleased={} rightReleased={} outputStage=paired-surface-release releaseCaptureAgeMs=None", std::process::id(), control.port, control.metric_incarnation, joined, stats.left_rendered.load(Ordering::Relaxed), stats.right_rendered.load(Ordering::Relaxed));
                    }
                    log_info!(
                        "split {:?} stats renderedFps={} joinedFps={} rendered={} joined={} gaps={} inputDrops={} incomplete={} pairP95Us={} pairMaxUs={} pairTimeouts={} unmatched={} pairedResumes={} perTileResumes={} idrTransmitAttempts={} idrUnsent={} idrStaleCancelled={} nacksSent={} nacksHealed={}",
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
                        stats.per_tile_idr_resumes.load(Ordering::Relaxed),
                        stats.idr_transmit_attempts.load(Ordering::Relaxed),
                        stats.idr_requests_unsent.load(Ordering::Relaxed),
                        stats.idr_requests_cancelled_stale.load(Ordering::Relaxed),
                        nacks_sent,
                        nacks_healed
                    );
                }
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
                // hostOffset/captureAge/wireMs are the clock-corrected
                // end-to-end segment: None means the LCP1/LCP2 exchange has
                // not converged on this session yet, never a zero age.
                if periodic {
                    let host_offset = stats.host_clock_offset();
                    let latency_log_value = |value: u64| {
                        (value != crate::renderer::LATENCY_MEASUREMENT_UNKNOWN).then_some(value)
                    };
                    let capture_age =
                        latency_log_value(control.capture_to_decoder_ms.load(Ordering::Relaxed));
                    let wire_age =
                        latency_log_value(control.wire_to_decoder_ms.load(Ordering::Relaxed));
                    log_info!(
                        "split {:?} latency recvToFeed({}) feedToReady({}) readyToRelease({}) recvToRelease({}) frozenInputs={} gapEpisodes={} gapCompleted={} gapAborted={} gapToIdr({}) idrToFirstOutput({}) gapToFirstOutput({}) hostOffsetMs={:?} captureAgeMs={:?} wireMs={:?} networkRttMs={:?} inputRttMs={:?}",
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
                        fmt(&telemetry.gap_to_first_output_us),
                        host_offset,
                        capture_age,
                        wire_age,
                        latency_log_value(control.network_rtt_ms.load(Ordering::Relaxed)),
                        latency_log_value(control.input_rtt_ms.load(Ordering::Relaxed))
                    );
                }
                let body = split_feedback_body(snapshot);
                send_authenticated(&socket, peer, &body, &crypto);
                last_feedback_rendered = rendered;
                last_feedback_joined = joined;
                last_feedback = Instant::now();
                // The system-audio opt-in rides this 1s cadence on the left
                // tile only: SNDON/SNDOFF are idempotent, so the periodic
                // re-assert heals a dropped command datagram without an ACK
                // plane, exactly like the single-session refresh loop. The
                // host gates the plane per viewer, so one carrier suffices —
                // the right tile must not double the command stream. A
                // first-loss immediate send does not repeat the command.
                if periodic && side == TileSide::Left {
                    send_authenticated(
                        &socket,
                        peer,
                        crate::audio_protocol::audio_stream_command(
                            control.audio_requested.load(Ordering::SeqCst),
                        ),
                        &crypto,
                    );
                    if control.audio_requested.load(Ordering::SeqCst) {
                        send_authenticated(
                            &socket,
                            peer,
                            crate::audio_protocol::audio_codec_command(
                                control.audio_opus_requested.load(Ordering::SeqCst),
                            ),
                            &crypto,
                        );
                    }
                }
            }
        }

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
                    let (episode_register, request_register) =
                        (&stats.recovery_episode, &stats.recovery_request);
                    handle_idr_request(
                        side,
                        "paired",
                        episode,
                        request,
                        episode_register.load(Ordering::Relaxed),
                        request_register.load(Ordering::Relaxed),
                        peer,
                        &socket,
                        &crypto,
                        &events,
                        &stats,
                    );
                }
                TileCommand::RequestTileIdr { episode, request } => {
                    // Per-tile recovery (R2): ownership is checked against
                    // THIS side's tile-scope registers and the sealed "IDR"
                    // always leaves through this tile's own socket, so the
                    // Host identifies the requesting side by source port.
                    let (episode_register, request_register) = stats.tile_scope(side);
                    handle_idr_request(
                        side,
                        "per-tile",
                        episode,
                        request,
                        episode_register.load(Ordering::Relaxed),
                        request_register.load(Ordering::Relaxed),
                        peer,
                        &socket,
                        &crypto,
                        &events,
                        &stats,
                    );
                }
                TileCommand::Stop { send_bye } => {
                    if send_bye {
                        if let Some(peer) = peer {
                            send_authenticated(
                                &socket,
                                peer,
                                crate::media_datagram::COMMAND_BYE,
                                &crypto,
                            );
                            log_info!(
                                "split {:?} sent stream close signal peer={} established={}",
                                side,
                                peer,
                                crypto.is_established()
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

        if side == TileSide::Left
            && crypto.is_established()
            && last_latency_probe.elapsed() >= LATENCY_PROBE_INTERVAL
        {
            if let Some(peer) = peer {
                latency_probe_sequence = latency_probe_sequence.wrapping_add(1).max(1);
                send_authenticated(
                    &socket,
                    peer,
                    &encode_latency_probe(latency_probe_sequence, crate::renderer::wall_clock_ms()),
                    &crypto,
                );
                last_latency_probe = Instant::now();
            }
        }

        // Selective retransmit (NACK/RTX): when this tile's resequencer is
        // blocked on an AU whose missing fragments are known, ask the host
        // once to re-send them and hold the reorder window open for the
        // bounded grace. Completion inside the grace avoids the paired-IDR
        // freeze entirely; expiry falls through to the unchanged gap
        // classification (SplitGapSignal::Loss), so the coordinator's gated
        // IDR request only fires after the grace.
        if let Some(peer) = peer {
            let network_rtt = control.network_rtt_ms.load(Ordering::Relaxed);
            let nack_outcome = crate::media_datagram::tick_nack_requester(
                &mut nack_requester,
                &mut sequencer,
                &reassembler,
                awaiting_keyframe,
                (network_rtt != crate::jni::LATENCY_UNKNOWN).then_some(network_rtt),
                std::time::Instant::now(),
                |body| send_authenticated(&socket, peer, body, &crypto),
            );
            nacks_sent = nacks_sent.saturating_add(nack_outcome.sent_messages);
            if nack_outcome.healed {
                nacks_healed = nacks_healed.saturating_add(1);
                log_info!(
                    "split {:?} nack healed access unit id={:?}: retransmit completed inside the grace, no freeze",
                    side,
                    nack_outcome.au_id
                );
            }
        }

        if wait_for_socket(&socket, 2) {
            match recv_split_batch(&socket, &mut batch_buffers) {
                Ok(batch) => {
                    // One recvmmsg drains a Wi-Fi microburst in a single
                    // syscall (MSG_WAITFORONE keeps the tail non-blocking);
                    // the per-wake processing cap stays
                    // split_receive_batch_limit(), exactly like the old
                    // recv_from loop.
                    for index in 0..batch.count {
                        let size = batch.lengths[index];
                        let Some(source) = batch.sources[index] else {
                            continue;
                        };
                        if !peer_allowed(Some(source), &expected_host) {
                            continue;
                        }
                        peer = Some(source);
                        let network_rtt = control.network_rtt_ms.load(Ordering::Relaxed);
                        sequencer.configure_nack_grace(
                            !awaiting_keyframe,
                            (network_rtt != crate::jni::LATENCY_UNKNOWN).then_some(network_rtt),
                        );
                        // Open before parsing: only datagrams sealed with the
                        // session media key are trusted on this socket. The
                        // in-place open decrypts inside the recvmmsg buffer —
                        // no per-datagram Vec on the RX hot path.
                        let Some(opened) = crypto.open_into(&mut batch_buffers[index][..size])
                        else {
                            continue;
                        };
                        let packet: &[u8] = opened;
                        if let Some(challenge) = crate::prepared_udp::is_challenge_packet(packet) {
                            crypto.establish();
                            if side == TileSide::Left {
                                control.input.lock().unwrap().reset_session();
                                control.audio.lock().unwrap().clear();
                            }
                            if let Some(reply) = crypto.seal(challenge) {
                                let _ = socket.send_to(&reply, source);
                            }
                        } else if side == TileSide::Left {
                            if crate::audio_protocol::accept_audio_packet(
                                packet,
                                &mut control.audio.lock().unwrap(),
                            ) {
                                control.audio_available.notify_one();
                                continue;
                            }
                            if let Some(ack) = parse_ack(packet) {
                                control.acknowledge_input(
                                    ack.sequence,
                                    (monotonic_ns().max(0) as u64) / 1_000,
                                );
                                if let Some(enabled) = ack.enabled {
                                    control
                                        .input_enabled
                                        .store(i8::from(enabled), Ordering::SeqCst);
                                }
                                continue;
                            }
                            if let Some(enabled) = parse_input_status(packet) {
                                control
                                    .input_enabled
                                    .store(i8::from(enabled), Ordering::SeqCst);
                                continue;
                            }
                            if let Some(reason) = parse_termination(packet) {
                                let code = reason.code();
                                control
                                    .termination_reason
                                    .store(i8::try_from(code).unwrap_or(-1), Ordering::SeqCst);
                                let _ = events.send(CoordinatorEvent::Fatal);
                                continue;
                            }
                        }

                        // LCP2 replies to the split clock-sync probes. Either
                        // tile socket can technically see one; the estimate
                        // lands in the shared RuntimeStats either way.
                        if let Some(response) = parse_latency_probe_response(packet) {
                            if let Some(estimate) =
                                estimate_latency(response, crate::renderer::wall_clock_ms())
                            {
                                crate::renderer::store_smoothed_latency(
                                    &control.network_rtt_ms,
                                    estimate.network_rtt_ms,
                                );
                                stats.record_host_clock_offset(estimate.host_clock_offset_ms);
                            }
                            continue;
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
                                &mut feedback_soon,
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
                                &mut feedback_soon,
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
                                for frame in sequencer.push_reassembled(frame, &reassembler) {
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
                                        &mut feedback_soon,
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
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => {
                    let _ = events.send(CoordinatorEvent::Fatal);
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
                &mut feedback_soon,
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
                flush_input(&socket, peer, &crypto, &control);
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
    ensure_fec_group, flush_input, process_frame, process_restored, rate, recv_split_batch,
    release_window, send_authenticated, send_authenticated_checked, wait_for_socket,
    SPLIT_MEDIA_BATCH,
};
