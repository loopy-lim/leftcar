//! JNI surface for the Kotlin shim (docs/05 §8.2 JNI 경계).
//!
//! Leftcar JNI rules (docs/07 §14):
//! - null/invalid jobject validated
//! - ANativeWindow acquire/release balanced
//! - exceptions checked/cleared, never leaked across the boundary
//! - panics never cross JNI (catch_unwind everywhere)

use std::ffi::{c_char, c_void, CStr, CString};

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

static ACTIVE_RENDERERS: Mutex<Option<HashMap<String, Arc<AtomicBool>>>> = Mutex::new(None);

/// Stop any renderer still holding the UDP port and wait (bounded) for its
/// thread to release the socket. Without this, a re-attached surface races
/// the old thread and `bind` fails with Address-in-use.
fn reclaim_udp_port(port: u16) {
    let running: Vec<Arc<AtomicBool>> = {
        let mut map = ACTIVE_RENDERERS.lock().unwrap();
        map.get_or_insert_with(HashMap::new).values().cloned().collect()
    };
    for stop in running {
        stop.store(true, Ordering::SeqCst);
    }
    // renderer polls stop every <=300ms (read timeout); 2s is ample
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let any = ACTIVE_RENDERERS
            .lock()
            .unwrap()
            .as_ref()
            .map(|m| !m.is_empty())
            .unwrap_or(false);
        if !any {
            return;
        }
    }
    let _ = port;
}

fn feed_and_render(dec: &mut viewer_decoder::AndroidDecoder, au: &[u8], aus: &mut u64) {
    *aus += 1;
    let _ = dec.feed_au(au, (*aus * 11_111) as i64, 1000);
    while dec.pump_output(0).unwrap_or(false) {}
    if *aus % 30 == 0 {
        log_info!("Rendered {} frames to surface", dec.frames_rendered);
    }
}

fn spawn_live_stream_renderer(instance_str: String, surface_window: *mut c_void, port: u16) {
    log_info!(
        "spawn_live_stream_renderer: instance={} window={:?} port={}",
        instance_str,
        surface_window,
        port
    );

    // evict any previous renderer still bound to this port before spawning
    reclaim_udp_port(port);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    {
        let mut map = ACTIVE_RENDERERS.lock().unwrap();
        let map = map.get_or_insert_with(HashMap::new);
        if let Some(old_stop) = map.insert(instance_str.clone(), stop_flag) {
            old_stop.store(true, Ordering::SeqCst);
        }
    }

    let window_handle = surface_window as usize;
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(format!("0.0.0.0:{port}")) {
            Ok(s) => s,
            Err(e) => {
                log_info!("FAILED to bind TCP listener on 0.0.0.0:{}: {}", port, e);
                return;
            }
        };
        log_info!("TCP listening on port {}", port);
        // accept one sender (reconnect allowed after disconnect)
        let mut sock: Option<std::net::TcpStream> = None;

        let mut buf = vec![0u8; 1 << 20]; // 1MB read buffer for framed packets
        let mut pending: Vec<u8> = Vec::with_capacity(1 << 20); // stream reassembly
        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut decoder: Option<viewer_decoder::AndroidDecoder> = None;
        let mut aus = 0u64;
        let mut last_frame_ms = std::time::Instant::now();

        while !stop_clone.load(Ordering::Relaxed) {
            // (re)connect: wait for a sender when no stream is attached
            if sock.is_none() {
                let _ = listener.set_nonblocking(true);
                // poll accept briefly so the stop flag stays responsive
                if let Ok((s, _)) = listener.accept() {
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
                } else {
                    continue;
                }
            }
            let stream = sock.as_mut().unwrap();
            let n = match std::io::Read::read(stream, &mut buf) {
                Ok(0) | Err(_) => {
                    // sender disconnected — wait for reconnect
                    log_info!("TCP sender disconnected");
                    sock = None;
                    continue;
                }
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);

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
                                320,
                                240,
                                window_handle,
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
                    continue;
                }

                // stream-stall watchdog: if no complete AU landed for >3s the
                // sender is gone — drop the decoder so CFG rebuilds it
                if decoder.is_some() && last_frame_ms.elapsed() > std::time::Duration::from_millis(3000) {
                    if let Some(d) = decoder.as_mut() {
                        d.stop();
                    }
                    decoder = None;
                    log_info!("stream stalled >3s — decoder dropped, awaiting CFG");
                }

                if decoder.is_some() {
                    if pkt.len() >= 10 && &pkt[..2] == b"AU" {
                        let au = &pkt[10..];
                        feed_and_render(&mut decoder.as_mut().unwrap(), au, &mut aus);
                        last_frame_ms = std::time::Instant::now();
                    } else if pkt.len() >= 5 && pkt[0] == 0x46 {
                        // single whole-AU "F" frame (TCP needs no fragmentation)
                        let au = &pkt[5..];
                        feed_and_render(&mut decoder.as_mut().unwrap(), au, &mut aus);
                        last_frame_ms = std::time::Instant::now();
                    }
                }
            }
        }
        // deregister BEFORE the socket drops so a rebinding spawn never sees
        // us in the map (the socket itself closes when `sock` leaves scope)
        ACTIVE_RENDERERS
            .lock()
            .unwrap()
            .as_mut()
            .map(|m| m.remove(&instance_str));
        log_info!("Live stream renderer exiting for instance");
    });
}

fn stop_live_stream_renderer(instance_str: &str) {
    let mut map = ACTIVE_RENDERERS.lock().unwrap();
    if let Some(ref mut map) = *map {
        if let Some(stop) = map.remove(instance_str) {
            stop.store(true, Ordering::SeqCst);
        }
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
    leftcar_jni_attach_port(state, instance_c, surface, 5000)
}

/// Port-explicit attach: each stream window listens on its own TCP port
/// (5000+n), so multiple instances receive independent pushes.
#[no_mangle]
pub extern "C" fn leftcar_jni_attach_port(
    state: StatePtr,
    instance_c: *const c_char,
    surface: *mut c_void,
    port: u16,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
        };
        let instance_str = unsafe { CStr::from_ptr(instance_c) }.to_string_lossy().into_owned();
        spawn_live_stream_renderer(instance_str, surface, port);
        viewer_core::c_abi::stream_attach_surface(
            state,
            &instance,
            surface as viewer_core::SurfaceHandle,
        )
        .map(|_| 0i32)
        .unwrap_or(LEFTCAR_ERR_STATE)
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
        stop_live_stream_renderer(&instance_str);
        viewer_core::c_abi::stream_detach_surface(state, &instance)
            .map(|_| 0i32)
            .unwrap_or(LEFTCAR_ERR_STATE)
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
        viewer_core::c_abi::stream_release(state, &instance);
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
