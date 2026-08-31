//! Android C ABI entry points delegated to by the small JNI conversion shim.

use crate::jni::*;
use crate::log_info;
use crate::net_guard::host_is_valid;
use crate::prepared_udp::split_ports;
use crate::renderer::single_session::{
    spawn_live_stream_renderer, stop_live_stream_renderer, suppress_resize_recovery,
    suspend_live_stream_renderer,
};
use crate::renderer::split_session::{self, SplitRendererLaunch};
use crate::usb_bridge::UsbBridge;
use std::ffi::{c_char, c_void, CStr};
use std::sync::Arc;

mod session_io;

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

/// Prepare the two consecutive UDP listeners used by the vertical 4K split.
/// The operation is atomic from the caller's perspective: if the right port
/// cannot be prepared, the left listener is rolled back as well.
#[no_mangle]
pub extern "C" fn leftcar_jni_prepare_split_port(
    base_port: u16,
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
        if transport != "udp" || !host_is_valid(&host) {
            return LEFTCAR_ERR_INVALID;
        }
        let Ok((left_port, right_port)) = split_ports(base_port) else {
            return LEFTCAR_ERR_INVALID;
        };
        if let Err(error) = prepare_udp_receiver(left_port, &host, "udp") {
            log_info!("failed to prepare split left listener: {error}");
            return LEFTCAR_ERR_STATE;
        }
        if let Err(error) = prepare_udp_receiver(right_port, &host, "udp") {
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

/// Attach the two SurfaceViews backing an exact 4K60 vertical split stream.
/// The core tracks the left surface as the logical Activity attachment; the
/// split renderer owns both decoders and releases the right window itself.
#[no_mangle]
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
        let Some(state) = (unsafe { state.as_mut() }) else {
            return LEFTCAR_ERR_NULL;
        };
        let Ok(instance) = (unsafe { cstr_instance(instance_c) }) else {
            return LEFTCAR_ERR_NULL;
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
        let host = if host_c.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(host_c) }
                .to_string_lossy()
                .into_owned()
        };
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
        let Some(left_receiver) = take_prepared_receiver(left_port, &host) else {
            return LEFTCAR_ERR_STATE;
        };
        let Some(right_receiver) = take_prepared_receiver(right_port, &host) else {
            drop(left_receiver);
            return LEFTCAR_ERR_STATE;
        };
        if viewer_core::c_abi::stream_attach_surface(
            state,
            &instance,
            left_surface as viewer_core::SurfaceHandle,
        )
        .is_err()
        {
            return LEFTCAR_ERR_STATE;
        }

        let control = Arc::new(RendererControl::new_split(base_port, fps));
        install_renderer(&instance_str, Arc::clone(&control));
        let launch = SplitRendererLaunch {
            instance: instance_str,
            expected_host: host,
            fps,
            left_window: left_surface as usize,
            right_window: right_surface as usize,
            left_receiver,
            right_receiver,
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
