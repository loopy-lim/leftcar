//! JNI bindings exposing the leftcar rustra host package to React Native
//! (docs/02 §9: TS UI -> @rustra/react-native -> NativeModules.Rustra -> here).
//!
//! invoke() routes through Package::invoke_json — the SAME path the host
//! contract tests use, so the on-device JS round trip (H09) exercises the
//! real rustra invocation machinery, not a reimplementation.

use std::ffi::{c_char, CStr, CString};
use std::sync::OnceLock;

use control_contract::host_package;
use rustra::prelude::Package;

static PACKAGE: OnceLock<Package> = OnceLock::new();

fn package() -> &'static Package {
    PACKAGE.get_or_init(host_package)
}

/// Result strings carry an "OK"/"ER" prefix so errors never look like data.
#[no_mangle]
pub extern "C" fn leftcar_rustra_start() -> i64 {
    let _ = package(); // eagerly build
    1
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn leftcar_rustra_invoke(
    command: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        let command = unsafe { CStr::from_ptr(command) }.to_string_lossy();
        let args = if args_json.is_null() {
            serde_json::json!({})
        } else {
            let s = unsafe { CStr::from_ptr(args_json) }
                .to_string_lossy()
                .into_owned();
            serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}))
        };
        package()
            .invoke_json(&command, args)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    });
    let payload = match result {
        Ok(Ok(json)) => format!("OK{json}"),
        Ok(Err(e)) => format!("ER{e}"),
        Err(_) => "ERpanic".to_string(),
    };
    let c = CString::new(payload).unwrap_or_else(|_| CString::new("ERnull").unwrap());
    c.into_raw()
}

#[no_mangle]
pub extern "C" fn leftcar_rustra_contract_hash() -> *mut c_char {
    let result = std::panic::catch_unwind(control_contract::contract_hash);
    let payload = result.unwrap_or_else(|_| "panic".to_string());
    let c = CString::new(payload).unwrap_or_else(|_| CString::new("err").unwrap());
    c.into_raw()
}

#[no_mangle]
pub extern "C" fn leftcar_rustra_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_json_adds_numbers() {
        let out = package()
            .invoke_json("addNumbers", serde_json::json!({"a": 20, "b": 22}))
            .expect("invoke");
        assert_eq!(out, serde_json::json!({"value": 42}));
    }
}

// -- JNI exports for dev.leftcar.viewer.RustraModule ---------------------------

mod jni {
    use super::*;
    use std::ffi::c_void;

    #[repr(C)]
    struct JString(c_void);
    #[repr(C)]
    struct JNIEnv(c_void);
    #[repr(C)]
    struct JClass(c_void);

    // JNIEnv vtable access identical to native/android-viewer (verified there):
    // GetStringUTFChars idx 169, ReleaseStringUTFChars idx 170, NewStringUTF idx 167
    unsafe fn vtable(env: *mut JNIEnv) -> *mut *mut c_void {
        *(env as *mut *mut *mut c_void)
    }

    unsafe fn get_utf(env: *mut JNIEnv, jstr: *mut JString) -> Option<CString> {
        if env.is_null() || jstr.is_null() {
            return None;
        }
        let fns = vtable(env);
        let get: unsafe extern "C" fn(*mut JNIEnv, *mut JString, *mut c_void) -> *const c_char =
            std::mem::transmute(*fns.add(169));
        let ptr = get(env, jstr, std::ptr::null_mut());
        if ptr.is_null() {
            return None;
        }
        let s = CStr::from_ptr(ptr).to_owned();
        let rel: unsafe extern "C" fn(*mut JNIEnv, *mut JString, *const c_char) =
            std::mem::transmute(*fns.add(170));
        rel(env, jstr, ptr);
        Some(s)
    }

    unsafe fn new_string_utf(env: *mut JNIEnv, s: &CStr) -> *mut JString {
        let fns = vtable(env);
        let new: unsafe extern "C" fn(*mut JNIEnv, *const c_char) -> *mut JString =
            std::mem::transmute(*fns.add(167));
        new(env, s.as_ptr())
    }

    // void-returning panic containment:
    #[no_mangle]
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub unsafe extern "C" fn Java_dev_leftcar_viewer_RustraModule_nativeStart(
        env: *mut JNIEnv,
        _class: *mut JClass,
    ) {
        let _ = std::panic::catch_unwind(|| {
            let _ = env;
            super::leftcar_rustra_start();
        });
    }

    #[no_mangle]
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub unsafe extern "C" fn Java_dev_leftcar_viewer_RustraModule_nativeInvoke(
        env: *mut JNIEnv,
        _class: *mut JClass,
        command: *mut JString,
        args_json: *mut JString,
    ) -> *mut JString {
        match std::panic::catch_unwind(|| -> Option<*mut JString> {
            let c = get_utf(env, command)?;
            let a = get_utf(env, args_json);
            let a_c = a.as_deref().unwrap_or(c"{}");
            let raw = super::leftcar_rustra_invoke(c.as_ptr(), a_c.as_ptr());
            let s = CStr::from_ptr(raw);
            let out = new_string_utf(env, s);
            super::leftcar_rustra_free(raw);
            Some(out)
        }) {
            Ok(Some(ptr)) => ptr,
            _ => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub unsafe extern "C" fn Java_dev_leftcar_viewer_RustraModule_nativeContractHash(
        env: *mut JNIEnv,
        _class: *mut JClass,
    ) -> *mut JString {
        std::panic::catch_unwind(|| {
            let raw = super::leftcar_rustra_contract_hash();
            let s = CStr::from_ptr(raw);
            let out = new_string_utf(env, s);
            super::leftcar_rustra_free(raw);
            out
        })
        .unwrap_or(std::ptr::null_mut())
    }
}
