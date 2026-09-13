use super::*;
use crate::renderer::split_latency::TileLatencyTelemetry;
use std::os::fd::AsRawFd;

// Qualcomm's low-latency decoder can briefly withhold the next compressed
// input slot while retiring the previous AU. A zero-timeout probe turned that
// sub-millisecond scheduling jitter into a dropped reference frame. Keep the
// wait far below one 60 Hz frame so it cannot build visible playback latency.
const SPLIT_DECODER_INPUT_TIMEOUT_US: i64 = 1_000;

pub(super) fn wait_for_socket(socket: &UdpSocket, timeout_ms: i32) -> bool {
    let mut descriptor = libc::pollfd {
        fd: socket.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
    result > 0 && descriptor.revents & libc::POLLIN != 0
}

/// Per-wake receive cap, shared with the gap-policy tests. One recvmmsg call
/// receives up to this many datagrams in a single syscall — the same
/// processing cap the previous per-wake recv_from loop enforced.
pub(super) const SPLIT_MEDIA_BATCH: usize =
    crate::renderer::split_gap_policy::split_receive_batch_limit();

pub(super) struct SplitMediaBatch {
    pub(super) count: usize,
    pub(super) lengths: [usize; SPLIT_MEDIA_BATCH],
    pub(super) sources: [Option<SocketAddr>; SPLIT_MEDIA_BATCH],
}

/// One recvmmsg (MSG_WAITFORONE) draining a Wi-Fi microburst in a single
/// syscall, mirroring the single-session media batch. The caller still
/// processes at most [SPLIT_MEDIA_BATCH] datagrams per wake.
pub(super) fn recv_split_batch(
    socket: &UdpSocket,
    buffers: &mut [[u8; crate::media_datagram::MEDIA_BUFFER_BYTES]; SPLIT_MEDIA_BATCH],
) -> std::io::Result<SplitMediaBatch> {
    let mut peers: [libc::sockaddr_in; SPLIT_MEDIA_BATCH] = unsafe { std::mem::zeroed() };
    let mut iovecs: [libc::iovec; SPLIT_MEDIA_BATCH] = std::array::from_fn(|index| libc::iovec {
        iov_base: buffers[index].as_mut_ptr().cast(),
        iov_len: crate::media_datagram::MEDIA_BUFFER_BYTES,
    });
    let mut messages: [libc::mmsghdr; SPLIT_MEDIA_BATCH] =
        std::array::from_fn(|_| unsafe { std::mem::zeroed() });
    for index in 0..SPLIT_MEDIA_BATCH {
        messages[index].msg_hdr.msg_name = (&mut peers[index] as *mut libc::sockaddr_in).cast();
        messages[index].msg_hdr.msg_namelen =
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        messages[index].msg_hdr.msg_iov = &mut iovecs[index];
        messages[index].msg_hdr.msg_iovlen = 1;
    }
    let count = unsafe {
        libc::recvmmsg(
            socket.as_raw_fd(),
            messages.as_mut_ptr(),
            SPLIT_MEDIA_BATCH as u32,
            libc::MSG_WAITFORONE,
            std::ptr::null_mut(),
        )
    };
    if count < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut lengths = [0usize; SPLIT_MEDIA_BATCH];
    let mut sources: [Option<SocketAddr>; SPLIT_MEDIA_BATCH] = std::array::from_fn(|_| None);
    for index in 0..count as usize {
        lengths[index] = messages[index].msg_len as usize;
        sources[index] = split_ipv4_socket_addr(&peers[index]);
    }
    Ok(SplitMediaBatch {
        count: count as usize,
        lengths,
        sources,
    })
}

fn split_ipv4_socket_addr(raw: &libc::sockaddr_in) -> Option<SocketAddr> {
    if i32::from(raw.sin_family) != libc::AF_INET {
        return None;
    }
    let octets = raw.sin_addr.s_addr.to_ne_bytes();
    Some(SocketAddr::V4(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::from(octets),
        u16::from_be(raw.sin_port),
    )))
}

