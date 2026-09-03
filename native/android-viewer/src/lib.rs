//! Leftcar Android viewer native library (C ABI, docs/05 §8.2).
//!
//! Exactly six exported symbols. Kotlin shim calls only these; encoded frame
//! bytes, codec config and decoder loops never enter Kotlin or TypeScript.
//!
//! Panics never cross the C boundary: every entry catches unwind and returns
//! a stable error code (docs/07 §14).

use std::ffi::{c_char, CStr};

pub mod cursor_protocol;
pub mod input_protocol;
#[cfg(any(target_os = "android", test))]
#[cfg_attr(test, allow(dead_code))]
pub mod jni;
#[cfg(target_os = "android")]
mod jni_exports;
#[cfg(target_os = "android")]
pub mod jni_wrappers;
pub mod media_datagram;
/// Host-testable media-plane peer admission check. Not android-gated so
/// `cargo test --workspace` (host target only in CI) exercises it.
pub mod net_guard;
pub mod prepared_tcp;
/// Port preflight used before Android opens a stream Activity. Keeping this
/// host-testable lets CI exercise the reachability race fix without an APK.
pub mod prepared_udp;
pub mod renderer;
mod socket_tuning;
pub mod usb_bridge;
use std::time::Duration;

pub const LEFTCAR_OK: i32 = 0;
pub const LEFTCAR_ERR_NULL: i32 = 1;
pub const LEFTCAR_ERR_STATE: i32 = 2;
pub const LEFTCAR_ERR_PANIC: i32 = 3;
pub const LEFTCAR_ERR_INVALID: i32 = 4;

#[cfg(any(target_os = "android", test))]
pub(crate) const LOCAL_TERMINATION_HOST_UNREACHABLE: i8 = 4;
#[cfg(any(target_os = "android", test))]
pub(crate) const LOCAL_TERMINATION_RENDER_STALLED: i8 = 5;

