//! JNI export shims for dev.leftcar.viewer.shim.ViewerNative.
//!
//! Kotlin calls these via `external fun`; they convert Java types (String,
//! Surface) and delegate to the C-string entries in jni.rs.

use std::ffi::{c_char, c_void, CString};

#[repr(C)]
struct jobject(c_void);
#[repr(C)]
struct JNIEnv(c_void);
#[repr(C)]
struct _jobject(c_void);

// JNI vtable access: GetStringUTFChars / ReleaseStringUTFChars / ExceptionCheck
// JNIEnv is a pointer to a struct whose first field points to a function table.
unsafe fn env_functions(env: *mut JNIEnv) -> *mut *mut c_void {
    // JNIEnv* points at a struct whose first field is the function table.
    *(env as *mut *mut *mut c_void)
}

const JNI_GET_STRING_UTF_CHARS: usize = 169;
const JNI_RELEASE_STRING_UTF_CHARS: usize = 170;
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
}

// Java signatures:
//   start(): long
//   attachSurface(long, String, Surface): int
//   surfaceChanged(long, String, int, int): int
//   detachSurface(long, String): int
//   updateWindowEvent(long, String, int, long): int
//   sendPointer(String, int, float, float, int, int, float, float): int
//   sendKey(String, int, int, int, boolean, int): int
//   releaseInput(String): int
//   release(long, String): int

macro_rules! jni_fn {
    ($(#[$meta:meta])* $name:ident($($arg:ty),*) -> $ret:ty, $body:expr) => {
        $(#[$meta])*
        #[no_mangle]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub unsafe extern "C" fn $name(_env: *mut JNIEnv, _class: *mut jobject, $($arg: $arg),*) -> $ret {
            $body(_env, $($arg),*)
        }
    };
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_start(
    _env: *mut JNIEnv,
    _class: *mut jobject,
) -> i64 {
    leftcar_jni_start() as i64
}

type AttachArgs = (i64, *mut jobject, *mut jobject);

fn attach_body(env: *mut JNIEnv, state: i64, jstr: *mut jobject, surface: *mut jobject) -> i32 {
    if unsafe { exception_pending(env) } {
        return 3;
    }
    let c = match unsafe { get_utf(env, jstr) } {
        Some(c) => c,
        None => return 1,
    };
    let window = unsafe { ANativeWindow_fromSurface(env, surface) };
    if window.is_null() {
        return 4;
    }
    // fromSurface already acquires one ref; the core owns it now and the
    // detach path releases via leftcar_jni_surface_ref(false).
    let r = unsafe { leftcar_jni_attach(state as *mut c_void, c.as_ptr(), window) };
    if r != 0 {
        unsafe { leftcar_jni_surface_ref(window, false) };
    }
    r
}

fn attach_port_body(
    env: *mut JNIEnv,
    state: i64,
    jstr: *mut jobject,
    surface: *mut jobject,
    port: i32,
    host: *mut jobject,
    width: i32,
    height: i32,
    fps: i32,
) -> i32 {
    if unsafe { exception_pending(env) } {
        return 3;
    }
    let c = match unsafe { get_utf(env, jstr) } {
        Some(c) => c,
        None => return 1,
    };
    let host = match unsafe { get_utf(env, host) } {
        Some(h) => h,
        None => return 1,
    };
    let window = unsafe { ANativeWindow_fromSurface(env, surface) };
    if window.is_null() {
        return 4;
    }
    let r = unsafe {
        leftcar_jni_attach_port(
            state as *mut c_void,
            c.as_ptr(),
            window,
            port as u16,
            host.as_ptr(),
            width.max(1) as u32,
            height.max(1) as u32,
            fps.clamp(1, 90) as u32,
        )
    };
    if r != 0 {
        unsafe { leftcar_jni_surface_ref(window, false) };
    }
    r
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_attachSurface(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
    surface: *mut jobject,
) -> i32 {
    std::panic::catch_unwind(|| attach_body(env, state, instance, surface)).unwrap_or(3)
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_attachSurfacePort(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
    surface: *mut jobject,
    port: i32,
    host: *mut jobject,
    width: i32,
    height: i32,
    fps: i32,
) -> i32 {
    std::panic::catch_unwind(|| {
        attach_port_body(
            env, state, instance, surface, port, host, width, height, fps,
        )
    })
    .unwrap_or(3)
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_surfaceChanged(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
    w: i32,
    h: i32,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_surface_changed(state as *mut c_void, c.as_ptr(), w as u32, h as u32) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_detachSurface(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_detach(state as *mut c_void, c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_updateWindowEvent(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
    event: i32,
    monotonic_ms: i64,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe {
        leftcar_jni_update_window(
            state as *mut c_void,
            c.as_ptr(),
            event as u32,
            monotonic_ms as u64,
        )
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_sendPointer(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    action: i32,
    x: f32,
    y: f32,
    buttons: i32,
    action_button: i32,
    horizontal_scroll: f32,
    vertical_scroll: f32,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe {
        leftcar_jni_input_pointer(
            c.as_ptr(),
            action as u32,
            x,
            y,
            buttons as u32,
            action_button as u32,
            horizontal_scroll,
            vertical_scroll,
        )
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_sendKey(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    key_code: i32,
    scan_code: i32,
    meta_state: i32,
    down: u8,
    repeat: i32,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe {
        leftcar_jni_input_key(
            c.as_ptr(),
            key_code as u32,
            scan_code as u32,
            meta_state as u32,
            down != 0,
            repeat as u32,
        )
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_releaseInput(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_input_release_all(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_release(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_release(state as *mut c_void, c.as_ptr()) }
}
