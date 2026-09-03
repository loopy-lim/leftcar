use super::super::*;
use super::SingleRendererLaunch;

pub(super) fn spawn(launch: SingleRendererLaunch) {
    std::thread::spawn(move || run(launch));
}

fn terminate_locally(control: &RendererControl, reason: i8, label: &str) {
    let retained_reason = control.record_termination_reason(reason);
    log_info!(
        "locally terminating stream: requestedReason={reason} retainedReason={retained_reason} ({label})"
    );
    control.send_bye.store(false, Ordering::SeqCst);
    control.stop.store(true, Ordering::SeqCst);
}

struct RenderHealthRuntime<'a> {
    control_socket: &'a std::net::UdpSocket,
    host_peer: Option<std::net::SocketAddr>,
    viewer_control_token: &'a [u8],
    control: &'a RendererControl,
    decoder: &'a mut Option<viewer_decoder::AndroidDecoder>,
    codec_config: &'a mut Option<viewer_decoder::CodecConfig>,
    awaiting_keyframe: &'a mut bool,
    reassembler: &'a mut FrameReassembler,
    frame_sequencer: &'a mut CompletedFrameSequencer,
    fec_groups: &'a mut HashMap<(u16, u16), FecGroup>,
    fec_group_order: &'a mut VecDeque<(u16, u16)>,
    completed_fec_groups: &'a mut CompletedFecGroups,
    last_frame_id: &'a mut Option<u16>,
    aus: &'a mut u64,
    recovery_gate: &'a mut RecoveryRequestGate,
}

impl RenderHealthRuntime<'_> {
    fn evaluate(mut self, render_health: &mut RenderHealthState, completed_access_units: u64) {
        let rendered_frames = self.control.rendered_frames.load(Ordering::Relaxed);
        match render_health.observe(
            std::time::Instant::now(),
            completed_access_units,
            rendered_frames,
        ) {
            RenderHealthAction::None => {}
            RenderHealthAction::RequestIdr => self.request_idr(),
            RenderHealthAction::RebuildDecoder => {
                reset_decoder(
                    &mut *self.decoder,
                    &mut *self.codec_config,
                    &mut *self.awaiting_keyframe,
                );
                self.reassembler.clear();
                self.frame_sequencer.clear();
                self.fec_groups.clear();
                self.fec_group_order.clear();
                self.completed_fec_groups.clear();
                *self.last_frame_id = None;
                *self.aus = 0;
                self.request_idr();
            }
            RenderHealthAction::TerminateRenderStalled => terminate_locally(
                self.control,
                crate::LOCAL_TERMINATION_RENDER_STALLED,
                "render stalled",
            ),
        }
    }

    fn request_idr(&mut self) {
        if let Some(peer) = self.host_peer {
            request_idr_debounced(
                self.control_socket,
                peer,
                self.viewer_control_token,
                self.recovery_gate,
                self.control,
            );
        }
    }
}