#[cfg(any(target_os = "android", test))]
pub(crate) const fn cacheable_termination_reason(reason: i8) -> Option<i8> {
    if reason >= 0 {
        Some(reason)
    } else {
        None
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn record_first_termination_reason(
    target: &std::sync::atomic::AtomicI8,
    reason: i8,
) -> i8 {
    if reason < 0 {
        return target.load(std::sync::atomic::Ordering::SeqCst);
    }
    match target.compare_exchange(
        -1,
        reason,
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
    ) {
        Ok(_) => reason,
        Err(existing) => existing,
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) const fn surface_release_latency_value(value: u64) -> i32 {
    if value == u64::MAX {
        0xffff
    } else if value > 0xfffe {
        0xfffe
    } else {
        value as i32
    }
}

#[cfg(test)]
mod surface_release_latency_tests {
    use super::*;

    #[test]
    fn surface_release_latency_preserves_unmeasured_and_clamps_values() {
        assert_eq!(surface_release_latency_value(u64::MAX), 0xffff);
        assert_eq!(surface_release_latency_value(42), 42);
        assert_eq!(surface_release_latency_value(u64::MAX - 1), 0xfffe);
    }
}

#[cfg(test)]
mod local_termination_reason_tests {
    use super::*;
    use std::sync::atomic::{AtomicI8, Ordering};

    #[test]
    fn local_termination_reasons_four_and_five_are_cacheable() {
        assert_eq!(LOCAL_TERMINATION_HOST_UNREACHABLE, 4);
        assert_eq!(LOCAL_TERMINATION_RENDER_STALLED, 5);
        assert_eq!(cacheable_termination_reason(-1), None);
        assert_eq!(
            cacheable_termination_reason(LOCAL_TERMINATION_HOST_UNREACHABLE),
            Some(4)
        );
        assert_eq!(
            cacheable_termination_reason(LOCAL_TERMINATION_RENDER_STALLED),
            Some(5)
        );
    }

    #[test]
    fn first_nonnegative_termination_reason_wins_atomically() {
        let host_first = AtomicI8::new(-1);
        assert_eq!(record_first_termination_reason(&host_first, 2), 2);
        assert_eq!(record_first_termination_reason(&host_first, 5), 2);
        assert_eq!(host_first.load(Ordering::SeqCst), 2);

        let local_four_first = AtomicI8::new(-1);
        assert_eq!(record_first_termination_reason(&local_four_first, 4), 4);
        assert_eq!(record_first_termination_reason(&local_four_first, 5), 4);
        assert_eq!(local_four_first.load(Ordering::SeqCst), 4);
    }
}

mod lifecycle {
    pub const ACTIVITY_CREATE: u32 = 1;
    pub const ACTIVITY_START: u32 = 2;
    pub const ACTIVITY_RESUME: u32 = 3;
    pub const FOCUS_GAIN: u32 = 4;
    pub const FOCUS_LOSS: u32 = 5;
    pub const SURFACE_CREATE: u32 = 6;
    pub const SURFACE_CHANGE: u32 = 7;
    pub const SURFACE_DESTROY: u32 = 8;
    pub const ACTIVITY_PAUSE: u32 = 9;
    pub const ACTIVITY_STOP: u32 = 10;
    pub const CONFIGURATION_CHANGE: u32 = 11;
    pub const TASK_REMOVE: u32 = 12;
    pub const PROCESS_DEATH: u32 = 13;
}

fn map_lifecycle(code: u32) -> Option<viewer_core::LifecycleEvent> {
    use viewer_core::LifecycleEvent as L;
    Some(match code {
        lifecycle::ACTIVITY_CREATE => L::ActivityCreate,
        lifecycle::ACTIVITY_START => L::ActivityStart,
        lifecycle::ACTIVITY_RESUME => L::ActivityResume,
        lifecycle::FOCUS_GAIN => L::FocusGain,
        lifecycle::FOCUS_LOSS => L::FocusLoss,
        lifecycle::SURFACE_CREATE => L::SurfaceCreate,
        lifecycle::SURFACE_CHANGE => L::SurfaceChange,
        lifecycle::SURFACE_DESTROY => L::SurfaceDestroy,
        lifecycle::ACTIVITY_PAUSE => L::ActivityPause,
        lifecycle::ACTIVITY_STOP => L::ActivityStop,
        lifecycle::CONFIGURATION_CHANGE => L::ConfigurationChange,
        lifecycle::TASK_REMOVE => L::TaskRemove,
        lifecycle::PROCESS_DEATH => L::ProcessDeath,
        _ => return None,
    })
}

/// # Safety
/// `instance_json` must be a valid NUL-terminated UTF-8 string.
unsafe fn parse_instance(
    instance_json: *const c_char,
) -> Result<domain::ids::StreamInstanceId, i32> {
    if instance_json.is_null() {
        return Err(LEFTCAR_ERR_NULL);
    }
    let cstr = unsafe { CStr::from_ptr(instance_json) };
    let s = cstr.to_str().map_err(|_| LEFTCAR_ERR_INVALID)?;
    domain::ids::StreamInstanceId::from_raw(s).map_err(|_| LEFTCAR_ERR_INVALID)
}

/// Raw-pointer logic lives in an unsafe impl block so the extern "C" entry
/// points stay safe functions (clippy not_unsafe_ptr_arg_deref) while the
/// invariants stay documented at one place.
struct Abi;
impl Abi {
    /// # Safety
    /// `state` from process_start, `instance_json` valid C string.
    unsafe fn attach(
        state: *mut viewer_core::ProcessState,
        instance_json: *const c_char,
        surface: viewer_core::SurfaceHandle,
    ) -> Result<(), i32> {
        let state = state.as_mut().ok_or(LEFTCAR_ERR_NULL)?;
        let instance = parse_instance(instance_json)?;
        viewer_core::c_abi::stream_attach_surface(state, &instance, surface).map_err(|e| match e {
            viewer_core::CAbiError::NullSurface => LEFTCAR_ERR_INVALID,
            _ => LEFTCAR_ERR_STATE,
        })
    }

    /// # Safety
    /// `state` from process_start, `instance_json` valid C string.
    unsafe fn surface_changed(
        state: *mut viewer_core::ProcessState,
        instance_json: *const c_char,
        width: u32,
        height: u32,
    ) -> Result<(), i32> {
        let state = state.as_mut().ok_or(LEFTCAR_ERR_NULL)?;
        let instance = parse_instance(instance_json)?;
        viewer_core::c_abi::stream_surface_changed(state, &instance, width, height);
        Ok(())
    }

    /// # Safety
    /// `state` from process_start, `instance_json` valid C string.
    unsafe fn detach(
        state: *mut viewer_core::ProcessState,
        instance_json: *const c_char,
    ) -> Result<(), i32> {
        let state = state.as_mut().ok_or(LEFTCAR_ERR_NULL)?;
        let instance = parse_instance(instance_json)?;
        viewer_core::c_abi::stream_detach_surface(state, &instance).map_err(|e| match e {
            viewer_core::CAbiError::DetachWithoutAttach
            | viewer_core::CAbiError::InstanceCrossing => LEFTCAR_ERR_STATE,
            _ => LEFTCAR_ERR_INVALID,
        })
    }

    /// # Safety
    /// `state` from process_start, `instance_json` valid C string.
    unsafe fn update_window_state(
        state: *mut viewer_core::ProcessState,
        instance_json: *const c_char,
        event_code: u32,
        monotonic_ms: u64,
    ) -> Result<(), i32> {
        let state = state.as_mut().ok_or(LEFTCAR_ERR_NULL)?;
        let instance = parse_instance(instance_json)?;
        let event = map_lifecycle(event_code).ok_or(LEFTCAR_ERR_INVALID)?;
        viewer_core::c_abi::stream_update_window_state(
            state,
            &instance,
            event,
            Duration::from_millis(monotonic_ms),
        );
        Ok(())
    }

    /// # Safety
    /// `state` from process_start, `instance_json` valid C string.
    unsafe fn release(
        state: *mut viewer_core::ProcessState,
        instance_json: *const c_char,
    ) -> Result<(), i32> {
        let state = state.as_mut().ok_or(LEFTCAR_ERR_NULL)?;
        let instance = parse_instance(instance_json)?;
        viewer_core::c_abi::stream_release(state, &instance);
        Ok(())
    }
}

/// Start the viewer process state. Returns an opaque handle.
#[no_mangle]
pub extern "C" fn leftcar_viewer_process_start() -> *mut viewer_core::ProcessState {
    let raw = viewer_core::c_abi::process_start();
    if raw.is_null() {
        std::ptr::null_mut()
    } else {
        raw
    }
}

/// Attach a Surface (opaque native-window handle) to a stream instance.
///
/// # Safety
/// `state` must come from `leftcar_viewer_process_start`; `instance_json` a
/// valid C string; `surface` a nonzero handle owned by the caller.
// C ABI boundary: callers (Kotlin shim) uphold pointer invariants; the lint
// cannot see across the FFI edge.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn leftcar_stream_attach_surface(
    state: *mut viewer_core::ProcessState,
    instance_json: *const c_char,
    surface: viewer_core::SurfaceHandle,
) -> i32 {
    let guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        Abi::attach(state, instance_json, surface)
    }));
    match guard {
        Ok(Ok(())) => LEFTCAR_OK,
        Ok(Err(code)) => code,
        Err(_) => LEFTCAR_ERR_PANIC,
    }
}

