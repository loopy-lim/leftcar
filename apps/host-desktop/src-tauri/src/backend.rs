//! Capture backend abstraction: the macOS shim FFI implementation and the
//! in-memory test fake share this trait (design §Tauri 호스트).

use control_contract::host::{
    phase_a_encoder_experiments, CaptureBackendInfo, DisplayInfo, EncoderExperiment,
    EncoderExperimentInfo, StatsInfo,
};
use control_contract::udp_stability::AppliedUdpStability;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

pub trait CaptureBackend: Send + Sync {
    fn platform(&self) -> &'static str {
        "unknown"
    }
    fn capture_backends(&self) -> Vec<CaptureBackendInfo> {
        vec![CaptureBackendInfo {
            id: "screenCaptureKit".into(),
            label: "ScreenCaptureKit".into(),
            hint: "default capture backend".into(),
        }]
    }
    fn encoder_experiments(&self) -> Result<Vec<EncoderExperimentInfo>, String> {
        Ok(vec![EncoderExperimentInfo {
            id: EncoderExperiment::Auto,
            label: "Automatic".into(),
            hint: "Host-selected encoder policy".into(),
            requires_reconnect: true,
        }])
    }
    fn supports_capture_backend(&self, id: &str) -> bool {
        self.capture_backends()
            .iter()
            .any(|backend| backend.id == id)
    }
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String>;
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        source_index: u32,
        ip: &str,
        port: u16,
        w: u32,
        h: u32,
        fps: u32,
        capture_backend: &str,
        media_transport: &str,
        content_mode: &str,
        encoder_experiment: EncoderExperiment,
        udp_stability: &AppliedUdpStability,
    ) -> Result<u32, String>;
    fn stop(&self, handle: u32) -> Result<(), String>;
    /// Stop while telling a still-live viewer why (LCT1 wire code). The
    /// default degrades to a silent stop for backends without a notice path.
    fn stop_with_reason(&self, handle: u32, _reason_code: u8) -> Result<(), String> {
        self.stop(handle)
    }
    fn stats(&self, handle: u32) -> Result<StatsInfo, String>;
    fn input_permission(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn request_input_permission(&self) -> Result<bool, String> {
        Ok(false)
    }
    /// Whether the OS currently lets this process capture the screen (macOS
    /// TCC "Screen Recording"). Backends without such a gate report `true`
    /// so the dashboard never warns about a permission that does not exist.
    fn screen_permission(&self) -> Result<bool, String> {
        Ok(true)
    }
    fn set_input_enabled(&self, _handle: u32, _enabled: bool) -> Result<(), String> {
        Err("remote input is unavailable in this capture backend".into())
    }
    fn set_quality_override(&self, _handle: u32, _quality: Option<f32>) -> Result<(), String> {
        Err("manual quality override is unavailable in this capture backend".into())
    }
}

/// In-memory backend for tests and UI development without the shim dylib.
pub struct FakeBackend {
    pub displays: Vec<DisplayInfo>,
    pub encoder_experiment: Mutex<EncoderExperiment>,
    /// Whether the catalog should advertise splitVertical as startable.
    pub advertise_split_vertical: bool,
    /// Number of successful stop() calls, for reconfigure rollback tests.
    pub stops: AtomicUsize,
    /// What input_permission() reports: the OS-granted state the start path
    /// auto-enables remote input under.
    pub input_permission: bool,
    /// Every set_input_enabled call, for auto-enable/carry-over assertions.
    pub input_calls: Mutex<Vec<(u32, bool)>>,
}

