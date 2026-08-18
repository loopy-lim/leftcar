//! FFI backend: drives the macOS capture shim dylib (v2 handle-based C ABI)
//! through libloading. Symbol set:
//!   leftcar_capture_list_displays() -> JSON [{index,name,width,height}]
//!   leftcar_capture_start_v2(ip, port, display, w, h, fps) -> handle
//!   leftcar_capture_stop_v2(handle)
//!   leftcar_capture_stats_v2(handle) -> JSON {frames,bytes,state,fps,kbps}
//!   leftcar_capture_free_string(ptr)
//!   leftcar_capture_last_error_v2() -> cstr

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
    let mut v = Vec::new();
    if let Ok(env) = std::env::var("LEFTCAR_CAPTURE_DYLIB") {
        v.push(PathBuf::from(env));
    }
    // cargo tauri dev runs from src-tauri; repo root is ../../..
    if let Ok(cwd) = std::env::current_dir() {
        let repo_root = cwd.join("../../..").join("native/macos-capture-shim/libleftcar_capture.dylib");
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
                    let backend = Self { _lib: lib, _path: path };
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
                .get::<unsafe extern "C" fn(CPtr, u16, u32, u32, u32, u32) -> u32>(b"leftcar_capture_start_v2")
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
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
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
            let f: Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_char> =
                lib.get(b"leftcar_capture_list_displays").map_err(|e| e.to_string())?;
            let ptr = f();
            let json = Self::take_string(ptr).ok_or("list_displays returned null")?;
            serde_json::from_str(&json).map_err(|e| format!("bad display json: {e}"))
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
    ) -> Result<u32, String> {
        let lib = self.lib()?;
        let c_ip = CString::new(ip).map_err(|_| "ip contains NUL")?;
        unsafe {
            let f: Symbol<
                unsafe extern "C" fn(*const std::ffi::c_char, u16, u32, u32, u32, u32) -> u32,
            > = lib.get(b"leftcar_capture_start_v2").map_err(|e| e.to_string())?;
            let handle = f(c_ip.as_ptr(), port, source_index, w, h, fps);
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
            let f: Symbol<unsafe extern "C" fn(u32) -> i32> =
                lib.get(b"leftcar_capture_stop_v2").map_err(|e| e.to_string())?;
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
            let f: Symbol<unsafe extern "C" fn(u32) -> *mut std::ffi::c_char> =
                lib.get(b"leftcar_capture_stats_v2").map_err(|e| e.to_string())?;
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
            })
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