/// Notify Surface geometry change.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn leftcar_stream_surface_changed(
    state: *mut viewer_core::ProcessState,
    instance_json: *const c_char,
    width: u32,
    height: u32,
) -> i32 {
    let guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        Abi::surface_changed(state, instance_json, width, height)
    }));
    match guard {
        Ok(Ok(())) => LEFTCAR_OK,
        Ok(Err(code)) => code,
        Err(_) => LEFTCAR_ERR_PANIC,
    }
}

/// Detach the Surface. At most one detach per attach (docs/05 §8.2).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn leftcar_stream_detach_surface(
    state: *mut viewer_core::ProcessState,
    instance_json: *const c_char,
) -> i32 {
    let guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        Abi::detach(state, instance_json)
    }));
    match guard {
        Ok(Ok(())) => LEFTCAR_OK,
        Ok(Err(code)) => code,
        Err(_) => LEFTCAR_ERR_PANIC,
    }
}

/// Forward an Android lifecycle event (see `lifecycle::` codes).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn leftcar_stream_update_window_state(
    state: *mut viewer_core::ProcessState,
    instance_json: *const c_char,
    event_code: u32,
    monotonic_ms: u64,
) -> i32 {
    let guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        Abi::update_window_state(state, instance_json, event_code, monotonic_ms)
    }));
    match guard {
        Ok(Ok(())) => LEFTCAR_OK,
        Ok(Err(code)) => code,
        Err(_) => LEFTCAR_ERR_PANIC,
    }
}

