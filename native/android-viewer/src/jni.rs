//! JNI surface for the Kotlin shim (docs/05 §8.2 JNI 경계).
//!
//! Leftcar JNI rules (docs/07 §14):
//! - null/invalid jobject validated
//! - ANativeWindow acquire/release balanced
//! - exceptions checked/cleared, never leaked across the boundary
//! - panics never cross JNI (catch_unwind everywhere)

use std::ffi::{c_char, c_void, CStr};

use crate::input_protocol::{
    encode_input, encode_latency_probe, encode_receiver_feedback, estimate_latency,
    normalized_axis, parse_ack, parse_input_status, parse_latency_probe_response,
    parse_termination, InputEvent, InputScheduler, ReceiverFeedback,
};
use crate::media_datagram::{
    classify_frame_gap, parse_fragment, parse_parity, recovery_request_suppressed,
    select_live_edge_frames, should_feed_frame, stale_frame_budget_ms, stale_streak_advance,
    CompletedFrameSequencer, FecGroup, FrameFragment, FrameGapReason, FrameReassembler,
    ReassembledFrame, ReceiverPressure, RecoveryRequestGate, RestoredFragment, PARITY_MARKER,
};
use crate::net_guard::{host_is_valid, peer_allowed};
use crate::prepared_tcp::PreparedTcpBridge;
use crate::prepared_udp::PreparedUdpReceiver;
use crate::usb_bridge::UsbBridge;
use std::os::fd::AsRawFd;

extern "C" {
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
}

const LEFTCAR_OK: i32 = 0;
const LEFTCAR_ERR_NULL: i32 = 1;
const LEFTCAR_ERR_STATE: i32 = 2;
const LEFTCAR_ERR_PANIC: i32 = 3;
const LEFTCAR_ERR_INVALID: i32 = 4;

type StatePtr = *mut viewer_core::ProcessState;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const c_char, fmt: *const c_char, ...) -> i32;
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
        if let Ok(c_msg) = std::ffi::CString::new(msg) {
            let tag = b"LeftcarNative\0";
            let fmt = b"%s\0";
            unsafe {
                __android_log_print(
                    4, // ANDROID_LOG_INFO
                    tag.as_ptr() as *const c_char,
                    fmt.as_ptr() as *const c_char,
                    c_msg.as_ptr(),
                );
            }
        }
    };
}

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct RendererControl {
    port: u16,
    input: Mutex<InputScheduler>,
    // -1 = waiting for authenticated Host state, 0 = locked, 1 = enabled.
    input_enabled: AtomicI8,
    rendered_frames: AtomicU64,
    stale_outputs: AtomicU64,
    stale_input_drops: AtomicU64,
    output_burst_discards: AtomicU64,
    decoder_input_drops: AtomicU64,
    frame_gaps: AtomicU64,
    last_feed_us: AtomicU64,
    network_rtt_ms: AtomicU64,
    capture_to_decoder_ms: AtomicU64,
    encode_to_decoder_ms: AtomicU64,
    wire_to_decoder_ms: AtomicU64,
    // Glass-to-glass: capture wall time of the newest rendered frame minus
    // the moment MediaCodec released it to the Surface. Includes the decoder
    // output wait the to-decoder estimates omit, which is the gap users feel.
    capture_to_render_ms: AtomicU64,
    // Repeated SurfaceView geometry updates during freeform resize can make
    // decoder/compositor stalls look like packet loss. Defer recovery IDRs
    // until the geometry has stayed stable, then emit at most one through the
    // existing RecoveryRequestGate.
    resize_recovery_suppressed_until_us: AtomicU64,
    stop: AtomicBool,
    // Surface destruction is not always the end of the Activity. During
    // freeform resize, release MediaCodec's ANativeWindow promptly but keep
    // the UDP listener alive until either a replacement Surface attaches or
    // the Activity performs its final release.
    suspend: AtomicBool,
    suspended: AtomicBool,
    // A surface can disappear briefly during a freeform resize/reconfiguration.
    // In that case the host must see EOF and use its existing reconnect path,
    // rather than receiving BYE and permanently stopping capture.
    send_bye: AtomicBool,
    // SurfaceHolder.surfaceDestroyed must not return while MediaCodec still
    // owns the ANativeWindow. The callback waits on this bounded flag before
    // releasing the native window reference.
    finished: AtomicBool,
    // Termination reason code received from the host (LCT1): 1 = feedback
    // health check, 2 = host operator forced stop, 3 = ordinary stop.
    // Negative means no notice arrived.
    termination_reason: AtomicI8,
}

impl RendererControl {
    fn termination_reason(&self) -> i8 {
        self.termination_reason.load(Ordering::SeqCst)
    }
}

static ACTIVE_RENDERERS: Mutex<Option<HashMap<String, Arc<RendererControl>>>> = Mutex::new(None);
static PREPARED_RECEIVERS: Mutex<Option<HashMap<u16, PreparedUdpReceiver>>> = Mutex::new(None);
static PREPARED_TCP_BRIDGES: Mutex<Option<HashMap<u16, PreparedTcpBridge>>> = Mutex::new(None);
static PREPARED_USB_BRIDGE: Mutex<Option<UsbBridge>> = Mutex::new(None);
/// A renderer exits within milliseconds after an LCT1 packet, while the
/// Activity polls every 250ms. Retain the reason by logical instance so the
/// UI cannot miss it between renderer cleanup and the next poll.
static TERMINATION_REASONS: Mutex<Option<HashMap<String, i8>>> = Mutex::new(None);

enum MediaBridge {
    Tcp(PreparedTcpBridge),
    Usb(UsbBridge),
}

impl MediaBridge {
    fn control_addr(&self) -> std::net::SocketAddr {
        match self {
            Self::Tcp(bridge) => bridge.control_addr(),
            Self::Usb(bridge) => bridge.control_addr(),
        }
    }

    fn drain_media(&self) {
        match self {
            Self::Tcp(bridge) => bridge.drain_media(),
            Self::Usb(bridge) => bridge.drain_media(),
        }
    }

    fn recv_media_timeout(&self, timeout: std::time::Duration) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            Self::Tcp(bridge) => bridge.recv_media_timeout(timeout),
            Self::Usb(bridge) => bridge.recv_media_timeout(timeout),
        }
    }
}

fn remove_renderer_if_current(instance: &str, control: &Arc<RendererControl>) {
    let removed_reason = {
        let mut map = ACTIVE_RENDERERS.lock().unwrap();
        let Some(map) = map.as_mut() else {
            return;
        };
        let is_current = map
            .get(instance)
            .map(|current| Arc::ptr_eq(current, control))
            .unwrap_or(false);
        if !is_current {
            return;
        }
        map.remove(instance);
        let reason = control.termination_reason();
        (reason >= 0).then_some(reason)
    };
    if let Some(reason) = removed_reason {
        TERMINATION_REASONS
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(instance.to_owned(), reason);
    }
}

