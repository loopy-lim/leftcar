use super::*;

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

#[allow(clippy::too_many_arguments)]
fn attach_split_body(
    env: *mut JNIEnv,
    state: i64,
    jstr: *mut jobject,
    left_surface: *mut jobject,
    right_surface: *mut jobject,
    port: i32,
    host: *mut jobject,
    width: i32,
    height: i32,
    fps: i32,
    decoder_name: *mut jobject,
) -> i32 {
    if unsafe { exception_pending(env) } || port <= 0 || port >= i32::from(u16::MAX) {
        return 4;
    }
    let instance = match unsafe { get_utf(env, jstr) } {
        Some(instance) => instance,
        None => return 1,
    };
    let host = match unsafe { get_utf(env, host) } {
        Some(host) => host,
        None => return 1,
    };
    let decoder_name = match unsafe { get_utf(env, decoder_name) } {
        Some(name) if !name.as_bytes().is_empty() => name,
        _ => return 1,
    };
    let left_window = unsafe { ANativeWindow_fromSurface(env, left_surface) };
    if left_window.is_null() {
        return 4;
    }
    let right_window = unsafe { ANativeWindow_fromSurface(env, right_surface) };
    if right_window.is_null() {
        unsafe { leftcar_jni_surface_ref(left_window, false) };
        return 4;
    }
    let result = unsafe {
        leftcar_jni_attach_split_port(
            state as *mut c_void,
            instance.as_ptr(),
            left_window,
            right_window,
            port as u16,
            host.as_ptr(),
            width.max(1) as u32,
            height.max(1) as u32,
            fps.clamp(1, 90) as u32,
            decoder_name.as_ptr(),
        )
    };
    if result != 0 {
        unsafe {
            leftcar_jni_surface_ref(left_window, false);
            leftcar_jni_surface_ref(right_window, false);
        }
    }
    result
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
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_rebindSurfacePort(
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
    if port <= 0 || port > i32::from(u16::MAX) {
        return 4;
    }
    let instance = match unsafe { get_utf(env, instance) } {
        Some(instance) => instance,
        None => return 1,
    };
    let host = match unsafe { get_utf(env, host) } {
        Some(host) => host,
        None => return 1,
    };
    let window = unsafe { ANativeWindow_fromSurface(env, surface) };
    if window.is_null() {
        return 4;
    }
    let result = unsafe {
        leftcar_jni_rebind_port(
            state as *mut c_void,
            instance.as_ptr(),
            window,
            port as u16,
            host.as_ptr(),
            width.max(1) as u32,
            height.max(1) as u32,
            fps.clamp(1, 90) as u32,
        )
    };
    if result != 0 {
        unsafe { leftcar_jni_surface_ref(window, false) };
    }
    result
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub unsafe extern "C" fn Java_dev_leftcar_viewer_shim_ViewerNative_attachSplitSurfaces(
    env: *mut JNIEnv,
    _class: *mut jobject,
    state: i64,
    instance: *mut jobject,
    left_surface: *mut jobject,
    right_surface: *mut jobject,
    port: i32,
    host: *mut jobject,
    width: i32,
    height: i32,
    fps: i32,
    decoder_name: *mut jobject,
) -> i32 {
    std::panic::catch_unwind(|| {
        attach_split_body(
            env,
            state,
            instance,
            left_surface,
            right_surface,
            port,
            host,
            width,
            height,
            fps,
            decoder_name,
        )
    })
    .unwrap_or(3)
}