/// Release the stream instance state.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn leftcar_stream_release(
    state: *mut viewer_core::ProcessState,
    instance_json: *const c_char,
) -> i32 {
    let guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        Abi::release(state, instance_json)
    }));
    match guard {
        Ok(Ok(())) => LEFTCAR_OK,
        Ok(Err(code)) => code,
        Err(_) => LEFTCAR_ERR_PANIC,
    }
}

// -- tests: panic containment + ABI surface --------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn six_abi_symbols_roundtrip() {
        let state = leftcar_viewer_process_start();
        assert!(!state.is_null());
        let instance = CString::new("instance-1").unwrap();
        let ip = instance.as_ptr();

        assert_eq!(leftcar_stream_attach_surface(state, ip, 0x1234), LEFTCAR_OK);
        assert_eq!(
            leftcar_stream_surface_changed(state, ip, 1920, 1080),
            LEFTCAR_OK
        );
        assert_eq!(
            leftcar_stream_update_window_state(state, ip, lifecycle::SURFACE_CREATE, 0),
            LEFTCAR_OK
        );
        assert_eq!(leftcar_stream_detach_surface(state, ip), LEFTCAR_OK);
        assert_eq!(leftcar_stream_release(state, ip), LEFTCAR_OK);
        // idempotent release of process
        unsafe { drop(Box::from_raw(state)) };
    }

    #[test]
    fn null_surface_handle_is_invalid() {
        let state = leftcar_viewer_process_start();
        let instance = CString::new("i").unwrap();
        assert_eq!(
            leftcar_stream_attach_surface(state, instance.as_ptr(), 0),
            LEFTCAR_ERR_INVALID
        );
        unsafe { drop(Box::from_raw(state)) };
    }

    #[test]
    fn null_pointers_return_null_error() {
        let instance = CString::new("i").unwrap();
        let ip = instance.as_ptr();
        assert_eq!(
            leftcar_stream_attach_surface(std::ptr::null_mut(), ip, 1),
            LEFTCAR_ERR_NULL
        );
        let state = leftcar_viewer_process_start();
        assert_eq!(
            leftcar_stream_attach_surface(state, std::ptr::null(), 1),
            LEFTCAR_ERR_NULL
        );
        unsafe { drop(Box::from_raw(state)) };
    }

    #[test]
    fn invalid_lifecycle_code_rejected() {
        let state = leftcar_viewer_process_start();
        let instance = CString::new("i").unwrap();
        assert_eq!(
            leftcar_stream_update_window_state(state, instance.as_ptr(), 9999, 0),
            LEFTCAR_ERR_INVALID
        );
        unsafe { drop(Box::from_raw(state)) };
    }

    #[test]
    fn double_detach_is_state_error_not_crash() {
        let state = leftcar_viewer_process_start();
        let instance = CString::new("i").unwrap();
        let ip = instance.as_ptr();
        leftcar_stream_attach_surface(state, ip, 1);
        leftcar_stream_detach_surface(state, ip);
        assert_eq!(leftcar_stream_detach_surface(state, ip), LEFTCAR_ERR_STATE);
        unsafe { drop(Box::from_raw(state)) };
    }
}