fn clear_cached_termination(instance: &str) {
    if let Some(reasons) = TERMINATION_REASONS.lock().unwrap().as_mut() {
        reasons.remove(instance);
    }
}

fn wait_for_renderer(control: &RendererControl) {
    // Accepted socket reads are bounded to 300 ms. Leave additional margin
    // for MediaCodec_stop/delete without hanging the Android UI indefinitely.
    for _ in 0..40 {
        if control.finished.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Stop any renderer still holding the UDP port and wait (bounded) for its
/// thread to release the socket. Without this, a re-attached surface races
/// the old thread and `bind` fails with Address-in-use.
fn reclaim_udp_port(port: u16) {
    let running: Vec<Arc<RendererControl>> = {
        let mut map = ACTIVE_RENDERERS.lock().unwrap();
        map.get_or_insert_with(HashMap::new)
            .values()
            .filter(|control| control.port == port)
            .cloned()
            .collect()
    };
    for control in running {
        control.send_bye.store(false, Ordering::SeqCst);
        control.suspend.store(false, Ordering::SeqCst);
        control.stop.store(true, Ordering::SeqCst);
        wait_for_renderer(&control);
    }
}

fn cancel_prepared_receiver(port: u16) -> bool {
    let prepared = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    prepared.is_some()
}

fn prepare_tcp_bridge(port: u16, expected_host: &str, transport: &str) -> Result<(), String> {
    let (bind_host, allowed_hosts) = if matches!(transport, "tcp" | "auto") {
        // Auto must be able to accept the direct Wi-Fi attempt first and the
        // loopback ADB fallback later. Admission is still restricted to the
        // paired Host address plus loopback; binding broadly does not broaden
        // the authenticated peer set.
        ("0.0.0.0", format!("{expected_host},127.0.0.1"))
    } else {
        ("127.0.0.1", "127.0.0.1".to_owned())
    };
    let bridge = PreparedTcpBridge::bind(port, bind_host, &allowed_hosts).map_err(|error| {
        format!("failed to prepare {transport} TCP media bridge on {bind_host}:{port}: {error}")
    })?;
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(port, bridge);
    Ok(())
}

fn cancel_tcp_bridge(port: u16) -> bool {
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port)
        .is_some()
}

fn take_tcp_bridge(port: u16) -> Option<PreparedTcpBridge> {
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port)
}

fn take_media_bridge(port: u16) -> Option<MediaBridge> {
    take_tcp_bridge(port).map(MediaBridge::Tcp).or_else(|| {
        PREPARED_USB_BRIDGE
            .lock()
            .unwrap()
            .take()
            .map(MediaBridge::Usb)
    })
}

fn prepare_udp_receiver(port: u16, expected_host: &str, transport: &str) -> Result<(), String> {
    if port == 0 || !host_is_valid(expected_host) {
        return Err("invalid prepared media port or host".into());
    }

    // A Host restart does not send a terminal packet to an existing UDP
    // renderer. The old Activity therefore keeps the media port and its
    // decoder alive while the control-plane recovery tries to prepare the
    // same port again. Reclaim that logical stream before binding the
    // replacement preflight listener; the subsequent Activity recreation
    // will attach a fresh renderer to the new Host session.
    reclaim_udp_port(port);

    let active_port = ACTIVE_RENDERERS
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|renderers| renderers.values().any(|control| control.port == port));
    if active_port {
        return Err(format!("UDP media port {port} is already active"));
    }

    // A retry for the same not-yet-opened window replaces both preflight
    // listeners, not only the UDP half.
    let _ = cancel_tcp_bridge(port);

    // A retry for the same not-yet-opened window replaces its old preflight.
    // Drop outside the map lock because the worker has a bounded join.
    let stale = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    drop(stale);

    let prepared_hosts = if matches!(transport, "tcp" | "adbTcp" | "usb" | "auto") {
        format!("{expected_host},127.0.0.1")
    } else {
        expected_host.to_owned()
    };
    let prepared = PreparedUdpReceiver::bind(port, prepared_hosts)
        .map_err(|error| format!("failed to prepare UDP media port {port}: {error}"))?;
    PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(port, prepared);
    if matches!(transport, "tcp" | "adbTcp" | "auto") {
        if let Err(error) = prepare_tcp_bridge(port, expected_host, transport) {
            let _ = cancel_prepared_receiver(port);
            return Err(error);
        }
    }
    Ok(())
}

fn take_prepared_receiver(port: u16, expected_host: &str) -> Option<PreparedUdpReceiver> {
    let prepared = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    match prepared {
        Some(prepared)
            if prepared
                .expected_host()
                .split(',')
                .map(str::trim)
                .any(|host| host == expected_host) =>
        {
            Some(prepared)
        }
        Some(_) => {
            log_info!("discarded prepared UDP port {port}: paired Host changed");
            None
        }
        None => None,
    }
}

#[derive(Clone)]
struct FramePacket {
    id: u16,
    au: Vec<u8>,
    capture_wall_ms: Option<u64>,
    encode_wall_ms: Option<u64>,
    send_wall_ms: Option<u64>,
}

fn queue_reassembled_frame(
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>],
    completed_count: &mut usize,
    peer: std::net::SocketAddr,
    reassembled: ReassembledFrame,
) {
    for completed in frame_sequencer.push(reassembled) {
        let frame = FramePacket {
            id: completed.id,
            au: completed.au,
            capture_wall_ms: completed.capture_wall_ms,
            encode_wall_ms: completed.encode_wall_ms,
            send_wall_ms: Some(completed.send_wall_ms),
        };
        if *completed_count < completed_frames.len() {
            completed_frames[*completed_count] = Some((peer, frame));
            *completed_count += 1;
        }
    }
}

fn queue_restored_fragments(
    restored: Option<Vec<RestoredFragment>>,
    reassembler: &mut FrameReassembler,
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>],
    completed_count: &mut usize,
    peer: std::net::SocketAddr,
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
        if let Some(reassembled) = reassembler.push(fragment) {
            queue_reassembled_frame(
                frame_sequencer,
                completed_frames,
                completed_count,
                peer,
                reassembled,
            );
        }
    }
}

