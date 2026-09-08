//! Control server: viewers connect (pull) and exchange newline-delimited
//! JSON `{"command","args"}` / `{"ok":true,"result"}` over TCP (design §제어평면).
//!
//! `addNumbers` delegates to the real rustra host_package so the H02 proof
//! path stays intact; stateful v1 stream commands dispatch locally.

use crate::backend::SharedBackend;
use control_contract::host::{
    CatalogView, EncoderExperiment, EncoderExperimentInfo, ReconfigureStreamInput,
    ReconfigureStreamOutput, SessionView, StartStreamInput, StartStreamOutput, StatusView,
};
use control_contract::udp_stability::{
    host_udp_stability_capabilities, resolve_udp_stability, AppliedUdpStability,
};

pub use control_contract::host::{StatsInfo, StatusView as StatusViewPublic};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    atomic::{AtomicU16, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Geometry and transport facts of the live session being reconfigured,
/// captured before the old backend handle is stopped.
struct ReconfigureSnapshot {
    handle: u32,
    source_index: u32,
    viewer_host: String,
    viewer_port: u16,
    capture_backend: String,
    media_transport: String,
    content_mode: String,
    encoder_experiment: EncoderExperiment,
    udp_stability: Option<AppliedUdpStability>,
    width: u32,
    height: u32,
    fps_target: u32,
    input_enabled: bool,
}

struct Session {
    handle: u32,
    source_index: u32,
    source_name: String,
    width: u32,
    height: u32,
    fps_target: u32,
    quality_state: String,
    capture_backend: String,
    content_mode: String,
    encoder_experiment: EncoderExperiment,
    viewer_addr: String,
    viewer_port: u16,
    media_transport: String,
    udp_stability: Option<AppliedUdpStability>,
    input_enabled: bool,
    input_rate_hz: u32,
    terminal_since: Option<Instant>,
    /// Control-plane tombstone retained long enough for the viewer's 2s
    /// status poll to observe why the host stopped this session.
    terminal_error: Option<String>,
    /// A forced stop removes the handle from the backend immediately. Keep
    /// that fact so terminal-state GC does not try to stop it a second time.
    backend_released: bool,
}

fn normalize_media_transport(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "udp" | "wifi" => Some("udp"),
        "tcp" | "wifitcp" | "wifi-tcp" => Some("tcp"),
        "adbtcp" | "adb-tcp" => Some("adbTcp"),
        "usb" | "aoap" => Some("usb"),
        "auto" | "both" => Some("auto"),
        _ => None,
    }
}

fn normalize_content_mode(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "interactive" | "latency" => Some("interactive"),
        "video" | "movie" => Some("video"),
        _ => None,
    }
}

fn validate_stream_shape(width: u32, height: u32, fps: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 || fps == 0 || fps > 90 {
        return Err("unsupported stream dimensions or fps".into());
    }
    Ok(())
}

fn validate_split_request(
    encoder_experiment: EncoderExperiment,
    width: u32,
    height: u32,
    fps: u32,
    concrete_transport: &str,
    viewer_port: u16,
) -> Result<(), String> {
    if encoder_experiment != EncoderExperiment::SplitVertical {
        return Ok(());
    }
    if (width, height, fps) != (3840, 2160, 60)
        || concrete_transport != "udp"
        || viewer_port == u16::MAX
    {
        return Err(
            "splitVertical requires 3840x2160 at 60fps over direct UDP and two consecutive viewer ports"
                .into(),
        );
    }
    Ok(())
}

fn validate_split_start(input: &StartStreamInput, concrete_transport: &str) -> Result<(), String> {
    validate_split_request(
        input.encoder_experiment,
        input.width,
        input.height,
        input.fps,
        concrete_transport,
        input.viewer_port,
    )
}

fn canonical_encoder_experiment(id: EncoderExperiment) -> EncoderExperimentInfo {
    let (label, hint) = match id {
        EncoderExperiment::Auto => ("자동", "호스트가 사용 가능한 인코더 경로를 선택합니다."),
        EncoderExperiment::RateControl => {
            ("레이트 컨트롤", "고정 레이트 컨트롤 경로를 사용합니다.")
        }
        EncoderExperiment::AdaptiveQp => (
            "적응형 QP",
            "화면 변화와 인코더 압력에 따라 Base QP를 조절합니다.",
        ),
        EncoderExperiment::EncoderPool => {
            ("인코더 풀", "인코더가 제공하는 픽셀 버퍼 풀을 사용합니다.")
        }
        EncoderExperiment::SplitVertical => (
            "4K 듀얼 인코더",
            "4K 화면을 좌우 두 하드웨어 인코더와 UDP 포트로 전송합니다.",
        ),
        EncoderExperiment::SplitHorizontal => {
            return EncoderExperimentInfo {
                id,
                label: String::new(),
                hint: String::new(),
                requires_reconnect: true,
            }
        }
    };
    EncoderExperimentInfo {
        id,
        label: label.into(),
        hint: hint.into(),
        requires_reconnect: true,
    }
}

fn advertised_encoder_experiments(
    experiments: Vec<EncoderExperimentInfo>,
) -> Vec<EncoderExperimentInfo> {
    let mut normalized = Vec::new();
    for entry in experiments {
        let id = match entry.id {
            EncoderExperiment::Auto
            | EncoderExperiment::RateControl
            | EncoderExperiment::AdaptiveQp
            | EncoderExperiment::EncoderPool
            | EncoderExperiment::SplitVertical => entry.id,
            EncoderExperiment::SplitHorizontal => continue,
        };
        if normalized
            .iter()
            .any(|existing: &EncoderExperimentInfo| existing.id == id)
        {
            continue;
        }
        normalized.push(canonical_encoder_experiment(id));
    }
    normalized
}

fn encoder_experiment_is_startable(
    advertised: &[EncoderExperimentInfo],
    requested: EncoderExperiment,
    split_diagnostic_enabled: bool,
) -> bool {
    match requested {
        EncoderExperiment::SplitVertical => {
            split_diagnostic_enabled
                || advertised
                    .iter()
                    .any(|entry| entry.id == EncoderExperiment::SplitVertical)
        }
        EncoderExperiment::SplitHorizontal => false,
        _ => advertised.iter().any(|entry| entry.id == requested),
    }
}

fn build_attempts(requested: &str, wifi_candidates: &[String]) -> Vec<(String, &'static str)> {
    let mut attempts = Vec::new();
    match requested {
        "usb" => attempts.push(("127.0.0.1".into(), "usb")),
        "tcp" => attempts.extend(
            wifi_candidates
                .iter()
                .cloned()
                .map(|candidate| (candidate, "tcp")),
        ),
        "udp" => attempts.extend(
            wifi_candidates
                .iter()
                .cloned()
                .map(|candidate| (candidate, "udp")),
        ),
        "adbTcp" => attempts.push(("127.0.0.1".into(), "adbTcp")),
        "auto" => {
            attempts.push(("127.0.0.1".into(), "usb"));
            attempts.extend(
                wifi_candidates
                    .iter()
                    .cloned()
                    .map(|candidate| (candidate, "udp")),
            );
            attempts.extend(
                wifi_candidates
                    .iter()
                    .cloned()
                    .map(|candidate| (candidate, "tcp")),
            );
            attempts.push(("127.0.0.1".into(), "adbTcp"));
        }
        _ => {}
    }
    attempts
}

fn adb_forward(port: u16) -> Result<(), String> {
    let mapping = format!("tcp:{port}");
    let output = Command::new("adb")
        .args(["forward", mapping.as_str(), mapping.as_str()])
        .output()
        .map_err(|error| format!("ADB를 실행하지 못했습니다: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            format!("ADB USB 포트 전달에 실패했습니다 (code={})", output.status)
        } else {
            format!("ADB USB 포트 전달에 실패했습니다: {detail}")
        })
    }
}

fn adb_remove_forward(port: u16) {
    let mapping = format!("tcp:{port}");
    let _ = Command::new("adb")
        .args(["forward", "--remove", mapping.as_str()])
        .output();
}

fn cleanup_media_transport(transport: &str, port: u16) {
    if transport == "adbTcp" {
        adb_remove_forward(port);
    } else if transport == "usb" {
        // The USB mux is shared by control and media. Stop only the
        // session-scoped loopback media proxy so the accessory link remains
        // available for the next stream and for control polling.
        crate::aoap_proxy::stop_media_proxy();
    }
}

const TERMINAL_SESSION_RETENTION: Duration = Duration::from_secs(5);
const STARTUP_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_STATS_INTERVAL: Duration = Duration::from_millis(100);
/// A healthy viewer polls `getStatus` every 2s, so a connected control
/// socket should never sit silent this long. Half-open connections (network
/// drop, laptop sleep, no FIN) would otherwise hold their task and socket
/// open forever.
const CONTROL_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ControlServer {
    backend: SharedBackend,
    pairing: std::sync::Arc<crate::pairing::PairingServer>,
    control_port: AtomicU16,
    sessions: Mutex<State>,
}

struct State {
    next: u32,
    live: HashMap<u32, Session>,
}

impl ControlServer {
    pub fn new(
        backend: SharedBackend,
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
    ) -> Self {
        Self {
            backend,
            pairing,
            control_port: AtomicU16::new(crate::PREFERRED_CONTROL_PORT),
            sessions: Mutex::new(State {
                next: 1,
                live: HashMap::new(),
            }),
        }
    }

    pub fn set_control_port(&self, port: u16) {
        self.control_port.store(port, Ordering::Release);
    }

    /// Restore the previous capture geometry after a failed reconfigure. On
    /// recovery the session stays live with the restored handle; otherwise it
    /// becomes a terminal tombstone carrying the original failure.
    async fn restore_previous_stream(
        &self,
        session_id: u32,
        previous: &ReconfigureSnapshot,
        failure: &str,
    ) -> Result<u32, String> {
        let recovered = self.start_replacement_from_snapshot(
            previous,
            previous.encoder_experiment,
            previous.width,
            previous.height,
            previous.fps_target,
        );
        let recovered = match recovered {
            Ok(handle) => self.wait_for_first_frame(handle).await.map(|_| handle),
            Err(recovery_error) => Err(recovery_error),
        };
        // Same carry-over as reconfigure: a restored backend handle restarts
        // with input off, so the session's enablement must be re-applied.
        let input_enabled = match &recovered {
            Ok(handle) if previous.input_enabled => {
                self.backend.set_input_enabled(*handle, true).is_ok()
            }
            _ => false,
        };
        let mut state = self.sessions.lock().unwrap();
        if let Some(session) = state.live.get_mut(&session_id) {
            match recovered {
                Ok(recovered) => {
                    session.handle = recovered;
                    session.input_enabled = input_enabled;
                    session.backend_released = false;
                }
                Err(_) => {
                    session.terminal_error = Some(failure.to_owned());
                    session.terminal_since = Some(Instant::now());
                    session.backend_released = true;
                }
            }
        }
        recovered.map_err(|_| format!("replacement stream failed: {failure}"))
    }

