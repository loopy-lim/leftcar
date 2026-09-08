use super::*;

mod worker;

struct SingleRendererLaunch {
    instance_str: String,
    window_handle: usize,
    port: u16,
    expected_host: String,
    width: u32,
    height: u32,
    fps: u32,
    tcp_bridge: Option<MediaBridge>,
    tcp_control_addr: Option<std::net::SocketAddr>,
    prepared_receiver: Option<crate::prepared_udp::PreparedUdpReceiver>,
    control_clone: Arc<RendererControl>,
}
pub(crate) fn spawn_live_stream_renderer(
    instance_str: String,
    surface_window: *mut c_void,
    port: u16,
    expected_host: String,
    width: u32,
    height: u32,
    fps: u32,
    tcp_bridge: Option<MediaBridge>,
) {
    let paired_host = expected_host.clone();
    let tcp_control_addr = tcp_bridge.as_ref().map(MediaBridge::control_addr);
    let expected_host = if tcp_bridge.is_some() {
        // The TCP bridge injects datagrams from loopback; retain the paired
        // Host IP as the Wi-Fi candidate as well so auto mode can use either
        // path without weakening admission to the whole LAN.
        format!("{expected_host},127.0.0.1")
    } else {
        expected_host
    };
    let width = width.max(1);
    let height = height.max(1);
    let fps = fps.clamp(1, 90);
    log_info!(
        "spawn_live_stream_renderer: instance={} window={:?} port={} size={}x{} fps={} host={}",
        instance_str,
        surface_window,
        port,
        width,
        height,
        fps,
        expected_host
    );

    // A replacement Surface owns the same logical instance. Stop its suspended
    // renderer as EOF (not BYE), then evict any other renderer on the port.
    stop_live_stream_renderer(&instance_str, false);
    reclaim_udp_port(port);
    let prepared_receiver = take_prepared_receiver(port, &paired_host);

    let control = Arc::new(RendererControl {
        port,
        split: false,
        input: Mutex::new(InputScheduler::new(fps)),
        audio: Mutex::new(crate::audio_protocol::AudioRing::default()),
        input_enabled: AtomicI8::new(-1),
        rendered_frames: AtomicU64::new(0),
        stale_outputs: AtomicU64::new(0),
        stale_input_drops: AtomicU64::new(0),
        output_burst_discards: AtomicU64::new(0),
        decoder_input_drops: AtomicU64::new(0),
        frame_gaps: AtomicU64::new(0),
        last_feed_us: AtomicU64::new(0),
        network_rtt_ms: AtomicU64::new(LATENCY_UNKNOWN),
        capture_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
        encode_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
        wire_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
        capture_to_surface_release_ms: AtomicU64::new(LATENCY_UNKNOWN),
        resize_recovery_suppressed_until_us: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        suspend: AtomicBool::new(false),
        suspended: AtomicBool::new(false),
        send_bye: AtomicBool::new(true),
        finished: AtomicBool::new(false),
        termination_reason: AtomicI8::new(-1),
        cursor_active: AtomicI8::new(-1),
        cursor_x: AtomicU16::new(0),
        cursor_y: AtomicU16::new(0),
        cursor_visible: AtomicBool::new(false),
        cursor_sequence: AtomicU32::new(0),
        cursor_requested: AtomicBool::new(false),
    });
    let control_clone = Arc::clone(&control);

    // Publishing a replacement renderer and clearing an old retained reason
    // are one lifecycle transition, so an old worker cannot cache after this
    // generation is visible to termination polling.
    install_renderer(&instance_str, control);

    let window_handle = surface_window as usize;
    worker::spawn(SingleRendererLaunch {
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
    });
}
