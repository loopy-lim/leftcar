//! JNI export shims for dev.leftcar.viewer.shim.ViewerNative.
//!
//! Kotlin calls these via `external fun`; they convert Java types (String,
//! Surface) and delegate to the C-string entries in jni.rs.

use std::ffi::{c_char, c_void, CString};

#[repr(C)]
pub struct jobject(c_void);
#[repr(C)]
pub struct JNIEnv(c_void);

// JNI vtable access: GetStringUTFChars / ReleaseStringUTFChars / ExceptionCheck
// JNIEnv is a pointer to a struct whose first field points to a function table.
unsafe fn env_functions(env: *mut JNIEnv) -> *mut *mut c_void {
    // JNIEnv* points at a struct whose first field is the function table.
    *(env as *mut *mut *mut c_void)
}

const JNI_GET_STRING_UTF_CHARS: usize = 169;
const JNI_RELEASE_STRING_UTF_CHARS: usize = 170;
const JNI_GET_ARRAY_LENGTH: usize = 171;
const JNI_SET_BYTE_ARRAY_REGION: usize = 208;
const JNI_EXCEPTION_CHECK: usize = 228;

unsafe fn get_utf(env: *mut JNIEnv, jstr: *mut jobject) -> Option<CString> {
    if env.is_null() || jstr.is_null() {
        return None;
    }
    let fns = env_functions(env);
    let get: unsafe extern "C" fn(*mut JNIEnv, *mut jobject, *mut c_void) -> *const c_char =
        std::mem::transmute(*fns.add(JNI_GET_STRING_UTF_CHARS));
    let ptr = get(env, jstr, std::ptr::null_mut());
    if ptr.is_null() {
        return None;
    }
    let s = std::ffi::CStr::from_ptr(ptr).to_owned();
    let fns = env_functions(env);
    let rel: unsafe extern "C" fn(*mut JNIEnv, *mut jobject, *const c_char) =
        std::mem::transmute(*fns.add(JNI_RELEASE_STRING_UTF_CHARS));
    rel(env, jstr, ptr);
    Some(s)
}

unsafe fn exception_pending(env: *mut JNIEnv) -> bool {
    if env.is_null() {
        return false;
    }
    let fns = env_functions(env);
    let check: unsafe extern "C" fn(*mut JNIEnv) -> u8 =
        std::mem::transmute(*fns.add(JNI_EXCEPTION_CHECK));
    check(env) != 0
}

extern "C" {
    fn ANativeWindow_fromSurface(env: *mut JNIEnv, surface: *mut jobject) -> *mut c_void;
    fn leftcar_jni_start() -> *mut c_void;
    fn leftcar_jni_attach(state: *mut c_void, instance: *const c_char, surface: *mut c_void)
        -> i32;
    fn leftcar_jni_prepare_port(port: u16, host: *const c_char, transport: *const c_char) -> i32;
    fn leftcar_jni_prepare_split_port(
        port: u16,
        host: *const c_char,
        transport: *const c_char,
    ) -> i32;
    fn leftcar_jni_prepare_usb(fd: i32) -> i32;
    fn leftcar_jni_usb_control_port() -> i32;
    fn leftcar_jni_cancel_prepared_port(port: u16) -> i32;
    fn leftcar_jni_cancel_prepared_split(port: u16) -> i32;
    fn leftcar_jni_attach_port(
        state: *mut c_void,
        instance: *const c_char,
        surface: *mut c_void,
        port: u16,
        host: *const c_char,
        width: u32,
        height: u32,
        fps: u32,
    ) -> i32;
    fn leftcar_jni_rebind_port(
        state: *mut c_void,
        instance: *const c_char,
        surface: *mut c_void,
        port: u16,
        host: *const c_char,
        width: u32,
        height: u32,
        fps: u32,
    ) -> i32;
    fn leftcar_jni_attach_split_port(
        state: *mut c_void,
        instance: *const c_char,
        left_surface: *mut c_void,
        right_surface: *mut c_void,
        port: u16,
        host: *const c_char,
        width: u32,
        height: u32,
        fps: u32,
        decoder_name: *const c_char,
    ) -> i32;
    fn leftcar_jni_surface_changed(
        state: *mut c_void,
        instance: *const c_char,
        w: u32,
        h: u32,
    ) -> i32;
    fn leftcar_jni_detach(state: *mut c_void, instance: *const c_char) -> i32;
    fn leftcar_jni_update_window(
        state: *mut c_void,
        instance: *const c_char,
        e: u32,
        t: u64,
    ) -> i32;
    fn leftcar_jni_release(state: *mut c_void, instance: *const c_char) -> i32;
    fn leftcar_jni_surface_ref(surface: *mut c_void, acquire: bool);
    fn leftcar_jni_input_pointer(
        instance: *const c_char,
        action: u32,
        x: f32,
        y: f32,
        buttons: u32,
        action_button: u32,
        horizontal_scroll: f32,
        vertical_scroll: f32,
    ) -> i32;
    fn leftcar_jni_input_key(
        instance: *const c_char,
        key_code: u32,
        scan_code: u32,
        meta_state: u32,
        down: bool,
        repeat: u32,
    ) -> i32;
    fn leftcar_jni_input_release_all(instance: *const c_char) -> i32;
    fn leftcar_jni_input_status(instance: *const c_char) -> i32;
    fn leftcar_jni_poll_audio(instance: *const c_char, out: *mut u8, capacity: usize) -> i32;
    fn leftcar_jni_cursor_state(instance: *const c_char) -> i64;
    fn leftcar_jni_set_cursor_stream(instance: *const c_char, enabled: bool) -> i32;
    fn leftcar_jni_stream_stats(instance: *const c_char) -> i64;
    fn leftcar_jni_stream_latency(instance: *const c_char) -> i64;
    fn leftcar_jni_termination_reason(instance: *const c_char) -> i32;
    fn leftcar_jni_surface_release_latency(instance: *const c_char) -> i32;
}

