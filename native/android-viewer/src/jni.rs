//! JNI surface for the Kotlin shim (docs/05 §8.2 JNI 경계).
//!
//! Leftcar JNI rules (docs/07 §14):
//! - null/invalid jobject validated
//! - ANativeWindow acquire/release balanced
//! - exceptions checked/cleared, never leaked across the boundary
//! - panics never cross JNI (catch_unwind everywhere)

use std::ffi::{c_char, c_void, CStr};

use crate::input_protocol::{encode_input, normalized_axis, parse_ack, InputEvent, InputScheduler};
use crate::media_datagram::{parse_fragment, FrameReassembler};
use crate::net_guard::{host_is_valid, peer_allowed};

#[repr(C)]
struct jobject;
#[repr(C)]
struct JNIEnv(c_void);
#[repr(C)]
struct JavaVM(c_void);
#[repr(C)]
struct JNINativeMethod {
    name: *const c_char,
    signature: *const c_char,
    fnPtr: *mut c_void,
}

extern "C" {
    fn ANativeWindow_fromSurface(env: *mut JNIEnv, surface: *mut jobject) -> *mut c_void;
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
}

unsafe extern "C" {
    fn GetJavaVM(env: *mut JNIEnv, vm: *mut *mut JavaVM) -> i32;
}

const LEFTCAR_OK: i32 = 0;
const LEFTCAR_ERR_NULL: i32 = 1;
const LEFTCAR_ERR_STATE: i32 = 2;
const LEFTCAR_ERR_PANIC: i32 = 3;
const LEFTCAR_ERR_INVALID: i32 = 4;

type StatePtr = *mut viewer_core::ProcessState;

unsafe fn instance_from_jstring(
    _env: *mut JNIEnv,
    jstr: *mut jobject,
) -> Result<viewer_core::StreamInstanceId, i32> {
    if jstr.is_null() {
        return Err(LEFTCAR_ERR_NULL);
    }
    // Read the String via JNI GetStringUTFChars through env vtable is
    // heavyweight; instead the shim passes UTF-8 through a global call:
    // we rely on the C-string path below (attachSurfaceCString). This stub
    // is intentionally unreachable from Kotlin (no external binding).
    let _ = _env;
    Err(LEFTCAR_ERR_INVALID)
}

/// JNI methods table (registered via JNI_OnLoad).
const METHODS: &[(&[u8], &[u8], *const c_void)] = &[];
pub const _METHODS_LEN: usize = METHODS.len();

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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct RendererControl {
    port: u16,
    input: Mutex<InputScheduler>,
    stop: AtomicBool,
    // Surface destruction is not always the end of the Activity. During an
    // XR/freeform resize, release MediaCodec's ANativeWindow promptly but keep
    // the UDP listener alive until either a replacement Surface attaches or
    // the Activity performs its final release.
    suspend: AtomicBool,
    suspended: AtomicBool,
    // A surface can disappear briefly during an XR resize/reconfiguration.
    // In that case the host must see EOF and use its existing reconnect path,
    // rather than receiving BYE and permanently stopping capture.
    send_bye: AtomicBool,
    // SurfaceHolder.surfaceDestroyed must not return while MediaCodec still
    // owns the ANativeWindow. The callback waits on this bounded flag before
    // releasing the native window reference.
    finished: AtomicBool,
}

static ACTIVE_RENDERERS: Mutex<Option<HashMap<String, Arc<RendererControl>>>> = Mutex::new(None);