pub(super) fn ensure_fec_group(
    groups: &mut HashMap<(u16, u16), FecGroup>,
    order: &mut VecDeque<(u16, u16)>,
    key: (u16, u16),
    id: u16,
    k: u8,
    base: u16,
    total: u16,
    fec_stats: &crate::renderer::fec_stats::FecRuntimeStats,
) {
    if groups.contains_key(&key) {
        return;
    }
    while groups.len() >= crate::media_datagram::MAX_ACTIVE_FEC_GROUPS {
        let Some(oldest) = order.pop_front() else {
            break;
        };
        if let Some(group) = groups.remove(&oldest) {
            fec_stats.record_unrecoverable_group(group.missing_data_fragments());
        }
    }
    if let Some(group) = FecGroup::new(id, k, base, total) {
        groups.insert(key, group);
        order.push_back(key);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_restored(
    restored: Option<Vec<RestoredFragment>>,
    side: TileSide,
    fps: u32,
    window: usize,
    reassembler: &mut FrameReassembler,
    sequencer: &mut CompletedFrameSequencer,
    config: &mut Option<viewer_decoder::CodecConfig>,
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    awaiting_keyframe: &mut bool,
    last_id: &mut Option<u16>,
    expanded_sequence: &mut u64,
    last_raw_sequence: &mut Option<u16>,
    frame_gaps: &mut u32,
    input_drops: &mut u32,
    input_pressure_started_ns: &mut Option<u64>,
    feedback_soon: &mut bool,
    decoder_name: &str,
    events: &mpsc::Sender<CoordinatorEvent>,
    stats: &RuntimeStats,
    control: &RendererControl,
    telemetry: &mut TileLatencyTelemetry,
) {
    for recovered in restored.into_iter().flatten() {
        let fragment = FrameFragment {
            index: recovered.index,
            count: recovered.count,
            id: recovered.id,
            capture_wall_ms: recovered.capture_wall_ms,
            encode_wall_ms: recovered.encode_wall_ms,
            send_wall_ms: recovered.send_wall_ms,
            payload: &recovered.payload,
        };
        let Some(frame) = reassembler.push(fragment) else {
            continue;
        };
        sequencer.configure_nack_grace(
            !*awaiting_keyframe,
            match control.network_rtt_ms.load(Ordering::Relaxed) {
                crate::jni::LATENCY_UNKNOWN => None,
                rtt => Some(rtt),
            },
        );
        for frame in sequencer.push_reassembled(frame, reassembler) {
            process_frame(
                side,
                frame,
                fps,
                window,
                config,
                decoder,
                awaiting_keyframe,
                last_id,
                expanded_sequence,
                last_raw_sequence,
                frame_gaps,
                input_drops,
                input_pressure_started_ns,
                feedback_soon,
                decoder_name,
                events,
                stats,
                control,
                telemetry,
            );
        }
    }
}

pub(super) fn flush_input(
    socket: &UdpSocket,
    peer: SocketAddr,
    crypto: &SharedMediaCrypto,
    control: &RendererControl,
) {
    if !crypto.is_established() {
        return;
    }
    for _ in 0..2 {
        let outbound = control
            .input
            .lock()
            .unwrap()
            .next_ready((monotonic_ns().max(0) as u64) / 1_000);
        let Some(outbound) = outbound else {
            break;
        };
        // Stamp before the send so the ack consumer measures the full
        // send->ack round trip (retransmit attempts overwrite the stamp, so
        // the EWMA measures the final successful attempt).
        if outbound.event.is_reliable() {
            control.record_reliable_input_send((monotonic_ns().max(0) as u64) / 1_000);
        }
        let Some(packet) = crypto.seal(&encode_input(&outbound)) else {
            break;
        };
        if socket.send_to(&packet, peer).is_err() {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_frame(
    side: TileSide,
    frame: ReassembledFrame,
    fps: u32,
    window: usize,
    config: &mut Option<viewer_decoder::CodecConfig>,
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    awaiting_keyframe: &mut bool,
    last_id: &mut Option<u16>,
    expanded_sequence: &mut u64,
    last_raw_sequence: &mut Option<u16>,
    frame_gaps: &mut u32,
    input_drops: &mut u32,
    input_pressure_started_ns: &mut Option<u64>,
    feedback_soon: &mut bool,
    decoder_name: &str,
    events: &mpsc::Sender<CoordinatorEvent>,
    stats: &RuntimeStats,
    control: &RendererControl,
    telemetry: &mut TileLatencyTelemetry,
) {
    let keyframe = crate::media_datagram::is_keyframe(&frame.au, viewer_decoder::VideoCodec::H264);
    let gap = decide_split_frame_gap(*last_id, frame.id, keyframe, *awaiting_keyframe);
    if gap.missing > 0 {
        stats.fec(side).record_gap_event(gap.missing);
        *frame_gaps = frame_gaps.saturating_add(u32::from(gap.missing));
        stats
            .frame_gaps
            .fetch_add(u64::from(gap.missing), Ordering::Relaxed);
    }
    *last_id = Some(frame.id);
    *awaiting_keyframe = gap.awaiting_keyframe_after;
    if gap.signal == SplitGapSignal::Loss {
        stats.delta_gap_recoveries.fetch_add(1, Ordering::Relaxed);
        // Mark the fast-path feedback NOW: the worker loop sends the LCF1
        // (with this tile's loss increase) at the top of the next iteration
        // — ahead of the command drain — so the loss report is on the wire
        // before the coordinator's IDR request for the same gap. The Host's
        // side-aware per-tile decision needs that ordering.
        *feedback_soon = true;
        telemetry.note_gap_started(monotonic_ns());
        let _ = events.send(CoordinatorEvent::NetworkGap(side));
    } else if gap.signal == SplitGapSignal::Idr && gap.missing > 0 {
        stats
            .keyframe_gap_recoveries
            .fetch_add(1, Ordering::Relaxed);
    }
    if !gap.feed {
        // The tile holds its last good Surface image and withholds this AU.
        // With a decoder present the freeze is user-visible, so count it in
        // the local frozen-input metric (the wire stale-frames field stays 0
        // because the Host reads it into its ABR loss signal).
        if decoder.is_some() {
            telemetry.note_frozen_input();
        }
        return;
    }
    let Some(config) = config.as_ref() else {
        return;
    };
    let (Some(sps), Some(pps)) = (config.sps.as_deref(), config.pps.as_deref()) else {
        return;
    };
    if decoder.is_none() {
        if !keyframe {
            return;
        }
        let created = unsafe {
            viewer_decoder::AndroidDecoder::new_video_named(viewer_decoder::VideoDecoderConfig {
                codec: config.codec,
                vps: config.vps.as_deref(),
                sps,
                pps,
                width: 1_920,
                height: 2_160,
                window,
                fps,
                codec_name: Some(decoder_name),
                allow_mime_fallback: false,
                max_frame_size: Some((1_920, 2_160)),
            })
        };
        let created = match created {
            Ok(created) => created,
            Err(error) => {
                log_info!("split {:?} decoder creation failed: {}", side, error);
                let _ = events.send(CoordinatorEvent::Fatal);
                return;
            }
        };
        let actual_decoder_name = created.codec_name();
        if actual_decoder_name != decoder_name {
            log_info!(
                "split {:?} rejected unexpected decoder: requested={} actual={}",
                side,
                decoder_name,
                actual_decoder_name
            );
            let _ = events.send(CoordinatorEvent::Fatal);
            return;
        }
        log_info!(
            "split {:?} hardware decoder ready: codec={} size=1920x2160 fps={}",
            side,
            actual_decoder_name,
            fps
        );
        *decoder = Some(created);
    }
    if gap.signal == SplitGapSignal::Idr {
        log_info!("split {:?} received IDR id={}", side, frame.id);
        let _ = events.send(CoordinatorEvent::Idr {
            side,
            generation: u64::from(frame.id),
        });
    }
    *expanded_sequence = expand_sequence(*expanded_sequence, *last_raw_sequence, frame.id);
    *last_raw_sequence = Some(frame.id);
    let pts_us = expanded_sequence.saturating_mul(1_000_000 / u64::from(fps.max(1))) as i64;
    let Some(decoder) = decoder.as_mut() else {
        return;
    };
    match decoder.queue_access_unit(&frame.au, pts_us, SPLIT_DECODER_INPUT_TIMEOUT_US) {
        Ok(viewer_decoder::FeedStatus::Queued { .. }) => {
            *input_pressure_started_ns = None;
            let queued_ns = monotonic_ns();
            telemetry.note_queued(frame.id, pts_us, queued_ns);
            // Clock-corrected end-to-end ages, mirroring the single-session
            // feed math: capture->decoder-feed and host-send->decoder-feed,
            // EWMA'd into the shared RendererControl the HUD and the LCF1
            // suffix read. Telemetry only — nothing here gates feeding,
            // scheduling, or presentation.
            let host_clock_offset_ms = stats
                .host_clock_offset()
                .unwrap_or(crate::renderer::HOST_CLOCK_OFFSET_UNKNOWN_MS);
            if let Some(age) =
                crate::renderer::clock_corrected_age_ms(frame.capture_wall_ms, host_clock_offset_ms)
            {
                crate::renderer::store_smoothed_latency(&control.capture_to_decoder_ms, age);
            }
            if let Some(age) = crate::renderer::clock_corrected_age_ms(
                Some(frame.send_wall_ms),
                host_clock_offset_ms,
            ) {
                crate::renderer::store_smoothed_latency(&control.wire_to_decoder_ms, age);
            }
            if gap.signal == SplitGapSignal::Idr {
                if let Some(gap_to_idr_us) = telemetry.note_idr(pts_us, queued_ns) {
                    log_info!(
                        "split {:?} gap freeze IDR queued after {}us",
                        side,
                        gap_to_idr_us
                    );
                }
            }
        }
        Ok(viewer_decoder::FeedStatus::InputUnavailable) => {
            *input_drops = input_drops.saturating_add(1);
            if *input_drops <= 3 || *input_drops % 60 == 0 {
                log_info!(
                    "split {:?} decoder input unavailable; drops={}",
                    side,
                    input_drops
                );
            }
            stats.input_drops.fetch_add(1, Ordering::Relaxed);
            control.record_split_loss(
                stats.frame_gaps.load(Ordering::Relaxed),
                stats.input_drops.load(Ordering::Relaxed),
            );
            let now_ns = monotonic_ns().max(0) as u64;
            let pressure_started_ns = *input_pressure_started_ns.get_or_insert(now_ns);
            let pressure_elapsed_ms = now_ns.saturating_sub(pressure_started_ns) / 1_000_000;
            let pressure = decide_split_input_pressure(keyframe, pressure_elapsed_ms);
            if pressure.recover {
                let _ = events.send(CoordinatorEvent::DecoderFailure(side));
                *awaiting_keyframe = true;
            }
        }
        Ok(viewer_decoder::FeedStatus::InputTooLarge { required, capacity }) => {
            *input_pressure_started_ns = None;
            *input_drops = input_drops.saturating_add(1);
            log_info!(
                "split {:?} decoder AU too large: required={} capacity={}",
                side,
                required,
                capacity
            );
            stats.input_drops.fetch_add(1, Ordering::Relaxed);
            control.record_split_loss(
                stats.frame_gaps.load(Ordering::Relaxed),
                stats.input_drops.load(Ordering::Relaxed),
            );
            let _ = events.send(CoordinatorEvent::DecoderFailure(side));
            *awaiting_keyframe = true;
        }
        Err(error) => {
            *input_pressure_started_ns = None;
            *input_drops = input_drops.saturating_add(1);
            log_info!("split {:?} decoder feed failed: {}", side, error);
            stats.input_drops.fetch_add(1, Ordering::Relaxed);
            control.record_split_loss(
                stats.frame_gaps.load(Ordering::Relaxed),
                stats.input_drops.load(Ordering::Relaxed),
            );
            let _ = events.send(CoordinatorEvent::DecoderFailure(side));
            *awaiting_keyframe = true;
        }
    }
}

pub(super) fn expand_sequence(current: u64, previous: Option<u16>, raw: u16) -> u64 {
    let Some(previous) = previous else {
        return u64::from(raw);
    };
    let mut epoch = current & !u64::from(u16::MAX);
    if raw < previous && previous.wrapping_sub(raw) > i16::MAX as u16 {
        epoch = epoch.saturating_add(1 << 16);
    }
    epoch | u64::from(raw)
}

pub(super) fn send_authenticated(
    socket: &UdpSocket,
    peer: SocketAddr,
    body: &[u8],
    crypto: &SharedMediaCrypto,
) {
    if !crypto.is_established() {
        return;
    }
    let Some(packet) = crypto.seal(body) else {
        return;
    };
    let _ = socket.send_to(&packet, peer);
}

/// One sealed send_to whose kernel result is reported. Acceptance by the
/// kernel is a transmit attempt only — actual delivery is never known from
/// UDP — so callers must treat `Ok(())` as `Transmitted`, not delivered.
pub(super) fn send_authenticated_checked(
    socket: &UdpSocket,
    peer: SocketAddr,
    body: &[u8],
    crypto: &SharedMediaCrypto,
) -> std::io::Result<()> {
    if !crypto.is_established() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sealed handshake not established",
        ));
    }
    let Some(packet) = crypto.seal(body) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sealed frame construction failed",
        ));
    };
    socket.send_to(&packet, peer).map(|_| ())
}

pub(super) fn rate(current: u64, previous: u64, elapsed_ms: u64) -> u16 {
    let delta = current.saturating_sub(previous);
    ((u128::from(delta) * 1_000 / u128::from(elapsed_ms.max(1))).min(u128::from(u16::MAX))) as u16
}

pub(in crate::renderer::split_session) fn monotonic_ns() -> i64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    if result != 0 {
        return 0;
    }
    now.tv_sec
        .saturating_mul(1_000_000_000)
        .saturating_add(now.tv_nsec)
}

pub(super) fn release_window(window: usize) {
    if window != 0 {
        unsafe { ANativeWindow_release(window as *mut c_void) };
    }
}