// Java signatures:
//   start(): long
//   prepareStream(int, String, String): int
//   cancelPreparedStream(int): int
//   attachSurface(long, String, Surface): int
//   surfaceChanged(long, String, int, int): int
//   detachSurface(long, String): int
//   updateWindowEvent(long, String, int, long): int
//   sendPointer(String, int, float, float, int, int, float, float): int
//   sendKey(String, int, int, int, boolean, int): int
//   releaseInput(String): int
//   inputStatus(String): int
//   cursorState(String): long
//   setCursorStream(String, boolean): int
//   streamStats(String): long
//   streamLatency(String): long
//   release(long, String): int

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_start(
    _env: *mut JNIEnv,
    _class: *mut jobject,
) -> i64 {
    leftcar_jni_start() as i64
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_prepareStream(
    env: *mut JNIEnv,
    _class: *mut jobject,
    port: i32,
    host: *mut jobject,
    transport: *mut jobject,
) -> i32 {
    if port <= 0 || port > i32::from(u16::MAX) {
        return 4;
    }
    let host = match unsafe { get_utf(env, host) } {
        Some(host) => host,
        None => return 1,
    };
    let transport = match unsafe { get_utf(env, transport) } {
        Some(transport) => transport,
        None => return 1,
    };
    unsafe { leftcar_jni_prepare_port(port as u16, host.as_ptr(), transport.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_prepareSplitStream(
    env: *mut JNIEnv,
    _class: *mut jobject,
    port: i32,
    host: *mut jobject,
    transport: *mut jobject,
) -> i32 {
    if port <= 0 || port >= i32::from(u16::MAX) {
        return 4;
    }
    let host = match unsafe { get_utf(env, host) } {
        Some(host) => host,
        None => return 1,
    };
    let transport = match unsafe { get_utf(env, transport) } {
        Some(transport) => transport,
        None => return 1,
    };
    unsafe { leftcar_jni_prepare_split_port(port as u16, host.as_ptr(), transport.as_ptr()) }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_prepareUsb(
    _env: *mut JNIEnv,
    _class: *mut jobject,
    fd: i32,
) -> i32 {
    std::panic::catch_unwind(|| unsafe { leftcar_jni_prepare_usb(fd) }).unwrap_or(3)
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_usbControlPort(
    _env: *mut JNIEnv,
    _class: *mut jobject,
) -> i32 {
    std::panic::catch_unwind(|| unsafe { leftcar_jni_usb_control_port() }).unwrap_or(-1)
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_cancelPreparedStream(
    _env: *mut JNIEnv,
    _class: *mut jobject,
    port: i32,
) -> i32 {
    if port <= 0 || port > i32::from(u16::MAX) {
        return 4;
    }
    unsafe { leftcar_jni_cancel_prepared_port(port as u16) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_cancelPreparedSplitStream(
    _env: *mut JNIEnv,
    _class: *mut jobject,
    port: i32,
) -> i32 {
    if port <= 0 || port >= i32::from(u16::MAX) {
        return 4;
    }
    unsafe { leftcar_jni_cancel_prepared_split(port as u16) }
}

mod attach;
mod input;
mod stats;
mod surface;