fn run(launch: SingleRendererLaunch) {
    let SingleRendererLaunch {
        instance_str,
        window_handle,
        port,
        expected_host,
        width,
        height,
        fps,
        tcp_bridge,
        tcp_control_addr,
        prepared_receiver,
        control_clone,
    } = launch;
    // Keep the TCP bridge alive for the lifetime of the renderer. It is
    // intentionally independent from Surface creation so Host can finish
    // the TCP reachability proof before the Activity attaches.
    let tcp_bridge = tcp_bridge;
    let (socket, prepared_token, prepared_peer) = match prepared_receiver {
        Some(prepared) => match prepared.into_socket_and_token() {
            Ok(parts) => {
                log_info!("claimed prepared UDP listener on port {port}");
                parts
            }
            Err(error) => {
                log_info!("FAILED to claim prepared UDP port {port}: {error}");
                remove_renderer_if_current(&instance_str, &control_clone);
                control_clone.finished.store(true, Ordering::SeqCst);
                return;
            }
        },
        None => match std::net::UdpSocket::bind(format!("0.0.0.0:{port}")) {
            Ok(socket) => (socket, Vec::new(), None),
            Err(e) => {
                log_info!("FAILED to bind UDP listener on 0.0.0.0:{}: {}", port, e);
                remove_renderer_if_current(&instance_str, &control_clone);
                control_clone.finished.store(true, Ordering::SeqCst);
                return;
            }
        },
    };
    let control_socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => socket,
        Err(error) => {
            log_info!("FAILED to bind dedicated UDP control socket: {error}");
            remove_renderer_if_current(&instance_str, &control_clone);
            control_clone.finished.store(true, Ordering::SeqCst);
            return;
        }
    };
    configure_single_session_sockets(&socket, &control_socket);
    log_info!(
        "UDP listening on port {} (accepting media only from {expected_host})",
        port
    );
    let mut recovery_gate = RecoveryRequestGate::default();
    if let Some(peer) = tcp_control_addr {
        // A longer TCP GOP must not introduce a startup deadlock: the
        // renderer may attach after the Host's first IDR was already
        // consumed by the preflight listener. Send the authenticated
        // recovery request directly into the bridge before any media
        // datagram establishes host_peer. Register it in the same gate
        // used by the LCH1/first-packet paths so startup cannot emit
        // duplicate IDR requests and create a false initial frame gap.
        if !prepared_token.is_empty() && recovery_gate.should_request(std::time::Instant::now()) {
            request_idr(&control_socket, peer, &prepared_token);
            log_info!("requested initial IDR through TCP media bridge at {peer}");
        }
    } else if let Some(peer) = prepared_peer {
        if !prepared_token.is_empty() && recovery_gate.should_request(std::time::Instant::now()) {
            request_idr(&control_socket, peer, &prepared_token);
            log_info!("requested initial IDR from prepared UDP host at {peer}");
        }
    }
    let mut buf = vec![0u8; 2_048];
    let mut media_buffers = [[0u8; MEDIA_DATAGRAM_BYTES]; MEDIA_BATCH_SIZE];
    let mut control_buf = vec![0u8; 512];
    // The preflight challenge has already authenticated this endpoint and
    // token. Seed both control paths so the cloned input worker and socket
    // loop remain live while the media plane is intentionally silent.
    let initial_control = initial_control_state(tcp_control_addr, prepared_peer, &prepared_token);
    let mut host_peer = initial_control.host_peer;
    let mut viewer_control_token = prepared_token;
    let input_endpoint: InputEndpoint =
        std::sync::Arc::new(std::sync::Mutex::new(initial_control.input_endpoint));
    let input_worker = match control_socket.try_clone() {
        Ok(worker_socket) => Some(spawn_input_worker(
            worker_socket,
            input_endpoint.clone(),
            control_clone.clone(),
        )),
        Err(error) => {
            log_info!("failed to clone input socket; input stays on media loop: {error}");
            None
        }
    };
    let mut reassembler = FrameReassembler::default();
    let mut frame_sequencer = CompletedFrameSequencer::default();
    let mut fec_groups: HashMap<(u16, u16), FecGroup> = HashMap::new();
    let mut fec_group_order = VecDeque::new();
    let mut completed_fec_groups = CompletedFecGroups::default();
    let mut codec_config: Option<viewer_decoder::CodecConfig> = None;
    let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
    let mut aus = 0u64;
    let mut completed_access_units = 0u64;
    let mut awaiting_keyframe = true;
    let mut last_frame_id: Option<u16> = None;
    let mut renderer_stats = RendererStats::default();
    let mut latency_probe_sequence = 0u32;
    let mut render_health = RenderHealthState::default();
    let mut control_health = ControlHealthState::default();
    let mut last_latency_probe = std::time::Instant::now() - LATENCY_PROBE_INTERVAL;
    let mut last_feedback_rendered_frames = 0u64;

    while !control_clone.stop.load(Ordering::Relaxed) {
        if control_clone.suspend.load(Ordering::SeqCst) {
            // MediaCodec must let go of the old ANativeWindow before
            // SurfaceHolder.surfaceDestroyed returns. Keep draining the
            // UDP socket while hidden: if the receiver stops reading, the
            // Host's bounded latest-frame queue overflows and degrades the
            // other visible stream even though this Surface is transient.
            reset_decoder(&mut decoder, &mut codec_config, &mut awaiting_keyframe);
            reassembler.clear();
            frame_sequencer.clear();
            fec_groups.clear();
            fec_group_order.clear();
            completed_fec_groups.clear();
            last_frame_id = None;
            render_health.rebase(
                completed_access_units,
                control_clone.rendered_frames.load(Ordering::Relaxed),
            );
            *input_endpoint.lock().unwrap() = None;
            control_clone.suspended.store(true, Ordering::SeqCst);
            while control_clone.suspend.load(Ordering::SeqCst)
                && !control_clone.stop.load(Ordering::SeqCst)
            {
                if let Some(bridge) = tcp_bridge.as_ref() {
                    bridge.drain_media();
                }
                match socket.recv_from(&mut buf) {
                    Ok((received, peer)) if peer_allowed(Some(peer), &expected_host) => {
                        host_peer = Some(peer);
                        let packet = &buf[..received];
                        if packet.len() > 4 && packet.len() <= 128 && &packet[..4] == b"LCH1" {
                            viewer_control_token.clear();
                            viewer_control_token.extend_from_slice(&packet[4..]);
                            *input_endpoint.lock().unwrap() =
                                Some((peer, viewer_control_token.clone()));
                            control_clone.input.lock().unwrap().reset_session();
                            let _ = socket.send_to(packet, peer);
                        }
                        // Video/config packets are intentionally discarded
                        // until a replacement Surface requests a fresh IDR.
                    }
                    Ok((_received, peer)) => {
                        log_info!("rejected suspended media datagram from {peer}");
                    }
                    Err(ref error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(error) => {
                        log_info!("UDP receive error while Surface detached: {error}");
                    }
                }
            }
            control_clone.suspended.store(false, Ordering::SeqCst);
            if let Some(peer) = host_peer {
                *input_endpoint.lock().unwrap() = Some((peer, viewer_control_token.clone()));
                request_idr_debounced(
                    &control_socket,
                    peer,
                    &viewer_control_token,
                    &mut recovery_gate,
                    &control_clone,
                );
                if control_clone.cursor_requested.load(Ordering::SeqCst) {
                    control_clone.cursor_active.store(0, Ordering::SeqCst);
                    send_viewer_command(&control_socket, peer, b"LCDON", &viewer_control_token);
                }
            }
            continue;
        }

        // A request suppressed by live resize must not be lost forever.
        // Retry from the render loop after the final geometry event; the
        // RecoveryRequestGate still coalesces this to one request per
        // cooldown until a keyframe arrives.
        if awaiting_keyframe {
            if let Some(peer) = host_peer {
                request_idr_debounced(
                    &control_socket,
                    peer,
                    &viewer_control_token,
                    &mut recovery_gate,
                    &control_clone,
                );
            }
        }

        if let Some(peer) = host_peer {
            // Pointer samples run at 2x stream FPS (180Hz for 90fps) and
            // never wait behind the media socket's fragment queue.
            if input_worker.is_none() {
                flush_input(&control_socket, peer, &viewer_control_token, &control_clone);
            }
            let probe_due = !viewer_control_token.is_empty()
                && last_latency_probe.elapsed() >= LATENCY_PROBE_INTERVAL;
            let next_probe_sequence = probe_due.then(|| latency_probe_sequence.wrapping_add(1));
            let control_health_action = run_control_probe_cycle(
                &mut control_health,
                next_probe_sequence,
                |control_health| loop {
                    match control_socket.recv_from(&mut control_buf) {
                        Ok((received, source)) if source == peer => {
                            let _ = consume_viewer_response(
                                &control_buf[..received],
                                &viewer_control_token,
                                control_health,
                                &control_clone,
                                &mut renderer_stats,
                            );
                        }
                        Ok((_received, source)) => {
                            log_info!(
                                "rejected control_clone datagram from unexpected peer {source}"
                            );
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => {
                            log_info!("UDP control_clone receive error: {error}");
                            break;
                        }
                    }
                },
                |sequence| {
                    send_latency_probe(&control_socket, peer, &viewer_control_token, sequence)
                },
            );
            if control_clone.stop.load(Ordering::Relaxed) {
                continue;
            }
            if let Some(sequence) = next_probe_sequence {
                let feedback_now = std::time::Instant::now();
                let feedback_elapsed_ms = feedback_now
                    .duration_since(last_latency_probe)
                    .as_millis()
                    .max(1) as u64;
                latency_probe_sequence = sequence;
                if control_health_action == ControlHealthAction::TerminateHostUnreachable {
                    terminate_locally(
                        &control_clone,
                        crate::LOCAL_TERMINATION_HOST_UNREACHABLE,
                        "host unreachable",
                    );
                }
                let rendered_frames = control_clone.rendered_frames.load(Ordering::Relaxed);
                let rendered_fps = rendered_fps_from_feedback(
                    rendered_frames,
                    last_feedback_rendered_frames,
                    feedback_elapsed_ms,
                );
                last_feedback_rendered_frames = rendered_frames;
                send_receiver_feedback(
                    &control_socket,
                    peer,
                    &viewer_control_token,
                    &renderer_stats,
                    reassembler.incomplete_evictions(),
                    &control_clone,
                    rendered_fps,
                );
                last_latency_probe = feedback_now;
                // A probe response that never arrives must decay the
                // smoothed estimates instead of freezing them: the host's
                // congestion controller reads these values via LCF1 and a
                // stale high RTT would hold the bitrate at its floor
                // forever.
                if let Some(last_response) = renderer_stats.last_probe_received {
                    if last_response.elapsed() >= PROBE_STALE_INTERVAL {
                        clear_stale_latency(&control_clone);
                    }
                }
            }
        }

        let batch = if let Some(bridge) = tcp_bridge.as_ref() {
            // TCP has already provided ordering and reliable delivery.
            // Do not convert its frames back into UDP: a large IDR burst
            // can overflow that local datagram queue even when the Wi-Fi
            // TCP connection itself has no loss.
            let packet = match bridge.recv_media_timeout(std::time::Duration::from_millis(2)) {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    RenderHealthRuntime {
                        control_socket: &control_socket,
                        host_peer,
                        viewer_control_token: &viewer_control_token,
                        control: &control_clone,
                        decoder: &mut decoder,
                        codec_config: &mut codec_config,
                        awaiting_keyframe: &mut awaiting_keyframe,
                        reassembler: &mut reassembler,
                        frame_sequencer: &mut frame_sequencer,
                        fec_groups: &mut fec_groups,
                        fec_group_order: &mut fec_group_order,
                        completed_fec_groups: &mut completed_fec_groups,
                        last_frame_id: &mut last_frame_id,
                        aus: &mut aus,
                        recovery_gate: &mut recovery_gate,
                    }
                    .evaluate(&mut render_health, completed_access_units);
                    continue;
                }
                Err(error) => {
                    log_info!("TCP media bridge receive error: {error}");
                    control_clone.send_bye.store(false, Ordering::SeqCst);
                    control_clone.stop.store(true, Ordering::SeqCst);
                    continue;
                }
            };
            if packet.len() > MEDIA_DATAGRAM_BYTES {
                log_info!(
                    "TCP media frame exceeds datagram capacity: {} bytes",
                    packet.len()
                );
                continue;
            }
            let Some(peer) = tcp_control_addr else {
                log_info!("TCP media frame arrived without a control_clone endpoint");
                continue;
            };
            let Some(peer) = sockaddr_in_for_addr(peer) else {
                log_info!("TCP media endpoint is not IPv4: {peer}");
                continue;
            };
            media_buffers[0][..packet.len()].copy_from_slice(&packet);
            let mut lengths = [0usize; MEDIA_BATCH_SIZE];
            lengths[0] = packet.len();
            let mut peers: [libc::sockaddr_in; MEDIA_BATCH_SIZE] = unsafe { std::mem::zeroed() };
            peers[0] = peer;
            MediaBatch {
                count: 1,
                lengths,
                peers,
            }
        } else {
            match recv_media_batch(&socket, &mut media_buffers) {
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    RenderHealthRuntime {
                        control_socket: &control_socket,
                        host_peer,
                        viewer_control_token: &viewer_control_token,
                        control: &control_clone,
                        decoder: &mut decoder,
                        codec_config: &mut codec_config,
                        awaiting_keyframe: &mut awaiting_keyframe,
                        reassembler: &mut reassembler,
                        frame_sequencer: &mut frame_sequencer,
                        fec_groups: &mut fec_groups,
                        fec_group_order: &mut fec_group_order,
                        completed_fec_groups: &mut completed_fec_groups,
                        last_frame_id: &mut last_frame_id,
                        aus: &mut aus,
                        recovery_gate: &mut recovery_gate,
                    }
                    .evaluate(&mut render_health, completed_access_units);
                    continue;
                }
                Err(e) => {
                    log_info!("UDP receive error: {e}");
                    continue;
                }
                Ok(batch) => batch,
            }
        };

        // A recvmmsg burst can complete more than one access unit. Keep
        // all completed AUs in this bounded, stack-backed batch and feed
        // them in wire order. The decoder already discards stale output
        // buffers at the Surface boundary; dropping here would turn a
        // recoverable scheduling burst into an artificial frame gap.
        let mut completed_frames: [Option<(std::net::SocketAddr, FramePacket)>; MEDIA_BATCH_SIZE] =
            std::array::from_fn(|_| None);
        let mut completed_count = 0usize;
        for (batch_index, media_buffer) in media_buffers.iter().enumerate().take(batch.count) {
            let received = batch.lengths[batch_index];
            let Some(peer) = ipv4_socket_addr(&batch.peers[batch_index]) else {
                continue;
            };

            if !peer_allowed(Some(peer), &expected_host) {
                log_info!("rejected media datagram from {peer}: not the paired host");
                continue;
            }
            if host_peer != Some(peer) {
                log_info!("UDP sender active: {peer}");
                host_peer = Some(peer);
                *input_endpoint.lock().unwrap() = Some((peer, viewer_control_token.clone()));
                reassembler.clear();
                frame_sequencer.clear();
                fec_groups.clear();
                fec_group_order.clear();
                completed_fec_groups.clear();
                reset_decoder(&mut decoder, &mut codec_config, &mut awaiting_keyframe);
                last_frame_id = None;
                aus = 0;
                render_health.rebase(
                    completed_access_units,
                    control_clone.rendered_frames.load(Ordering::Relaxed),
                );
                control_health = ControlHealthState::default();
                control_clone.input_enabled.store(-1, Ordering::SeqCst);
                control_clone
                    .network_rtt_ms
                    .store(LATENCY_UNKNOWN, Ordering::Relaxed);
                control_clone
                    .capture_to_decoder_ms
                    .store(LATENCY_UNKNOWN, Ordering::Relaxed);
                control_clone
                    .encode_to_decoder_ms
                    .store(LATENCY_UNKNOWN, Ordering::Relaxed);
                control_clone
                    .wire_to_decoder_ms
                    .store(LATENCY_UNKNOWN, Ordering::Relaxed);
                control_clone
                    .capture_to_surface_release_ms
                    .store(LATENCY_UNKNOWN, Ordering::Relaxed);
                renderer_stats.host_clock_offset_ms = None;
                if tcp_control_addr.is_none() {
                    recovery_gate = RecoveryRequestGate::default();
                    request_idr_debounced(
                        &control_socket,
                        peer,
                        &viewer_control_token,
                        &mut recovery_gate,
                        &control_clone,
                    );
                }
            }

            let packet = &media_buffer[..received];
            if packet.len() > 4 && packet.len() <= 128 && &packet[..4] == b"LCH1" {
                viewer_control_token.clear();
                viewer_control_token.extend_from_slice(&packet[4..]);
                control_health = ControlHealthState::default();
                *input_endpoint.lock().unwrap() = Some((peer, viewer_control_token.clone()));
                control_clone.input.lock().unwrap().reset_session();
                // Drop any cursor state from a previous session so a
                // rebind cannot keep showing stale coordinates forever.
                control_clone.cursor_active.store(-1, Ordering::SeqCst);
                control_clone.cursor_sequence.store(0, Ordering::SeqCst);
                if let Err(error) = socket.send_to(packet, peer) {
                    log_info!("failed to echo UDP reachability challenge: {error}");
                } else {
                    log_info!("UDP reachability challenge verified for {peer}");
                }
                request_idr_debounced(
                    &control_socket,
                    peer,
                    &viewer_control_token,
                    &mut recovery_gate,
                    &control_clone,
                );
                if control_clone.cursor_requested.load(Ordering::SeqCst) {
                    control_clone.cursor_active.store(0, Ordering::SeqCst);
                    send_viewer_command(&control_socket, peer, b"LCDON", &viewer_control_token);
                }
                continue;
            }
            // Keep accepting responses on the legacy media socket during a
            // rolling Host/Viewer upgrade.
            if consume_viewer_response(
                packet,
                &viewer_control_token,
                &mut control_health,
                &control_clone,
                &mut renderer_stats,
            ) {
                continue;
            }
            if queue_parity_packet(
                packet,
                peer,
                &mut fec_groups,
                &mut fec_group_order,
                &mut completed_fec_groups,
                &mut reassembler,
                &mut frame_sequencer,
                &mut completed_frames,
                &mut completed_count,
                &mut renderer_stats,
            ) {
                continue;
            }
            if handle_codec_config_packet(
                packet,
                window_handle,
                width,
                height,
                fps,
                &mut decoder,
                &mut codec_config,
                &mut awaiting_keyframe,
            ) {
                continue;
            }

            queue_fragment_packet(
                packet,
                peer,
                &mut fec_groups,
                &mut fec_group_order,
                &mut completed_fec_groups,
                &mut reassembler,
                &mut frame_sequencer,
                &mut completed_frames,
                &mut completed_count,
                &mut renderer_stats,
            );
        }

        present_completed_frames(
            completed_frames,
            completed_count,
            &control_socket,
            &viewer_control_token,
            fps,
            &control_clone,
            &mut codec_config,
            &mut decoder,
            &mut aus,
            &mut completed_access_units,
            &mut renderer_stats,
            &mut last_frame_id,
            &mut awaiting_keyframe,
            &mut recovery_gate,
        );
        RenderHealthRuntime {
            control_socket: &control_socket,
            host_peer,
            viewer_control_token: &viewer_control_token,
            control: &control_clone,
            decoder: &mut decoder,
            codec_config: &mut codec_config,
            awaiting_keyframe: &mut awaiting_keyframe,
            reassembler: &mut reassembler,
            frame_sequencer: &mut frame_sequencer,
            fec_groups: &mut fec_groups,
            fec_group_order: &mut fec_group_order,
            completed_fec_groups: &mut completed_fec_groups,
            last_frame_id: &mut last_frame_id,
            aus: &mut aus,
            recovery_gate: &mut recovery_gate,
        }
        .evaluate(&mut render_health, completed_access_units);
    }
    // A transient Surface destruction keeps the host session alive. BYE
    // is reserved for the Activity's final release.
    if control_clone.send_bye.load(Ordering::SeqCst) {
        if let Some(peer) = host_peer {
            control_clone
                .input
                .lock()
                .unwrap()
                .push(InputEvent::ReleaseAll);
            flush_input(&control_socket, peer, &viewer_control_token, &control_clone);
            send_viewer_command(&control_socket, peer, b"BYE", &viewer_control_token);
            log_info!("Sent stream close signal for instance {}", instance_str);
        }
    } else {
        log_info!(
            "Transient Surface detach for instance {}; host will reconnect",
            instance_str
        );
    }
    if let Some(decoder) = decoder.as_mut() {
        decoder.stop();
    }
    *input_endpoint.lock().unwrap() = None;
    if let Some(worker) = input_worker {
        let _ = worker.join();
    }
    drop(decoder);
    drop(control_socket);
    drop(socket);
    control_clone.suspended.store(false, Ordering::SeqCst);
    remove_renderer_if_current(&instance_str, &control_clone);
    // Publish `finished` only after persisting any Host or local reason; callers
    // waiting to reuse this instance/port may clear the cache as soon as
    // they observe completion.
    control_clone.finished.store(true, Ordering::SeqCst);
    log_info!("Live stream renderer exiting for instance");
}
