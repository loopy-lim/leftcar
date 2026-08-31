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
