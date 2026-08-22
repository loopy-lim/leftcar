//! FFI backend: drives the macOS capture shim dylib (v2 handle-based C ABI)
//! through libloading. Symbol set:
//!   leftcar_capture_list_displays() -> JSON [{index,name,width,height}]
//!   leftcar_capture_start_v2(ip, port, display, w, h, fps) -> handle
//!   leftcar_capture_stop_v2(handle)
//!   leftcar_capture_stats_v2(handle) -> JSON {frames,bytes,state,fps,kbps}
//!   leftcar_capture_free_string(ptr)
//!   leftcar_capture_last_error_v2() -> cstr
//!   leftcar_capture_input_permission_v1() -> granted
//!   leftcar_capture_request_input_permission_v1() -> granted
//!   leftcar_capture_set_input_enabled_v1(handle, enabled)

use crate::backend::CaptureBackend;
use control_contract::host::{DisplayInfo, StatsInfo};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::path::PathBuf;

pub struct FfiBackend {
    _lib: Library,
    _path: PathBuf,
}

unsafe impl Send for FfiBackend {}
unsafe impl Sync for FfiBackend {}

fn dylib_candidates() -> Vec<PathBuf> {
    if let Ok(env) = std::env::var("LEFTCAR_CAPTURE_DYLIB") {
        // An explicit override is authoritative. Falling back to a bundled
        // or checkout-relative dylib here makes missing-path tests pass or
        // fail depending on unrelated build artifacts, and can load a stale
        // shim when the caller selected a different one.
        return vec![PathBuf::from(env)];
    }
    let mut v = Vec::new();
    // A bundled Tauri app is commonly launched with `/` as its cwd. Walk up
    // from the executable so the dev checkout still works in that case.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            v.push(contents.join("Resources/libleftcar_capture.dylib"));
        }
        for ancestor in exe.ancestors() {
            v.push(ancestor.join("native/macos-capture-shim/libleftcar_capture.dylib"));
        }
    }
    // cargo tauri dev normally runs from src-tauri; repo root is ../../..
    if let Ok(cwd) = std::env::current_dir() {
        let repo_root = cwd
            .join("../../..")
            .join("native/macos-capture-shim/libleftcar_capture.dylib");
        v.push(repo_root);
        v.push(cwd.join("native/macos-capture-shim/libleftcar_capture.dylib"));
    }
    v
}

impl FfiBackend {
    pub fn new() -> Result<Self, String> {
        let mut last_err = "no dylib candidates".to_string();
        for path in dylib_candidates() {
            if !path.exists() {
                last_err = format!("dylib not found at {}", path.display());
                continue;
            }
            match unsafe { Library::new(&path) } {
                Ok(lib) => {
                    let backend = Self {
                        _lib: lib,
                        _path: path,
                    };
                    backend.verify_symbols()?;
                    return Ok(backend);
                }
                Err(e) => last_err = format!("dlopen {}: {e}", path.display()),
            }
        }
        Err(last_err)
    }

    fn verify_symbols(&self) -> Result<(), String> {
        unsafe {
            let lib = self.lib()?;
            type CPtr = *mut std::ffi::c_char;
            let _ = lib
                .get::<unsafe extern "C" fn(CPtr, u16, u32, u32, u32, u32) -> u32>(
                    b"leftcar_capture_start_v2",
                )
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn(u32) -> i32>(b"leftcar_capture_stop_v2")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn(u32) -> CPtr>(b"leftcar_capture_stats_v2")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn() -> CPtr>(b"leftcar_capture_list_displays")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn(CPtr)>(b"leftcar_capture_free_string")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn() -> i32>(b"leftcar_capture_input_permission_v1")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn() -> i32>(
                    b"leftcar_capture_request_input_permission_v1",
                )
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn(u32, i32) -> i32>(
                    b"leftcar_capture_set_input_enabled_v1",
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn lib(&self) -> Result<&Library, String> {
        // Library is stored in Self; get() needs &self lifetime — safe here
        // because FfiBackend is kept alive by the SharedBackend Arc.
        unsafe { Ok(&*(&self._lib as *const Library)) }
    }

    fn take_string(ptr: *mut std::ffi::c_char) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc_free_string(ptr) };
        Some(s)
    }
}

// The shim exports its own free; use it through the same library. To keep
// borrow rules simple we look the symbol up per call (rare path: stats poll).
unsafe fn libc_free_string(ptr: *mut std::ffi::c_char) {
    // best-effort: use free() from libc — the shim allocates with strdup
    extern "C" {
        fn free(p: *mut core::ffi::c_void);
    }
    free(ptr as *mut core::ffi::c_void);
}

impl CaptureBackend for FfiBackend {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        let lib = self.lib()?;
        unsafe {
            let f: Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_char> = lib
                .get(b"leftcar_capture_list_displays")
                .map_err(|e| e.to_string())?;
            let ptr = f();
            let json = Self::take_string(ptr).ok_or("list_displays returned null")?;
            let displays: Vec<DisplayInfo> =
                serde_json::from_str(&json).map_err(|e| format!("bad display json: {e}"))?;
            if displays.is_empty() {
                let last_error: Symbol<unsafe extern "C" fn() -> *const std::ffi::c_char> = lib
                    .get(b"leftcar_capture_last_error_v2")
                    .map_err(|e| e.to_string())?;
                let message = last_error();
                if !message.is_null() {
                    let message = CStr::from_ptr(message).to_string_lossy();
                    if !message.is_empty() {
                        return Err(message.into_owned());
                    }
                }
            }
            Ok(displays)
        }
    }

