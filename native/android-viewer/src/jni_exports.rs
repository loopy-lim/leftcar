//! Android C ABI entry points delegated to by the small JNI conversion shim.

use crate::jni::*;
use crate::log_info;
use crate::media_crypto::SharedMediaCrypto;
use crate::net_guard::host_is_valid;
use crate::prepared_udp::split_ports;
use crate::usb_bridge::UsbBridge;
use std::ffi::{c_char, c_void, CStr};
use std::sync::Arc;

/// Decode the media key delivered through JNI. Exactly 32 bytes; the JS side
/// generates them with `randomBytes(32)` and the Kotlin bridge copies the
/// array verbatim.
fn media_key_from_parts(key: *const u8, key_len: usize) -> Option<[u8; 32]> {
    if key.is_null() || key_len != 32 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(key, key_len) };
    bytes.try_into().ok()
}

// The renderer session modules are Android-gated in renderer/mod.rs (they
// drive MediaCodec/ANativeWindow through the decoder). Host test builds only
// need the registry-based polling exports, so the four session entry points
// collapse to documented stubs off-Android. No host test may call them.
#[cfg(target_os = "android")]
use crate::renderer::single_session::{
    spawn_live_stream_renderer, stop_live_stream_renderer, suppress_resize_recovery,
    suspend_live_stream_renderer,
};
#[cfg(target_os = "android")]
use crate::renderer::split_session::{self, SplitRendererLaunch};

#[cfg(not(target_os = "android"))]
mod session_stubs {
    use std::ffi::c_void;

    pub(crate) fn suppress_resize_recovery(_instance_str: &str) {}
    pub(crate) fn suspend_live_stream_renderer(_instance_str: &str) {}
    pub(crate) fn stop_live_stream_renderer(_instance_str: &str, _send_bye: bool) {}
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_live_stream_renderer(
        _instance: String,
        _surface: *mut c_void,
        _port: u16,
        _host: String,
        _width: u32,
        _height: u32,
        _fps: u32,
        _bridge: Option<crate::jni::MediaBridge>,
    ) {
    }
}

#[cfg(not(target_os = "android"))]
use session_stubs::{
    spawn_live_stream_renderer, stop_live_stream_renderer, suppress_resize_recovery,
    suspend_live_stream_renderer,
};

mod session_io;

/// Shared preamble for the state-taking exports: resolve the process state
/// pointer and the stream instance id, or yield LEFTCAR_ERR_NULL.
///
/// # Safety
/// `state` must be a valid `StatePtr` and `instance_c` a valid C string.
unsafe fn state_and_instance<'a>(
    state: StatePtr,
    instance_c: *const c_char,
) -> Result<
    (
        &'a mut viewer_core::ProcessState,
        viewer_core::StreamInstanceId,
    ),
    i32,
> {
    let Some(state) = (unsafe { state.as_mut() }) else {
        return Err(LEFTCAR_ERR_NULL);
    };
    let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
        return Err(LEFTCAR_ERR_NULL);
    };
    Ok((state, instance))
}

fn host_from_cstr(host_c: *const c_char) -> String {
    if host_c.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(host_c) }
            .to_string_lossy()
            .into_owned()
    }
}