#[derive(Default)]
struct RendererStats {
    queued: u64,
    input_drops: u64,
    frame_gaps: u64,
    intentional_live_edge_gaps: u64,
    recovery_skipped_frames: u64,
    max_feed_us: u64,
    stale_inputs: u64,
    completed_batch: usize,
    live_edge_batch: usize,
    max_completed_batch: usize,
    pressure: ReceiverPressure,
    consecutive_stale: u32,
    // NTP-style authenticated probes estimate Host clock minus Android clock.
    // Do not infer this from the first video frame: that would erase the very
    // one-way delivery latency the HUD is intended to show.
    host_clock_offset_ms: Option<i128>,
    // When the last latency-probe response arrived. Silence here means the
    // RTT/stage estimates are stale and the HUD must show "unmeasured"
    // instead of a frozen number.
    last_probe_received: Option<std::time::Instant>,
}

const LATENCY_UNKNOWN: u64 = u64::MAX;
const LATENCY_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const RESIZE_RECOVERY_SUPPRESSION_US: u64 = 350_000;
/// Probe responses arriving after this silence window leave the smoothed
/// latency estimates stale; decay them toward "unknown" instead of freezing.
const PROBE_STALE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
fn is_keyframe(au: &[u8], codec: viewer_decoder::VideoCodec) -> bool {
    viewer_decoder::split_annexb(au)
        .iter()
        .any(|nal| match codec {
            viewer_decoder::VideoCodec::H264 => {
                viewer_decoder::nal_type(nal.bytes) == Some(viewer_decoder::NAL_IDR)
            }
            viewer_decoder::VideoCodec::Hevc => viewer_decoder::hevc_nal_type(nal.bytes)
                .is_some_and(|nal_type| matches!(nal_type, 19..=21)),
        })
}

fn reset_decoder(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    codec_config: &mut Option<viewer_decoder::CodecConfig>,
    awaiting_keyframe: &mut bool,
) {
    if let Some(d) = decoder.as_mut() {
        d.stop();
    }
    *decoder = None;
    *codec_config = None;
    *awaiting_keyframe = true;
}

/// A missing encoded AU invalidates the reference chain of subsequent delta
/// frames. Keep the MediaCodec instance running, but wait for the next IDR keyframe
/// before feeding additional frames to the decoder.
fn resync_decoder_after_frame_gap(awaiting_keyframe: &mut bool) {
    *awaiting_keyframe = true;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedOutcome {
    Queued,
    ResyncRequired,
    FatalError,
}

fn send_viewer_command(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    command: &[u8],
    token: &[u8],
) {
    if token.is_empty() {
        return;
    }
    let mut authenticated = Vec::with_capacity(command.len() + token.len());
    authenticated.extend_from_slice(command);
    authenticated.extend_from_slice(token);
    if let Err(error) = socket.send_to(&authenticated, peer) {
        log_info!("failed to send viewer command: {error}");
    }
}

fn monotonic_us() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_millis() as u64)
        .unwrap_or(0)
}

fn send_latency_probe(
    socket: &std::net::UdpSocket,
    peer: std::net::SocketAddr,
    token: &[u8],
    sequence: u32,
) {
    if token.is_empty() {
        return;
    }
    let probe = encode_latency_probe(sequence, wall_clock_ms(), token);
    if let Err(error) = socket.send_to(&probe, peer) {
        log_info!("failed to send latency probe: {error}");
    }
}

fn feedback_latency_value(value: u64) -> u16 {
    if value == LATENCY_UNKNOWN {
        u16::MAX
    } else {
        value.min(u64::from(u16::MAX - 1)) as u16
    }
}

