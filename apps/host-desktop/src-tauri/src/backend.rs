//! Capture backend abstraction: the macOS shim FFI implementation and the
//! in-memory test fake share this trait (design §Tauri 호스트).

use control_contract::host::{DisplayInfo, StatsInfo};
use std::sync::Arc;

pub trait CaptureBackend: Send + Sync {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String>;
    fn start(
        &self,
        source_index: u32,
        ip: &str,
        port: u16,
        w: u32,
        h: u32,
        fps: u32,
    ) -> Result<u32, String>;
    fn stop(&self, handle: u32) -> Result<(), String>;
    fn stats(&self, handle: u32) -> Result<StatsInfo, String>;
}

/// In-memory backend for tests and UI development without the shim dylib.
pub struct FakeBackend {
    pub displays: Vec<DisplayInfo>,
}

impl CaptureBackend for FakeBackend {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Ok(self.displays.clone())
    }

    fn start(
        &self,
        _source_index: u32,
        _ip: &str,
        _port: u16,
        _w: u32,
        _h: u32,
        _fps: u32,
    ) -> Result<u32, String> {
        Ok(7)
    }

    fn stop(&self, handle: u32) -> Result<(), String> {
        if handle == 7 {
            Ok(())
        } else {
            Err("no such handle".into())
        }
    }

    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        if handle != 7 {
            return Err("no such handle".into());
        }
        Ok(StatsInfo {
            frames: 100,
            bytes: 1_000_000,
            state: "running".into(),
            fps: 90,
            kbps: 12000,
        })
    }
}

pub type SharedBackend = Arc<dyn CaptureBackend>;