    fn start(
        &self,
        source_index: u32,
        ip: &str,
        port: u16,
        w: u32,
        h: u32,
        fps: u32,
        capture_backend: &str,
    ) -> Result<u32, String> {
        let lib = self.lib()?;
        let c_ip = CString::new(ip).map_err(|_| "ip contains NUL")?;
        let c_backend = CString::new(capture_backend).map_err(|_| "backend contains NUL")?;
        unsafe {
            type StartV3 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
            ) -> u32;
            let handle = match lib.get::<StartV3>(b"leftcar_capture_start_v3") {
                Ok(f) => f(
                    c_ip.as_ptr(),
                    port,
                    source_index,
                    w,
                    h,
                    fps,
                    c_backend.as_ptr(),
                ),
                Err(_) if capture_backend == "screenCaptureKit" => {
                    let f: Symbol<
                        unsafe extern "C" fn(
                            *const std::ffi::c_char,
                            u16,
                            u32,
                            u32,
                            u32,
                            u32,
                        ) -> u32,
                    > = lib
                        .get(b"leftcar_capture_start_v2")
                        .map_err(|e| e.to_string())?;
                    f(c_ip.as_ptr(), port, source_index, w, h, fps)
                }
                Err(_) => {
                    return Err("capture shim does not support selectable backends".into());
                }
            };
            if handle == 0 {
                let err_f: Symbol<unsafe extern "C" fn() -> *const std::ffi::c_char> = lib
                    .get(b"leftcar_capture_last_error_v2")
                    .map_err(|e| e.to_string())?;
                let msg = if err_f().is_null() {
                    "unknown".into()
                } else {
                    CStr::from_ptr(err_f()).to_string_lossy().into_owned()
                };
                return Err(msg);
            }
            Ok(handle)
        }
    }

    fn stop(&self, handle: u32) -> Result<(), String> {
        let lib = self.lib()?;
        unsafe {
            let f: Symbol<unsafe extern "C" fn(u32) -> i32> = lib
                .get(b"leftcar_capture_stop_v2")
                .map_err(|e| e.to_string())?;
            let rc = f(handle);
            if rc != 0 {
                return Err(format!("stop({handle}) rc={rc}"));
            }
            Ok(())
        }
    }

    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        let lib = self.lib()?;
        unsafe {
            let f: Symbol<unsafe extern "C" fn(u32) -> *mut std::ffi::c_char> = lib
                .get(b"leftcar_capture_stats_v2")
                .map_err(|e| e.to_string())?;
            let ptr = f(handle);
            let json = Self::take_string(ptr).ok_or("stats returned null")?;
            let v: serde_json::Value =
                serde_json::from_str(&json).map_err(|e| format!("bad stats json: {e}"))?;
            Ok(StatsInfo {
                frames: v["frames"].as_i64().unwrap_or(0),
                bytes: v["bytes"].as_i64().unwrap_or(0),
                state: v["state"].as_str().unwrap_or("unknown").into(),
                fps: v["fps"].as_u64().unwrap_or(0) as u32,
                kbps: v["kbps"].as_u64().unwrap_or(0) as u32,
                fps_target: v["fpsTarget"].as_u64().unwrap_or(0) as u32,
                dropped: v["dropped"].as_i64().unwrap_or(0),
                network_dropped: v["networkDropped"].as_i64().unwrap_or(0),
                capture_queue_dropped: v["captureQueueDropped"].as_i64().unwrap_or(0),
                capture_to_encode_us: v["captureToEncodeUs"].as_u64().unwrap_or(0),
                max_capture_to_encode_us: v["maxCaptureToEncodeUs"].as_u64().unwrap_or(0),
                capture_queue_wait_us: v["captureQueueWaitUs"].as_u64().unwrap_or(0),
                max_capture_queue_wait_us: v["maxCaptureQueueWaitUs"].as_u64().unwrap_or(0),
                encode_output_us: v["encodeOutputUs"].as_u64().unwrap_or(0),
                max_encode_output_us: v["maxEncodeOutputUs"].as_u64().unwrap_or(0),
                send_block_us: v["sendBlockUs"].as_u64().unwrap_or(0),
                max_send_block_us: v["maxSendBlockUs"].as_u64().unwrap_or(0),
                pending_frame: v["pendingFrame"].as_u64().unwrap_or(0) as u32,
                capture_backend: v["captureBackend"]
                    .as_str()
                    .unwrap_or("screenCaptureKit")
                    .into(),
                media_transport: v["mediaTransport"].as_str().unwrap_or("udp").into(),
                first_capture_ms: v["firstCaptureMs"].as_u64().unwrap_or(0),
                first_encode_ms: v["firstEncodeMs"].as_u64().unwrap_or(0),
                first_send_ms: v["firstSendMs"].as_u64().unwrap_or(0),
                current_bitrate: v["currentBitrate"].as_u64().unwrap_or(0) as u32,
                capture_interval_p95_us: v["captureIntervalP95Us"].as_u64().unwrap_or(0),
                capture_to_encode_p95_us: v["captureToEncodeP95Us"].as_u64().unwrap_or(0),
                capture_queue_wait_p95_us: v["captureQueueWaitP95Us"].as_u64().unwrap_or(0),
                encode_output_p95_us: v["encodeOutputP95Us"].as_u64().unwrap_or(0),
                send_block_p95_us: v["sendBlockP95Us"].as_u64().unwrap_or(0),
                error: v["error"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            })
        }
    }

    fn input_permission(&self) -> Result<bool, String> {
        let lib = self.lib()?;
        unsafe {
            let function: Symbol<unsafe extern "C" fn() -> i32> = lib
                .get(b"leftcar_capture_input_permission_v1")
                .map_err(|error| error.to_string())?;
            Ok(function() == 1)
        }
    }

    fn request_input_permission(&self) -> Result<bool, String> {
        let lib = self.lib()?;
        unsafe {
            let function: Symbol<unsafe extern "C" fn() -> i32> = lib
                .get(b"leftcar_capture_request_input_permission_v1")
                .map_err(|error| error.to_string())?;
            Ok(function() == 1)
        }
    }

    fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
        let lib = self.lib()?;
        unsafe {
            let function: Symbol<unsafe extern "C" fn(u32, i32) -> i32> = lib
                .get(b"leftcar_capture_set_input_enabled_v1")
                .map_err(|error| error.to_string())?;
            let result = function(handle, i32::from(enabled));
            if result == 0 {
                return Ok(());
            }
            let error_function: Symbol<
                unsafe extern "C" fn() -> *const std::ffi::c_char,
            > = lib
                .get(b"leftcar_capture_last_error_v2")
                .map_err(|error| error.to_string())?;
            let pointer = error_function();
            let message = if pointer.is_null() {
                format!("set input enabled failed with rc={result}")
            } else {
                CStr::from_ptr(pointer).to_string_lossy().into_owned()
            };
            Err(message)
        }
    }
}

