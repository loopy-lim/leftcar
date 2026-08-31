use super::*;

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_inputStatus(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return -1,
    };
    unsafe { leftcar_jni_input_status(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_streamStats(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i64 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return -1,
    };
    unsafe { leftcar_jni_stream_stats(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_streamLatency(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i64 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return -1,
    };
    unsafe { leftcar_jni_stream_latency(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// # Safety
/// JNI supplies a valid environment and Java string reference for the
/// duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_terminationReason(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return -1,
    };
    unsafe { leftcar_jni_termination_reason(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// # Safety
/// JNI supplies a valid environment and Java string reference for the
/// duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_surfaceReleaseLatency(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 0xffff,
    };
    unsafe { leftcar_jni_surface_release_latency(c.as_ptr()) }
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