impl CaptureBackend for FakeBackend {
    fn platform(&self) -> &'static str {
        "test"
    }

    fn capture_backends(&self) -> Vec<CaptureBackendInfo> {
        vec![CaptureBackendInfo {
            id: "screenCaptureKit".into(),
            label: "ScreenCaptureKit".into(),
            hint: "test backend".into(),
        }]
    }

    fn encoder_experiments(&self) -> Result<Vec<EncoderExperimentInfo>, String> {
        let mut experiments = phase_a_encoder_experiments();
        if self.advertise_split_vertical {
            // control.rs re-canonicalizes advertised entries, so a raw entry
            // is enough here.
            experiments.push(EncoderExperimentInfo {
                id: EncoderExperiment::SplitVertical,
                label: String::new(),
                hint: String::new(),
                requires_reconnect: true,
            });
        }
        Ok(experiments)
    }

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
        _media_transport: &str,
        _content_mode: &str,
        encoder_experiment: EncoderExperiment,
        _udp_stability: &AppliedUdpStability,
    ) -> Result<u32, String> {
        *self.encoder_experiment.lock().unwrap() = encoder_experiment;
        Ok(7)
    }

    fn stop(&self, handle: u32) -> Result<(), String> {
        if handle == 7 {
            self.stops.fetch_add(1, Ordering::SeqCst);
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
            encoder_experiment_diagnostics_available: true,
            encoder_experiment_requested: {
                let experiment = *self.encoder_experiment.lock().unwrap();
                experiment.as_str().into()
            },
            encoder_experiment_applied: {
                let experiment = *self.encoder_experiment.lock().unwrap();
                match experiment {
                    EncoderExperiment::Auto => EncoderExperiment::RateControl.as_str(),
                    _ => experiment.as_str(),
                }
                .into()
            },
            encoder_experiment_fallback_reason: None,
            encoder_frame_drops: 0,
            encoder_frame_drop_fps: 0,
            valid_encode_output_fps: 90,
            encode_submit_call_p50_us: 0,
            encode_submit_call_p95_us: 0,
            encoder_callback_p50_us: 0,
            encoder_callback_p95_us: 0,
            packetization_in_flight: 0,
            base_frame_qp: {
                (*self.encoder_experiment.lock().unwrap() == EncoderExperiment::AdaptiveQp)
                    .then_some(32)
            },
            base_frame_qp_changes: 0,
            capture_fps: 90,
            encode_submit_fps: 90,
            encode_output_fps: 90,
            rendered_fps: Some(90),
            capture_callbacks: 100,
            encode_output_callbacks: 100,
            encode_submit_failures: 0,
            encode_in_flight: 0,
            dropped: 0,
            network_dropped: 0,
            network_queue_dropped: 0,
            recovery_frames_dropped: 0,
            udp_send_failures: 0,
            udp_send_retries: 0,
            recovery_keyframes: 0,
            recovery_requests_suppressed: 0,
            capture_queue_dropped: 0,
            capture_to_encode_us: 0,
            max_capture_to_encode_us: 0,
            capture_queue_wait_us: 0,
            max_capture_queue_wait_us: 0,
            encode_output_us: 0,
            max_encode_output_us: 0,
            packetization_us: 0,
            max_packetization_us: 0,
            send_block_us: 0,
            max_send_block_us: 0,
            send_pace_us: 0,
            max_send_pace_us: 0,
            pending_frame: 0,
            pending_frame_bytes: 0,
            pending_frame_oldest_age_us: 0,
            capture_backend: "screenCaptureKit".into(),
            media_transport: "udp".into(),
            first_capture_ms: 20,
            first_encode_ms: 25,
            first_send_ms: 26,
            current_bitrate: 12_000_000,
            bitrate_floor_collapse_count: 7,
            bitrate_floor_collapse_last_reason: "resolution_fallback_floor_reached".into(),
            encoder_mode: "unknown".into(),
            encoder_id: "unknown".into(),
            encoder_hardware_accelerated: None,
            encoder_preset: "unknown".into(),
            encoder_profile: "unknown".into(),
            encoder_applied_properties: Vec::new(),
            encoder_unsupported_properties: Vec::new(),
            encoder_rejected_properties: Vec::new(),
            encoder_fallback_reason: None,
            quality_hint: None,
            quality_override: None,
            quality_adaptation_checks: 0,
            quality_adaptation_changes: 0,
            quality_adaptation_rejections: 0,
            quality_adaptation_last_status: "not_checked".into(),
            capture_interval_p95_us: 16_667,
            capture_to_encode_p95_us: 8_000,
            capture_queue_wait_p95_us: 1_000,
            encode_output_p95_us: 7_000,
            packetization_p95_us: 0,
            encode_output_interval_p95_us: 11_111,
            send_block_p95_us: 1_000,
            send_pace_p95_us: 0,
            last_au_bytes: 0,
            last_au_fragments: 0,
            last_au_parity: 0,
            last_au_datagrams: 0,
            last_au_expected_datagrams: 0,
            last_au_send_us: 0,
            last_au_is_keyframe: false,
            max_au_bytes: 0,
            max_au_fragments: 0,
            sent_datagrams: 0,
            sent_parity_datagrams: 0,
            error: None,
            receiver_frame_gaps: 0,
            receiver_input_drops: 0,
            receiver_incomplete_aus: 0,
            receiver_stale_frames: 0,
            receiver_stale_input_drops: None,
            receiver_output_burst_discards: 0,
            receiver_rtt_ms: None,
            receiver_wire_ms: None,
            receiver_feedback_age_ms: None,
            ..StatsInfo::default()
        })
    }

    fn input_permission(&self) -> Result<bool, String> {
        Ok(self.input_permission)
    }

    fn request_input_permission(&self) -> Result<bool, String> {
        Ok(self.input_permission)
    }

    fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
        if handle == 7 {
            self.input_calls.lock().unwrap().push((handle, enabled));
            Ok(())
        } else {
            Err("no such handle".into())
        }
    }

    fn set_quality_override(&self, handle: u32, _quality: Option<f32>) -> Result<(), String> {
        if handle == 7 {
            Ok(())
        } else {
            Err("no such handle".into())
        }
    }
}

pub type SharedBackend = Arc<dyn CaptureBackend>;