pub fn default_dylib_path() -> Option<PathBuf> {
    dylib_candidates().into_iter().find(|p| p.exists())
}

pub fn dylib_report() -> String {
    match default_dylib_path() {
        Some(p) => format!("shim dylib: {}", p.display()),
        None => "shim dylib NOT FOUND (build native/macos-capture-shim)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_dylib_reports_error() {
        // SAFETY: tests run single-threaded per-process here; this env var is
        // only consulted inside FfiBackend::new within this test.
        // First candidate points nowhere; later repo-relative candidates are
        // skipped by pointing cwd nowhere meaningful via a temp dir.
        let tmp = std::env::temp_dir().join("leftcar-ffi-test-cwd");
        std::fs::create_dir_all(&tmp).unwrap();
        let prev_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        std::env::set_var("LEFTCAR_CAPTURE_DYLIB", "/nonexistent/leftcar.so");
        let r = FfiBackend::new();
        std::env::set_current_dir(prev_cwd).unwrap();
        std::env::remove_var("LEFTCAR_CAPTURE_DYLIB");
        assert!(r.is_err(), "expected Err for missing dylib");
        let msg = match r {
            Err(m) => m,
            Ok(_) => unreachable!(),
        };
        assert!(msg.contains("not found") || msg.contains("dlopen"), "{msg}");
    }

    #[test]
    fn report_mentions_status() {
        let r = dylib_report();
        assert!(r.contains("dylib"), "{r}");
    }
}
