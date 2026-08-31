use super::super::presentation_sync::{ReadyFrame, TileSide};
use super::super::split_gap_policy::{
    decide_split_frame_gap, decide_split_input_pressure, split_receive_batch_limit, SplitGapSignal,
};
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
    let fec_stats = stats.fec(side);

    while !control.stop_requested() {
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
                            let _ = events.send(CoordinatorEvent::Presented {
                                side,
                                pts_us: output.pts_us,
                                generation,
                                succeeded: false,
                            });
                            let _ = events.send(CoordinatorEvent::Fatal);
                        } else {
                            rendered = rendered.saturating_add(1);
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
                }
                TileCommand::EnterRecovery => {
                    log_info!("split {:?} decoder entering recovery", side);
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
                TileCommand::RequestIdr => {
                    if let Some(peer) = peer {
                        log_info!("split {:?} requesting paired IDR from {}", side, peer);
                        send_authenticated(&socket, peer, COMMAND_IDR, &token);
                    }
                }
                TileCommand::Stop => break,
            }
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
                            }
                            let _ = socket.send_to(packet, source);
                        } else if side == TileSide::Left {
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
                            );
                            if complete {
                                fec_groups.remove(&key);
                                fec_group_order.retain(|queued| *queued != key);
                                completed_fec_groups.remember(key);
                            }
                        } else if packet.starts_with(b"CFG") || packet.starts_with(b"CF2") {
                            if let Some(next_config) = viewer_decoder::parse_codec_config(packet) {
                                if next_config.requires_decoder_reset(config.as_ref()) {
                                    decoder = None;
                                    awaiting_keyframe = true;
                                }
                                config = Some(next_config);
                            }
                        } else if let Some(fragment) = parse_fragment(packet) {
                            fec_stats.record_data_datagram();
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

        if let Some(decoder) = decoder.as_mut() {
            loop {
                match decoder.dequeue_ready_output(0) {
                    Ok(Some(output)) => {
                        let _ = events.send(CoordinatorEvent::Ready {
                            side,
                            frame: ReadyFrame {
                                pts_us: output.pts_us,
                                output,
                            },
                            ready_ns: monotonic_ns(),
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
                    "split {:?} stats renderedFps={} joinedFps={} rendered={} joined={} gaps={} inputDrops={} incomplete={} pairP95Us={} pairMaxUs={} pairTimeouts={} unmatched={}",
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
                    snapshot.unmatched_output_drops
                );
                let body = split_feedback_body(snapshot);
                send_authenticated(&socket, peer, &body, &token);
                last_feedback_rendered = rendered;
                last_feedback_joined = joined;
                last_feedback = Instant::now();
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
    send_authenticated, wait_for_socket,
};