fn remove_renderer_if_current(instance: &str, control: &Arc<RendererControl>) {
    let mut map = ACTIVE_RENDERERS.lock().unwrap();
    if let Some(map) = map.as_mut() {
        let is_current = map
            .get(instance)
            .map(|current| Arc::ptr_eq(current, control))
            .unwrap_or(false);
        if is_current {
            map.remove(instance);
        }
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

#[derive(Clone)]
struct FramePacket {
    id: u16,
    au: Vec<u8>,
    host_wall_ms: Option<u64>,
}

#[derive(Default)]
struct RendererStats {
    queued: u64,
    input_drops: u64,
    frame_gaps: u64,
    max_feed_us: u64,
    // Host and Android wall clocks are not guaranteed to be synchronized.
    // Calibrate their fixed offset from the first received frame so the
    // diagnostic age is useful instead of reporting a large negative value.
    host_clock_offset_ms: Option<i128>,
}

fn is_keyframe(au: &[u8]) -> bool {
    viewer_decoder::split_annexb(au)
        .iter()
        .any(|nal| viewer_decoder::nal_type(nal.bytes) == Some(viewer_decoder::NAL_IDR))
}

fn reset_decoder(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    sps: &mut Vec<u8>,
    pps: &mut Vec<u8>,
    awaiting_keyframe: &mut bool,
) {
    if let Some(d) = decoder.as_mut() {
        d.stop();
    }
    *decoder = None;
    sps.clear();
    pps.clear();
    *awaiting_keyframe = true;
}

/// A missing encoded AU can invalidate the reference chain of every later
/// H.264 delta frame. Drop the codec but retain SPS/PPS bookkeeping; the host
/// sends CFG before the requested IDR and that packet recreates MediaCodec.
/// Android requires codec-specific data to be resubmitted after `flush`, and
/// vendor behavior differs, so a clean recreate is safer than a flush whose
/// next input is only an IDR.
fn resync_decoder_after_frame_gap(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    awaiting_keyframe: &mut bool,
) {
    if *awaiting_keyframe {
        return;
    }
    if let Some(decoder) = decoder.as_mut() {
        decoder.stop();
    }
    *decoder = None;
    *awaiting_keyframe = true;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedOutcome {
    Queued,
    ResyncRequired,
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

fn feed_and_render(
    dec: &mut viewer_decoder::AndroidDecoder,
    frame: &FramePacket,
    aus: &mut u64,
    fps: u32,
    stats: &mut RendererStats,
) -> FeedOutcome {
    *aus += 1;
    let frame_us = 1_000_000u64 / u64::from(fps.max(1));
    let pts_us = aus.saturating_mul(frame_us) as i64;
    let started = std::time::Instant::now();
    // Give a hardware codec at most 1 ms to hand back an input slot. This
    // absorbs normal scheduler jitter without allowing a frame backlog to
    // become visible interaction latency.
    let result = dec.feed_au_status(&frame.au, pts_us, 1_000);
    let feed_us = started.elapsed().as_micros() as u64;
    stats.max_feed_us = stats.max_feed_us.max(feed_us);

    let queued = match result {
        Ok(viewer_decoder::FeedStatus::Queued { .. }) => {
            stats.queued += 1;
            while dec.pump_output(0).unwrap_or(false) {}
            FeedOutcome::Queued
        }
        Ok(viewer_decoder::FeedStatus::InputUnavailable) => {
            stats.input_drops += 1;
            FeedOutcome::ResyncRequired
        }
        Ok(viewer_decoder::FeedStatus::InputTooLarge { required, capacity }) => {
            stats.input_drops += 1;
            log_info!(
                "decoder input AU too large: required={} capacity={}; resyncing",
                required,
                capacity
            );
            FeedOutcome::ResyncRequired
        }
        Err(e) => {
            log_info!("decoder feed failed: {}", e);
            FeedOutcome::ResyncRequired
        }
    };

    let wire_age_ms = frame.host_wall_ms.and_then(|sent| {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|now| now.as_millis() as i128)?;
        let raw_age = now_ms - sent as i128;
        let offset = stats.host_clock_offset_ms.get_or_insert(raw_age);
        Some(raw_age - *offset)
    });

    if dec.frames_rendered % 30 == 0 && dec.frames_rendered > 0 {
        log_info!(
            "Rendered {} frames; queued={} inputDrops={} frameGaps={} feedUs={} maxFeedUs={} wireAgeMs={:?}",
            dec.frames_rendered,
            stats.queued,
            stats.input_drops,
            stats.frame_gaps,
            feed_us,
            stats.max_feed_us,
            wire_age_ms
        );
    }
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
) {
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

    let control = Arc::new(RendererControl {
        port,
        input: Mutex::new(InputScheduler::new(fps)),
        stop: AtomicBool::new(false),
        suspend: AtomicBool::new(false),
        suspended: AtomicBool::new(false),
        send_bye: AtomicBool::new(true),
        finished: AtomicBool::new(false),
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
        let socket = match std::net::UdpSocket::bind(format!("0.0.0.0:{port}")) {
            Ok(socket) => socket,
            Err(e) => {
                log_info!("FAILED to bind UDP listener on 0.0.0.0:{}: {}", port, e);
                control_clone.finished.store(true, Ordering::SeqCst);
                remove_renderer_if_current(&instance_str, &control_clone);
                return;
            }
        };
        // Input motion is transmitted at 2x stream FPS (120Hz at 60fps,
        // 180Hz at 90fps). A 2ms receive timeout lets the same authenticated
        // session socket service that scheduler without a second network
        // transport or a frame-rate-sized delay.
        let _ = socket.set_read_timeout(Some(std::time::Duration::from_millis(2)));
        use std::os::fd::AsRawFd;
        let receive_buffer: libc::c_int = 4 * 1024 * 1024;
        unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &receive_buffer as *const _ as *const libc::c_void,
                std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
            );
        }
        log_info!(
            "UDP listening on port {} (accepting media only from {expected_host})",
            port
        );
        let mut buf = vec![0u8; 2_048];
        let mut host_peer: Option<std::net::SocketAddr> = None;
        let mut viewer_control_token = Vec::new();
        let mut reassembler = FrameReassembler::default();
        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
        let mut aus = 0u64;
        let mut awaiting_keyframe = true;
        let mut last_frame_id: Option<u16> = None;
        let mut renderer_stats = RendererStats::default();

        while !control_clone.stop.load(Ordering::Relaxed) {
            if control_clone.suspend.load(Ordering::SeqCst) {
                // MediaCodec must let go of the old ANativeWindow before
                // SurfaceHolder.surfaceDestroyed returns. Keep the UDP socket
                // so a subsequent final release can still send BYE.
                reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                reassembler.clear();
                last_frame_id = None;
                control_clone.suspended.store(true, Ordering::SeqCst);
                while control_clone.suspend.load(Ordering::SeqCst)
                    && !control_clone.stop.load(Ordering::SeqCst)
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                control_clone.suspended.store(false, Ordering::SeqCst);
                if let Some(peer) = host_peer {
                    request_idr(&socket, peer, &viewer_control_token);
                }
                continue;
            }

            if let Some(peer) = host_peer {
                flush_input(&socket, peer, &viewer_control_token, &control_clone);
            }

            let (received, peer) = match socket.recv_from(&mut buf) {
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
                Ok(received) => received,
            };

            if !peer_allowed(Some(peer), &expected_host) {
                log_info!("rejected media datagram from {peer}: not the paired host");
                continue;
            }
            if host_peer != Some(peer) {
                log_info!("UDP sender active: {peer}");
                host_peer = Some(peer);
                reassembler.clear();
                reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                last_frame_id = None;
                aus = 0;
            }

            let packet = &buf[..received];
            if packet.len() > 4 && packet.len() <= 128 && &packet[..4] == b"LCH1" {
                viewer_control_token.clear();
                viewer_control_token.extend_from_slice(&packet[4..]);
                control_clone.input.lock().unwrap().reset_session();
                if let Err(error) = socket.send_to(packet, peer) {
                    log_info!("failed to echo UDP reachability challenge: {error}");
                } else {
                    log_info!("UDP reachability challenge verified for {peer}");
                }
                continue;
            }
            if let Some(sequence) = parse_ack(packet, &viewer_control_token) {
                control_clone.input.lock().unwrap().acknowledge(sequence);
                continue;
            }
            if packet.len() >= 3 && &packet[..3] == b"CFG" {
                log_info!("Received CFG datagram ({} bytes)", packet.len());
                let mut off = 3usize;
                while off + 4 <= packet.len() {
                    let l = u32::from_be_bytes(packet[off..off + 4].try_into().unwrap()) as usize;
                    off += 4;
                    if off + l > packet.len() {
                        break;
                    }
                    let nal = &packet[off..off + l];
                    if nal.len() > 4 {
                        let t = viewer_decoder::nal_type(&nal[4..]);
                        if t == Some(viewer_decoder::NAL_SPS) {
                            sps = nal.to_vec();
                        } else if t == Some(viewer_decoder::NAL_PPS) {
                            pps = nal.to_vec();
                        }
                    }
                    off += l;
                }
                if !sps.is_empty() && !pps.is_empty() && decoder.is_none() {
                    unsafe {
                        log_info!(
                            "Creating AndroidDecoder with Surface window=0x{:x} sps={}B pps={}B",
                            window_handle,
                            sps.len(),
                            pps.len()
                        );
                        match viewer_decoder::AndroidDecoder::new_h264(
                            &sps,
                            &pps,
                            width,
                            height,
                            window_handle,
                            fps,
                        ) {
                            Ok(d) => {
                                log_info!("AndroidDecoder created successfully!");
                                decoder = Some(d);
                            }
                            Err(e) => {
                                log_info!("AndroidDecoder creation FAILED: {}", e);
                            }
                        }
                    }
                }
                // CFG is emitted with an IDR on the host. Do not feed
                // delta frames until that recovery keyframe arrives.
                awaiting_keyframe = true;
                continue;
            }

            let Some(fragment) = parse_fragment(packet) else {
                continue;
            };
            let Some(completed) = reassembler.push(fragment) else {
                continue;
            };
            let frame = FramePacket {
                id: completed.id,
                au: completed.au,
                host_wall_ms: Some(completed.host_wall_ms),
            };
            let keyframe = is_keyframe(&frame.au);
            let frame_gap = last_frame_id
                .map(|previous| !viewer_decoder::frame_id_is_next(previous, frame.id))
                .unwrap_or(false);
            last_frame_id = Some(frame.id);

            if frame_gap {
                renderer_stats.frame_gaps += 1;
                log_info!(
                    "UDP access-unit gap detected at id={}; awaiting next IDR",
                    frame.id
                );
                if keyframe {
                    awaiting_keyframe = false;
                } else {
                    resync_decoder_after_frame_gap(&mut decoder, &mut awaiting_keyframe);
                    request_idr(&socket, peer, &viewer_control_token);
                    continue;
                }
            }

            if let Some(dec) = decoder.as_mut() {
                if awaiting_keyframe && !keyframe {
                    continue;
                }
                match feed_and_render(dec, &frame, &mut aus, fps, &mut renderer_stats) {
                    FeedOutcome::Queued => {
                        awaiting_keyframe = false;
                    }
                    FeedOutcome::ResyncRequired => {
                        resync_decoder_after_frame_gap(&mut decoder, &mut awaiting_keyframe);
                        request_idr(&socket, peer, &viewer_control_token);
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
                flush_input(&socket, peer, &viewer_control_token, &control_clone);
                send_viewer_command(&socket, peer, b"BYE", &viewer_control_token);
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
        drop(decoder);
        drop(socket);
        control_clone.suspended.store(false, Ordering::SeqCst);
        control_clone.finished.store(true, Ordering::SeqCst);
        remove_renderer_if_current(&instance_str, &control_clone);
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
        control.send_bye.store(send_bye, Ordering::SeqCst);
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
        spawn_live_stream_renderer(instance_str, surface, port, host, width, height, fps);
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

#[allow(unused)]
fn unused(_: *mut JNIEnv, _: *mut JavaVM, _: *const JNINativeMethod) {
    let _ = instance_from_jstring as unsafe fn(*mut JNIEnv, *mut jobject) -> _;
    let _ = METHODS;
    let _ = [(&b"start\0"[..], b"()J\0".as_ptr())];
}
