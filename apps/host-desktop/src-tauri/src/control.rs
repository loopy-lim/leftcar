//! Control server: viewers connect (pull) and exchange newline-delimited
//! JSON `{"command","args"}` / `{"ok":true,"result"}` over TCP (design §제어평면).
//!
//! `addNumbers` delegates to the real rustra host_package so the H02 proof
//! path stays intact; stateful v1 stream commands dispatch locally.

use crate::backend::SharedBackend;
use control_contract::host::{
    CatalogView, SessionView, StartStreamInput, StartStreamOutput, StatusView,
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

struct Session {
    handle: u32,
    source_index: u32,
    source_name: String,
    viewer_addr: String,
    viewer_port: u16,
    media_transport: String,
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
                    send_block_us: 0,
                    max_send_block_us: 0,
                    send_pace_us: 0,
                    max_send_pace_us: 0,
                    pending_frame: 0,
                    capture_backend: "unknown".into(),
                    media_transport: "unknown".into(),
                    first_capture_ms: 0,
                    first_encode_ms: 0,
                    first_send_ms: 0,
                    current_bitrate: 0,
                    capture_interval_p95_us: 0,
                    capture_to_encode_p95_us: 0,
                    capture_queue_wait_p95_us: 0,
                    encode_output_p95_us: 0,
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
                });

                if let Some(error) = &s.terminal_error {
                    metrics.state = "stopped".into();
                    metrics.fps = 0;
                    metrics.kbps = 0;
                    metrics.pending_frame = 0;
                    metrics.error = Some(error.clone());
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
                    state: metrics.state,
                    fps: metrics.fps,
                    kbps: metrics.kbps,
                    fps_target: metrics.fps_target,
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
                    send_block_us: metrics.send_block_us,
                    max_send_block_us: metrics.max_send_block_us,
                    send_pace_us: metrics.send_pace_us,
                    max_send_pace_us: metrics.max_send_pace_us,
                    pending_frame: metrics.pending_frame,
                    frames: metrics.frames,
                    bytes: metrics.bytes,
                    capture_backend: metrics.capture_backend,
                    media_transport: metrics.media_transport,
                    first_capture_ms: metrics.first_capture_ms,
                    first_encode_ms: metrics.first_encode_ms,
                    first_send_ms: metrics.first_send_ms,
                    current_bitrate: metrics.current_bitrate,
                    capture_interval_p95_us: metrics.capture_interval_p95_us,
                    capture_to_encode_p95_us: metrics.capture_to_encode_p95_us,
                    capture_queue_wait_p95_us: metrics.capture_queue_wait_p95_us,
                    encode_output_p95_us: metrics.encode_output_p95_us,
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
                    (None, None) => self
                        .pairing
                        .pair_by_code(&input.code, &input.device_id, &input.device_name),
                    _ => Err(crate::pairing::PairingServerError::PairingFailed),
                };
                match res {
                    Ok(token) => ok(json!({ "token": token })),
                    Err(_) => err("pairing failed"),
                }
            }
            "getCatalog" => match self.backend.list_displays() {
                Ok(displays) => ok(CatalogView {
                    platform: self.backend.platform().into(),
                    capture_backends: self.backend.capture_backends(),
                    media_host: crate::local_lan_ip().filter(|address| {
                        address
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok_and(|address| address.is_private())
                    }),
                    displays,
                }),
                Err(e) => err(&e),
            },
            "startStream" => {
                let input: StartStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                if input.width == 0
                    || input.height == 0
                    || input.width > 8192
                    || input.height > 8192
                    || input.fps == 0
                    || input.fps > 90
                {
                    return err("unsupported stream dimensions or fps");
                }
                if !self
                    .backend
                    .supports_capture_backend(&input.capture_backend)
                {
                    return err("unsupported capture backend");
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
                    None => return err(&format!("unsupported content mode: {}", input.content_mode)),
                };

                // Do not claim a normal Android USB device merely because a
                // cable was attached. AOAP negotiation is an explicit stream
                // request; `auto` may fall back to Wi-Fi, while an explicit
                // USB request reports the negotiation failure to the viewer.
                if matches!(requested_transport, "usb" | "auto") {
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
                let attempts = if usb_control {
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
                                    viewer_addr,
                                    viewer_port: input.viewer_port,
                                    media_transport: transport.into(),
                                    input_enabled: false,
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
                        })
                    }
                    None => err(&format!(
                        "all viewer addresses failed: {}",
                        last_error.unwrap_or_else(|| "no viewer addresses".into())
                    )),
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
                send_block_us: 0,
                max_send_block_us: 0,
                send_pace_us: 0,
                max_send_pace_us: 0,
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

        let line = request(&mut sock, "getStatus", "{}", &token).await;
        assert!(line.contains("\"state\":\"running\""), "{line}");
        assert!(line.contains("\"inputEnabled\":false"), "{line}");
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
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
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
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
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
    fn remote_input_is_host_opt_in_per_session() {
        let server = ControlServer::new(backend(), test_pairing());
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                viewer_addr: "192.168.0.2:5001".into(),
                viewer_port: 5001,
                media_transport: "udp".into(),
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
}