fn send_receiver_feedback(
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

fn store_smoothed_latency(target: &AtomicU64, sample: u64) {
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
fn clear_stale_latency(control: &RendererControl) {
    for target in [
        &control.network_rtt_ms,
        &control.capture_to_decoder_ms,
        &control.encode_to_decoder_ms,
        &control.wire_to_decoder_ms,
        &control.capture_to_render_ms,
    ] {
        target.store(LATENCY_UNKNOWN, Ordering::Relaxed);
    }
}

fn clock_corrected_age_ms(
    host_wall_ms: Option<u64>,
    host_clock_offset_ms: Option<i128>,
) -> Option<u64> {
    let host_wall_ms = host_wall_ms?;
    let offset = host_clock_offset_ms?;
    let age = i128::from(wall_clock_ms()) - i128::from(host_wall_ms) + offset;
    (0..=60_000).contains(&age).then_some(age as u64)
}

fn flush_input(
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

fn request_idr(socket: &std::net::UdpSocket, peer: std::net::SocketAddr, token: &[u8]) {
    send_viewer_command(socket, peer, b"IDR", token);
}

fn request_idr_debounced(
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

fn suppress_resize_recovery(instance_str: &str) {
    let control = ACTIVE_RENDERERS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(instance_str).cloned());
    if let Some(control) = control {
        control.resize_recovery_suppressed_until_us.fetch_max(
            monotonic_us().saturating_add(RESIZE_RECOVERY_SUPPRESSION_US),
            Ordering::Relaxed,
        );
    }
}

const MEDIA_BATCH_SIZE: usize = 16;
const MEDIA_DATAGRAM_BYTES: usize = 2_048;
/// Media is disposable. Waiting for a codec slot or output buffer would make
/// every newer frame arrive behind an older one, so the hot path is strictly
/// non-blocking and recovers from a missed AU at the next IDR.
const DECODER_FEED_TIMEOUT_US: i64 = 0;

struct MediaBatch {
    count: usize,
    lengths: [usize; MEDIA_BATCH_SIZE],
    peers: [libc::sockaddr_in; MEDIA_BATCH_SIZE],
}

fn recv_media_batch(
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

fn ipv4_socket_addr(raw: &libc::sockaddr_in) -> Option<std::net::SocketAddr> {
    if i32::from(raw.sin_family) != libc::AF_INET {
        return None;
    }
    let octets = raw.sin_addr.s_addr.to_ne_bytes();
    Some(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::from(octets),
        u16::from_be(raw.sin_port),
    )))
}

fn sockaddr_in_for_addr(addr: std::net::SocketAddr) -> Option<libc::sockaddr_in> {
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

type InputEndpoint = std::sync::Arc<std::sync::Mutex<Option<(std::net::SocketAddr, Vec<u8>)>>>;

fn spawn_input_worker(
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

fn consume_viewer_response(
    packet: &[u8],
    token: &[u8],
    latency_probe_sequence: u32,
    control: &RendererControl,
    stats: &mut RendererStats,
) -> bool {
    if let Some(response) = parse_latency_probe_response(packet, token) {
        if response.sequence == latency_probe_sequence {
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
        control.termination_reason.store(code, Ordering::SeqCst);
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
    false
}

fn feed_and_render(
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
    // Glass-to-glass: when this input produced a render, the frame that hit
    // the Surface is the one these timestamps describe (the decoder drains
    // stale outputs without rendering them). Record the render instant so the
    // HUD can show the latency users actually perceive.
    if dec.frames_rendered > rendered_before {
        if let Some(age) = capture_age_ms {
            store_smoothed_latency(&control.capture_to_render_ms, age);
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
    control
        .rendered_frames
        .store(dec.frames_rendered, Ordering::Relaxed);
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

fn spawn_live_stream_renderer(
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
    // A reused port/instance belongs to a new host session; do not leak the
    // prior Activity's terminal reason into it.
    clear_cached_termination(&instance_str);
    let prepared_receiver = take_prepared_receiver(port, &paired_host);

    let control = Arc::new(RendererControl {
        port,
        input: Mutex::new(InputScheduler::new(fps)),
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
        capture_to_render_ms: AtomicU64::new(LATENCY_UNKNOWN),
        resize_recovery_suppressed_until_us: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        suspend: AtomicBool::new(false),
        suspended: AtomicBool::new(false),
        send_bye: AtomicBool::new(true),
        finished: AtomicBool::new(false),
        termination_reason: AtomicI8::new(-1),
    });
    let control_clone = Arc::clone(&control);

    {
        let mut map = ACTIVE_RENDERERS.lock().unwrap();
        let map = map.get_or_insert_with(HashMap::new);
        if let Some(old_control) = map.insert(instance_str.clone(), control) {
            old_control.stop.store(true, Ordering::SeqCst);
        }
    }

    let window_handle = surface_window as usize;
    std::thread::spawn(move || {
        // Keep the TCP bridge alive for the lifetime of the renderer. It is
        // intentionally independent from Surface creation so Host can finish
        // the TCP reachability proof before the Activity attaches.
        let tcp_bridge = tcp_bridge;
        let (socket, prepared_token) = match prepared_receiver {
            Some(prepared) => match prepared.into_socket_and_token() {
                Ok(parts) => {
                    log_info!("claimed prepared UDP listener on port {port}");
                    parts
                }
                Err(error) => {
                    log_info!("FAILED to claim prepared UDP port {port}: {error}");
                    control_clone.finished.store(true, Ordering::SeqCst);
                    remove_renderer_if_current(&instance_str, &control_clone);
                    return;
                }
            },
            None => match std::net::UdpSocket::bind(format!("0.0.0.0:{port}")) {
                Ok(socket) => (socket, Vec::new()),
                Err(e) => {
                    log_info!("FAILED to bind UDP listener on 0.0.0.0:{}: {}", port, e);
                    control_clone.finished.store(true, Ordering::SeqCst);
                    remove_renderer_if_current(&instance_str, &control_clone);
                    return;
                }
            },
        };
        let control_socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(socket) => socket,
            Err(error) => {
                log_info!("FAILED to bind dedicated UDP control socket: {error}");
                control_clone.finished.store(true, Ordering::SeqCst);
                remove_renderer_if_current(&instance_str, &control_clone);
                return;
            }
        };
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
            if !prepared_token.is_empty() && recovery_gate.should_request(std::time::Instant::now())
            {
                request_idr(&control_socket, peer, &prepared_token);
                log_info!("requested initial IDR through TCP media bridge at {peer}");
            }
        }
        let mut buf = vec![0u8; 2_048];
        let mut media_buffers = [[0u8; MEDIA_DATAGRAM_BYTES]; MEDIA_BATCH_SIZE];
        let mut control_buf = vec![0u8; 512];
        let mut host_peer: Option<std::net::SocketAddr> = None;
        let mut viewer_control_token = prepared_token;
        let input_endpoint: InputEndpoint = std::sync::Arc::new(std::sync::Mutex::new(None));
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
        let mut codec_config: Option<viewer_decoder::CodecConfig> = None;
        let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
        let mut aus = 0u64;
        let mut awaiting_keyframe = true;
        let mut last_frame_id: Option<u16> = None;
        let mut renderer_stats = RendererStats::default();
        let mut latency_probe_sequence = 0u32;
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
                last_frame_id = None;
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
                if !viewer_control_token.is_empty()
                    && last_latency_probe.elapsed() >= LATENCY_PROBE_INTERVAL
                {
                    latency_probe_sequence = latency_probe_sequence.wrapping_add(1);
                    send_latency_probe(
                        &control_socket,
                        peer,
                        &viewer_control_token,
                        latency_probe_sequence,
                    );
                    let rendered_frames = control_clone.rendered_frames.load(Ordering::Relaxed);
                    let rendered_fps = rendered_frames
                        .saturating_sub(last_feedback_rendered_frames)
                        .min(u64::from(u16::MAX)) as u16;
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
                    last_latency_probe = std::time::Instant::now();
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
                loop {
                    match control_socket.recv_from(&mut control_buf) {
                        Ok((received, source)) if source == peer => {
                            let _ = consume_viewer_response(
                                &control_buf[..received],
                                &viewer_control_token,
                                latency_probe_sequence,
                                &control_clone,
                                &mut renderer_stats,
                            );
                        }
                        Ok((_received, source)) => {
                            log_info!("rejected control datagram from unexpected peer {source}");
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => {
                            log_info!("UDP control receive error: {error}");
                            break;
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
                    Ok(None) => continue,
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
                    log_info!("TCP media frame arrived without a control endpoint");
                    continue;
                };
                let Some(peer) = sockaddr_in_for_addr(peer) else {
                    log_info!("TCP media endpoint is not IPv4: {peer}");
                    continue;
                };
                media_buffers[0][..packet.len()].copy_from_slice(&packet);
                let mut lengths = [0usize; MEDIA_BATCH_SIZE];
                lengths[0] = packet.len();
                let mut peers: [libc::sockaddr_in; MEDIA_BATCH_SIZE] =
                    unsafe { std::mem::zeroed() };
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
            let mut completed_frames: [Option<(std::net::SocketAddr, FramePacket)>;
                MEDIA_BATCH_SIZE] = std::array::from_fn(|_| None);
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
                    reset_decoder(&mut decoder, &mut codec_config, &mut awaiting_keyframe);
                    last_frame_id = None;
                    aus = 0;
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
                        .capture_to_render_ms
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
                    *input_endpoint.lock().unwrap() = Some((peer, viewer_control_token.clone()));
                    control_clone.input.lock().unwrap().reset_session();
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
                    continue;
                }
                // Keep accepting responses on the legacy media socket during a
                // rolling Host/Viewer upgrade.
                if consume_viewer_response(
                    packet,
                    &viewer_control_token,
                    latency_probe_sequence,
                    &control_clone,
                    &mut renderer_stats,
                ) {
                    continue;
                }
                if packet.first() == Some(&PARITY_MARKER) {
                    let Some(parity) = parse_parity(packet) else {
                        continue;
                    };
                    let key = (parity.id, parity.base);
                    if !fec_groups.contains_key(&key) {
                        while fec_groups.len() >= 4 {
                            let Some(oldest) = fec_group_order.pop_front() else {
                                break;
                            };
                            fec_groups.remove(&oldest);
                        }
                        let Some(group) =
                            FecGroup::new(parity.id, parity.k, parity.base, parity.total)
                        else {
                            continue;
                        };
                        fec_groups.insert(key, group);
                        fec_group_order.push_back(key);
                    }
                    let mut restored = None;
                    let mut complete = false;
                    if let Some(group) = fec_groups.get_mut(&key) {
                        restored = group.push_parity_and_restore(parity);
                        complete = group.is_complete();
                    }
                    if let Some(restored) = restored.as_ref() {
                        renderer_stats.pressure.record_fec_recovery(restored.len());
                    }
                    queue_restored_fragments(
                        restored,
                        &mut reassembler,
                        &mut frame_sequencer,
                        &mut completed_frames,
                        &mut completed_count,
                        peer,
                    );
                    if complete {
                        fec_groups.remove(&key);
                        fec_group_order.retain(|queued| *queued != key);
                    }
                    continue;
                }
                if packet.starts_with(b"CFG") || packet.starts_with(b"CF2") {
                    let marker = if packet.starts_with(b"CF2") {
                        "CF2"
                    } else {
                        "CFG"
                    };
                    log_info!("Received {} datagram ({} bytes)", marker, packet.len());
                    let Some(config) = viewer_decoder::parse_codec_config(packet) else {
                        log_info!("Ignoring malformed {} codec configuration", marker);
                        awaiting_keyframe = true;
                        continue;
                    };
                    if decoder.is_none() {
                        let sps = config.sps.as_deref().expect("complete codec config SPS");
                        let pps = config.pps.as_deref().expect("complete codec config PPS");
                        let codec_name = match config.codec {
                            viewer_decoder::VideoCodec::H264 => "c2.qti.avc.decoder.low_latency",
                            viewer_decoder::VideoCodec::Hevc => "c2.qti.hevc.decoder.low_latency",
                        };
                        log_info!(
                            "Creating {:?} AndroidDecoder with Surface window=0x{:x} vps={}B sps={}B pps={}B",
                            config.codec,
                            window_handle,
                            config.vps.as_ref().map_or(0, Vec::len),
                            sps.len(),
                            pps.len()
                        );
                        let created = unsafe {
                            viewer_decoder::AndroidDecoder::new_video_named(
                                viewer_decoder::VideoDecoderConfig {
                                    codec: config.codec,
                                    vps: config.vps.as_deref(),
                                    sps,
                                    pps,
                                    width,
                                    height,
                                    window: window_handle,
                                    fps,
                                    codec_name: Some(codec_name),
                                },
                            )
                        };
                        match created {
                            Ok(d) => {
                                log_info!(
                                    "AndroidDecoder created successfully: codec={:?} actualCodec={}",
                                    config.codec,
                                    d.codec_name()
                                );
                                decoder = Some(d);
                                codec_config = Some(config);
                            }
                            Err(e) => {
                                log_info!("AndroidDecoder creation FAILED: {}", e);
                            }
                        }
                    }
                    // CFG/CF2 is emitted with an IDR on the host. Do not feed
                    // delta frames until that recovery keyframe arrives.
                    awaiting_keyframe = true;
                    continue;
                }

                let Some(fragment) = parse_fragment(packet) else {
                    continue;
                };
                let group_base = (fragment.index / 8) * 8;
                let group_k = (fragment.count - group_base).min(8);
                let group_key = (fragment.id, group_base);
                if group_k > 1 && !fec_groups.contains_key(&group_key) {
                    while fec_groups.len() >= 4 {
                        let Some(oldest) = fec_group_order.pop_front() else {
                            break;
                        };
                        fec_groups.remove(&oldest);
                    }
                    if let Some(group) =
                        FecGroup::new(fragment.id, group_k as u8, group_base, fragment.count)
                    {
                        fec_groups.insert(group_key, group);
                        fec_group_order.push_back(group_key);
                    }
                }
                let mut restored = None;
                if let Some(group) = fec_groups.get_mut(&group_key) {
                    restored = group.push_data_and_restore(fragment);
                    if group.is_complete() {
                        fec_groups.remove(&group_key);
                        fec_group_order.retain(|queued| *queued != group_key);
                    }
                }
                if let Some(restored) = restored.as_ref() {
                    renderer_stats.pressure.record_fec_recovery(restored.len());
                }
                queue_restored_fragments(
                    restored,
                    &mut reassembler,
                    &mut frame_sequencer,
                    &mut completed_frames,
                    &mut completed_count,
                    peer,
                );
                let Some(reassembled) = reassembler.push(fragment) else {
                    continue;
                };
                queue_reassembled_frame(
                    &mut frame_sequencer,
                    &mut completed_frames,
                    &mut completed_count,
                    peer,
                    reassembled,
                );
            }

            let completed_frames = completed_frames
                .into_iter()
                .take(completed_count)
                .flatten()
                .collect::<Vec<_>>();
            let completed_batch = completed_frames.len();
            let selection = select_live_edge_frames(completed_frames);
            renderer_stats.completed_batch = completed_batch;
            renderer_stats.live_edge_batch = selection.frames.len();
            renderer_stats.max_completed_batch =
                renderer_stats.max_completed_batch.max(completed_batch);
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
                let keyframe = is_keyframe(&frame.au, codec);
                let capture_age_ms = clock_corrected_age_ms(
                    frame.capture_wall_ms,
                    renderer_stats.host_clock_offset_ms,
                );
                let rtt_ms = control_clone.network_rtt_ms.load(Ordering::Relaxed);
                let stale_budget_ms =
                    stale_frame_budget_ms((rtt_ms != LATENCY_UNKNOWN).then_some(rtt_ms));
                // A single tail-latency delta should render late instead of
                // invalidating the whole reference chain. Enter recovery only
                // after three consecutive over-budget deltas.
                let over_budget =
                    !keyframe && capture_age_ms.is_some_and(|age| age > stale_budget_ms);
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
                    last_frame_id = None;
                    resync_decoder_after_frame_gap(&mut awaiting_keyframe);
                    request_idr_debounced(
                        &control_socket,
                        peer,
                        &viewer_control_token,
                        &mut recovery_gate,
                        &control_clone,
                    );
                    continue;
                }

                {
                    let gap_reason = classify_frame_gap(
                        last_frame_id,
                        frame.id,
                        frame_was_intentionally_discarded,
                        awaiting_keyframe,
                    );
                    last_frame_id = Some(frame.id);

                    if keyframe {
                        log_info!(
                            "Received IDR access unit id={} gapReason={:?} awaitingKeyframe={}",
                            frame.id,
                            gap_reason,
                            awaiting_keyframe
                        );
                    }

                    match gap_reason {
                        FrameGapReason::None => {}
                        FrameGapReason::NetworkLoss { missing } => {
                            renderer_stats.frame_gaps += 1;
                            control_clone
                                .frame_gaps
                                .store(renderer_stats.frame_gaps, Ordering::Relaxed);
                            log_info!(
                                "UDP access-unit gap detected at id={} reason=networkLoss missing={} hardRecovery={}",
                                frame.id,
                                missing,
                                !keyframe && missing >= 2
                            );
                            if keyframe {
                                awaiting_keyframe = false;
                            } else if missing >= 2 {
                                // A single missing AU can often be concealed
                                // by MediaCodec. Do not turn every isolated
                                // Wi-Fi loss into a keyframe burst; require a
                                // larger hole or an actual decoder rejection.
                                resync_decoder_after_frame_gap(&mut awaiting_keyframe);
                                request_idr_debounced(
                                    &control_socket,
                                    peer,
                                    &viewer_control_token,
                                    &mut recovery_gate,
                                    &control_clone,
                                );
                            }
                        }
                        FrameGapReason::LiveEdgeDiscard { missing } => {
                            renderer_stats.intentional_live_edge_gaps += 1;
                            log_info!(
                                "UDP access-unit gap detected at id={} reason=liveEdgeDiscard missing={} awaitingKeyframe={}",
                                frame.id,
                                missing,
                                awaiting_keyframe
                            );
                            if keyframe {
                                awaiting_keyframe = false;
                            } else if !awaiting_keyframe {
                                // The selected delta may depend on one of the
                                // AUs intentionally discarded by the
                                // live-edge policy. Start one coalesced
                                // recovery boundary; the outer gate prevents
                                // another IDR request for this same episode.
                                resync_decoder_after_frame_gap(&mut awaiting_keyframe);
                                request_idr_debounced(
                                    &control_socket,
                                    peer,
                                    &viewer_control_token,
                                    &mut recovery_gate,
                                    &control_clone,
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
                                awaiting_keyframe = false;
                            }
                        }
                    }

                    if should_feed_frame(gap_reason, keyframe) {
                        if let Some(dec) = decoder.as_mut() {
                            if !(awaiting_keyframe && !keyframe) {
                                match feed_and_render(
                                    dec,
                                    &frame,
                                    &mut aus,
                                    fps,
                                    &mut renderer_stats,
                                    &control_clone,
                                ) {
                                    FeedOutcome::Queued => {
                                        awaiting_keyframe = false;
                                        if keyframe {
                                            recovery_gate.recovered();
                                        }
                                    }
                                    FeedOutcome::ResyncRequired => {
                                        resync_decoder_after_frame_gap(&mut awaiting_keyframe);
                                        request_idr_debounced(
                                            &control_socket,
                                            peer,
                                            &viewer_control_token,
                                            &mut recovery_gate,
                                            &control_clone,
                                        );
                                    }
                                    FeedOutcome::FatalError => {
                                        reset_decoder(
                                            &mut decoder,
                                            &mut codec_config,
                                            &mut awaiting_keyframe,
                                        );
                                        request_idr_debounced(
                                            &control_socket,
                                            peer,
                                            &viewer_control_token,
                                            &mut recovery_gate,
                                            &control_clone,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
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
        // Publish `finished` only after persisting an LCT1 reason; callers
        // waiting to reuse this instance/port may clear the cache as soon as
        // they observe completion.
        control_clone.finished.store(true, Ordering::SeqCst);
        log_info!("Live stream renderer exiting for instance");
    });
}

fn suspend_live_stream_renderer(instance_str: &str) {
    let control = ACTIVE_RENDERERS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(instance_str).cloned());
    if let Some(control) = control {
        control.suspend.store(true, Ordering::SeqCst);
        // The socket read timeout is 100 ms. Wait until the decoder has been
        // dropped before releasing the native window reference.
        for _ in 0..40 {
            if control.suspended.load(Ordering::SeqCst) || control.finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

fn stop_live_stream_renderer(instance_str: &str, send_bye: bool) {
    let control = ACTIVE_RENDERERS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(instance_str).cloned());
    if let Some(control) = control {
        // Activity.onDestroy follows a host-initiated finish. Preserve the
        // renderer's earlier decision not to send BYE back to a host that has
        // already torn the session down.
        let should_send_bye = send_bye && control.termination_reason() < 0;
        control.send_bye.store(should_send_bye, Ordering::SeqCst);
        control.suspend.store(false, Ordering::SeqCst);
        control.stop.store(true, Ordering::SeqCst);
        wait_for_renderer(&control);
    }
}

// -- C-string entry points the JNI wrappers call -------------------------------

/// Convert a Java String to Rust via pre-fetched UTF8 (the wrapper does it).
#[no_mangle]
pub extern "C" fn leftcar_jni_start() -> StatePtr {
    viewer_core::c_abi::process_start()
}

#[no_mangle]
pub extern "C" fn leftcar_jni_attach(
    state: StatePtr,
    instance_c: *const c_char,
    surface: *mut c_void, // ANativeWindow*, already acquired
) -> i32 {
    // Legacy no-host entry: the media listener must never be reachable
    // without a paired host IP — an unpaired window would accept video from
    // any LAN sender. Fail loudly instead of attaching a dead surface.
    let _ = (state, instance_c, surface);
    log_info!("leftcar_jni_attach: rejected — no paired host IP (use attach_port)");
    LEFTCAR_ERR_INVALID
}

/// Bind and authenticate the media port before React Native asks the Host to
/// start capture. The renderer later claims this exact socket in
/// `leftcar_jni_attach_port`, eliminating the Activity-start race.
#[no_mangle]
pub extern "C" fn leftcar_jni_prepare_port(
    port: u16,
    host_c: *const c_char,
    transport_c: *const c_char,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        if host_c.is_null() {
            return LEFTCAR_ERR_NULL;
        }
        let host = unsafe { CStr::from_ptr(host_c) }
            .to_string_lossy()
            .into_owned();
        let transport = if transport_c.is_null() {
            "udp".to_owned()
        } else {
            unsafe { CStr::from_ptr(transport_c) }
                .to_string_lossy()
                .into_owned()
        };
        if !matches!(
            transport.as_str(),
            "udp" | "tcp" | "adbTcp" | "usb" | "auto"
        ) {
            return LEFTCAR_ERR_INVALID;
        }
        match prepare_udp_receiver(port, &host, &transport) {
            Ok(()) => {
                log_info!(
                    "prepared {} listener(s) on port {port} for {host}",
                    transport
                );
                LEFTCAR_OK
            }
            Err(error) => {
                log_info!("failed to prepare UDP listener: {error}");
                LEFTCAR_ERR_STATE
            }
        }
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

/// Claim the current Android UsbAccessory file descriptor. The Kotlin module
/// calls this before `prepare_port`; the bridge's loopback control port is
/// then used by the JS control client.
#[no_mangle]
pub extern "C" fn leftcar_jni_prepare_usb(fd: i32) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let bridge = match UsbBridge::start(fd) {
            Ok(bridge) => bridge,
            Err(error) => {
                log_info!("failed to prepare USB bridge: {error}");
                return LEFTCAR_ERR_STATE;
            }
        };
        *PREPARED_USB_BRIDGE.lock().unwrap() = Some(bridge);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_usb_control_port() -> i32 {
    let guard = std::panic::catch_unwind(|| {
        PREPARED_USB_BRIDGE
            .lock()
            .unwrap()
            .as_ref()
            .map(|bridge| bridge.control_port() as i32)
            .unwrap_or(0)
    });
    guard.unwrap_or(-1)
}

/// Idempotent rollback for a Host start failure or a window launch failure.
#[no_mangle]
pub extern "C" fn leftcar_jni_cancel_prepared_port(port: u16) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        if cancel_prepared_receiver(port) {
            log_info!("cancelled prepared UDP listener on port {port}");
        }
        if cancel_tcp_bridge(port) {
            log_info!("cancelled prepared ADB TCP bridge on port {port}");
        }
        PREPARED_USB_BRIDGE.lock().unwrap().take();
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

/// Port-explicit attach: each stream window listens on its own UDP port
/// (5000+n), so multiple instances receive independent pushes. `host_c` is
/// the control-plane host IP; the media listener accepts only that peer.
#[no_mangle]
pub extern "C" fn leftcar_jni_attach_port(
    state: StatePtr,
    instance_c: *const c_char,
    surface: *mut c_void,
    port: u16,
    host_c: *const c_char,
    width: u32,
    height: u32,
    fps: u32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        // No paired host = no stream. Validate BEFORE attaching the surface:
        // on this error path the wrapper releases its ANativeWindow ref and
        // the core must not still hold a registered handle (double release).
        let host = if host_c.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(host_c) }
                .to_string_lossy()
                .into_owned()
        };
        if !host_is_valid(&host) {
            log_info!("leftcar_jni_attach_port: invalid paired host {host:?} — refusing");
            return LEFTCAR_ERR_INVALID;
        }
        if viewer_core::c_abi::stream_attach_surface(
            state,
            &instance,
            surface as viewer_core::SurfaceHandle,
        )
        .is_err()
        {
            return LEFTCAR_ERR_STATE;
        }
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        let tcp_bridge = take_media_bridge(port);
        spawn_live_stream_renderer(
            instance_str,
            surface,
            port,
            host,
            width,
            height,
            fps,
            tcp_bridge,
        );
        0
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_surface_changed(
    state: StatePtr,
    instance_c: *const c_char,
    w: u32,
    h: u32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        let instance_str = unsafe { CStr::from_ptr(instance_c) }.to_string_lossy();
        suppress_resize_recovery(&instance_str);
        viewer_core::c_abi::stream_surface_changed(state, &instance, w, h);
        0
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_detach(state: StatePtr, instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        let instance_str = unsafe { CStr::from_ptr(instance_c) }.to_string_lossy();
        suspend_live_stream_renderer(&instance_str);
        let surface = state.attached_surface(&instance);
        match viewer_core::c_abi::stream_detach_surface(state, &instance) {
            Ok(()) => {
                if let Some(surface) = surface {
                    unsafe { ANativeWindow_release(surface as *mut c_void) };
                }
                0
            }
            Err(_) => LEFTCAR_ERR_STATE,
        }
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_update_window(
    state: StatePtr,
    instance_c: *const c_char,
    event_code: u32,
    monotonic_ms: u64,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Some(event) = map_event(event_code) else {
            return LEFTCAR_ERR_INVALID;
        };
        viewer_core::c_abi::stream_update_window_state(
            state,
            &instance,
            event,
            std::time::Duration::from_millis(monotonic_ms),
        );
        0
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

fn active_input_control(instance_c: *const c_char) -> Result<Arc<RendererControl>, i32> {
    if instance_c.is_null() {
        return Err(LEFTCAR_ERR_NULL);
    }
    let instance = unsafe { CStr::from_ptr(instance_c) }
        .to_str()
        .map_err(|_| LEFTCAR_ERR_INVALID)?;
    ACTIVE_RENDERERS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(instance).cloned())
        .ok_or(LEFTCAR_ERR_STATE)
}

/// Return the authenticated Host input state for the in-stream lock badge.
/// -1 means the status packet has not arrived yet; 0/1 are locked/enabled.
#[no_mangle]
pub extern "C" fn leftcar_jni_input_status(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        i32::from(control.input_enabled.load(Ordering::SeqCst))
    });
    guard.unwrap_or(-1)
}

fn pack_stream_stats(control: &RendererControl) -> i64 {
    const FRAME_MASK: u64 = (1 << 28) - 1;
    let rendered = control
        .rendered_frames
        .load(Ordering::Relaxed)
        .min(FRAME_MASK);
    let stale = control.stale_outputs.load(Ordering::Relaxed).min(0x0fff);
    let input_drops = control
        .decoder_input_drops
        .load(Ordering::Relaxed)
        .min(0xff);
    let gaps = control.frame_gaps.load(Ordering::Relaxed).min(0xff);
    let feed_ms = control
        .last_feed_us
        .load(Ordering::Relaxed)
        .saturating_add(500)
        / 1_000;
    (rendered | (stale << 28) | (input_drops << 40) | (gaps << 48) | (feed_ms.min(0xff) << 56))
        as i64
}

/// Compact native diagnostics for the in-stream HUD.
/// bits 0..27 rendered, 28..39 stale skips, 40..47 decoder input drops,
/// 48..55 frame gaps, 56..63 latest decoder feed milliseconds.
#[no_mangle]
pub extern "C" fn leftcar_jni_stream_stats(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        pack_stream_stats(&control)
    });
    guard.unwrap_or(-1)
}

/// Pack separated stale-input and decoder-burst discard counters for the
/// measurement spike. The high 32 bits are input-policy observations and the
/// low 32 bits are decoder output-burst discards.
#[no_mangle]
pub extern "C" fn leftcar_jni_skip_breakdown(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        let input = control
            .stale_input_drops
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX));
        let burst = control
            .output_burst_discards
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX));
        ((input << 32) | burst) as i64
    });
    guard.unwrap_or(-1)
}

/// Authenticated stage latency for the HUD, packed as four unsigned 16-bit
/// milliseconds: LAN RTT, capture-to-decoder, encode-to-decoder, wire-to-decoder.
/// `0xffff` means the NTP-style probe or L2 timestamp has not converged yet.
#[no_mangle]
pub extern "C" fn leftcar_jni_stream_latency(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        let encode = |value: u64| {
            if value == LATENCY_UNKNOWN {
                0xffff
            } else {
                value.min(0xfffe)
            }
        };
        let network = encode(control.network_rtt_ms.load(Ordering::Relaxed));
        let capture = encode(control.capture_to_decoder_ms.load(Ordering::Relaxed));
        let encoded = encode(control.encode_to_decoder_ms.load(Ordering::Relaxed));
        let wire = encode(control.wire_to_decoder_ms.load(Ordering::Relaxed));
        (network | (capture << 16) | (encoded << 32) | (wire << 48)) as i64
    });
    guard.unwrap_or(-1)
}

/// Host-initiated termination reason for this stream, or -1 when the host has
/// not terminated it. 1 = feedback health check (connection lost), 2 = host
/// operator forced stop, 3 = ordinary host stop. The Activity polls this
/// alongside the stats HUD and closes its window on any non-negative value.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn leftcar_jni_termination_reason(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        if instance_c.is_null() {
            return -1;
        }
        let instance = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        let active_reason = ACTIVE_RENDERERS
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|renderers| renderers.get(&instance))
            .map(|control| control.termination_reason())
            .filter(|reason| *reason >= 0);
        if let Some(reason) = active_reason {
            return i32::from(reason);
        }
        TERMINATION_REASONS
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|reasons| reasons.get(&instance).copied())
            .map(i32::from)
            .unwrap_or(-1)
    });
    guard.unwrap_or(-1)
}

/// Glass-to-glass latency (capture → Surface render) in milliseconds.
/// `0xffff` means the clock-corrected estimate has not converged. This is the
/// number users perceive; the to-decoder stages omit the decoder output wait.
#[no_mangle]
pub extern "C" fn leftcar_jni_render_latency(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| match active_input_control(instance_c) {
        Ok(control) => {
            let value = control.capture_to_render_ms.load(Ordering::Relaxed);
            if value == LATENCY_UNKNOWN {
                0xffff
            } else {
                value.min(0xfffe) as i32
            }
        }
        Err(_) => 0xffff,
    });
    guard.unwrap_or(0xffff)
}

