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
        capture_backend: &str,
    ) -> Result<u32, String>;
    fn stop(&self, handle: u32) -> Result<(), String>;
    fn stats(&self, handle: u32) -> Result<StatsInfo, String>;
    fn input_permission(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn request_input_permission(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn set_input_enabled(&self, _handle: u32, _enabled: bool) -> Result<(), String> {
        Err("remote input is unavailable in this capture backend".into())
    }
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
        _capture_backend: &str,
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
            fps_target: 60,
            dropped: 0,
            network_dropped: 0,
            capture_queue_dropped: 0,
            capture_to_encode_us: 0,
            max_capture_to_encode_us: 0,
            capture_queue_wait_us: 0,
            max_capture_queue_wait_us: 0,
            encode_output_us: 0,
            max_encode_output_us: 0,
            send_block_us: 0,
            max_send_block_us: 0,
            pending_frame: 0,
            capture_backend: "screenCaptureKit".into(),
            media_transport: "udp".into(),
            first_capture_ms: 20,
            first_encode_ms: 25,
            first_send_ms: 26,
            current_bitrate: 12_000_000,
            capture_interval_p95_us: 16_667,
            capture_to_encode_p95_us: 8_000,
            capture_queue_wait_p95_us: 1_000,
            encode_output_p95_us: 7_000,
            send_block_p95_us: 1_000,
            error: None,
        })
    }

    fn input_permission(&self) -> Result<bool, String> {
        Ok(true)
    }

    fn request_input_permission(&self) -> Result<bool, String> {
        Ok(true)
    }

    fn set_input_enabled(&self, handle: u32, _enabled: bool) -> Result<(), String> {
        if handle == 7 {
            Ok(())
        } else {
            Err("no such handle".into())
        }
    }
}

pub type SharedBackend = Arc<dyn CaptureBackend>;