    fn start_replacement_from_snapshot(
        &self,
        previous: &ReconfigureSnapshot,
        encoder_experiment: EncoderExperiment,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<u32, String> {
        let default_udp_stability = AppliedUdpStability {
            requested: control_contract::udp_stability::UdpStabilityProfile::Auto,
            applied: control_contract::udp_stability::UdpStabilityProfile::Auto,
            burst_datagrams: 0,
            fec_parity_shards: 0,
            adaptive_pacing: false,
            fallback_reason: None,
        };
        let udp_stability = previous
            .udp_stability
            .as_ref()
            .unwrap_or(&default_udp_stability);
        self.backend.start(
            previous.source_index,
            &previous.viewer_host,
            previous.viewer_port,
            width,
            height,
            fps,
            &previous.capture_backend,
            &previous.media_transport,
            &previous.content_mode,
            encoder_experiment,
            udp_stability,
        )
    }

    async fn reconfigure_stream(
        &self,
        input: ReconfigureStreamInput,
    ) -> Result<ReconfigureStreamOutput, String> {
        validate_stream_shape(input.width, input.height, input.fps)?;
        if !matches!(
            input.quality_state.as_str(),
            "native" | "fallback" | "downshifting" | "upshifting"
        ) {
            return Err("unsupported quality state".into());
        }
        let settled_quality_state = match input.quality_state.as_str() {
            "fallback" | "downshifting" => "fallback",
            _ => "native",
        };

        let previous = {
            let state = self.sessions.lock().unwrap();
            let session = state
                .live
                .get(&input.session)
                .filter(|session| !session.backend_released)
                .ok_or_else(|| format!("no such session {}", input.session))?;
            let viewer_host = session
                .viewer_addr
                .rsplit_once(':')
                .map(|(host, _)| host.to_owned())
                .ok_or_else(|| "session viewer address is invalid".to_owned())?;
            ReconfigureSnapshot {
                handle: session.handle,
                source_index: session.source_index,
                viewer_host,
                viewer_port: session.viewer_port,
                capture_backend: session.capture_backend.clone(),
                media_transport: session.media_transport.clone(),
                content_mode: session.content_mode.clone(),
                encoder_experiment: session.encoder_experiment,
                udp_stability: session.udp_stability.clone(),
                width: session.width,
                height: session.height,
                fps_target: session.fps_target,
                input_enabled: session.input_enabled,
            }
        };

        let requested_transport = normalize_media_transport(&previous.media_transport)
            .ok_or_else(|| format!("unsupported media transport: {}", previous.media_transport))?;
        // Resolve the replacement experiment and validate it BEFORE stopping
        // the previous backend: explicit unsupported or malformed input must
        // leave the live stream unchanged.
        let replacement_encoder_experiment = match input.encoder_experiment {
            Some(requested) => {
                let advertised = match self.backend.encoder_experiments() {
                    Ok(experiments) => advertised_encoder_experiments(experiments),
                    Err(error) => return Err(error),
                };
                let split_diagnostic_enabled = std::env::var("LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC")
                    .is_ok_and(|value| value == "1");
                if !encoder_experiment_is_startable(
                    &advertised,
                    requested,
                    split_diagnostic_enabled,
                ) {
                    return Err(format!(
                        "unsupported encoder experiment: {}",
                        requested.as_str()
                    ));
                }
                requested
            }
            None => {
                // Older request without a mode: retain the previous
                // experiment, demoting split to the single path when
                // leaving exact 4K (legacy behavior).
                if previous.encoder_experiment == EncoderExperiment::SplitVertical
                    && (input.width != 3_840 || input.height != 2_160)
                {
                    EncoderExperiment::Auto
                } else {
                    previous.encoder_experiment
                }
            }
        };

        // Split keeps the exact 4K60 direct-UDP consecutive-port
        // restriction; other modes use the supported single path. Both
        // branches resolve the replacement experiment FIRST and share this
        // validation, so legacy mode-omitted requests cannot retain an
        // invalid split shape (e.g. exact 4K at 90/30fps) either.
        validate_split_request(
            replacement_encoder_experiment,
            input.width,
            input.height,
            input.fps,
            requested_transport,
            previous.viewer_port,
        )?;

        self.backend.stop(previous.handle)?;
        cleanup_media_transport(&previous.media_transport, previous.viewer_port);

        let replacement_handle = match self.start_replacement_from_snapshot(
            &previous,
            replacement_encoder_experiment,
            input.width,
            input.height,
            input.fps,
        ) {
            Ok(handle) => match self.wait_for_first_frame(handle).await {
                Ok(()) => handle,
                Err(error) => {
                    let _ = self.backend.stop(handle);
                    cleanup_media_transport(&previous.media_transport, previous.viewer_port);
                    let _ = self
                        .restore_previous_stream(input.session, &previous, &error)
                        .await;
                    return Err(format!("replacement stream startup failed: {error}"));
                }
            },
            Err(error) => {
                cleanup_media_transport(&previous.media_transport, previous.viewer_port);
                let _ = self
                    .restore_previous_stream(input.session, &previous, &error)
                    .await;
                return Err(format!("replacement stream failed: {error}"));
            }
        };

        // The replacement backend handle starts with remote input disabled;
        // re-apply the session's enablement so a resolution switch never
        // silently strips the viewer of control.
        let input_enabled = previous.input_enabled
            && self
                .backend
                .set_input_enabled(replacement_handle, true)
                .is_ok();

        let mut state = self.sessions.lock().unwrap();
        let session = state
            .live
            .get_mut(&input.session)
            .ok_or_else(|| format!("session {} ended during reconfigure", input.session))?;
        session.handle = replacement_handle;
        session.input_enabled = input_enabled;
        session.width = input.width;
        session.height = input.height;
        session.fps_target = input.fps;
        session.quality_state = settled_quality_state.into();
        session.encoder_experiment = replacement_encoder_experiment;
        session.terminal_error = None;
        session.terminal_since = None;
        session.backend_released = false;
        Ok(ReconfigureStreamOutput {
            session: input.session,
            width: input.width,
            height: input.height,
            fps: input.fps,
            quality_state: settled_quality_state.into(),
            encoder_experiment: Some(replacement_encoder_experiment),
        })
    }

    async fn wait_for_first_frame(&self, handle: u32) -> Result<(), String> {
        let deadline = Instant::now() + STARTUP_FIRST_FRAME_TIMEOUT;
        loop {
            let stats = self.backend.stats(handle)?;
            // `running` only means that the capture backend accepted the
            // start request. It does not prove that capture, encode, and the
            // media socket produced a frame. Opening the viewer on that state
            // creates a black stream that the recovery loop cannot distinguish
            // from a healthy session. Require the first packet instead.
            if stats.first_send_ms > 0 {
                return Ok(());
            }
            if let Some(error) = stats.error.filter(|error| !error.is_empty()) {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "no video frame produced within {}s",
                    STARTUP_FIRST_FRAME_TIMEOUT.as_secs()
                ));
            }
            tokio::time::sleep(STARTUP_STATS_INTERVAL).await;
        }
    }

    pub async fn bind(&self, addr: &str) -> std::io::Result<std::net::SocketAddr> {
        TcpListener::bind(addr).await?.local_addr()
    }

    /// Accept loop — runs until the process exits.
    pub async fn run(self: std::sync::Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((sock, _)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        let peer = sock
                            .peer_addr()
                            .map(|a| a.ip().to_string())
                            .unwrap_or_default();
                        handle_conn(sock, &server, &peer).await;
                    });
                }
                Err(e) => {
                    eprintln!("control accept error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Snapshot for the Tauri UI (`get_status` command reuses this).
    pub fn snapshot(&self) -> StatusView {
        let now = Instant::now();
        let (sessions, expired) = {
            let mut state = self.sessions.lock().unwrap();
            let mut sessions = Vec::with_capacity(state.live.len());
            let mut expired_ids = Vec::new();

            for (id, s) in &mut state.live {
                let mut metrics = self.backend.stats(s.handle).unwrap_or_else(|_| StatsInfo {
                    frames: 0,
                    bytes: 0,
                    state: "stopped".into(),
                    fps: 0,
                    kbps: 0,
                    fps_target: 0,
                    encoder_experiment_diagnostics_available: false,
                    encoder_experiment_requested: "auto".into(),
                    encoder_experiment_applied: "rateControl".into(),
                    encoder_experiment_fallback_reason: None,
                    encoder_frame_drops: 0,
                    encoder_frame_drop_fps: 0,
                    valid_encode_output_fps: 0,
                    encode_submit_call_p50_us: 0,
                    encode_submit_call_p95_us: 0,
                    encoder_callback_p50_us: 0,
                    encoder_callback_p95_us: 0,
                    packetization_in_flight: 0,
                    base_frame_qp: None,
                    base_frame_qp_changes: 0,
                    capture_fps: 0,
                    encode_submit_fps: 0,
                    encode_output_fps: 0,
                    rendered_fps: None,
                    capture_callbacks: 0,
                    encode_output_callbacks: 0,
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
                    capture_backend: "unknown".into(),
                    media_transport: "unknown".into(),
                    first_capture_ms: 0,
                    first_encode_ms: 0,
                    first_send_ms: 0,
                    current_bitrate: 0,
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
                    capture_interval_p95_us: 0,
                    capture_to_encode_p95_us: 0,
                    capture_queue_wait_p95_us: 0,
                    encode_output_p95_us: 0,
                    packetization_p95_us: 0,
                    encode_output_interval_p95_us: 0,
                    send_block_p95_us: 0,
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
                    error: Some("backend stats unavailable".into()),
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
                });

                if let Some(error) = &s.terminal_error {
                    metrics.state = "stopped".into();
                    metrics.fps = 0;
                    metrics.kbps = 0;
                    metrics.pending_frame = 0;
                    metrics.error = Some(error.clone());
                }

                // Android Back/close is an intentional viewer action. Some
                // backends surface it through their error field, but the
                // control contract must expose it as an ordinary stop.
                if metrics.error.as_deref() == Some("viewer closed stream") {
                    metrics.state = "stopped".into();
                }

                let terminal = matches!(metrics.state.as_str(), "error" | "stopped" | "unknown");
                if terminal {
                    let terminal_since = s.terminal_since.get_or_insert(now);
                    if now.duration_since(*terminal_since) >= TERMINAL_SESSION_RETENTION {
                        expired_ids.push(*id);
                        continue;
                    }
                } else {
                    s.terminal_since = None;
                }

                sessions.push(SessionView {
                    session: *id,
                    source_index: s.source_index,
                    source_name: s.source_name.clone(),
                    viewer_addr: s.viewer_addr.clone(),
                    width: s.width,
                    height: s.height,
                    state: metrics.state,
                    fps: metrics.fps,
                    kbps: metrics.kbps,
                    fps_target: s.fps_target,
                    quality_state: s.quality_state.clone(),
                    udp_stability: s.udp_stability.clone(),
                    encoder_experiment_diagnostics_available: metrics
                        .encoder_experiment_diagnostics_available,
                    encoder_experiment_requested: metrics.encoder_experiment_requested,
                    encoder_experiment_applied: metrics.encoder_experiment_applied,
                    encoder_experiment_fallback_reason: metrics.encoder_experiment_fallback_reason,
                    encoder_frame_drops: metrics.encoder_frame_drops,
                    encoder_frame_drop_fps: metrics.encoder_frame_drop_fps,
                    valid_encode_output_fps: metrics.valid_encode_output_fps,
                    encode_submit_call_p50_us: metrics.encode_submit_call_p50_us,
                    encode_submit_call_p95_us: metrics.encode_submit_call_p95_us,
                    encoder_callback_p50_us: metrics.encoder_callback_p50_us,
                    encoder_callback_p95_us: metrics.encoder_callback_p95_us,
                    packetization_in_flight: metrics.packetization_in_flight,
                    base_frame_qp: metrics.base_frame_qp,
                    base_frame_qp_changes: metrics.base_frame_qp_changes,
                    capture_fps: metrics.capture_fps,
                    encode_submit_fps: metrics.encode_submit_fps,
                    encode_output_fps: metrics.encode_output_fps,
                    rendered_fps: metrics.rendered_fps,
                    capture_callbacks: metrics.capture_callbacks,
                    encode_output_callbacks: metrics.encode_output_callbacks,
                    encode_submit_failures: metrics.encode_submit_failures,
                    encode_in_flight: metrics.encode_in_flight,
                    input_enabled: s.input_enabled,
                    input_rate_hz: s.input_rate_hz,
                    dropped: metrics.dropped,
                    network_dropped: metrics.network_dropped,
                    network_queue_dropped: metrics.network_queue_dropped,
                    recovery_frames_dropped: metrics.recovery_frames_dropped,
                    udp_send_failures: metrics.udp_send_failures,
                    udp_send_retries: metrics.udp_send_retries,
                    recovery_keyframes: metrics.recovery_keyframes,
                    recovery_requests_suppressed: metrics.recovery_requests_suppressed,
                    capture_queue_dropped: metrics.capture_queue_dropped,
                    capture_to_encode_us: metrics.capture_to_encode_us,
                    max_capture_to_encode_us: metrics.max_capture_to_encode_us,
                    capture_queue_wait_us: metrics.capture_queue_wait_us,
                    max_capture_queue_wait_us: metrics.max_capture_queue_wait_us,
                    encode_output_us: metrics.encode_output_us,
                    max_encode_output_us: metrics.max_encode_output_us,
                    packetization_us: metrics.packetization_us,
                    max_packetization_us: metrics.max_packetization_us,
                    send_block_us: metrics.send_block_us,
                    max_send_block_us: metrics.max_send_block_us,
                    send_pace_us: metrics.send_pace_us,
                    max_send_pace_us: metrics.max_send_pace_us,
                    pending_frame: metrics.pending_frame,
                    pending_frame_bytes: metrics.pending_frame_bytes,
                    pending_frame_oldest_age_us: metrics.pending_frame_oldest_age_us,
                    frames: metrics.frames,
                    bytes: metrics.bytes,
                    capture_backend: metrics.capture_backend,
                    media_transport: metrics.media_transport,
                    first_capture_ms: metrics.first_capture_ms,
                    first_encode_ms: metrics.first_encode_ms,
                    first_send_ms: metrics.first_send_ms,
                    current_bitrate: metrics.current_bitrate,
                    bitrate_floor_collapse_count: metrics.bitrate_floor_collapse_count,
                    bitrate_floor_collapse_last_reason: metrics.bitrate_floor_collapse_last_reason,
                    encoder_mode: metrics.encoder_mode,
                    encoder_id: metrics.encoder_id,
                    encoder_hardware_accelerated: metrics.encoder_hardware_accelerated,
                    encoder_preset: metrics.encoder_preset,
                    encoder_profile: metrics.encoder_profile,
                    encoder_applied_properties: metrics.encoder_applied_properties,
                    encoder_unsupported_properties: metrics.encoder_unsupported_properties,
                    encoder_rejected_properties: metrics.encoder_rejected_properties,
                    encoder_fallback_reason: metrics.encoder_fallback_reason,
                    quality_hint: metrics.quality_hint,
                    quality_override: metrics.quality_override,
                    quality_adaptation_checks: metrics.quality_adaptation_checks,
                    quality_adaptation_changes: metrics.quality_adaptation_changes,
                    quality_adaptation_rejections: metrics.quality_adaptation_rejections,
                    quality_adaptation_last_status: metrics.quality_adaptation_last_status,
                    capture_interval_p95_us: metrics.capture_interval_p95_us,
                    capture_to_encode_p95_us: metrics.capture_to_encode_p95_us,
                    capture_queue_wait_p95_us: metrics.capture_queue_wait_p95_us,
                    encode_output_p95_us: metrics.encode_output_p95_us,
                    packetization_p95_us: metrics.packetization_p95_us,
                    encode_output_interval_p95_us: metrics.encode_output_interval_p95_us,
                    send_block_p95_us: metrics.send_block_p95_us,
                    send_pace_p95_us: metrics.send_pace_p95_us,
                    last_au_bytes: metrics.last_au_bytes,
                    last_au_fragments: metrics.last_au_fragments,
                    last_au_parity: metrics.last_au_parity,
                    last_au_datagrams: metrics.last_au_datagrams,
                    last_au_expected_datagrams: metrics.last_au_expected_datagrams,
                    last_au_send_us: metrics.last_au_send_us,
                    last_au_is_keyframe: metrics.last_au_is_keyframe,
                    max_au_bytes: metrics.max_au_bytes,
                    max_au_fragments: metrics.max_au_fragments,
                    sent_datagrams: metrics.sent_datagrams,
                    sent_parity_datagrams: metrics.sent_parity_datagrams,
                    error: metrics.error,
                    receiver_frame_gaps: metrics.receiver_frame_gaps,
                    receiver_input_drops: metrics.receiver_input_drops,
                    receiver_incomplete_aus: metrics.receiver_incomplete_aus,
                    receiver_stale_frames: metrics.receiver_stale_frames,
                    receiver_stale_input_drops: metrics.receiver_stale_input_drops,
                    receiver_output_burst_discards: metrics.receiver_output_burst_discards,
                    receiver_rtt_ms: metrics.receiver_rtt_ms,
                    receiver_wire_ms: metrics.receiver_wire_ms,
                    receiver_feedback_age_ms: metrics.receiver_feedback_age_ms,
                    udp_stability_profile: metrics.udp_stability_profile,
                    udp_burst_datagrams: metrics.udp_burst_datagrams,
                    udp_pacing_rate_multiplier: metrics.udp_pacing_rate_multiplier,
                    udp_fec_parity_shards: metrics.udp_fec_parity_shards,
                    udp_adaptive_pacing: metrics.udp_adaptive_pacing,
                    udp_burst_reason: metrics.udp_burst_reason,
                    receiver_media_datagrams: metrics.receiver_media_datagrams,
                    receiver_data_datagrams: metrics.receiver_data_datagrams,
                    receiver_parity_datagrams: metrics.receiver_parity_datagrams,
                    receiver_fec_restored_fragments: metrics.receiver_fec_restored_fragments,
                    receiver_unrecoverable_fec_groups: metrics.receiver_unrecoverable_fec_groups,
                    receiver_max_missing_data_fragments: metrics
                        .receiver_max_missing_data_fragments,
                    receiver_one_frame_gap_events: metrics.receiver_one_frame_gap_events,
                    receiver_multi_frame_gap_events: metrics.receiver_multi_frame_gap_events,
                    receiver_paired_idr_episodes: metrics.receiver_paired_idr_episodes,
                    receiver_suppressed_recovery_requests: metrics
                        .receiver_suppressed_recovery_requests,
                    receiver_fec_decode_failures: metrics.receiver_fec_decode_failures,
                    split_direction: metrics.split_direction,
                    split_preparation_p50_us: metrics.split_preparation_p50_us,
                    split_preparation_p95_us: metrics.split_preparation_p95_us,
                    split_pair_admission_drops: metrics.split_pair_admission_drops,
                    encoded_pair_callback_p50_us: metrics.encoded_pair_callback_p50_us,
                    encoded_pair_callback_p95_us: metrics.encoded_pair_callback_p95_us,
                    encoded_pair_timeouts: metrics.encoded_pair_timeouts,
                    encoded_pair_drops: metrics.encoded_pair_drops,
                    left_valid_encode_output_fps: metrics.left_valid_encode_output_fps,
                    right_valid_encode_output_fps: metrics.right_valid_encode_output_fps,
                    left_encoder_frame_drops: metrics.left_encoder_frame_drops,
                    right_encoder_frame_drops: metrics.right_encoder_frame_drops,
                    left_bitrate_bps: metrics.left_bitrate_bps,
                    right_bitrate_bps: metrics.right_bitrate_bps,
                    aggregate_bitrate_bps: metrics.aggregate_bitrate_bps,
                    left_receiver_loss: metrics.left_receiver_loss,
                    right_receiver_loss: metrics.right_receiver_loss,
                    left_rendered_fps: metrics.left_rendered_fps,
                    right_rendered_fps: metrics.right_rendered_fps,
                    joined_rendered_fps: metrics.joined_rendered_fps,
                    pair_ready_delta_p95_us: metrics.pair_ready_delta_p95_us,
                    pair_ready_delta_max_us: metrics.pair_ready_delta_max_us,
                    pair_sync_timeouts: metrics.pair_sync_timeouts,
                    unmatched_output_drops: metrics.unmatched_output_drops,
                    paired_recovery_requests: metrics.paired_recovery_requests,
                    paired_recovery_keyframes: metrics.paired_recovery_keyframes,
                    split_test_injected_drops: metrics.split_test_injected_drops,
                    split_flow_active_leases: metrics.split_flow_active_leases,
                    split_flow_capacity: metrics.split_flow_capacity,
                    split_pre_encode_admission_drops: metrics.split_pre_encode_admission_drops,
                    split_encoded_queue_depth: metrics.split_encoded_queue_depth,
                    split_encoded_queue_oldest_us: metrics.split_encoded_queue_oldest_us,
                    split_capture_queue_oldest_us: metrics.split_capture_queue_oldest_us,
                    split_recovery_boundary_discards: metrics.split_recovery_boundary_discards,
                    split_post_encode_delta_drops: metrics.split_post_encode_delta_drops,
                    split_wire_pairs_attempted: metrics.split_wire_pairs_attempted,
                    split_wire_pair_send_failures: metrics.split_wire_pair_send_failures,
                    split_keyframe_gap_recoveries: metrics.split_keyframe_gap_recoveries,
                    split_delta_gap_recoveries: metrics.split_delta_gap_recoveries,
                });
            }

            let expired = expired_ids
                .into_iter()
                .filter_map(|id| state.live.remove(&id))
                .collect::<Vec<_>>();
            (sessions, expired)
        };

        for session in expired {
            if !session.backend_released {
                if let Err(error) = self.backend.stop(session.handle) {
                    eprintln!(
                        "failed to release terminal session {}: {error}",
                        session.handle
                    );
                }
            }
            cleanup_media_transport(&session.media_transport, session.viewer_port);
        }

        StatusView { sessions }
    }

    pub fn input_permission(&self) -> Result<bool, String> {
        self.backend.input_permission()
    }

    pub fn platform(&self) -> &'static str {
        self.backend.platform()
    }

    pub fn request_input_permission(&self) -> Result<bool, String> {
        self.backend.request_input_permission()
    }

    pub fn screen_permission(&self) -> Result<bool, String> {
        self.backend.screen_permission()
    }

    pub fn set_session_input(&self, session_id: u32, enabled: bool) -> Result<(), String> {
        let handle = {
            let state = self.sessions.lock().unwrap();
            state
                .live
                .get(&session_id)
                .filter(|session| !session.backend_released)
                .map(|session| session.handle)
                .ok_or_else(|| format!("no such session {session_id}"))?
        };
        self.backend.set_input_enabled(handle, enabled)?;
        let mut state = self.sessions.lock().unwrap();
        let session = state
            .live
            .get_mut(&session_id)
            .ok_or_else(|| format!("session {session_id} ended while changing input"))?;
        session.input_enabled = enabled;
        Ok(())
    }

    pub fn set_session_quality(&self, session_id: u32, quality: Option<f32>) -> Result<(), String> {
        if let Some(value) = quality {
            if !(0.25..=0.5).contains(&value) {
                return Err("quality override must be between 0.25 and 0.50".into());
            }
        }
        let handle = {
            let state = self.sessions.lock().unwrap();
            state
                .live
                .get(&session_id)
                .filter(|session| !session.backend_released)
                .map(|session| session.handle)
                .ok_or_else(|| format!("no such session {session_id}"))?
        };
        self.backend.set_quality_override(handle, quality)
    }

    /// Operator-forced termination: stop the capture session and tell the
    /// still-live viewer (LCT1 code 2) so it closes its window and shows why
    /// instead of waiting for a media timeout or auto-restarting.
    pub fn force_stop_session(&self, session_id: u32) -> Result<(), String> {
        let handle = {
            let state = self.sessions.lock().unwrap();
            state
                .live
                .get(&session_id)
                .filter(|session| !session.backend_released)
                .map(|session| session.handle)
                .ok_or_else(|| format!("no such session {session_id}"))?
        };
        self.backend.stop_with_reason(handle, 2)?;

        let mut state = self.sessions.lock().unwrap();
        let session = state
            .live
            .get_mut(&session_id)
            .ok_or_else(|| format!("session {session_id} ended while stopping"))?;
        session.input_enabled = false;
        session.terminal_since = Some(Instant::now());
        session.terminal_error = Some("host operator stopped the stream".into());
        session.backend_released = true;
        cleanup_media_transport(&session.media_transport, session.viewer_port);
        Ok(())
    }

    /// A viewer restart can leave the old capture handle alive until its TCP
    /// write notices the closed socket. Do not allow two hosts to push into
    /// the same viewer endpoint: their H.264 AU ids would interleave and the
    /// receiver would correctly enter keyframe recovery over and over.
    fn stop_sessions_for_viewer(&self, viewer_addr: &str) {
        let stale = {
            let mut state = self.sessions.lock().unwrap();
            let ids: Vec<u32> = state
                .live
                .iter()
                .filter_map(|(id, session)| (session.viewer_addr == viewer_addr).then_some(*id))
                .collect();
            ids.into_iter()
                .filter_map(|id| state.live.remove(&id))
                .collect::<Vec<_>>()
        };

        for session in stale {
            if !session.backend_released {
                if let Err(error) = self.backend.stop(session.handle) {
                    eprintln!(
                        "failed to stop stale viewer session {}: {error}",
                        session.handle
                    );
                }
            }
            cleanup_media_transport(&session.media_transport, session.viewer_port);
        }
    }

    pub(crate) async fn dispatch(
        &self,
        command: &str,
        args: serde_json::Value,
        viewer_ip: &str,
    ) -> serde_json::Value {
        match command {
            "requestUsb" => match crate::aoap_control::ensure_usb_accessory().await {
                Ok(()) => ok(json!({ "attached": true })),
                Err(error) => err(&error),
            },
            "beginPairing" => {
                // Local operator/diagnostic entry point. This exposes the same
                // short-lived pairing offer as the Tauri pairing window, but
                // it is never reachable from a LAN or tailnet peer.
                if !is_loopback_peer(viewer_ip) {
                    return err("unauthorized");
                }
                let Some(host_ip) = crate::local_lan_ip() else {
                    return err("no LAN interface found");
                };
                let port = self.control_port.load(Ordering::Acquire);
                ok(self.pairing.begin_pairing(&host_ip, port))
            }
            "pair" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct PairArgs {
                    #[serde(default)]
                    offer_id: Option<String>,
                    #[serde(default)]
                    secret: Option<String>,
                    code: String,
                    device_id: String,
                    #[serde(default)]
                    device_name: String,
                }
                let input: PairArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(_) => return err("pairing failed"),
                };
                let res = match (input.offer_id.as_deref(), input.secret.as_deref()) {
                    (Some(offer_id), Some(secret)) => self.pairing.pair(
                        offer_id,
                        secret,
                        &input.code,
                        &input.device_id,
                        &input.device_name,
                    ),
                    (None, None) => {
                        self.pairing
                            .pair_by_code(&input.code, &input.device_id, &input.device_name)
                    }
                    _ => Err(crate::pairing::PairingServerError::PairingFailed),
                };
                match res {
                    Ok(token) => ok(json!({ "token": token })),
                    // 승인 기반 페어링: 시크릿은 맞지만 Mac 사용자의 허용이
                    // 아직 없다. 뷰어는 이 상태를 보고 폴링을 계속한다.
                    Err(crate::pairing::PairingServerError::Pending) => {
                        ok(json!({ "status": "pending" }))
                    }
                    Err(crate::pairing::PairingServerError::Rejected) => err("pairing rejected"),
                    Err(_) => err("pairing failed"),
                }
            }
            "getCatalog" => {
                let displays = match self.backend.list_displays() {
                    Ok(displays) => displays,
                    Err(e) => return err(&e),
                };
                let encoder_experiments = match self.backend.encoder_experiments() {
                    Ok(experiments) => advertised_encoder_experiments(experiments),
                    Err(e) => return err(&e),
                };
                ok(CatalogView {
                    platform: self.backend.platform().into(),
                    capture_backends: self.backend.capture_backends(),
                    media_host: crate::local_lan_ip().filter(|address| {
                        address
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok_and(|address| address.is_private())
                    }),
                    displays,
                    encoder_experiments,
                    reconfigure_encoder_experiment: Some(true),
                    udp_stability_capabilities: Some(host_udp_stability_capabilities()),
                })
            }
            "startStream" => {
                let input: StartStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                if let Err(e) = validate_stream_shape(input.width, input.height, input.fps) {
                    return err(&e);
                }
                if !self
                    .backend
                    .supports_capture_backend(&input.capture_backend)
                {
                    return err("unsupported capture backend");
                }
                let advertised = match self.backend.encoder_experiments() {
                    Ok(experiments) => advertised_encoder_experiments(experiments),
                    Err(error) => return err(&error),
                };
                let split_diagnostic_enabled = std::env::var("LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC")
                    .is_ok_and(|value| value == "1");
                if !encoder_experiment_is_startable(
                    &advertised,
                    input.encoder_experiment,
                    split_diagnostic_enabled,
                ) {
                    return err(&format!(
                        "unsupported encoder experiment: {}",
                        input.encoder_experiment.as_str()
                    ));
                }
                let name = self
                    .backend
                    .list_displays()
                    .ok()
                    .and_then(|d| d.get(input.source_index as usize).cloned())
                    .map(|d| d.name)
                    .unwrap_or_else(|| format!("display {}", input.source_index));
                let requested_transport = normalize_media_transport(&input.media_transport)
                    .ok_or_else(|| {
                        format!("unsupported media transport: {}", input.media_transport)
                    });
                let requested_transport = match requested_transport {
                    Ok(value) => value,
                    Err(error) => return err(&error),
                };
                let content_mode = match normalize_content_mode(&input.content_mode) {
                    Some(value) => value,
                    None => {
                        return err(&format!("unsupported content mode: {}", input.content_mode))
                    }
                };
                if input.udp_stability.is_some() && !matches!(requested_transport, "udp" | "auto") {
                    return err("UDP 안정성 설정은 UDP 또는 자동 전송에서만 사용할 수 있습니다");
                }
                let udp_stability = match resolve_udp_stability(
                    input.udp_stability.as_ref(),
                    &host_udp_stability_capabilities(),
                ) {
                    Ok(applied) => applied,
                    Err(error) => return err(&error),
                };
                if let Err(error) = validate_split_start(&input, requested_transport) {
                    return err(&error);
                }

                // Do not claim a normal Android USB device merely because a
                // cable was attached. AOAP negotiation is an explicit stream
                // request; `auto` may fall back to Wi-Fi, while an explicit
                // USB request reports the negotiation failure to the viewer.
                if input.encoder_experiment != EncoderExperiment::SplitVertical
                    && matches!(requested_transport, "usb" | "auto")
                {
                    if let Err(error) = crate::aoap_control::ensure_usb_accessory().await {
                        if requested_transport == "usb" {
                            return err(&error);
                        }
                        eprintln!("AOAP unavailable; continuing with transport fallback: {error}");
                    }
                }

                // A non-bypassable VPN can route a local control connection
                // through a LAN subnet router, so its TCP peer is not always
                // the viewer's physical Wi-Fi address. Consider claimed
                // addresses only when they are private and either on the
                // peer's /24 or the authenticated control peer is a tailnet
                // CGNAT address. USB is deliberately different: adb forward
                // terminates on the Host, so the only valid media peer is the
                // Host loopback address.
                let usb_control = viewer_ip == "usb";
                let mut wifi_candidates = input
                    .viewer_ips
                    .iter()
                    .take(4)
                    .filter(|candidate| same_private_lan_candidate(candidate, viewer_ip))
                    .cloned()
                    .collect::<Vec<_>>();
                if !usb_control
                    && !wifi_candidates
                        .iter()
                        .any(|candidate| candidate == viewer_ip)
                {
                    wifi_candidates.push(viewer_ip.to_owned());
                }
                let attempts = if input.encoder_experiment == EncoderExperiment::SplitVertical {
                    build_attempts("udp", &wifi_candidates)
                } else if usb_control {
                    build_attempts("usb", &[])
                } else {
                    build_attempts(requested_transport, &wifi_candidates)
                };
                let mut last_error = None;
                let mut started = None;
                for (candidate, transport) in attempts {
                    let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                    self.stop_sessions_for_viewer(&viewer_addr);
                    if transport == "adbTcp" {
                        if let Err(error) = adb_forward(input.viewer_port) {
                            last_error = Some(error);
                            continue;
                        }
                    }
                    if transport == "usb" {
                        if let Err(error) = crate::aoap_proxy::start_media_proxy(input.viewer_port)
                        {
                            last_error = Some(error);
                            continue;
                        }
                    }
                    match self.backend.start(
                        input.source_index,
                        &candidate,
                        input.viewer_port,
                        input.width,
                        input.height,
                        input.fps,
                        &input.capture_backend,
                        transport,
                        content_mode,
                        input.encoder_experiment,
                        &udp_stability,
                    ) {
                        Ok(handle) => match self.wait_for_first_frame(handle).await {
                            Ok(()) => {
                                started = Some((handle, candidate, transport));
                                break;
                            }
                            Err(error) => {
                                let _ = self.backend.stop(handle);
                                cleanup_media_transport(transport, input.viewer_port);
                                last_error = Some(format!(
                                    "{candidate} ({transport}) startup failed: {error}"
                                ));
                            }
                        },
                        Err(e) => {
                            cleanup_media_transport(transport, input.viewer_port);
                            last_error = Some(format!("{candidate} ({transport}): {e}"));
                        }
                    }
                }
                match started {
                    Some((handle, candidate, transport)) => {
                        let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                        // Remote input turns itself on with the session when
                        // the OS input permission is already granted, so the
                        // viewer never has to ask; without it the host banner
                        // and manual toggle remain the path in. A failure to
                        // apply degrades to disabled rather than failing the
                        // stream.
                        let input_enabled = self.backend.input_permission().unwrap_or(false)
                            && self.backend.set_input_enabled(handle, true).is_ok();
                        let session_id = {
                            let mut st = self.sessions.lock().unwrap();
                            let id = st.next;
                            st.next += 1;
                            st.live.insert(
                                id,
                                Session {
                                    handle,
                                    source_index: input.source_index,
                                    source_name: name,
                                    width: input.width,
                                    height: input.height,
                                    fps_target: input.fps,
                                    quality_state: "native".into(),
                                    capture_backend: input.capture_backend.clone(),
                                    content_mode: content_mode.into(),
                                    encoder_experiment: input.encoder_experiment,
                                    viewer_addr,
                                    viewer_port: input.viewer_port,
                                    media_transport: transport.into(),
                                    udp_stability: (transport == "udp")
                                        .then(|| udp_stability.clone()),
                                    input_enabled,
                                    input_rate_hz: input.fps.saturating_mul(2).clamp(30, 240),
                                    terminal_since: None,
                                    terminal_error: None,
                                    backend_released: false,
                                },
                            );
                            id
                        };
                        ok(StartStreamOutput {
                            session: session_id,
                            width: input.width,
                            height: input.height,
                            fps: input.fps,
                            quality_state: "native".into(),
                            udp_stability: (transport == "udp").then_some(udp_stability),
                        })
                    }
                    None => err(&format!(
                        "all viewer addresses failed: {}",
                        last_error.unwrap_or_else(|| "no viewer addresses".into())
                    )),
                }
            }
            "reconfigureStream" => {
                let input: ReconfigureStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                match self.reconfigure_stream(input).await {
                    Ok(output) => ok(output),
                    Err(error) => err(&error),
                }
            }
            "stopStream" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct StopArgs {
                    session: u32,
                    /// LCT1 wire code the host forwards to the viewer:
                    /// 2 = operator-forced, 3 = ordinary stop. Optional for
                    /// backward compatibility with older viewers.
                    #[serde(default)]
                    reason: Option<u8>,
                }
                let input: StopArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                let removed = {
                    let mut st = self.sessions.lock().unwrap();
                    st.live.remove(&input.session)
                };
                match removed {
                    Some(s) => {
                        let result = if s.backend_released {
                            Ok(())
                        } else {
                            match input.reason {
                                Some(code) => self.backend.stop_with_reason(s.handle, code),
                                None => self.backend.stop(s.handle),
                            }
                        };
                        match result {
                            Ok(()) => {
                                cleanup_media_transport(&s.media_transport, s.viewer_port);
                                ok(json!({}))
                            }
                            Err(e) => err(&e),
                        }
                    }
                    None => err(&format!("no such session {}", input.session)),
                }
            }
            "getStatus" => ok(self.snapshot()),
            _ => {
                // delegate stateless commands to the real rustra package (H02 path)
                match control_contract::host::host_package().invoke_json(command, args) {
                    Ok(v) => ok(v),
                    Err(e) => err(&e.to_string()),
                }
            }
        }
    }

    pub(crate) fn authorize_token(&self, token: &str) -> bool {
        self.pairing.authorize(token)
    }
}

