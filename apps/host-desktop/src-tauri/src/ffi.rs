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
//!   leftcar_capture_set_quality_v1(handle, quality_percent)
//!   leftcar_capture_has_persistent_access_v1() -> granted

use crate::backend::CaptureBackend;
use control_contract::host::{
    CaptureBackendInfo, DisplayInfo, EncoderExperiment, EncoderExperimentInfo, StatsInfo,
};
use control_contract::udp_stability::AppliedUdpStability;
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

#[cfg(target_os = "macos")]
fn managed_mode_library() -> Result<&'static Library, String> {
    static LIBRARY: std::sync::OnceLock<Result<Library, String>> = std::sync::OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let mut last_error = "capture dylib not found".to_string();
            for path in dylib_candidates() {
                if !path.exists() {
                    continue;
                }
                match unsafe { Library::new(&path) } {
                    Ok(library) => return Ok(library),
                    Err(error) => last_error = format!("dlopen {}: {error}", path.display()),
                }
            }
            Err(last_error)
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[cfg(target_os = "macos")]
pub(crate) fn register_managed_display_mode(
    display_id: u32,
    generation: u64,
    logical_width: u32,
    logical_height: u32,
    pixel_width: u32,
    pixel_height: u32,
) -> Result<(), String> {
    unsafe {
        let function: Symbol<unsafe extern "C" fn(u32, u64, u32, u32, u32, u32) -> i32> =
            managed_mode_library()?
                .get(b"leftcar_capture_register_managed_display_mode_v1")
                .map_err(|error| error.to_string())?;
        let result = function(
            display_id,
            generation,
            logical_width,
            logical_height,
            pixel_width,
            pixel_height,
        );
        (result == 0)
            .then_some(())
            .ok_or_else(|| format!("managed display mode registration rc={result}"))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn clear_managed_display_mode(display_id: u32, generation: u64) -> Result<(), String> {
    unsafe {
        let function: Symbol<unsafe extern "C" fn(u32, u64)> = managed_mode_library()?
            .get(b"leftcar_capture_clear_managed_display_mode_v1")
            .map_err(|error| error.to_string())?;
        function(display_id, generation);
    }
    Ok(())
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

    /// Stop with a viewer-visible reason (LCT1 code 2 = operator-forced).
    /// Distinct from `stop` so the ordinary shutdown path keeps its current
    /// semantics on shims without the v3 symbol.
    fn stop_with_notice(&self, handle: u32, reason_code: i32) -> Result<(), String> {
        let lib = self.lib()?;
        unsafe {
            let f: Symbol<unsafe extern "C" fn(u32, i32) -> i32> = lib
                .get(b"leftcar_capture_stop_v3")
                .map_err(|e| format!("stop_v3 unavailable: {e}"))?;
            let rc = f(handle, reason_code);
            if rc != 0 {
                return Err(format!("stop({handle}) rc={rc}"));
            }
            Ok(())
        }
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
            let _ = lib
                .get::<unsafe extern "C" fn(u32, i32) -> i32>(b"leftcar_capture_set_quality_v1")
                .map_err(|e| e.to_string())?;
            let _ = lib
                .get::<unsafe extern "C" fn() -> i32>(b"leftcar_capture_has_persistent_access_v1")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn lib(&self) -> Result<&Library, String> {
        // Library is stored in Self; get() needs &self lifetime — safe here
        // because FfiBackend is kept alive by the SharedBackend Arc.
        unsafe { Ok(&*(&self._lib as *const Library)) }
    }

    fn take_string(&self, ptr: *mut std::ffi::c_char) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        let free = unsafe {
            self.lib().ok().and_then(|lib| {
                lib.get::<unsafe extern "C" fn(*mut std::ffi::c_char)>(
                    b"leftcar_capture_free_string",
                )
                .ok()
            })
        };
        if let Some(free) = free {
            unsafe { free(ptr) };
        }
        Some(s)
    }
}

fn macos_capture_backends(persistent_access: bool) -> Vec<CaptureBackendInfo> {
    let automatic = CaptureBackendInfo {
        id: "cgDisplayStream".into(),
        label: "자동 화면 공유".into(),
        hint: "화면 선택기 없이 바로 연결".into(),
    };
    if !persistent_access {
        return vec![automatic];
    }
    vec![
        CaptureBackendInfo {
            id: "screenCaptureKit".into(),
            label: "지속 화면 공유".into(),
            hint: "Apple 지속 캡처 승인됨 · 선택기 없이 연결".into(),
        },
        automatic,
    ]
}

fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn bounded_u32(value: &serde_json::Value, key: &str) -> u32 {
    value[key]
        .as_u64()
        .map(|number| number.min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

fn bounded_i32(value: &serde_json::Value, key: &str) -> Option<i32> {
    value[key]
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
}

fn parse_encoder_experiments_json(json: &str) -> Result<Vec<EncoderExperimentInfo>, String> {
    serde_json::from_str(json).map_err(|error| format!("bad encoder experiment json: {error}"))
}

fn is_supported_encoder_experiment(experiment: EncoderExperiment) -> bool {
    matches!(
        experiment,
        EncoderExperiment::Auto
            | EncoderExperiment::RateControl
            | EncoderExperiment::AdaptiveQp
            | EncoderExperiment::EncoderPool
            | EncoderExperiment::SplitVertical
    )
}

fn has_complete_encoder_experiment_diagnostics(value: &serde_json::Value) -> bool {
    const REQUIRED_KEYS: [&str; 13] = [
        "encoderExperimentRequested",
        "encoderExperimentApplied",
        "encoderExperimentFallbackReason",
        "encoderFrameDrops",
        "encoderFrameDropFps",
        "validEncodeOutputFps",
        "encodeSubmitCallP50Us",
        "encodeSubmitCallP95Us",
        "encoderCallbackP50Us",
        "encoderCallbackP95Us",
        "packetizationInFlight",
        "baseFrameQp",
        "baseFrameQpChanges",
    ];

    value
        .as_object()
        .is_some_and(|object| REQUIRED_KEYS.iter().all(|key| object.contains_key(*key)))
}

fn parse_stats_json(json: &str) -> Result<StatsInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad stats json: {e}"))?;
    Ok(StatsInfo {
        frames: v["frames"].as_i64().unwrap_or(0),
        bytes: v["bytes"].as_i64().unwrap_or(0),
        state: v["state"].as_str().unwrap_or("unknown").into(),
        fps: bounded_u32(&v, "fps"),
        kbps: bounded_u32(&v, "kbps"),
        fps_target: bounded_u32(&v, "fpsTarget"),
        encoder_experiment_diagnostics_available: has_complete_encoder_experiment_diagnostics(&v),
        encoder_experiment_requested: v["encoderExperimentRequested"]
            .as_str()
            .unwrap_or("auto")
            .into(),
        encoder_experiment_applied: v["encoderExperimentApplied"]
            .as_str()
            .unwrap_or("rateControl")
            .into(),
        encoder_experiment_fallback_reason: v["encoderExperimentFallbackReason"]
            .as_str()
            .map(str::to_owned),
        encoder_frame_drops: v["encoderFrameDrops"].as_i64().unwrap_or(0),
        encoder_frame_drop_fps: bounded_u32(&v, "encoderFrameDropFps"),
        valid_encode_output_fps: bounded_u32(&v, "validEncodeOutputFps"),
        encode_submit_call_p50_us: v["encodeSubmitCallP50Us"].as_u64().unwrap_or(0),
        encode_submit_call_p95_us: v["encodeSubmitCallP95Us"].as_u64().unwrap_or(0),
        encoder_callback_p50_us: v["encoderCallbackP50Us"].as_u64().unwrap_or(0),
        encoder_callback_p95_us: v["encoderCallbackP95Us"].as_u64().unwrap_or(0),
        packetization_in_flight: bounded_u32(&v, "packetizationInFlight"),
        base_frame_qp: bounded_i32(&v, "baseFrameQp"),
        base_frame_qp_changes: v["baseFrameQpChanges"].as_i64().unwrap_or(0),
        capture_fps: bounded_u32(&v, "captureFps"),
        encode_submit_fps: bounded_u32(&v, "encodeSubmitFps"),
        encode_output_fps: bounded_u32(&v, "encodeOutputFps"),
        rendered_fps: v["receiverRenderedFps"].as_u64().map(|value| value as u32),
        capture_callbacks: v["captureCallbacks"].as_i64().unwrap_or(0),
        encode_output_callbacks: v["encodeOutputCallbacks"].as_i64().unwrap_or(0),
        encode_submit_failures: v["encodeSubmitFailures"].as_i64().unwrap_or(0),
        encode_in_flight: bounded_u32(&v, "encodeInFlight"),
        dropped: v["dropped"].as_i64().unwrap_or(0),
        network_dropped: v["networkDropped"].as_i64().unwrap_or(0),
        network_queue_dropped: v["networkQueueDropped"].as_i64().unwrap_or(0),
        recovery_frames_dropped: v["recoveryFramesDropped"].as_i64().unwrap_or(0),
        udp_send_failures: v["udpSendFailures"].as_i64().unwrap_or(0),
        udp_send_retries: v["udpSendRetries"].as_i64().unwrap_or(0),
        recovery_keyframes: v["recoveryKeyframes"].as_i64().unwrap_or(0),
        recovery_requests_suppressed: v["recoveryRequestsSuppressed"].as_i64().unwrap_or(0),
        capture_queue_dropped: v["captureQueueDropped"].as_i64().unwrap_or(0),
        capture_to_encode_us: v["captureToEncodeUs"].as_u64().unwrap_or(0),
        max_capture_to_encode_us: v["maxCaptureToEncodeUs"].as_u64().unwrap_or(0),
        capture_queue_wait_us: v["captureQueueWaitUs"].as_u64().unwrap_or(0),
        max_capture_queue_wait_us: v["maxCaptureQueueWaitUs"].as_u64().unwrap_or(0),
        encode_output_us: v["encodeOutputUs"].as_u64().unwrap_or(0),
        max_encode_output_us: v["maxEncodeOutputUs"].as_u64().unwrap_or(0),
        packetization_us: v["packetizationUs"].as_u64().unwrap_or(0),
        max_packetization_us: v["maxPacketizationUs"].as_u64().unwrap_or(0),
        send_block_us: v["sendBlockUs"].as_u64().unwrap_or(0),
        max_send_block_us: v["maxSendBlockUs"].as_u64().unwrap_or(0),
        send_pace_us: v["sendPaceUs"].as_u64().unwrap_or(0),
        max_send_pace_us: v["maxSendPaceUs"].as_u64().unwrap_or(0),
        pending_frame: bounded_u32(&v, "pendingFrame"),
        pending_frame_bytes: v["pendingFrameBytes"].as_u64().unwrap_or(0),
        pending_frame_oldest_age_us: v["pendingFrameOldestAgeUs"].as_u64().unwrap_or(0),
        capture_backend: v["captureBackend"]
            .as_str()
            .unwrap_or("screenCaptureKit")
            .into(),
        media_transport: v["mediaTransport"].as_str().unwrap_or("udp").into(),
        first_capture_ms: v["firstCaptureMs"].as_u64().unwrap_or(0),
        first_encode_ms: v["firstEncodeMs"].as_u64().unwrap_or(0),
        first_send_ms: v["firstSendMs"].as_u64().unwrap_or(0),
        current_bitrate: bounded_u32(&v, "currentBitrate"),
        bitrate_floor_collapse_count: v["bitrateFloorCollapseCount"].as_i64().unwrap_or(0),
        bitrate_floor_collapse_last_reason: v["bitrateFloorCollapseLastReason"]
            .as_str()
            .unwrap_or("none")
            .into(),
        encoder_mode: v["encoderMode"].as_str().unwrap_or("unknown").into(),
        encoder_id: v["encoderID"].as_str().unwrap_or("unknown").into(),
        encoder_hardware_accelerated: v["encoderHardwareAccelerated"].as_bool(),
        encoder_preset: v["encoderPreset"].as_str().unwrap_or("unknown").into(),
        encoder_profile: v["encoderProfile"].as_str().unwrap_or("unknown").into(),
        encoder_applied_properties: string_array(&v, "encoderAppliedProperties"),
        encoder_unsupported_properties: string_array(&v, "encoderUnsupportedProperties"),
        encoder_rejected_properties: string_array(&v, "encoderRejectedProperties"),
        encoder_fallback_reason: v["encoderFallbackReason"].as_str().map(str::to_owned),
        quality_hint: v["qualityHint"].as_f64().map(|value| value as f32),
        quality_override: v["qualityOverride"].as_f64().map(|value| value as f32),
        quality_adaptation_checks: v["qualityAdaptationChecks"].as_i64().unwrap_or(0),
        quality_adaptation_changes: v["qualityAdaptationChanges"].as_i64().unwrap_or(0),
        quality_adaptation_rejections: v["qualityAdaptationRejections"].as_i64().unwrap_or(0),
        quality_adaptation_last_status: v["qualityAdaptationLastStatus"]
            .as_str()
            .unwrap_or("not_checked")
            .into(),
        capture_interval_p95_us: v["captureIntervalP95Us"].as_u64().unwrap_or(0),
        capture_to_encode_p95_us: v["captureToEncodeP95Us"].as_u64().unwrap_or(0),
        capture_queue_wait_p95_us: v["captureQueueWaitP95Us"].as_u64().unwrap_or(0),
        encode_output_p95_us: v["encodeOutputP95Us"].as_u64().unwrap_or(0),
        packetization_p95_us: v["packetizationP95Us"].as_u64().unwrap_or(0),
        encode_output_interval_p95_us: v["encodeOutputIntervalP95Us"].as_u64().unwrap_or(0),
        send_block_p95_us: v["sendBlockP95Us"].as_u64().unwrap_or(0),
        send_pace_p95_us: v["sendPaceP95Us"].as_u64().unwrap_or(0),
        last_au_bytes: v["lastAuBytes"].as_u64().unwrap_or(0),
        last_au_fragments: bounded_u32(&v, "lastAuFragments"),
        last_au_parity: bounded_u32(&v, "lastAuParity"),
        last_au_datagrams: bounded_u32(&v, "lastAuDatagrams"),
        last_au_expected_datagrams: bounded_u32(&v, "lastAuExpectedDatagrams"),
        last_au_send_us: v["lastAuSendUs"].as_u64().unwrap_or(0),
        last_au_is_keyframe: v["lastAuIsKeyframe"].as_bool().unwrap_or(false),
        max_au_bytes: v["maxAuBytes"].as_u64().unwrap_or(0),
        max_au_fragments: bounded_u32(&v, "maxAuFragments"),
        sent_datagrams: v["sentDatagrams"].as_i64().unwrap_or(0),
        sent_parity_datagrams: v["sentParityDatagrams"].as_i64().unwrap_or(0),
        receiver_frame_gaps: v["receiverFrameGaps"].as_i64().unwrap_or(0),
        receiver_input_drops: v["receiverInputDrops"].as_i64().unwrap_or(0),
        receiver_incomplete_aus: v["receiverIncompleteAus"]
            .as_i64()
            .or_else(|| v["receiverIncompleteAUs"].as_i64())
            .unwrap_or(0),
        receiver_stale_frames: v["receiverStaleFrames"].as_i64().unwrap_or(0),
        receiver_stale_input_drops: v["receiverStaleInputDrops"].as_i64(),
        receiver_output_burst_discards: v["receiverOutputBurstDiscards"].as_i64().unwrap_or(0),
        receiver_rtt_ms: v["receiverRttMs"]
            .as_u64()
            .map(|value| value.min(u32::MAX as u64) as u32),
        receiver_wire_ms: v["receiverWireMs"]
            .as_u64()
            .map(|value| value.min(u32::MAX as u64) as u32),
        receiver_feedback_age_ms: v["receiverFeedbackAgeMs"].as_u64(),
        udp_stability_profile: v["udpStabilityProfile"].as_str().unwrap_or("legacy").into(),
        udp_burst_datagrams: bounded_u32(&v, "udpBurstDatagrams"),
        udp_pacing_rate_multiplier: bounded_u32(&v, "udpPacingRateMultiplier").max(1),
        udp_fec_parity_shards: bounded_u32(&v, "udpFecParityShards"),
        udp_adaptive_pacing: v["udpAdaptivePacing"].as_bool().unwrap_or(false),
        udp_burst_reason: v["udpBurstReason"].as_str().unwrap_or("fixed").into(),
        receiver_media_datagrams: v["receiverMediaDatagrams"].as_u64().unwrap_or(0),
        receiver_data_datagrams: v["receiverDataDatagrams"].as_u64().unwrap_or(0),
        receiver_parity_datagrams: v["receiverParityDatagrams"].as_u64().unwrap_or(0),
        receiver_fec_restored_fragments: v["receiverFecRestoredFragments"].as_u64().unwrap_or(0),
        receiver_unrecoverable_fec_groups: bounded_u32(&v, "receiverUnrecoverableFecGroups"),
        receiver_max_missing_data_fragments: bounded_u32(&v, "receiverMaxMissingDataFragments"),
        receiver_one_frame_gap_events: bounded_u32(&v, "receiverOneFrameGapEvents"),
        receiver_multi_frame_gap_events: bounded_u32(&v, "receiverMultiFrameGapEvents"),
        receiver_paired_idr_episodes: bounded_u32(&v, "receiverPairedIdrEpisodes"),
        receiver_suppressed_recovery_requests: bounded_u32(
            &v,
            "receiverSuppressedRecoveryRequests",
        ),
        receiver_fec_decode_failures: bounded_u32(&v, "receiverFecDecodeFailures"),
        split_direction: v["splitDirection"].as_str().map(str::to_owned),
        split_preparation_p50_us: v["splitPreparationP50Us"].as_u64().unwrap_or(0),
        split_preparation_p95_us: v["splitPreparationP95Us"].as_u64().unwrap_or(0),
        split_pair_admission_drops: v["splitPairAdmissionDrops"].as_i64().unwrap_or(0),
        encoded_pair_callback_p50_us: v["encodedPairCallbackP50Us"].as_u64().unwrap_or(0),
        encoded_pair_callback_p95_us: v["encodedPairCallbackP95Us"].as_u64().unwrap_or(0),
        encoded_pair_timeouts: v["encodedPairTimeouts"].as_i64().unwrap_or(0),
        encoded_pair_drops: v["encodedPairDrops"].as_i64().unwrap_or(0),
        left_valid_encode_output_fps: bounded_u32(&v, "leftValidEncodeOutputFps"),
        right_valid_encode_output_fps: bounded_u32(&v, "rightValidEncodeOutputFps"),
        left_encoder_frame_drops: v["leftEncoderFrameDrops"].as_i64().unwrap_or(0),
        right_encoder_frame_drops: v["rightEncoderFrameDrops"].as_i64().unwrap_or(0),
        left_bitrate_bps: v["leftBitrateBps"].as_u64().unwrap_or(0),
        right_bitrate_bps: v["rightBitrateBps"].as_u64().unwrap_or(0),
        aggregate_bitrate_bps: v["aggregateBitrateBps"].as_u64().unwrap_or(0),
        left_receiver_loss: v["leftReceiverLoss"].as_u64().unwrap_or(0),
        right_receiver_loss: v["rightReceiverLoss"].as_u64().unwrap_or(0),
        left_rendered_fps: bounded_u32(&v, "leftRenderedFps"),
        right_rendered_fps: bounded_u32(&v, "rightRenderedFps"),
        joined_rendered_fps: bounded_u32(&v, "joinedRenderedFps"),
        pair_ready_delta_p95_us: v["pairReadyDeltaP95Us"].as_u64().unwrap_or(0),
        pair_ready_delta_max_us: v["pairReadyDeltaMaxUs"].as_u64().unwrap_or(0),
        pair_sync_timeouts: v["pairSyncTimeouts"].as_i64().unwrap_or(0),
        unmatched_output_drops: v["unmatchedOutputDrops"].as_i64().unwrap_or(0),
        paired_recovery_requests: v["pairedRecoveryRequests"].as_i64().unwrap_or(0),
        paired_recovery_keyframes: v["pairedRecoveryKeyframes"].as_i64().unwrap_or(0),
        split_test_injected_drops: v["splitTestInjectedDrops"].as_i64().unwrap_or(0),
        split_flow_active_leases: bounded_u32(&v, "splitFlowActiveLeases"),
        split_flow_capacity: bounded_u32(&v, "splitFlowCapacity"),
        split_pre_encode_admission_drops: v["splitPreEncodeAdmissionDrops"].as_i64().unwrap_or(0),
        split_encoded_queue_depth: bounded_u32(&v, "splitEncodedQueueDepth"),
        split_encoded_queue_oldest_us: v["splitEncodedQueueOldestUs"].as_u64().unwrap_or(0),
        split_capture_queue_oldest_us: v["splitCaptureQueueOldestUs"].as_u64(),
        split_recovery_boundary_discards: v["splitRecoveryBoundaryDiscards"].as_i64().unwrap_or(0),
        split_post_encode_delta_drops: v["splitPostEncodeDeltaDrops"].as_i64().unwrap_or(0),
        split_wire_pairs_attempted: v["splitWirePairsAttempted"].as_i64().unwrap_or(0),
        split_wire_pair_send_failures: v["splitWirePairSendFailures"].as_i64().unwrap_or(0),
        split_keyframe_gap_recoveries: v["splitKeyframeGapRecoveries"].as_i64().unwrap_or(0),
        split_delta_gap_recoveries: v["splitDeltaGapRecoveries"].as_i64().unwrap_or(0),
        error: v["error"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}

impl CaptureBackend for FfiBackend {
    fn stop_with_reason(&self, handle: u32, reason_code: u8) -> Result<(), String> {
        self.stop_with_notice(handle, i32::from(reason_code))
    }

    fn platform(&self) -> &'static str {
        "macos"
    }

    fn capture_backends(&self) -> Vec<CaptureBackendInfo> {
        let persistent_access = self
            .lib()
            .ok()
            .and_then(|lib| unsafe {
                lib.get::<unsafe extern "C" fn() -> i32>(
                    b"leftcar_capture_has_persistent_access_v1",
                )
                .ok()
                .map(|f| f() == 1)
            })
            .unwrap_or(false);
        macos_capture_backends(persistent_access)
    }

    fn encoder_experiments(&self) -> Result<Vec<EncoderExperimentInfo>, String> {
        let lib = self.lib()?;
        unsafe {
            type CPtr = *mut std::ffi::c_char;
            let function = match lib
                .get::<unsafe extern "C" fn() -> CPtr>(b"leftcar_capture_encoder_experiments_v1")
            {
                Ok(function) => function,
                Err(_) => {
                    return Ok(vec![EncoderExperimentInfo {
                        id: EncoderExperiment::Auto,
                        label: "Automatic".into(),
                        hint: "Host-selected encoder policy".into(),
                        requires_reconnect: true,
                    }]);
                }
            };
            let ptr = function();
            let json = self
                .take_string(ptr)
                .ok_or("encoder experiment capability returned null")?;
            let experiments = parse_encoder_experiments_json(&json)?;
            Ok(experiments
                .into_iter()
                .filter(|entry| is_supported_encoder_experiment(entry.id))
                .collect())
        }
    }

    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        let lib = self.lib()?;
        unsafe {
            let f: Symbol<unsafe extern "C" fn() -> *mut std::ffi::c_char> = lib
                .get(b"leftcar_capture_list_displays")
                .map_err(|e| e.to_string())?;
            let ptr = f();
            let json = self.take_string(ptr).ok_or("list_displays returned null")?;
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
        media_transport: &str,
        content_mode: &str,
        encoder_experiment: EncoderExperiment,
        udp_stability: &AppliedUdpStability,
    ) -> Result<u32, String> {
        let lib = self.lib()?;
        let c_ip = CString::new(ip).map_err(|_| "ip contains NUL")?;
        let c_backend = CString::new(capture_backend).map_err(|_| "backend contains NUL")?;
        let c_transport =
            CString::new(media_transport).map_err(|_| "media transport contains NUL")?;
        let c_content_mode = CString::new(content_mode).map_err(|_| "content mode contains NUL")?;
        let c_encoder_experiment = CString::new(encoder_experiment.as_str())
            .map_err(|_| "encoder experiment contains NUL")?;
        let c_udp_profile = CString::new(udp_stability.applied.as_str())
            .map_err(|_| "UDP stability profile contains NUL")?;
        unsafe {
            type StartV7 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                u8,
                u8,
                i32,
            ) -> u32;
            type StartV6 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
            ) -> u32;
            type StartV5 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
            ) -> u32;
            type StartV4 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
                *const std::ffi::c_char,
            ) -> u32;
            type StartV3 = unsafe extern "C" fn(
                *const std::ffi::c_char,
                u16,
                u32,
                u32,
                u32,
                u32,
                *const std::ffi::c_char,
            ) -> u32;
            let legacy_udp = udp_stability.burst_datagrams == 8
                && udp_stability.fec_parity_shards == 2
                && !udp_stability.adaptive_pacing;
            let v7_handle = if media_transport == "udp" {
                match lib.get::<StartV7>(b"leftcar_capture_start_v7") {
                    Ok(f) => Some(f(
                        c_ip.as_ptr(),
                        port,
                        source_index,
                        w,
                        h,
                        fps,
                        c_backend.as_ptr(),
                        c_transport.as_ptr(),
                        c_content_mode.as_ptr(),
                        c_encoder_experiment.as_ptr(),
                        c_udp_profile.as_ptr(),
                        udp_stability.burst_datagrams,
                        udp_stability.fec_parity_shards,
                        i32::from(udp_stability.adaptive_pacing),
                    )),
                    Err(_) if !legacy_udp => {
                        return Err(
                            "capture shim does not support the requested UDP stability profile"
                                .into(),
                        );
                    }
                    Err(_) => None,
                }
            } else {
                None
            };
            let handle = if let Some(handle) = v7_handle {
                handle
            } else if let Ok(f) = lib.get::<StartV6>(b"leftcar_capture_start_v6") {
                f(
                    c_ip.as_ptr(),
                    port,
                    source_index,
                    w,
                    h,
                    fps,
                    c_backend.as_ptr(),
                    c_transport.as_ptr(),
                    c_content_mode.as_ptr(),
                    c_encoder_experiment.as_ptr(),
                )
            } else if encoder_experiment != EncoderExperiment::Auto {
                return Err(format!(
                    "capture shim does not support encoder experiment {}",
                    encoder_experiment.as_str()
                ));
            } else if let Ok(f) = lib.get::<StartV5>(b"leftcar_capture_start_v5") {
                f(
                    c_ip.as_ptr(),
                    port,
                    source_index,
                    w,
                    h,
                    fps,
                    c_backend.as_ptr(),
                    c_transport.as_ptr(),
                    c_content_mode.as_ptr(),
                )
            } else {
                match lib.get::<StartV4>(b"leftcar_capture_start_v4") {
                    Ok(f) if content_mode == "interactive" => f(
                        c_ip.as_ptr(),
                        port,
                        source_index,
                        w,
                        h,
                        fps,
                        c_backend.as_ptr(),
                        c_transport.as_ptr(),
                    ),
                    Ok(_) => {
                        return Err(
                            "capture shim does not support the requested content mode".into()
                        );
                    }
                    Err(_) if media_transport == "udp" && content_mode == "interactive" => {
                        match lib.get::<StartV3>(b"leftcar_capture_start_v3") {
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
                                    )
                                        -> u32,
                                > = lib
                                    .get(b"leftcar_capture_start_v2")
                                    .map_err(|e| e.to_string())?;
                                f(c_ip.as_ptr(), port, source_index, w, h, fps)
                            }
                            Err(_) => {
                                return Err(
                                    "capture shim does not support selectable backends".into()
                                );
                            }
                        }
                    }
                    Err(_) => {
                        return Err(
                            "capture shim does not support the requested media transport".into(),
                        );
                    }
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
            // v3 sends an LCT1 termination notice so a live viewer closes its
            // window immediately; older shims fall back to the silent v2 stop.
            if let Ok(f) =
                lib.get::<unsafe extern "C" fn(u32, i32) -> i32>(b"leftcar_capture_stop_v3")
            {
                let rc = f(handle, 3);
                if rc != 0 {
                    return Err(format!("stop({handle}) rc={rc}"));
                }
                return Ok(());
            }
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
            let json = self.take_string(ptr).ok_or("stats returned null")?;
            parse_stats_json(&json)
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
            let error_function: Symbol<unsafe extern "C" fn() -> *const std::ffi::c_char> = lib
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

    fn set_quality_override(&self, handle: u32, quality: Option<f32>) -> Result<(), String> {
        let lib = self.lib()?;
        let quality_percent = quality
            .map(|value| (value * 100.0).round() as i32)
            .unwrap_or(0);
        unsafe {
            let function: Symbol<unsafe extern "C" fn(u32, i32) -> i32> = lib
                .get(b"leftcar_capture_set_quality_v1")
                .map_err(|error| error.to_string())?;
            let result = function(handle, quality_percent);
            if result == 0 {
                return Ok(());
            }
            let error_function: Symbol<unsafe extern "C" fn() -> *const std::ffi::c_char> = lib
                .get(b"leftcar_capture_last_error_v2")
                .map_err(|error| error.to_string())?;
            let pointer = error_function();
            let message = if pointer.is_null() {
                format!("set quality override failed with rc={result}")
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
    use control_contract::host::EncoderExperiment;

    #[test]
    fn parse_stats_json_preserves_known_capture_queue_age_and_unknown_legacy_age() {
        for (payload, expected) in [
            (r#"{"splitCaptureQueueOldestUs":45600}"#, Some(45_600)),
            (r#"{"splitCaptureQueueOldestUs":0}"#, Some(0)),
            (r#"{"state":"running"}"#, None),
            (r#"{"splitCaptureQueueOldestUs":null}"#, None),
            (r#"{"splitCaptureQueueOldestUs":-1}"#, None),
        ] {
            let stats = parse_stats_json(payload).unwrap();
            assert_eq!(stats.split_capture_queue_oldest_us, expected, "{payload}");
        }
    }

    #[test]
    fn parse_stats_json_preserves_encoder_diagnostics() {
        let stats = parse_stats_json(
            r#"{
  "frames":1,
  "bytes":2,
  "state":"running",
  "encoderMode":"ave",
  "encoderID":"com.apple.videotoolbox.videoencoder.ave.avc",
  "encoderHardwareAccelerated":true,
  "encoderPreset":"high-speed",
  "encoderProfile":"main",
  "encoderAppliedProperties":["HighSpeed","Quality"],
  "encoderUnsupportedProperties":["SuggestedLookAheadFrameCount"],
  "encoderRejectedProperties":["Quality=-12900"],
  "encoderFallbackReason":null,
  "bitrateFloorCollapseCount":7,
  "bitrateFloorCollapseLastReason":"resolution_fallback_floor_reached"
}"#,
        )
        .unwrap();
        assert_eq!(stats.encoder_mode, "ave");
        assert_eq!(stats.encoder_hardware_accelerated, Some(true));
        assert_eq!(stats.encoder_applied_properties, ["HighSpeed", "Quality"]);
        assert_eq!(stats.encoder_rejected_properties, ["Quality=-12900"]);
        assert_eq!(stats.bitrate_floor_collapse_count, 7);
        assert_eq!(
            stats.bitrate_floor_collapse_last_reason,
            "resolution_fallback_floor_reached"
        );
    }

    #[test]
    fn parse_encoder_experiment_capabilities_keeps_phase_a_ids() {
        let capabilities = parse_encoder_experiments_json(
            r#"[
                {"id":"auto","label":"Automatic","hint":"automatic","requiresReconnect":true},
                {"id":"adaptiveQp","label":"Adaptive QP","hint":"qp","requiresReconnect":true},
                {"id":"splitVertical","label":"4K split","hint":"dual","requiresReconnect":true}
            ]"#,
        )
        .unwrap();
        assert_eq!(capabilities.len(), 3);
        assert_eq!(capabilities[0].id, EncoderExperiment::Auto);
        assert_eq!(capabilities[1].id, EncoderExperiment::AdaptiveQp);
        assert_eq!(capabilities[2].id, EncoderExperiment::SplitVertical);
        assert!(capabilities
            .iter()
            .all(|entry| is_supported_encoder_experiment(entry.id)));
        assert!(!is_supported_encoder_experiment(
            EncoderExperiment::SplitHorizontal
        ));
        assert!(capabilities.iter().all(|entry| entry.requires_reconnect));
    }

    #[test]
    fn parse_stats_json_maps_new_experiment_metrics_and_old_shim_defaults() {
        let stats = parse_stats_json(
            r#"{
  "frames":1,
  "bytes":2,
  "state":"running",
  "fps":60,
  "kbps":1000,
  "fpsTarget":60,
  "encoderExperimentRequested":"adaptiveQp",
  "encoderExperimentApplied":"adaptiveQp",
  "encoderExperimentFallbackReason":null,
  "encoderFrameDrops":7,
  "encoderFrameDropFps":8,
  "validEncodeOutputFps":59,
  "encodeSubmitCallP50Us":111,
  "encodeSubmitCallP95Us":222,
  "encoderCallbackP50Us":333,
  "encoderCallbackP95Us":444,
  "packetizationInFlight":2,
  "baseFrameQp":31,
  "baseFrameQpChanges":5,
  "splitFlowActiveLeases":3,
  "splitFlowCapacity":5,
  "splitPreEncodeAdmissionDrops":8,
  "splitEncodedQueueDepth":1,
  "splitEncodedQueueOldestUs":12300,
  "splitRecoveryBoundaryDiscards":4,
  "splitPostEncodeDeltaDrops":0,
  "splitWirePairsAttempted":600,
  "splitWirePairSendFailures":2,
  "splitKeyframeGapRecoveries":1,
  "splitDeltaGapRecoveries":2,
  "udpPacingRateMultiplier":2
}"#,
        )
        .unwrap();
        assert_eq!(stats.encoder_experiment_requested, "adaptiveQp");
        assert_eq!(stats.encoder_experiment_applied, "adaptiveQp");
        assert_eq!(stats.encoder_experiment_fallback_reason, None);
        assert_eq!(stats.encoder_frame_drops, 7);
        assert_eq!(stats.encoder_frame_drop_fps, 8);
        assert_eq!(stats.valid_encode_output_fps, 59);
        assert_eq!(stats.encode_submit_call_p50_us, 111);
        assert_eq!(stats.encode_submit_call_p95_us, 222);
        assert_eq!(stats.encoder_callback_p50_us, 333);
        assert_eq!(stats.encoder_callback_p95_us, 444);
        assert_eq!(stats.packetization_in_flight, 2);
        assert_eq!(stats.base_frame_qp, Some(31));
        assert_eq!(stats.base_frame_qp_changes, 5);
        assert_eq!(stats.split_flow_active_leases, 3);
        assert_eq!(stats.split_flow_capacity, 5);
        assert_eq!(stats.split_pre_encode_admission_drops, 8);
        assert_eq!(stats.split_encoded_queue_depth, 1);
        assert_eq!(stats.split_encoded_queue_oldest_us, 12_300);
        assert_eq!(stats.split_recovery_boundary_discards, 4);
        assert_eq!(stats.split_post_encode_delta_drops, 0);
        assert_eq!(stats.split_wire_pairs_attempted, 600);
        assert_eq!(stats.split_wire_pair_send_failures, 2);
        assert_eq!(stats.split_keyframe_gap_recoveries, 1);
        assert_eq!(stats.split_delta_gap_recoveries, 2);
        assert_eq!(stats.udp_pacing_rate_multiplier, 2);
        assert!(stats.encoder_experiment_diagnostics_available);

        let old = parse_stats_json(r#"{"state":"running"}"#).unwrap();
        assert_eq!(old.encoder_experiment_requested, "auto");
        assert_eq!(old.encoder_experiment_applied, "rateControl");
        assert_eq!(old.base_frame_qp, None);
        assert_eq!(old.encoder_frame_drops, 0);
        assert_eq!(old.split_flow_active_leases, 0);
        assert_eq!(old.split_wire_pair_send_failures, 0);
        assert_eq!(old.split_delta_gap_recoveries, 0);
        assert_eq!(old.udp_pacing_rate_multiplier, 1);
        assert!(!old.encoder_experiment_diagnostics_available);

        let partial = parse_stats_json(
            r#"{
  "encoderExperimentRequested":"adaptiveQp",
  "encoderExperimentApplied":"adaptiveQp",
  "encoderFrameDrops":0,
  "encoderFrameDropFps":0,
  "validEncodeOutputFps":0,
  "encodeSubmitCallP50Us":0,
  "encodeSubmitCallP95Us":0,
  "encoderCallbackP50Us":0,
  "encoderCallbackP95Us":0,
  "packetizationInFlight":0,
  "baseFrameQp":null,
  "baseFrameQpChanges":0
}"#,
        )
        .unwrap();
        assert!(!partial.encoder_experiment_diagnostics_available);
    }

    #[test]
    fn parse_stats_json_rejects_out_of_range_base_qp_and_clamps_u32_metrics() {
        let stats = parse_stats_json(
            r#"{
              "encoderFrameDropFps":4294967296,
              "validEncodeOutputFps":4294967297,
              "packetizationInFlight":4294967298,
              "baseFrameQp":2147483648
            }"#,
        )
        .unwrap();
        assert_eq!(stats.encoder_frame_drop_fps, u32::MAX);
        assert_eq!(stats.valid_encode_output_fps, u32::MAX);
        assert_eq!(stats.packetization_in_flight, u32::MAX);
        assert_eq!(stats.base_frame_qp, None);
    }

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

    #[test]
    fn unapproved_build_advertises_only_the_automatic_pickerless_backend() {
        let backends = macos_capture_backends(false);
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "cgDisplayStream");
    }

    #[test]
    fn approved_build_prefers_persistent_screen_capture_kit() {
        let backends = macos_capture_backends(true);
        assert_eq!(backends[0].id, "screenCaptureKit");
        assert_eq!(backends[1].id, "cgDisplayStream");
    }
}
