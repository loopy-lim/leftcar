use super::*;

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
