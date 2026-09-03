use super::*;
use crate::cursor_protocol::{parse_cursor_sample, CursorSample};

pub(super) struct MediaBatch {
    pub(super) count: usize,
    pub(super) lengths: [usize; MEDIA_BATCH_SIZE],
    pub(super) peers: [libc::sockaddr_in; MEDIA_BATCH_SIZE],
}

pub(super) fn recv_media_batch(
    socket: &std::net::UdpSocket,
    buffers: &mut [[u8; MEDIA_DATAGRAM_BYTES]; MEDIA_BATCH_SIZE],
) -> std::io::Result<MediaBatch> {
    let mut peers: [libc::sockaddr_in; MEDIA_BATCH_SIZE] = unsafe { std::mem::zeroed() };
    let mut iovecs: [libc::iovec; MEDIA_BATCH_SIZE] = std::array::from_fn(|index| libc::iovec {
        iov_base: buffers[index].as_mut_ptr().cast(),
        iov_len: MEDIA_DATAGRAM_BYTES,
    });
    let mut messages: [libc::mmsghdr; MEDIA_BATCH_SIZE] =
        std::array::from_fn(|_| unsafe { std::mem::zeroed() });
    for index in 0..MEDIA_BATCH_SIZE {
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
            MEDIA_BATCH_SIZE as u32,
            libc::MSG_WAITFORONE,
            std::ptr::null_mut(),
        )
    };
    if count < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut lengths = [0usize; MEDIA_BATCH_SIZE];
    for index in 0..count as usize {
        lengths[index] = messages[index].msg_len as usize;
    }
    Ok(MediaBatch {
        count: count as usize,
        lengths,
        peers,
    })
}

pub(super) fn ipv4_socket_addr(raw: &libc::sockaddr_in) -> Option<std::net::SocketAddr> {
    if i32::from(raw.sin_family) != libc::AF_INET {
        return None;
    }
    let octets = raw.sin_addr.s_addr.to_ne_bytes();
    Some(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::from(octets),
        u16::from_be(raw.sin_port),
    )))
}

pub(super) fn sockaddr_in_for_addr(addr: std::net::SocketAddr) -> Option<libc::sockaddr_in> {
    let std::net::SocketAddr::V4(addr) = addr else {
        return None;
    };
    let mut raw: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    raw.sin_family = libc::AF_INET as libc::sa_family_t;
    raw.sin_port = addr.port().to_be();
    raw.sin_addr = libc::in_addr {
        s_addr: u32::from_ne_bytes(addr.ip().octets()),
    };
    Some(raw)
}

pub(super) type InputEndpoint =
    std::sync::Arc<std::sync::Mutex<Option<(std::net::SocketAddr, Vec<u8>)>>>;

pub(super) fn spawn_input_worker(
    socket: std::net::UdpSocket,
    endpoint: InputEndpoint,
    control: std::sync::Arc<RendererControl>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("leftcar-input".into())
        .spawn(move || {
            while !control.stop.load(Ordering::Relaxed) {
                let target = endpoint.lock().unwrap().clone();
                if let Some((peer, token)) = target {
                    flush_input(&socket, peer, &token, &control);
                }
                std::thread::park_timeout(std::time::Duration::from_millis(2));
            }
        })
        .expect("leftcar input worker must start")
}

pub(super) fn consume_viewer_response(
    packet: &[u8],
    token: &[u8],
    control_health: &mut ControlHealthState,
    control: &RendererControl,
    stats: &mut RendererStats,
) -> bool {
    if let Some(response) = parse_latency_probe_response(packet, token) {
        if control_health.probe_acknowledged(response.sequence) {
            if let Some(estimate) = estimate_latency(response, wall_clock_ms()) {
                store_smoothed_latency(&control.network_rtt_ms, estimate.network_rtt_ms);
                stats.host_clock_offset_ms = Some(estimate.host_clock_offset_ms);
                stats.last_probe_received = Some(std::time::Instant::now());
            }
        }
        return true;
    }
    if let Some(reason) = parse_termination(packet, token) {
        let code = match reason {
            crate::input_protocol::TerminationReason::HealthCheck => 1,
            crate::input_protocol::TerminationReason::HostForced => 2,
            crate::input_protocol::TerminationReason::HostStopped => 3,
        };
        log_info!(
            "host terminated stream: reason={} ({})",
            code,
            match reason {
                crate::input_protocol::TerminationReason::HealthCheck => "health check",
                crate::input_protocol::TerminationReason::HostForced => "forced stop",
                crate::input_protocol::TerminationReason::HostStopped => "stopped",
            }
        );
        control.record_termination_reason(code);
        // Do not send BYE: the host initiated this termination and already
        // tore its session down. Stop the loop so the Activity can observe
        // the reason and close the window.
        control.send_bye.store(false, Ordering::SeqCst);
        control.stop.store(true, Ordering::SeqCst);
        return true;
    }
    if let Some(ack) = parse_ack(packet, token) {
        control.input.lock().unwrap().acknowledge(ack.sequence);
        if let Some(enabled) = ack.enabled {
            control
                .input_enabled
                .store(if enabled { 1 } else { 0 }, Ordering::SeqCst);
        }
        return true;
    }
    if let Some(enabled) = parse_input_status(packet, token) {
        control
            .input_enabled
            .store(if enabled { 1 } else { 0 }, Ordering::SeqCst);
        return true;
    }
    if let Some(sample) = parse_cursor_sample(packet, token) {
        apply_cursor_sample(control, sample);
        return true;
    }
    false
}

fn apply_cursor_sample(control: &RendererControl, sample: CursorSample) {
    // UDP may reorder: a stale sample must never overwrite a newer one.
    // Sequence 0 is the fresh-session sentinel, so the first sample is
    // always accepted (host sequences start at 1).
    let current = control.cursor_sequence.load(Ordering::SeqCst);
    if current != 0 && sample.sequence <= current {
        return;
    }
    control.cursor_x.store(sample.x, Ordering::SeqCst);
    control.cursor_y.store(sample.y, Ordering::SeqCst);
    control.cursor_visible.store(sample.visible, Ordering::SeqCst);
    control.cursor_sequence.store(sample.sequence, Ordering::SeqCst);
    control.cursor_active.store(1, Ordering::SeqCst);
}

pub(super) fn configure_single_session_sockets(
    socket: &std::net::UdpSocket,
    control_socket: &std::net::UdpSocket,
) {
    // Media remains bounded to a short wait; input/probe traffic has a
    // separate non-blocking socket and receive queue.
    let _ = socket.set_read_timeout(Some(std::time::Duration::from_millis(2)));
    let _ = control_socket.set_nonblocking(true);
    let receive_buffer: libc::c_int = 512 * 1024;
    let media_tos: libc::c_int = 0x88;
    let control_tos: libc::c_int = 0xb8;
    unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &receive_buffer as *const _ as *const libc::c_void,
            std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
        );
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_TOS,
            &media_tos as *const _ as *const libc::c_void,
            std::mem::size_of_val(&media_tos) as libc::socklen_t,
        );
        libc::setsockopt(
            control_socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_TOS,
            &control_tos as *const _ as *const libc::c_void,
            std::mem::size_of_val(&control_tos) as libc::socklen_t,
        );
    }
}