fn transport_from_cstr(transport_c: *const c_char) -> String {
    if transport_c.is_null() {
        "udp".to_owned()
    } else {
        unsafe { CStr::from_ptr(transport_c) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
use session_io::leftcar_jni_termination_reason;

extern "C" {
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
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
    key: *const u8,
    key_len: usize,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(media_key) = media_key_from_parts(key, key_len) else {
            return LEFTCAR_ERR_INVALID;
        };
        if host_c.is_null() {
            return LEFTCAR_ERR_NULL;
        }
        let host = host_from_cstr(host_c);
        let transport = transport_from_cstr(transport_c);
        if !matches!(
            transport.as_str(),
            "udp" | "tcp" | "adbTcp" | "usb" | "auto"
        ) {
            return LEFTCAR_ERR_INVALID;
        }
        match prepare_udp_receiver(port, &host, &transport, &media_key) {
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

/// Prepare the two consecutive UDP listeners used by the vertical 4K split.
/// The operation is atomic from the caller's perspective: if the right port
/// cannot be prepared, the left listener is rolled back as well.
#[no_mangle]
pub extern "C" fn leftcar_jni_prepare_split_port(
    base_port: u16,
    host_c: *const c_char,
    transport_c: *const c_char,
    key: *const u8,
    key_len: usize,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(media_key) = media_key_from_parts(key, key_len) else {
            return LEFTCAR_ERR_INVALID;
        };
        if host_c.is_null() {
            return LEFTCAR_ERR_NULL;
        }
        let host = host_from_cstr(host_c);
        let transport = transport_from_cstr(transport_c);
        if transport != "udp" || !host_is_valid(&host) {
            return LEFTCAR_ERR_INVALID;
        }
        let Ok((left_port, right_port)) = split_ports(base_port) else {
            return LEFTCAR_ERR_INVALID;
        };
        // Both tiles share ONE media-crypto instance: their sends must stay
        // inside the host's single c2s replay window, so the counter sequence
        // must not fork between the left and right sockets.
        let shared: SharedMediaCrypto =
            Arc::new(crate::media_crypto::MediaSessionCrypto::new(media_key));
        {
            let mut guard = crate::jni::MEDIA_CRYPTO.lock().unwrap();
            let map = guard.get_or_insert_with(Default::default);
            map.insert(left_port, Arc::clone(&shared));
            map.insert(right_port, Arc::clone(&shared));
        }
        if let Err(error) = prepare_split_receiver(left_port, &host, &shared) {
            log_info!("failed to prepare split left listener: {error}");
            return LEFTCAR_ERR_STATE;
        }
        if let Err(error) = prepare_split_receiver(right_port, &host, &shared) {
            let _ = cancel_prepared_receiver(left_port);
            log_info!("failed to prepare split right listener: {error}");
            return LEFTCAR_ERR_STATE;
        }
        log_info!("prepared split UDP listeners on ports {left_port}/{right_port} for {host}");
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

/// Claim the current Android UsbAccessory file descriptor. The Kotlin module
/// calls this before `prepare_port`; the bridge's loopback control port is
/// then used by the JS control client.
#[no_mangle]
pub extern "C" fn leftcar_jni_prepare_usb(fd: i32, key: *const u8, key_len: usize) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(media_key) = media_key_from_parts(key, key_len) else {
            return LEFTCAR_ERR_INVALID;
        };
        let bridge = match UsbBridge::start(fd, media_key) {
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

/// Re-point the live USB bridge's media crypto at a shared instance. Used
/// when the accessory attached before JS generated the session key.
#[no_mangle]
pub extern "C" fn leftcar_jni_set_usb_media_key(key: *const u8, key_len: usize) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Some(key) = media_key_from_parts(key, key_len) else {
            return LEFTCAR_ERR_INVALID;
        };
        let bridge = PREPARED_USB_BRIDGE.lock().unwrap();
        match bridge.as_ref() {
            Some(bridge) => {
                bridge.set_media_crypto(std::sync::Arc::new(
                    crate::media_crypto::MediaSessionCrypto::new(key),
                ));
                LEFTCAR_OK
            }
            None => LEFTCAR_ERR_STATE,
        }
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

#[no_mangle]
pub extern "C" fn leftcar_jni_cancel_prepared_split(base_port: u16) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let Ok((left_port, right_port)) = split_ports(base_port) else {
            return LEFTCAR_ERR_INVALID;
        };
        let _ = cancel_prepared_receiver(left_port);
        let _ = cancel_prepared_receiver(right_port);
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
        };
        // No paired host = no stream. Validate BEFORE attaching the surface:
        // on this error path the wrapper releases its ANativeWindow ref and
        // the core must not still hold a registered handle (double release).
        let host = host_from_cstr(host_c);
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

/// Replace a live single-stream renderer without releasing the Activity's
/// Surface. The old worker is joined before Surface ownership is transferred,
/// so packets from the previous generation cannot publish into the new one.
#[no_mangle]
pub extern "C" fn leftcar_jni_rebind_port(
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
        };
        if surface.is_null() || port == 0 {
            return LEFTCAR_ERR_INVALID;
        }
        let host = host_from_cstr(host_c);
        if !host_is_valid(&host) {
            return LEFTCAR_ERR_INVALID;
        }
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        stop_live_stream_renderer(&instance_str, false);
        reclaim_udp_port(port);
        let old_surface = state.attached_surface(&instance);
        if viewer_core::c_abi::stream_detach_surface(state, &instance).is_err() {
            return LEFTCAR_ERR_STATE;
        }
        if let Some(old_surface) = old_surface {
            unsafe { ANativeWindow_release(old_surface as *mut c_void) };
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
        spawn_live_stream_renderer(
            instance_str,
            surface,
            port,
            host,
            width,
            height,
            fps,
            take_media_bridge(port),
        );
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

/// Attach the two SurfaceViews backing an exact 4K60 vertical split stream.
/// The core tracks the left surface as the logical Activity attachment; the
/// split renderer owns both decoders and releases the right window itself.
/// Android-only: the split session drives two hardware decoders, and host
/// test builds never spawn it.
#[no_mangle]
#[cfg(target_os = "android")]
pub extern "C" fn leftcar_jni_attach_split_port(
    state: StatePtr,
    instance_c: *const c_char,
    left_surface: *mut c_void,
    right_surface: *mut c_void,
    base_port: u16,
    host_c: *const c_char,
    width: u32,
    height: u32,
    fps: u32,
    decoder_name_c: *const c_char,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
        };
        if left_surface.is_null() || right_surface.is_null() {
            return LEFTCAR_ERR_NULL;
        }
        if width != 3840 || height != 2160 || fps != 60 {
            return LEFTCAR_ERR_INVALID;
        }
        let decoder_name = if decoder_name_c.is_null() {
            return LEFTCAR_ERR_NULL;
        } else {
            match unsafe { CStr::from_ptr(decoder_name_c) }.to_str() {
                Ok(name) if !name.is_empty() => name.to_owned(),
                _ => return LEFTCAR_ERR_INVALID,
            }
        };
        let host = host_from_cstr(host_c);
        let Ok((left_port, right_port)) = split_ports(base_port) else {
            return LEFTCAR_ERR_INVALID;
        };
        if !host_is_valid(&host) {
            return LEFTCAR_ERR_INVALID;
        }
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();

        stop_live_stream_renderer(&instance_str, false);
        reclaim_udp_port(left_port);
        reclaim_udp_port(right_port);
        // Claim both preflight sockets atomically: either both come back or
        // the store is left untouched, so a half-present pair can never
        // strand the other side's UDP listener (P0-6).
        let Some(receivers) = take_split_receivers(left_port, right_port, &host) else {
            return LEFTCAR_ERR_STATE;
        };
        if viewer_core::c_abi::stream_attach_surface(
            state,
            &instance,
            left_surface as viewer_core::SurfaceHandle,
        )
        .is_err()
        {
            // Return the claimed sockets (with any captured challenge token)
            // so a retried attach can claim them again instead of always
            // failing on an empty store. On Android the wrapper releases both
            // ANativeWindow refs when this returns non-zero
            // (attach_split_body), so no surface ownership is leaked here.
            restore_prepared_receiver(left_port, receivers.left);
            restore_prepared_receiver(right_port, receivers.right);
            return LEFTCAR_ERR_STATE;
        }

        let control = Arc::new(RendererControl::new_split(base_port, fps));
        install_renderer(&instance_str, Arc::clone(&control));
        let launch = SplitRendererLaunch {
            instance: instance_str,
            expected_host: host.clone(),
            fps,
            left_window: left_surface as usize,
            right_window: right_surface as usize,
            left_receiver: receivers.left,
            right_receiver: receivers.right,
            decoder_name,
            control: Arc::clone(&control),
        };
        if let Err(error) = split_session::spawn(launch) {
            remove_renderer_if_current(
                unsafe { CStr::from_ptr(instance_c) }
                    .to_string_lossy()
                    .as_ref(),
                &control,
            );
            control.mark_finished();
            let _ = viewer_core::c_abi::stream_detach_surface(state, &instance);
            // `spawn` already consumed both receivers into the coordinator
            // thread, so they cannot be put back. Best-effort fresh binds
            // keep a retried attach from hitting an empty store forever.
            // 같은 세션 키로 수신기를 되살린다 — 좌우 타일이 하나의 크립토
            // 인스턴스를 공유하는 규칙(prepare_split_stream과 동일)도 지킨다.
            // 포트마다 인스턴스를 새로 만들면 c2s 카운터가 갈라져 호스트의
            // 단일 재생 창이 두 번째 타일의 도전 에코를 재생으로 거부한다.
            // 크립토가 이미 소모됐다면 바인드를 건너뛴다 — 키 없는 평문
            // 수신기는 만들지 않는다.
            if let (Some(left_crypto), Some(right_crypto)) = (
                crate::jni::media_crypto_for(left_port),
                crate::jni::media_crypto_for(right_port),
            ) {
                debug_assert_eq!(left_crypto.session_key(), right_crypto.session_key());
                reclaim_udp_port(left_port);
                reclaim_udp_port(right_port);
                let _ = crate::jni::prepare_split_receiver(left_port, &host, &left_crypto);
                let _ = crate::jni::prepare_split_receiver(right_port, &host, &left_crypto);
            }
            log_info!("failed to start split renderer: {error}");
            return LEFTCAR_ERR_STATE;
        }
        LEFTCAR_OK
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
        };
        let instance_str = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        let split = active_renderer(&instance_str).is_some_and(|control| control.is_split());
        if split {
            stop_live_stream_renderer(&instance_str, false);
        } else {
            suspend_live_stream_renderer(&instance_str);
        }
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
        };
        let Some(event) = crate::map_lifecycle(event_code) else {
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
        let (state, instance) = match unsafe { state_and_instance(state, instance_c) } {
            Ok(resolved) => resolved,
            Err(code) => return code,
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
    use std::sync::atomic::Ordering;

    #[test]
    fn local_termination_survives_renderer_removal_until_reuse() {
        for (instance, reason) in [
            (
                "termination-cache-host-unreachable",
                crate::LOCAL_TERMINATION_HOST_UNREACHABLE,
            ),
            (
                "termination-cache-render-stalled",
                crate::LOCAL_TERMINATION_RENDER_STALLED,
            ),
        ] {
            let control = Arc::new(RendererControl::new_split(50_000, 60));
            control.termination_reason.store(reason, Ordering::SeqCst);
            install_renderer(instance, Arc::clone(&control));

            remove_renderer_if_current(instance, &control);
            assert!(active_renderer(instance).is_none());
            let instance_c = CString::new(instance).unwrap();
            assert_eq!(
                leftcar_jni_termination_reason(instance_c.as_ptr()),
                i32::from(reason)
            );

            clear_cached_termination(instance);
            assert_eq!(leftcar_jni_termination_reason(instance_c.as_ptr()), -1);
        }
    }
}

#[cfg(test)]
mod split_prepare_tests {
    use super::*;
    use crate::jni::{media_crypto_for, take_media_crypto};
    use std::ffi::CString;

    fn free_split_base_port() -> u16 {
        let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let base = probe.local_addr().unwrap().port();
        drop(probe);
        if std::net::UdpSocket::bind(("127.0.0.1", base.saturating_add(1))).is_ok() {
            base
        } else {
            free_split_base_port()
        }
    }

    /// 좌우 타일은 하나의 크립토 인스턴스를 공유해야 한다 — 인스턴스가 포트마다
    /// 갈라지면 c2s 카운터가 두 벌이 되어 호스트의 단일 재생 창이 두 번째
    /// 타일의 도전 에코를 재생으로 거부한다(0c4cee0 폴백이 저질렀던 실패).
    #[test]
    fn split_prepare_registers_one_shared_crypto_instance() {
        // 전역 PREPARED_RECEIVERS/MEDIA_CRYPTO를 건드린다 — 절대 상태를
        // 검증하는 다른 스토어 테스트와 직렬화한다.
        let _serial = crate::jni::TEST_STORE_LOCK.lock().unwrap();
        let base = free_split_base_port();
        let Ok((left, right)) = split_ports(base) else {
            panic!("no split port pair around {base}");
        };
        let host = CString::new("127.0.0.1").unwrap();
        let key: [u8; 32] = core::array::from_fn(|i| (i + 1) as u8);
        let rc = leftcar_jni_prepare_split_port(
            base,
            host.as_ptr(),
            std::ptr::null(),
            key.as_ptr(),
            key.len(),
        );
        assert_eq!(rc, LEFTCAR_OK);
        let left_crypto = media_crypto_for(left).expect("left crypto registered");
        let right_crypto = media_crypto_for(right).expect("right crypto registered");
        assert!(
            Arc::ptr_eq(&left_crypto, &right_crypto),
            "both tiles must share one crypto instance"
        );
        assert_eq!(left_crypto.session_key(), key);

        take_media_crypto(left);
        take_media_crypto(right);
        let _ = crate::jni::cancel_prepared_receiver(left);
        let _ = crate::jni::cancel_prepared_receiver(right);
    }
}
