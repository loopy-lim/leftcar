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
/// # Safety
/// JNI supplies a valid environment and Java string reference for the
/// duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_cursorState(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
) -> i64 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return -1,
    };
    unsafe { leftcar_jni_cursor_state(c.as_ptr()) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// # Safety
/// JNI supplies a valid environment and Java string reference for the
/// duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_setCursorStream(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    enabled: u8,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_set_cursor_stream(c.as_ptr(), enabled != 0) }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// # Safety
/// JNI supplies a valid environment and Java string reference for the
/// duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_setAudioStream(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    enabled: u8,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    unsafe { leftcar_jni_set_audio_stream(c.as_ptr(), enabled != 0) }
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

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// # Safety
/// JNI supplies a valid environment, Java string reference, and a
/// caller-owned byte array for the duration of this call.
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_pollAudio(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    out: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 0,
    };
    if env.is_null() || out.is_null() {
        return 0;
    }
    let fns = env_functions(env);
    let length: unsafe extern "C" fn(*mut JNIEnv, *mut jobject) -> i32 =
        std::mem::transmute(*fns.add(JNI_GET_ARRAY_LENGTH));
    let capacity = length(env, out);
    if capacity <= 0 {
        return 0;
    }
    // One bounded stack copy keeps the drain lock short; the playback thread
    // owns this buffer between polls.
    let mut staged = vec![0u8; capacity as usize];
    let written = unsafe { leftcar_jni_poll_audio(c.as_ptr(), staged.as_mut_ptr(), staged.len()) };
    if written <= 0 {
        return 0;
    }
    let fns = env_functions(env);
    let set_region: unsafe extern "C" fn(
        *mut JNIEnv,
        *mut jobject,
        i32,
        i32,
        *const u8,
    ) = std::mem::transmute(*fns.add(JNI_SET_BYTE_ARRAY_REGION));
    set_region(env, out, 0, written, staged.as_ptr());
    if unsafe { exception_pending(env) } {
        return 0;
    }
    written
}
