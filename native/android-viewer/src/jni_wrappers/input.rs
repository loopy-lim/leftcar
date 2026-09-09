use super::*;

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
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_sendText(
    env: *mut JNIEnv,
    _class: *mut jobject,
    instance: *mut jobject,
    payload: *mut jobject,
) -> i32 {
    let c = match unsafe { get_utf(env, instance) } {
        Some(c) => c,
        None => return 1,
    };
    if env.is_null() || payload.is_null() {
        return 1;
    }
    // The Kotlin side hands over UTF-8 bytes instead of a Java string so
    // supplementary characters survive the JNI boundary (modified UTF-8
    // would mangle them into CESU-8 surrogate pairs).
    let fns = env_functions(env);
    let length: unsafe extern "C" fn(*mut JNIEnv, *mut jobject) -> i32 =
        std::mem::transmute(*fns.add(JNI_GET_ARRAY_LENGTH));
    let len = length(env, payload);
    if len <= 0 {
        return 0;
    }
    let mut staged = vec![0u8; len as usize];
    let get_region: unsafe extern "C" fn(*mut JNIEnv, *mut jobject, i32, i32, *mut u8) =
        std::mem::transmute(*fns.add(JNI_GET_BYTE_ARRAY_REGION));
    get_region(env, payload, 0, len, staged.as_mut_ptr());
    if unsafe { exception_pending(env) } {
        return 1;
    }
    unsafe { leftcar_jni_input_text(c.as_ptr(), staged.as_ptr(), staged.len()) }
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
