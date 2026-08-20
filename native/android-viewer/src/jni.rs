//! JNI surface for the Kotlin shim (docs/05 §8.2 JNI 경계).
//!
//! Leftcar JNI rules (docs/07 §14):
//! - null/invalid jobject validated
//! - ANativeWindow acquire/release balanced
//! - exceptions checked/cleared, never leaked across the boundary
//! - panics never cross JNI (catch_unwind everywhere)

use std::ffi::{c_char, c_void, CStr};

use crate::net_guard::peer_allowed;

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
    stop: AtomicBool,
    // Surface destruction is not always the end of the Activity. During an
    // XR/freeform resize, release MediaCodec's ANativeWindow promptly but keep
    // the TCP connection alive until either a replacement Surface attaches or
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

fn parse_frame_packet(pkt: &[u8]) -> Option<FramePacket> {
    if pkt.len() < 5 || pkt[0] != 0x46 {
        return None;
    }
    let id = u16::from_le_bytes([pkt[3], pkt[4]]);
    let mut offset = 5usize;
    let host_wall_ms = if pkt.len() >= 15 && pkt[5..7] == [0x4c, 0x54] {
        offset = 15;
        Some(u64::from_be_bytes(pkt[7..15].try_into().ok()?))
    } else {
        None
    };
    let payload = pkt.get(offset..)?;
    // Older hosts nested the AU+PTS envelope inside F. Keep compatibility
    // while the current host sends raw Annex-B after the F header.
    let au = if payload.len() >= 10 && &payload[..2] == b"AU" {
        &payload[10..]
    } else {
        payload
    };
    if au.is_empty() {
        return None;
    }
    Some(FramePacket {
        id,
        au: au.to_vec(),
        host_wall_ms,
    })
}