async fn handle_conn(sock: TcpStream, server: &ControlServer, peer: &str) {
    let (rd, mut wr) = sock.into_split();
    let mut lines = BufReader::new(rd).lines();
    loop {
        // Reap half-open connections: a peer that vanished without FIN never
        // unblocks this read otherwise, and its task leaks for the process
        // lifetime. The viewer's 2s status poll makes 15s a generous budget.
        let line = match tokio::time::timeout(CONTROL_IDLE_TIMEOUT, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break,
            Ok(Err(_)) | Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<(String, serde_json::Value, Option<String>), String> =
            serde_json::from_str(&line)
                .map(|v: Envelope| (v.command, v.args, v.token))
                .map_err(|e| format!("bad request: {e}"));
        let resp = match parsed {
            Ok((cmd, args, token)) => {
                // Auth gate: `pair` is the only command reachable without a
                // token over the network. The loopback-only beginPairing
                // command is an operator diagnostic equivalent to opening the
                // local Tauri pairing panel. Anything else requires a token
                // issued by a completed pairing; on failure the connection is
                // closed, not just the request rejected (design §2).
                let local_pairing = cmd == "beginPairing" && is_loopback_peer(peer);
                if cmd != "pair"
                    && !local_pairing
                    && !server.pairing.authorize(token.as_deref().unwrap_or(""))
                {
                    write_line(
                        &mut wr,
                        &serde_json::to_string(&err("unauthorized")).unwrap_or_default(),
                    )
                    .await;
                    break;
                }
                let out = server.dispatch(&cmd, args, peer).await;
                serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
            }
            Err(e) => format!("{{\"ok\":false,\"error\":{}}}", json!(e)),
        };
        if wr.write_all(resp.as_bytes()).await.is_err() || wr.write_all(b"\n").await.is_err() {
            break;
        }
    }
}

fn is_loopback_peer(peer: &str) -> bool {
    peer.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn same_private_lan_candidate(candidate: &str, peer: &str) -> bool {
    let Ok(candidate) = candidate.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let Ok(peer) = peer.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let candidate_octets = candidate.octets();
    let peer_octets = peer.octets();
    let same_private_subnet = peer.is_private() && candidate_octets[..3] == peer_octets[..3];
    let tailnet_peer = peer_octets[0] == 100 && (peer_octets[1] & 0b1100_0000) == 64;
    candidate.is_private() && (same_private_subnet || tailnet_peer)
}

async fn write_line(wr: &mut tokio::net::tcp::OwnedWriteHalf, body: &str) {
    let _ = wr.write_all(body.as_bytes()).await;
    let _ = wr.write_all(b"\n").await;
}

#[derive(serde::Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    token: Option<String>,
}

fn ok<T: serde::Serialize>(result: T) -> serde_json::Value {
    json!({ "ok": true, "result": result })
}

fn err(error: &str) -> serde_json::Value {
    json!({ "ok": false, "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_vertical_requires_exact_4k60_udp_and_safe_base_port() {
        let parse = |width, height, fps, media_transport: &str, viewer_port| {
            serde_json::from_value::<StartStreamInput>(serde_json::json!({
                "sourceIndex": 0,
                "viewerPort": viewer_port,
                "width": width,
                "height": height,
                "fps": fps,
                "mediaTransport": media_transport,
                "encoderExperiment": "splitVertical"
            }))
            .unwrap()
        };
        assert!(validate_split_start(&parse(3840, 2160, 60, "udp", 5002), "udp").is_ok());
        assert!(validate_split_start(&parse(3840, 2160, 60, "tcp", 5002), "tcp").is_err());
        assert!(validate_split_start(&parse(3840, 2160, 60, "udp", 65535), "udp").is_err());
        assert!(validate_split_start(&parse(2560, 1440, 60, "udp", 5002), "udp").is_err());
    }

    #[test]
    fn product_normalization_keeps_verified_split_and_diagnostic_is_fallback() {
        let advertised = advertised_encoder_experiments(vec![
            canonical_encoder_experiment(EncoderExperiment::Auto),
            canonical_encoder_experiment(EncoderExperiment::SplitVertical),
            canonical_encoder_experiment(EncoderExperiment::SplitHorizontal),
        ]);

        assert_eq!(
            advertised.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![EncoderExperiment::Auto, EncoderExperiment::SplitVertical]
        );
        assert!(encoder_experiment_is_startable(
            &advertised,
            EncoderExperiment::SplitVertical,
            false
        ));
        assert!(encoder_experiment_is_startable(
            &advertised,
            EncoderExperiment::SplitVertical,
            true
        ));
        assert!(!encoder_experiment_is_startable(
            &[canonical_encoder_experiment(EncoderExperiment::Auto)],
            EncoderExperiment::SplitHorizontal,
            true
        ));
        assert!(encoder_experiment_is_startable(
            &[canonical_encoder_experiment(EncoderExperiment::Auto)],
            EncoderExperiment::SplitVertical,
            true
        ));
    }
    use crate::backend::{CaptureBackend, FakeBackend};
    use control_contract::host::DisplayInfo;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn backend() -> SharedBackend {
        Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        })
    }

    struct TerminalBackend {
        stopped: Arc<AtomicUsize>,
    }

    impl CaptureBackend for TerminalBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(Vec::new())
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
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
        ) -> Result<u32, String> {
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                frames: 10,
                bytes: 1_000,
                state: "error".into(),
                fps: 0,
                kbps: 0,
                fps_target: 60,
                encoder_experiment_diagnostics_available: true,
                encoder_experiment_requested: "auto".into(),
                encoder_experiment_applied: "rateControl".into(),
                encoder_experiment_fallback_reason: None,
                encoder_frame_drops: 0,
                encoder_frame_drop_fps: 0,
                valid_encode_output_fps: 0,
                encode_submit_call_p50_us: 0,
                encode_submit_call_p95_us: 0,
                encoder_callback_p50_us: 0,
                encoder_callback_p95_us: 0,
                packetization_in_flight: 0,
                base_frame_qp: None,
                base_frame_qp_changes: 0,
                capture_fps: 0,
                encode_submit_fps: 0,
                encode_output_fps: 0,
                rendered_fps: None,
                capture_callbacks: 0,
                encode_output_callbacks: 0,
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
                encode_output_interval_p95_us: 0,
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
                error: Some("viewer closed stream".into()),
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
    }

    async fn spawn_server() -> std::net::SocketAddr {
        spawn_server_with_pairing(test_pairing()).await
    }

    fn test_pairing() -> std::sync::Arc<crate::pairing::PairingServer> {
        std::sync::Arc::new(crate::pairing::PairingServer::new(
            "leftcar-host".into(),
            None,
        ))
    }

    async fn spawn_server_with_pairing(
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::sync::Arc::new(ControlServer::new(backend(), pairing));
        server.set_control_port(addr.port());
        tokio::spawn(async move { server.run(listener).await });
        addr
    }

    async fn request(
        sock: &mut tokio::net::TcpStream,
        cmd: &str,
        args: &str,
        token: &str,
    ) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(
            format!("{{\"command\":\"{cmd}\",\"args\":{args},\"token\":\"{token}\"}}\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            sock.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        String::from_utf8(buf).unwrap()
    }

    /// Pair via the real pairing flow and return the issued token.
    async fn pair_token(
        sock: &mut tokio::net::TcpStream,
        pairing: &crate::pairing::PairingServer,
    ) -> String {
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": view.code,
            "deviceId": "test-viewer",
            "deviceName": "Test Viewer",
        });
        let line = request(sock, "pair", &args.to_string(), "").await;
        let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        resp["result"]["token"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn catalog_start_status_stop_roundtrip() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "getCatalog", "{}", &token).await;
        assert!(line.contains("\"displays\""), "{line}");
        assert!(line.contains("\"platform\":\"test\""), "{line}");
        assert!(line.contains("\"captureBackends\""), "{line}");

        let line = request(
            &mut sock,
            "startStream",
            r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");
        assert!(line.contains("\"width\":1920"), "{line}");
        assert!(line.contains("\"height\":1080"), "{line}");
        assert!(line.contains("\"qualityState\":\"native\""), "{line}");

        let line = request(
            &mut sock,
            "reconfigureStream",
            r#"{"session":1,"width":2560,"height":1440,"fps":90,"qualityState":"fallback"}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");
        assert!(line.contains("\"width\":2560"), "{line}");
        assert!(line.contains("\"height\":1440"), "{line}");
        assert!(line.contains("\"qualityState\":\"fallback\""), "{line}");

        let line = request(
            &mut sock,
            "reconfigureStream",
            r#"{"session":1,"width":0,"height":1440,"fps":90,"qualityState":"fallback"}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"ok\":false"), "{line}");

        let line = request(&mut sock, "getStatus", "{}", &token).await;
        assert!(line.contains("\"state\":\"running\""), "{line}");
        assert!(line.contains("\"width\":2560"), "{line}");
        assert!(line.contains("\"qualityState\":\"fallback\""), "{line}");
        assert!(line.contains("\"inputEnabled\":true"), "{line}");
        assert!(line.contains("\"bitrateFloorCollapseCount\":7"), "{line}");
        assert!(line.contains("\"inputRateHz\":180"), "{line}");

        let line = request(&mut sock, "stopStream", r#"{"session":1}"#, &token).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    #[tokio::test]
    async fn add_numbers_delegates_to_rustra() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;
        let line = request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#, &token).await;
        assert!(line.contains("\"value\":42"), "{line}");
    }

    #[tokio::test]
    async fn unknown_command_and_restart_session_ids() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "nope", "{}", &token).await;
        assert!(line.contains("\"ok\":false"), "{line}");

        for i in 1..=2 {
            let line = request(
                &mut sock,
                "startStream",
                r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
                &token,
            )
            .await;
            assert!(line.contains(&format!("\"session\":{i}")), "{line}");
            let line = request(
                &mut sock,
                "stopStream",
                &format!("{{\"session\":{i}}}"),
                &token,
            )
            .await;
            assert!(line.contains("\"ok\":true"), "{line}");
        }
    }

    #[tokio::test]
    async fn unauthenticated_getcatalog_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
        assert!(line.contains("\"ok\":false"), "{line}");

        // connection is closed: the next request hits EOF or reset
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _ = sock
            .write_all(b"{\"command\":\"getCatalog\",\"args\":{}}\n")
            .await;
        let mut buf = [0u8; 16];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        assert_eq!(n, 0, "connection must be closed after unauthorized");
    }

    #[tokio::test]
    async fn loopback_operator_can_begin_pairing_without_a_token() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "beginPairing", "{}", "").await;
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], true, "{line}");
        assert_eq!(response["result"]["expires_in_secs"], 120, "{line}");
        assert_eq!(response["result"]["code"].as_str().unwrap().len(), 6);
        let payload: serde_json::Value =
            serde_json::from_str(response["result"]["qr_payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["v"], 1);
        assert_eq!(payload["p"], addr.port());
    }

    #[tokio::test]
    async fn six_digit_code_alone_pairs_against_the_live_host_offer() {
        let pairing = test_pairing();
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let args = json!({
            "code": view.code,
            "deviceId": "code-only-viewer",
            "deviceName": "Code Only Viewer",
        });

        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        assert!(line.contains("\"token\":"), "{line}");
        assert_eq!(pairing.list_devices().len(), 1);
    }

    #[tokio::test]
    async fn approval_pairing_returns_pending_then_token_after_approval() {
        let pairing = test_pairing();
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value =
            serde_json::from_str(&view.qr_payload).unwrap();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": "",
            "deviceId": "approval-viewer",
            "deviceName": "Approval Viewer",
        });

        // Mac 승인 전: 오류가 아니라 pending 상태로 답한다.
        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["result"]["status"], "pending", "{line}");
        assert_eq!(pairing.list_devices().len(), 0);

        // Mac 사용자가 [허용]을 누른 뒤 같은 요청은 토큰으로 답한다.
        pairing
            .approve_pending(payload["id"].as_str().unwrap())
            .unwrap();
        let line = request(&mut sock, "pair", &args.to_string(), "").await;
        assert!(line.contains("\"ok\":true"), "{line}");
        assert!(line.contains("\"token\":"), "{line}");
        assert_eq!(pairing.list_devices().len(), 1);
    }

    #[test]
    fn operator_pairing_is_strictly_loopback_only() {
        assert!(is_loopback_peer("127.0.0.1"));
        assert!(is_loopback_peer("::1"));
        assert!(!is_loopback_peer("192.168.0.18"));
        assert!(!is_loopback_peer("100.77.109.50"));
        assert!(!is_loopback_peer("localhost"));
    }

    #[test]
    fn media_candidate_must_be_private_and_on_the_control_peers_lan() {
        assert!(same_private_lan_candidate("192.168.0.18", "192.168.0.170"));
        assert!(same_private_lan_candidate("192.168.0.18", "100.80.133.120"));
        assert!(!same_private_lan_candidate("192.168.1.18", "192.168.0.170"));
        assert!(!same_private_lan_candidate("1.2.3.4", "192.168.0.170"));
        assert!(!same_private_lan_candidate("192.168.0.18", "100.128.0.1"));
    }

    #[test]
    fn normalize_media_transport_accepts_usb_without_reinterpreting_adb() {
        assert_eq!(normalize_media_transport("usb"), Some("usb"));
        assert_eq!(normalize_media_transport("AOAP"), Some("usb"));
        assert_eq!(normalize_media_transport("adbTcp"), Some("adbTcp"));
    }

    #[test]
    fn advertised_encoder_experiments_keeps_vertical_split_and_hides_horizontal_split() {
        let normalized = advertised_encoder_experiments(vec![
            EncoderExperimentInfo {
                id: EncoderExperiment::Auto,
                label: "wrong auto".into(),
                hint: "wrong auto hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::SplitHorizontal,
                label: "reserved".into(),
                hint: "reserved".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::AdaptiveQp,
                label: "wrong qp".into(),
                hint: "wrong qp hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::Auto,
                label: "duplicate auto".into(),
                hint: "duplicate auto hint".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::SplitVertical,
                label: "reserved".into(),
                hint: "reserved".into(),
                requires_reconnect: false,
            },
            EncoderExperimentInfo {
                id: EncoderExperiment::AdaptiveQp,
                label: "duplicate qp".into(),
                hint: "duplicate qp hint".into(),
                requires_reconnect: false,
            },
        ]);

        assert_eq!(
            normalized,
            vec![
                EncoderExperimentInfo {
                    id: EncoderExperiment::Auto,
                    label: "자동".into(),
                    hint: "호스트가 사용 가능한 인코더 경로를 선택합니다.".into(),
                    requires_reconnect: true,
                },
                EncoderExperimentInfo {
                    id: EncoderExperiment::AdaptiveQp,
                    label: "적응형 QP".into(),
                    hint: "화면 변화와 인코더 압력에 따라 Base QP를 조절합니다.".into(),
                    requires_reconnect: true,
                },
                EncoderExperimentInfo {
                    id: EncoderExperiment::SplitVertical,
                    label: "4K 듀얼 인코더".into(),
                    hint: "4K 화면을 좌우 두 하드웨어 인코더와 UDP 포트로 전송합니다.".into(),
                    requires_reconnect: true,
                },
            ]
        );
    }

    #[test]
    fn advertised_encoder_experiments_keeps_empty_input_empty() {
        assert!(advertised_encoder_experiments(Vec::new()).is_empty());
    }

    #[test]
    fn auto_attempts_usb_then_udp_then_tcp() {
        let attempts = build_attempts("auto", &["192.168.1.50".into()]);
        assert_eq!(
            attempts,
            vec![
                ("127.0.0.1".into(), "usb"),
                ("192.168.1.50".into(), "udp"),
                ("192.168.1.50".into(), "tcp"),
                ("127.0.0.1".into(), "adbTcp"),
            ]
        );
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "deadbeef").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
    }

    #[test]
    fn terminal_sessions_are_retained_briefly_then_released() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
        );
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
            },
        );

        let first = server.snapshot();
        assert_eq!(first.sessions.len(), 1);
        assert_eq!(
            first.sessions[0].error.as_deref(),
            Some("viewer closed stream")
        );
        assert_eq!(first.sessions[0].state, "stopped");

        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        let second = server.snapshot();
        assert!(second.sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn forced_stop_is_retained_as_a_tombstone_without_double_stopping() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
        );
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                input_enabled: true,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
            },
        );

        server.force_stop_session(1).unwrap();
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
        let retained = server.snapshot();
        assert_eq!(retained.sessions.len(), 1);
        assert_eq!(retained.sessions[0].state, "stopped");
        assert!(!retained.sessions[0].input_enabled);
        assert_eq!(
            retained.sessions[0].error.as_deref(),
            Some("host operator stopped the stream")
        );

        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        assert!(server.snapshot().sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn set_session_input_toggles_a_live_session() {
        let server = ControlServer::new(backend(), test_pairing());
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                width: 1_920,
                height: 1_080,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "interactive".into(),
                encoder_experiment: EncoderExperiment::Auto,
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
                udp_stability: None,
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
            },
        );

        assert!(server.input_permission().unwrap());
        assert!(!server.snapshot().sessions[0].input_enabled);
        server.set_session_input(1, true).unwrap();
        let session = server.snapshot().sessions.remove(0);
        assert!(session.input_enabled);
        assert_eq!(session.input_rate_hz, 120);
    }

    #[test]
    fn screen_permission_passes_the_backend_answer_through() {
        // The fake has no TCC gate, so the trait default reports granted.
        let server = ControlServer::new(backend(), test_pairing());
        assert!(server.screen_permission().unwrap());

        struct ScreenDeniedBackend;
        impl CaptureBackend for ScreenDeniedBackend {
            fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
                Ok(Vec::new())
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
                _encoder_experiment: EncoderExperiment,
                _udp_stability: &AppliedUdpStability,
            ) -> Result<u32, String> {
                Err("not under test".into())
            }

            fn stop(&self, _handle: u32) -> Result<(), String> {
                Ok(())
            }

            fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
                Err("not under test".into())
            }

            fn screen_permission(&self) -> Result<bool, String> {
                Ok(false)
            }
        }

        let server = ControlServer::new(Arc::new(ScreenDeniedBackend), test_pairing());
        assert!(!server.screen_permission().unwrap());
    }

    fn input_test_backend(permission: bool) -> Arc<FakeBackend> {
        Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: permission,
            input_calls: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn start_stream_auto_enables_input_when_permission_is_granted() {
        let fake = input_test_backend(true);
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        let session = state.live.values().next().unwrap();
        assert!(session.input_enabled, "{resp}");
        assert_eq!(*fake.input_calls.lock().unwrap(), vec![(7, true)], "{resp}");
    }

    #[tokio::test]
    async fn start_stream_leaves_input_off_without_permission() {
        let fake = input_test_backend(false);
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));

        let resp = server
            .dispatch(
                "startStream",
                serde_json::json!({
                    "sourceIndex": 0,
                    "viewerPort": 5001,
                    "width": 1920,
                    "height": 1080,
                    "fps": 60,
                    "mediaTransport": "udp"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        let state = server.sessions.lock().unwrap();
        let session = state.live.values().next().unwrap();
        assert!(!session.input_enabled, "{resp}");
        assert!(fake.input_calls.lock().unwrap().is_empty(), "{resp}");
    }

    #[tokio::test]
    async fn reconfigure_stream_carries_input_enablement_to_replacement() {
        let fake = input_test_backend(true);
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);
        server.set_session_input(1, true).unwrap();
        assert_eq!(*fake.input_calls.lock().unwrap(), vec![(7, true)]);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");

        // The replacement backend handle must re-receive the enablement the
        // viewer already had; a resolution switch cannot silently strip
        // control.
        assert_eq!(
            *fake.input_calls.lock().unwrap(),
            vec![(7, true), (7, true)],
            "{resp}"
        );
        let state = server.sessions.lock().unwrap();
        assert!(state.live.get(&1).unwrap().input_enabled, "{resp}");
    }

    /// Seed one live UDP session directly (the dispatcher's split start
    /// validation requires the diagnostic flag; these tests target the
    /// reconfigure path with a session already established).
    fn seed_live_session(
        server: &ControlServer,
        encoder_experiment: EncoderExperiment,
        width: u32,
        height: u32,
    ) {
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                width,
                height,
                fps_target: 60,
                quality_state: "native".into(),
                capture_backend: "screenCaptureKit".into(),
                content_mode: "video".into(),
                encoder_experiment,
                viewer_addr: "192.168.0.9:5002".into(),
                viewer_port: 5002,
                media_transport: "udp".into(),
                udp_stability: None,
                input_enabled: false,
                input_rate_hz: 180,
                terminal_since: None,
                terminal_error: None,
                backend_released: false,
            },
        );
    }

    /// Backend whose capture stats report a positive split capture queue age.
    struct CaptureQueueAgeBackend;

    impl CaptureBackend for CaptureQueueAgeBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(Vec::new())
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
            _encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
        ) -> Result<u32, String> {
            Ok(9)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                split_capture_queue_oldest_us: Some(45_600),
                ..StatsInfo::default()
            })
        }
    }

    #[tokio::test]
    async fn status_response_forwards_split_capture_queue_oldest_us() {
        // 네이티브 캡처 stats의 splitCaptureQueueOldestUs가 status 응답까지
        // 생존해야 한다 (파싱 → SessionView 전달 → Host JSON).
        let server = Arc::new(ControlServer::new(
            Arc::new(CaptureQueueAgeBackend),
            test_pairing(),
        ));
        seed_live_session(&server, EncoderExperiment::Auto, 3840, 2160);

        let resp = server
            .dispatch("getStatus", serde_json::json!({}), "192.168.0.9")
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            resp["result"]["sessions"][0]["splitCaptureQueueOldestUs"], 45_600,
            "{resp}"
        );
    }

    /// Backend that records every start attempt and can fail specific
    /// experiments, to exercise the reconfigure rollback path.
    struct ReplacingBackend {
        starts: Mutex<Vec<(EncoderExperiment, u32, u32)>>,
        fail_experiments: Vec<EncoderExperiment>,
        stops: AtomicUsize,
    }

    impl CaptureBackend for ReplacingBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }])
        }

        fn encoder_experiments(
            &self,
        ) -> Result<Vec<control_contract::host::EncoderExperimentInfo>, String> {
            let mut experiments = control_contract::host::phase_a_encoder_experiments();
            experiments.push(control_contract::host::EncoderExperimentInfo {
                id: EncoderExperiment::SplitVertical,
                label: String::new(),
                hint: String::new(),
                requires_reconnect: true,
            });
            Ok(experiments)
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            width: u32,
            height: u32,
            _fps: u32,
            _capture_backend: &str,
            _media_transport: &str,
            _content_mode: &str,
            encoder_experiment: EncoderExperiment,
            _udp_stability: &AppliedUdpStability,
        ) -> Result<u32, String> {
            self.starts
                .lock()
                .unwrap()
                .push((encoder_experiment, width, height));
            if self.fail_experiments.contains(&encoder_experiment) {
                return Err("simulated replacement failure".into());
            }
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                state: "running".into(),
                first_send_ms: 26,
                ..StatsInfo::default()
            })
        }
    }

    #[tokio::test]
    async fn reconfigure_stream_explicit_split_targets_exact_4k_and_reports_actual_mode() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: true,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        // 교체 백엔드는 요청된 split 모드로 시작한다.
        assert_eq!(
            *fake.encoder_experiment.lock().unwrap(),
            EncoderExperiment::SplitVertical,
            "{resp}"
        );
        // 라이브 기록과 수락 응답이 실제 모드로 일치한다.
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
        assert_eq!((session.width, session.height), (3840, 2160));
        drop(state);
        assert_eq!(
            resp["result"]["encoderExperiment"], "splitVertical",
            "{resp}"
        );
    }

    #[tokio::test]
    async fn reconfigure_stream_explicit_auto_leaves_split_for_single_path() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::SplitVertical),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 2560,
                    "height": 1440,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "auto"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(
            *fake.encoder_experiment.lock().unwrap(),
            EncoderExperiment::Auto,
            "{resp}"
        );
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        drop(state);
        assert_eq!(resp["result"]["encoderExperiment"], "auto", "{resp}");
    }

    #[tokio::test]
    async fn reconfigure_stream_rejects_unsupported_split_shape_before_stopping() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: true,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 2560,
                    "height": 1440,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("splitVertical"), "{resp}");
        // 검증은 이전 백엔드 stop 전에 수행된다: 라이브 스트림은 그대로다.
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_rejects_unsupported_requested_mode_before_stopping() {
        let fake = Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 3840,
                height: 2160,
            }],
            encoder_experiment: Mutex::new(EncoderExperiment::Auto),
            advertise_split_vertical: false,
            stops: AtomicUsize::new(0),
            input_permission: true,
            input_calls: Mutex::new(Vec::new()),
        });
        let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitHorizontal"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        let message = resp["error"].as_str().unwrap_or_default();
        assert!(message.contains("unsupported encoder experiment"), "{resp}");
        assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    #[tokio::test]
    async fn reconfigure_stream_failure_restores_previous_actual_mode() {
        let backend = Arc::new(ReplacingBackend {
            starts: Mutex::new(Vec::new()),
            fail_experiments: vec![EncoderExperiment::SplitVertical],
            stops: AtomicUsize::new(0),
        });
        let server = Arc::new(ControlServer::new(backend.clone(), test_pairing()));
        seed_live_session(&server, EncoderExperiment::Auto, 2560, 1440);

        let resp = server
            .dispatch(
                "reconfigureStream",
                serde_json::json!({
                    "session": 1,
                    "width": 3840,
                    "height": 2160,
                    "fps": 60,
                    "qualityState": "native",
                    "encoderExperiment": "splitVertical"
                }),
                "192.168.0.9",
            )
            .await;
        assert_eq!(resp["ok"], false, "{resp}");
        // 교체 실패 시 정확히 이전 실제 모드/형태로 복구된다.
        assert_eq!(
            *backend.starts.lock().unwrap(),
            vec![
                (EncoderExperiment::SplitVertical, 3840, 2160),
                (EncoderExperiment::Auto, 2560, 1440),
            ]
        );
        assert_eq!(backend.stops.load(Ordering::SeqCst), 1);
        let state = server.sessions.lock().unwrap();
        let session = state.live.get(&1).unwrap();
        assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
        assert_eq!((session.width, session.height), (2560, 1440));
        assert_eq!(session.handle, 7);
        assert!(!session.backend_released);
        assert!(session.terminal_error.is_none());
    }

    // split 재구성 회귀 테스트(구 viewer_display_matching 모듌에서 이전)
    #[tokio::test]
        async fn reconfigure_stream_demotes_split_replacement_for_non_4k_target() {
            // 4K split 세션을 1080p로 재구성하면 교체 캡처가 Auto 단일 인코더로
            // 시작해야 한다 — 뷰어는 이미 demote된 단일 prepare/rebind 경로로
            // 전환했으므로, 교체 세션이 splitVertical로 시작되면 (엔진 검증
            // 실패 또는 포트 기하 불일치로) 복구된 이전 세션은 뷰어 피드백을
            // 영원히 받지 못하고 feedback timeout으로 죽는다 (실기 2026-09-07
            // 4K split → 1080p 프리셋 실패 재현).
            let fake = Arc::new(FakeBackend {
                displays: vec![DisplayInfo {
                    index: 0,
                    name: "Main".into(),
                    width: 1920,
                    height: 1080,
                }],
                encoder_experiment: Mutex::new(EncoderExperiment::Auto),
                advertise_split_vertical: false,
                stops: AtomicUsize::new(0),
                input_permission: true,
                input_calls: Mutex::new(Vec::new()),
            });
            let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
            // dispatcher의 split 시작 검증은 진단 플래그를 요구하므로 라이브
            // split 세션을 직접 시드한다 — 이 테스트의 대상은 reconfigure의
            // 교체 세션 실험 선택이다.
            server.sessions.lock().unwrap().live.insert(
                1,
                Session {
                    handle: 7,
                    source_index: 0,
                    source_name: "Main".into(),
                    width: 3840,
                    height: 2160,
                    fps_target: 60,
                    quality_state: "native".into(),
                    capture_backend: "screenCaptureKit".into(),
                    content_mode: "video".into(),
                    encoder_experiment: EncoderExperiment::SplitVertical,
                    viewer_addr: "192.168.0.9:5002".into(),
                    viewer_port: 5002,
                    media_transport: "udp".into(),
                    udp_stability: None,
                    input_enabled: false,
                    input_rate_hz: 180,
                    terminal_since: None,
                    terminal_error: None,
                    backend_released: false,
                },
            );

            let resp = server
                .dispatch(
                    "reconfigureStream",
                    serde_json::json!({
                        "session": 1,
                        "width": 1920,
                        "height": 1080,
                        "fps": 60,
                        "qualityState": "native"
                    }),
                    "192.168.0.9",
                )
                .await;
            assert_eq!(resp["ok"], true, "{resp}");
            assert_eq!(resp["result"]["width"], 1920, "{resp}");
            // 구 요청(mode 생략)도 실제 수락 모드를 응답에 보고한다.
            assert_eq!(resp["result"]["encoderExperiment"], "auto", "{resp}");
            // 교체 캡처 백엔드는 demote된 Auto로 시작해야 한다.
            assert_eq!(
                *fake.encoder_experiment.lock().unwrap(),
                EncoderExperiment::Auto,
                "replacement capture must start with the demoted encoder experiment",
            );
            // 세션 기록도 교체 결과와 일치한다.
            let state = server.sessions.lock().unwrap();
            let session = state.live.get(&1).unwrap();
            assert_eq!(session.width, 1920);
            assert_eq!(session.height, 1080);
            assert_eq!(session.encoder_experiment, EncoderExperiment::Auto);
            assert_eq!(session.handle, 7);
        }

    #[tokio::test]
        async fn reconfigure_stream_omitted_mode_rejects_split_4k_90fps_before_stopping() {
            // mode 생략(구 뷰어) 요청이 정확한 4K를 유지하면 이전 split 실험이
            // 유지되지만, fps가 60이 아니면 split 제약 위반이다. 교체 실험을
            // 검증하기 전에는 이전 백엔드를 멈추지 않는다: 거부 시 라이브
            // 스트림과 기록은 그대로여야 한다.
            let fake = Arc::new(FakeBackend {
                displays: vec![DisplayInfo {
                    index: 0,
                    name: "Main".into(),
                    width: 3840,
                    height: 2160,
                }],
                encoder_experiment: Mutex::new(EncoderExperiment::Auto),
                advertise_split_vertical: false,
                stops: AtomicUsize::new(0),
                input_permission: true,
                input_calls: Mutex::new(Vec::new()),
            });
            let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
            seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

            let resp = server
                .dispatch(
                    "reconfigureStream",
                    serde_json::json!({
                        "session": 1,
                        "width": 3840,
                        "height": 2160,
                        "fps": 90,
                        "qualityState": "native"
                    }),
                    "192.168.0.9",
                )
                .await;
            assert_eq!(resp["ok"], false, "{resp}");
            let message = resp["error"].as_str().unwrap_or_default();
            assert!(message.contains("splitVertical"), "{resp}");
            assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
            let state = server.sessions.lock().unwrap();
            let session = state.live.get(&1).unwrap();
            assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
            assert_eq!((session.width, session.height), (3840, 2160));
            assert_eq!(session.fps_target, 60);
            assert_eq!(session.handle, 7);
            assert!(!session.backend_released);
            assert!(session.terminal_error.is_none());
        }

    #[tokio::test]
        async fn reconfigure_stream_omitted_mode_rejects_split_4k_30fps_before_stopping() {
            let fake = Arc::new(FakeBackend {
                displays: vec![DisplayInfo {
                    index: 0,
                    name: "Main".into(),
                    width: 3840,
                    height: 2160,
                }],
                encoder_experiment: Mutex::new(EncoderExperiment::Auto),
                advertise_split_vertical: false,
                stops: AtomicUsize::new(0),
                input_permission: true,
                input_calls: Mutex::new(Vec::new()),
            });
            let server = Arc::new(ControlServer::new(fake.clone(), test_pairing()));
            seed_live_session(&server, EncoderExperiment::SplitVertical, 3840, 2160);

            let resp = server
                .dispatch(
                    "reconfigureStream",
                    serde_json::json!({
                        "session": 1,
                        "width": 3840,
                        "height": 2160,
                        "fps": 30,
                        "qualityState": "native"
                    }),
                    "192.168.0.9",
                )
                .await;
            assert_eq!(resp["ok"], false, "{resp}");
            let message = resp["error"].as_str().unwrap_or_default();
            assert!(message.contains("splitVertical"), "{resp}");
            assert_eq!(fake.stops.load(Ordering::SeqCst), 0, "{resp}");
            let state = server.sessions.lock().unwrap();
            let session = state.live.get(&1).unwrap();
            assert_eq!(session.encoder_experiment, EncoderExperiment::SplitVertical);
            assert_eq!((session.width, session.height), (3840, 2160));
            assert_eq!(session.fps_target, 60);
            assert_eq!(session.handle, 7);
            assert!(!session.backend_released);
            assert!(session.terminal_error.is_none());
        }

}