/// Queue a native Android pointer event. `x` and `y` are normalized to the
/// actual video Surface before crossing JNI; Rust clamps once more at the
/// fixed-point wire boundary.
#[no_mangle]
pub extern "C" fn leftcar_jni_input_pointer(
    instance_c: *const c_char,
    action: u32,
    x: f32,
    y: f32,
    buttons: u32,
    action_button: u32,
    horizontal_scroll: f32,
    vertical_scroll: f32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        let event = match action {
            1 => InputEvent::PointerMove {
                x: normalized_axis(x),
                y: normalized_axis(y),
                buttons,
            },
            2 | 3 => InputEvent::PointerButton {
                x: normalized_axis(x),
                y: normalized_axis(y),
                button: u8::try_from(action_button).unwrap_or(0),
                down: action == 2,
                buttons,
            },
            4 => InputEvent::Scroll {
                horizontal_milli_lines: (horizontal_scroll.clamp(-1000.0, 1000.0) * 1_000.0).round()
                    as i32,
                vertical_milli_lines: (vertical_scroll.clamp(-1000.0, 1000.0) * 1_000.0).round()
                    as i32,
            },
            _ => return LEFTCAR_ERR_INVALID,
        };
        control.input.lock().unwrap().push(event);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_input_key(
    instance_c: *const c_char,
    key_code: u32,
    scan_code: u32,
    meta_state: u32,
    down: bool,
    repeat: u32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        let (Ok(key_code), Ok(scan_code), Ok(repeat)) = (
            u16::try_from(key_code),
            u16::try_from(scan_code),
            u16::try_from(repeat),
        ) else {
            return LEFTCAR_ERR_INVALID;
        };
        control.input.lock().unwrap().push(InputEvent::Key {
            key_code,
            scan_code,
            meta_state,
            down,
            repeat,
        });
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_input_release_all(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        control.input.lock().unwrap().push(InputEvent::ReleaseAll);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_release(state: StatePtr, instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        stop_live_stream_renderer(&instance_str, true);
        let surface = state.attached_surface(&instance);
        viewer_core::c_abi::stream_release(state, &instance);
        if let Some(surface) = surface {
            unsafe { ANativeWindow_release(surface as *mut c_void) };
        }
        0
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

unsafe fn cstr_instance(c: *const c_char) -> Result<viewer_core::StreamInstanceId, ()> {
    if c.is_null() {
        return Err(());
    }
    let s = CStr::from_ptr(c).to_string_lossy();
    viewer_core::StreamInstanceId::from_raw(s).map_err(|_| ())
}

fn map_event(code: u32) -> Option<viewer_core::LifecycleEvent> {
    use viewer_core::LifecycleEvent as L;
    Some(match code {
        1 => L::ActivityCreate,
        2 => L::ActivityStart,
        3 => L::ActivityResume,
        4 => L::FocusGain,
        5 => L::FocusLoss,
        6 => L::SurfaceCreate,
        7 => L::SurfaceChange,
        8 => L::SurfaceDestroy,
        9 => L::ActivityPause,
        10 => L::ActivityStop,
        11 => L::ConfigurationChange,
        12 => L::TaskRemove,
        13 => L::ProcessDeath,
        _ => return None,
    })
}

/// Balance helper used by the JNI wrapper: acquire on attach, release on
/// detach. Both are exposed so the wrapper never hides a ref change.
#[no_mangle]
pub extern "C" fn leftcar_jni_surface_ref(surface: *mut c_void, acquire: bool) {
    if surface.is_null() {
        return;
    }
    unsafe {
        if acquire {
            ANativeWindow_acquire(surface);
        } else {
            ANativeWindow_release(surface);
        }
    }
}

#[cfg(test)]
mod termination_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn cached_host_termination_survives_renderer_removal_until_reuse() {
        let instance = "termination-cache-test";
        clear_cached_termination(instance);
        TERMINATION_REASONS
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(instance.to_owned(), 2);
        let instance_c = CString::new(instance).unwrap();

        assert_eq!(leftcar_jni_termination_reason(instance_c.as_ptr()), 2);
        clear_cached_termination(instance);
        assert_eq!(leftcar_jni_termination_reason(instance_c.as_ptr()), -1);
    }
}