fn parse_legacy_au_packet(pkt: &[u8]) -> Option<FramePacket> {
    if pkt.len() < 10 || &pkt[..2] != b"AU" {
        return None;
    }
    let au = &pkt[10..];
    if au.is_empty() {
        return None;
    }
    Some(FramePacket {
        id: 0,
        au: au.to_vec(),
        host_wall_ms: None,
    })
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
/// H.264 delta frame. Flush the existing codec but retain SPS/PPS so the next
/// IDR can recover immediately without waiting for a full reconnect. If a
/// vendor codec rejects flush, drop it and let the next CFG recreate it.
fn resync_decoder_after_frame_gap(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    awaiting_keyframe: &mut bool,
) {
    if *awaiting_keyframe {
        return;
    }
    let flush_error = decoder.as_mut().and_then(|decoder| decoder.flush().err());
    if let Some(error) = flush_error {
        log_info!("decoder flush after frame gap failed: {error}");
        if let Some(decoder) = decoder.as_mut() {
            decoder.stop();
        }
        *decoder = None;
    }
    *awaiting_keyframe = true;
}

fn feed_and_render(
    dec: &mut viewer_decoder::AndroidDecoder,
    frame: &FramePacket,
    aus: &mut u64,
    fps: u32,
    stats: &mut RendererStats,
) -> bool {
    *aus += 1;
    let frame_us = 1_000_000u64 / u64::from(fps.max(1));
    let pts_us = aus.saturating_mul(frame_us) as i64;
    let started = std::time::Instant::now();
    let result = dec.feed_au_status(&frame.au, pts_us, 0);
    let feed_us = started.elapsed().as_micros() as u64;
    stats.max_feed_us = stats.max_feed_us.max(feed_us);

    let queued = match result {
        Ok(viewer_decoder::FeedStatus::Queued { .. }) => {
            stats.queued += 1;
            while dec.pump_output(0).unwrap_or(false) {}
            true
        }
        Ok(viewer_decoder::FeedStatus::InputUnavailable) => {
            stats.input_drops += 1;
            false
        }
        Err(e) => {
            log_info!("decoder feed failed: {}", e);
            false
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
    let fps = fps.clamp(1, 60);
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
        let listener = match std::net::TcpListener::bind(format!("0.0.0.0:{port}")) {
            Ok(s) => s,
            Err(e) => {
                log_info!("FAILED to bind TCP listener on 0.0.0.0:{}: {}", port, e);
                control_clone.finished.store(true, Ordering::SeqCst);
                remove_renderer_if_current(&instance_str, &control_clone);
                return;
            }
        };
        log_info!(
            "TCP listening on port {} (accepting media only from {expected_host})",
            port
        );
        // accept one sender (reconnect allowed after disconnect)
        let mut sock: Option<std::net::TcpStream> = None;

        let mut buf = vec![0u8; 1 << 20]; // 1MB read buffer for framed packets
        let mut pending: Vec<u8> = Vec::with_capacity(1 << 20); // stream reassembly
        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
        let mut aus = 0u64;
        let mut last_frame_ms = std::time::Instant::now();
        let mut awaiting_keyframe = true;
        let mut last_frame_id: Option<u16> = None;
        let mut latest_frame: Option<FramePacket> = None;
        let mut latest_keyframe: Option<FramePacket> = None;
        let mut renderer_stats = RendererStats::default();

        while !control_clone.stop.load(Ordering::Relaxed) {
            if control_clone.suspend.load(Ordering::SeqCst) {
                // MediaCodec must let go of the old ANativeWindow before
                // SurfaceHolder.surfaceDestroyed returns. Keep the accepted
                // socket around so a subsequent final release can still send
                // BYE and the host does not leave a phantom running session.
                reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                pending.clear();
                last_frame_id = None;
                latest_frame = None;
                latest_keyframe = None;
                control_clone.suspended.store(true, Ordering::SeqCst);
                while control_clone.suspend.load(Ordering::SeqCst)
                    && !control_clone.stop.load(Ordering::SeqCst)
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                control_clone.suspended.store(false, Ordering::SeqCst);
                continue;
            }

            // (re)connect: wait for a sender when no stream is attached
            if sock.is_none() {
                let _ = listener.set_nonblocking(true);
                // poll accept briefly so the stop flag stays responsive
                if let Ok((s, peer)) = listener.accept() {
                    if !peer_allowed(Some(peer), &expected_host) {
                        // Drop the socket (never assign it) and keep waiting
                        // for the paired host: a LAN attacker must not be able
                        // to push forged H.264 into the decoder surface.
                        log_info!("rejected media connection from {peer}: not the paired host");
                        drop(s);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    let _ = s.set_nodelay(true);
                    // accepted sockets inherit O_NONBLOCK from the listener on
                    // Linux — clear it so read_timeout (blocking poll) works;
                    // otherwise read returns EAGAIN instantly and we treat it
                    // as a disconnect
                    use std::os::fd::AsRawFd;
                    unsafe {
                        let fd = s.as_raw_fd();
                        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
                        if flags >= 0 {
                            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
                        }
                    }
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(300)));
                    log_info!("TCP sender connected");
                    sock = Some(s);
                    pending.clear();
                    reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                    last_frame_id = None;
                    latest_frame = None;
                    latest_keyframe = None;
                    aus = 0;
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
            }
            let stream = sock.as_mut().unwrap();
            let n = match std::io::Read::read(stream, &mut buf) {
                Ok(0) => {
                    // sender disconnected — wait for reconnect
                    log_info!("TCP sender disconnected (EOF)");
                    sock = None;
                    reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                    last_frame_id = None;
                    latest_frame = None;
                    latest_keyframe = None;
                    continue;
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => {
                    log_info!("TCP sender read error: {e}");
                    sock = None;
                    reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                    last_frame_id = None;
                    latest_frame = None;
                    latest_keyframe = None;
                    continue;
                }
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);
            if pending.len() > 8 * 1024 * 1024 {
                log_info!("pending TCP bytes exceeded 8MB — dropping stale stream data");
                pending.clear();
                reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                last_frame_id = None;
                latest_frame = None;
                latest_keyframe = None;
                continue;
            }

            // Keep only the newest complete AU from this read batch. This is
            // the viewer-side drop boundary: if decode falls behind, old
            // frames are discarded before they become interaction latency.
            latest_frame = None;
            latest_keyframe = None;

            // extract complete [u32 len][payload] frames from the stream
            loop {
                if pending.len() < 4 {
                    break;
                }
                let len = u32::from_be_bytes(pending[..4].try_into().unwrap()) as usize;
                if len > 4 * 1024 * 1024 {
                    // framing corrupted — hard reset the connection
                    log_info!("frame len {} exceeds cap — resetting connection", len);
                    sock = None;
                    pending.clear();
                    last_frame_id = None;
                    reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                    break;
                }
                if pending.len() < 4 + len {
                    break; // need more bytes
                }
                let pkt: Vec<u8> = pending[4..4 + len].to_vec();
                pending.drain(..4 + len);

                if pkt.len() >= 3 && &pkt[..3] == b"CFG" {
                    log_info!("Received CFG packet ({} bytes)", pkt.len());
                    let mut off = 3usize;
                    while off + 4 <= pkt.len() {
                        let l = u32::from_be_bytes(pkt[off..off + 4].try_into().unwrap()) as usize;
                        off += 4;
                        if off + l > pkt.len() {
                            break;
                        }
                        let nal = &pkt[off..off + l];
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
                        last_frame_ms = std::time::Instant::now();
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

                let frame = parse_legacy_au_packet(&pkt).or_else(|| parse_frame_packet(&pkt));
                if let Some(frame) = frame {
                    let keyframe = is_keyframe(&frame.au);
                    let frame_gap = last_frame_id
                        .map(|previous| !viewer_decoder::frame_id_is_next(previous, frame.id))
                        .unwrap_or(false);
                    last_frame_id = Some(frame.id);

                    if frame_gap {
                        renderer_stats.frame_gaps += 1;
                        log_info!(
                            "encoded frame gap detected at id={}; awaiting next IDR",
                            frame.id
                        );
                        resync_decoder_after_frame_gap(&mut decoder, &mut awaiting_keyframe);
                        latest_frame = None;
                        latest_keyframe = None;
                    }

                    if keyframe {
                        latest_keyframe = Some(frame.clone());
                    }
                    if !frame_gap {
                        latest_frame = Some(frame);
                    }
                }
            }

            // stream-stall watchdog: if no AU was accepted for >3s, rebuild
            // the codec rather than displaying a stale decoder state.
            if decoder.is_some() && last_frame_ms.elapsed() > std::time::Duration::from_millis(3000)
            {
                reset_decoder(&mut decoder, &mut sps, &mut pps, &mut awaiting_keyframe);
                latest_frame = None;
                latest_keyframe = None;
                log_info!("stream stalled >3s — decoder dropped, awaiting CFG");
            }

            if let Some(dec) = decoder.as_mut() {
                if awaiting_keyframe {
                    if let Some(frame) = latest_keyframe.take() {
                        if feed_and_render(dec, &frame, &mut aus, fps, &mut renderer_stats) {
                            awaiting_keyframe = false;
                            last_frame_ms = std::time::Instant::now();
                            if latest_frame.as_ref().map(|f| f.id) == Some(frame.id) {
                                latest_frame = None;
                            }
                        }
                    }
                }
                if !awaiting_keyframe {
                    if let Some(frame) = latest_frame.take() {
                        if feed_and_render(dec, &frame, &mut aus, fps, &mut renderer_stats) {
                            last_frame_ms = std::time::Instant::now();
                        }
                    }
                }
            }
        }
        // A transient Surface destruction is deliberately an EOF only. The
        // host's heartbeat reconnect then attaches to the next Surface. BYE
        // is reserved for final Activity release.
        if control_clone.send_bye.load(Ordering::SeqCst) {
            if let Some(mut stream) = sock.take() {
                let payload = b"BYE";
                let len = (payload.len() as u32).to_be_bytes();
                let mut goodbye = Vec::with_capacity(4 + payload.len());
                goodbye.extend_from_slice(&len);
                goodbye.extend_from_slice(payload);
                let _ = std::io::Write::write_all(&mut stream, &goodbye);
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
        drop(sock);
        drop(listener);
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
        // The socket read timeout is 300 ms. Wait until the decoder has been
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

/// Port-explicit attach: each stream window listens on its own TCP port
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
        if viewer_core::c_abi::stream_attach_surface(
            state,
            &instance,
            surface as viewer_core::SurfaceHandle,
        )
        .is_err()
        {
            return LEFTCAR_ERR_STATE;
        }
        // No paired host = no stream. Validate before spawning the renderer
        // so the caller sees the failure instead of a silently black window.
        let host = if host_c.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(host_c) }.to_string_lossy().into_owned()
        };
        if !peer_allowed(None, &host) {
            log_info!("leftcar_jni_attach_port: invalid paired host {host:?} — refusing");
            return LEFTCAR_ERR_INVALID;
        }
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        spawn_live_stream_renderer(
            instance_str,
            surface,
            port,
            host,
            width,
            height,
            fps,
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
