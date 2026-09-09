use super::*;

pub(super) fn send_viewer_command(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    command: &[u8],
    token: &[u8],
) -> bool {
    if token.is_empty() {
        return false;
    }
    let authenticated = crate::media_datagram::frame_authenticated(command, token);
    if let Err(error) = socket.send_to(&authenticated, peer) {
        log_info!("failed to send viewer command: {error}");
        false
    } else {
        true
    }
}

pub(super) fn monotonic_us() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub(super) fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn send_latency_probe(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    token: &[u8],
    sequence: u32,
) -> bool {
    if token.is_empty() {
        return false;
    }
    let probe = encode_latency_probe(sequence, wall_clock_ms(), token);
    if let Err(error) = socket.send_to(&probe, peer) {
        log_info!("failed to send latency probe: {error}");
        false
    } else {
        true
    }
}

pub(super) fn feedback_latency_value(value: u64) -> u16 {
    if value == LATENCY_UNKNOWN {
        u16::MAX
    } else {
        value.min(u64::from(u16::MAX - 1)) as u16
    }
}

pub(super) fn send_receiver_feedback(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    token: &[u8],
    stats: &RendererStats,
    incomplete_aus: u64,
    control: &RendererControl,
    rendered_fps: u16,
) {
    if token.is_empty() {
        return;
    }
    let feedback = encode_receiver_feedback(
        ReceiverFeedback {
            frame_gaps: stats.frame_gaps.min(u64::from(u32::MAX)) as u32,
            input_drops: stats.input_drops.min(u64::from(u32::MAX)) as u32,
            incomplete_aus: incomplete_aus.min(u64::from(u32::MAX)) as u32,
            stale_frames: stats.stale_inputs.min(u64::from(u32::MAX)) as u32,
            network_rtt_ms: feedback_latency_value(control.network_rtt_ms.load(Ordering::Relaxed)),
            wire_to_decoder_ms: feedback_latency_value(
                control.wire_to_decoder_ms.load(Ordering::Relaxed),
            ),
            stale_input_drops: control
                .stale_input_drops
                .load(Ordering::Relaxed)
                .min(u64::from(u32::MAX)) as u32,
            output_burst_discards: control
                .output_burst_discards
                .load(Ordering::Relaxed)
                .min(u64::from(u32::MAX)) as u32,
            rendered_fps,
        },
        token,
    );
    if let Err(error) = socket.send_to(&feedback, peer) {
        log_info!("failed to send receiver feedback: {error}");
    }
}

pub(super) fn store_smoothed_latency(target: &AtomicU64, sample: u64) {
    let previous = target.load(Ordering::Relaxed);
    let next = if previous == LATENCY_UNKNOWN {
        sample
    } else {
        previous.saturating_mul(3).saturating_add(sample) / 4
    };
    target.store(next, Ordering::Relaxed);
}

/// Probe silence makes every clock-corrected value unmeasured. Reset
/// immediately: decaying a stale 100ms sample through 50/25/12ms would display
/// an apparent improvement that never happened and would mislead congestion
/// control for several more seconds.
pub(super) fn clear_stale_latency(control: &RendererControl) {
    for target in [
        &control.network_rtt_ms,
        &control.capture_to_decoder_ms,
        &control.encode_to_decoder_ms,
        &control.wire_to_decoder_ms,
        &control.capture_to_surface_release_ms,
    ] {
        target.store(LATENCY_UNKNOWN, Ordering::Relaxed);
    }
}

pub(super) fn clock_corrected_age_ms(
    host_wall_ms: Option<u64>,
    host_clock_offset_ms: Option<i128>,
) -> Option<u64> {
    let host_wall_ms = host_wall_ms?;
    let offset = host_clock_offset_ms?;
    let age = i128::from(wall_clock_ms()) - i128::from(host_wall_ms) + offset;
    (0..=60_000).contains(&age).then_some(age as u64)
}

pub(super) fn flush_input(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    token: &[u8],
    control: &RendererControl,
) {
    if token.is_empty() {
        return;
    }
    // Two candidates allow an immediately due reliable transition and the
    // newest coalesced pointer position to share one socket-loop tick.
    for _ in 0..2 {
        let outbound = control.input.lock().unwrap().next_ready(monotonic_us());
        let Some(outbound) = outbound else { break };
        let packet = encode_input(&outbound, token);
        if let Err(error) = socket.send_to(&packet, peer) {
            log_info!("failed to send input datagram: {error}");
            break;
        }
    }
}

pub(super) fn request_idr(socket: &std::net::UdpSocket, peer: std::net::SocketAddr, token: &[u8]) {
    send_viewer_command(socket, peer, crate::media_datagram::COMMAND_IDR, token);
}

pub(super) fn request_idr_debounced(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    token: &[u8],
    gate: &mut RecoveryRequestGate,
    control: &RendererControl,
) {
    if token.is_empty()
        || recovery_request_suppressed(
            monotonic_us(),
            control
                .resize_recovery_suppressed_until_us
                .load(Ordering::Relaxed),
        )
    {
        return;
    }
    if gate.should_request(std::time::Instant::now()) {
        request_idr(socket, peer, token);
    }
}
