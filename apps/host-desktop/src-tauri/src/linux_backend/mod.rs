//! Linux Host backend: wlr-screencopy capture + H.264 via hardware NVENC
//! (software openh264 fallback) + wlr virtual pointer/keyboard injection,
//! wired into the shared control and media planes. The session shape mirrors
//! `windows_backend` (two OS threads per session: input receiver,
//! capture→encode→send worker).

mod convert;
mod encoder;
mod input;
mod nvenc;
mod wayland;

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::CaptureBackend;
use crate::input_gate::InputGate;
use crate::media_feedback::{latency_probe_response, ReceiverFeedback};
use crate::media_pacing::{DrainIo, MediaPacer};
use crate::media_sender::{MediaSender, MediaSocket, TerminationReason};
use crate::wire::{self, InputDecision, InputSequencer};
use control_contract::host::{CaptureBackendInfo, DisplayInfo, EncoderExperiment, StatsInfo};
use control_contract::udp_stability::AppliedUdpStability;
use input::InputInjector;
use secure_channel::DatagramSealer;
use wayland::{GlobalAvailability, OutputInfo, ScreenCopier};

const CAPTURE_BACKEND_ID: &str = "wlrootsScreencopy";

/// The viewer emits authenticated traffic at 1 Hz (latency probes and
/// receiver feedback). Silence beyond this budget while a session is running
/// means the peer is gone: the UDP socket may never error, so the
/// input receiver reaps the session itself. Same budget as Windows.
const VIEWER_CONTACT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct LinuxBackend {
    availability: GlobalAvailability,
    next_handle: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<LinuxSession>>>,
}

pub(crate) struct LinuxSession {
    access: crate::source_grants::CaptureAccess,
    stop: AtomicBool,
    input_gate: InputGate,
    force_keyframe: AtomicBool,
    /// Milliseconds since input worker start of the last authenticated viewer
    /// datagram (feedback, probe, input, or BYE). Zero until first contact.
    last_viewer_contact_ms: AtomicU64,
    last_receiver_feedback: Mutex<Option<Instant>>,
    pub(crate) stats: Mutex<StatsInfo>,
    injector: Option<Mutex<InputInjector>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    /// Shares this exact session's endpoint, writer lock and nonce sequence.
    notice_sender: MediaSender,
    /// c2s opener for every viewer datagram (input, feedback, probes, BYE).
    crypto_rx: DatagramSealer,
}

impl LinuxBackend {
    pub fn new() -> Result<Self, String> {
        // Fail before the UI opens when the compositor cannot serve capture at
        // all — the dashboard would otherwise show a catalog that can never
        // start (same fail-early rule as the WGC probe on Windows).
        let availability = wayland::probe_globals()?;
        if !availability.supports_capture() {
            return Err(
                "this Wayland compositor does not advertise wl_output + zwlr_screencopy_manager_v1 (a wlroots-based compositor like Hyprland is required)"
                    .into(),
            );
        }
        if !availability.supports_input() {
            eprintln!(
                "leftcar: virtual input protocols are unavailable; remote input stays disabled"
            );
        }
        Ok(Self {
            availability,
            next_handle: AtomicU32::new(1),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    fn retire(&self, handle: u32, reason: Option<TerminationReason>) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(&handle)
            .ok_or_else(|| format!("no such Linux capture handle {handle}"))?;
        session.stop.store(true, Ordering::Release);
        session.input_gate.retire();
        let _ = session.notice_sender.notify_retirement(reason);
        let released_input = release_all(&session.injector);
        let threads = std::mem::take(&mut *session.threads.lock().unwrap());
        for thread in threads {
            let _ = thread.join();
        }
        session.stats.lock().unwrap().state = "stopped".into();
        released_input
    }
}

impl CaptureBackend for LinuxBackend {
    fn platform(&self) -> &'static str {
        "linux"
    }

    fn stop_with_reason(&self, handle: u32, reason_code: u8) -> Result<(), String> {
        self.retire(handle, Some(TerminationReason::try_from(reason_code)?))
    }

    fn capture_backends(&self) -> Vec<CaptureBackendInfo> {
        vec![CaptureBackendInfo {
            id: CAPTURE_BACKEND_ID.into(),
            label: "wlroots Screencopy".into(),
            hint: "권장 · Wayland 화면 복사 + NVENC 하드웨어 H.264(소프트웨어 폴백)".into(),
        }]
    }

    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        let outputs = wayland::list_outputs()?;
        if outputs.is_empty() {
            return Err("Wayland reported no active outputs".into());
        }
        Ok(outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| DisplayInfo {
                // Without a stable wl_output v4 name there is no grant key —
                // the output stays listed but ungrantable (Windows mirrors
                // follow the same rule).
                source_id: output
                    .stable_name
                    .then(|| format!("linux:output:{}", output.name)),
                index: index as u32,
                name: output.name,
                width: output.width.max(0) as u32,
                height: output.height.max(0) as u32,
            })
            .collect())
    }

    fn start(
        &self,
        _source_index: u32,
        ip: &str,
        port: u16,
        width: u32,
        height: u32,
        fps: u32,
        capture_backend: &str,
        media_transport: &str,
        _content_mode: &str,
        _encoder_experiment: EncoderExperiment,
        _udp_stability: &AppliedUdpStability,
        media_key: &[u8; 32],
        access: Option<&crate::source_grants::CaptureAccess>,
    ) -> Result<u32, String> {
        if media_transport != "udp" {
            return Err(
                "Linux backend supports Wi-Fi UDP media in v1 (USB/AOAP follows later)".into(),
            );
        }
        if capture_backend != CAPTURE_BACKEND_ID {
            return Err(format!(
                "unsupported Linux capture backend: {capture_backend}"
            ));
        }
        let access = access.ok_or("Host source authorization is required")?;
        let _admission = access.lease.enter().ok_or("source authorization revoked")?;
        let target = select_output(&access.source_id)?;

        // Random LCH1 nonce: freshness for the reachability proof. The media
        // key — possession of it — is the actual authentication. The receive
        // side opens viewer frames under the derived c2s key.
        let mut nonce = [0u8; 32];
        secure_channel::random_bytes(&mut nonce);
        let challenge = wire::challenge(&nonce);
        let crypto_rx = DatagramSealer::new(secure_channel::media_keys(media_key).c2s);
        let socket = UdpSocket::bind(("0.0.0.0", 0))
            .map_err(|error| format!("bind media socket: {error}"))?;
        let peer = SocketAddr::new(
            ip.parse()
                .map_err(|error| format!("invalid viewer IP {ip}: {error}"))?,
            port,
        );
        socket
            .set_write_timeout(Some(Duration::from_millis(20)))
            .map_err(|error| format!("configure media socket: {error}"))?;
        let socket = Arc::new(socket);
        let sender = MediaSender::new(MediaSocket::UdpTo(socket.clone(), peer), media_key)
            .with_access(access);
        prove_udp_reachability(&socket, &sender, &crypto_rx, &challenge, ip, port)?;
        let input_socket = socket
            .try_clone()
            .map_err(|error| format!("clone Linux input socket: {error}"))?;

        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
        let mut initial_stats = initial_stats(width, height, fps);
        initial_stats.media_transport = media_transport.into();
        initial_stats.capture_backend = CAPTURE_BACKEND_ID.into();
        let session = Arc::new(LinuxSession {
            access: access.clone(),
            stop: AtomicBool::new(false),
            input_gate: InputGate::new(sender.clone()),
            force_keyframe: AtomicBool::new(true),
            last_viewer_contact_ms: AtomicU64::new(0),
            last_receiver_feedback: Mutex::new(None),
            stats: Mutex::new(initial_stats),
            injector: build_injector(self.availability, &target)?,
            threads: Mutex::new(Vec::new()),
            notice_sender: sender.clone(),
            crypto_rx,
        });

        let _ = session.input_gate.publish();
        let input_session = session.clone();
        let input_sender = sender.clone();
        let input_thread = std::thread::Builder::new()
            .name(format!("leftcar-linux-input-{handle}"))
            .spawn(move || run_udp_input(input_socket, input_sender, input_session))
            .map_err(|error| format!("spawn Linux input receiver: {error}"))?;

        let capture_session = session.clone();
        let target_name = target.name.clone();
        let capture_thread = match std::thread::Builder::new()
            .name(format!("leftcar-linux-capture-{handle}"))
            .spawn(move || {
                if let Err(error) = run_capture(
                    sender,
                    capture_session.clone(),
                    &target_name,
                    width,
                    height,
                    fps,
                ) {
                    let mut stats = capture_session.stats.lock().unwrap();
                    stats.state = "error".into();
                    stats.error = Some(error);
                    capture_session.stop.store(true, Ordering::Release);
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                // Never leave the already-running input thread orphaned: stop
                // the session so its loop exits, then join it.
                session.stop.store(true, Ordering::Release);
                session.input_gate.retire();
                // Best-effort cleanup on an already-failing path.
                let _ = release_all(&session.injector);
                let _ = input_thread.join();
                return Err(format!("spawn Linux capture worker: {error}"));
            }
        };
        session
            .threads
            .lock()
            .unwrap()
            .extend([input_thread, capture_thread]);
        self.sessions.lock().unwrap().insert(handle, session);
        Ok(handle)
    }

    fn stop(&self, handle: u32) -> Result<(), String> {
        // Reconfigure/ordinary retirement leaves the existing Viewer window.
        self.retire(handle, None)
    }

    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(&handle)
            .map(|session| {
                let mut stats = session.stats.lock().unwrap().clone();
                stats.receiver_feedback_age_ms = session
                    .last_receiver_feedback
                    .lock()
                    .unwrap()
                    .map(|last| last.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                stats
            })
            .ok_or_else(|| format!("no such Linux capture handle {handle}"))
    }

    fn input_permission(&self) -> Result<bool, String> {
        // wlr virtual input protocols have no permission prompt; availability
        // at startup is the whole story. Missing protocols keep input off.
        Ok(self.availability.supports_input())
    }

    fn request_input_permission(&self) -> Result<bool, String> {
        self.input_permission()
    }

    fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&handle)
            .cloned()
            .ok_or_else(|| format!("no such Linux capture handle {handle}"))?;
        let _operation = if enabled {
            Some(
                session
                    .access
                    .lease
                    .enter()
                    .ok_or("source authorization revoked")?,
            )
        } else {
            None
        };
        let Some(injector) = &session.injector else {
            // Fail closed: the control plane leaves the session locked, same
            // as the macOS path without OS input permission.
            return Err("remote input is unavailable on this host".into());
        };
        session.input_gate.set_enabled(enabled);
        if !enabled {
            injector.lock().unwrap().release_all()?;
        }
        Ok(())
    }
}

/// Virtual input devices live on the input thread; the injector is created up
/// front so `set_input_enabled` can always release held keys even before the
/// thread used them. Without the compositor's virtual input protocols the
/// session starts capture-only and input stays locked (F-6: degrade, not fail).
fn build_injector(
    availability: GlobalAvailability,
    target: &OutputInfo,
) -> Result<Option<Mutex<InputInjector>>, String> {
    if !availability.supports_input() {
        return Ok(None);
    }
    let devices = wayland::VirtualInput::new()?;
    let outputs = wayland::list_outputs()?;
    let min_x = outputs.iter().map(|o| o.x).min().unwrap_or(0);
    let min_y = outputs.iter().map(|o| o.y).min().unwrap_or(0);
    let max_x = outputs.iter().map(|o| o.x + o.width).max().unwrap_or(0);
    let max_y = outputs.iter().map(|o| o.y + o.height).max().unwrap_or(0);
    let injector = InputInjector::new(
        devices,
        target.x - min_x,
        target.y - min_y,
        target.width.max(1) as u32,
        target.height.max(1) as u32,
        (max_x - min_x).max(1) as u32,
        (max_y - min_y).max(1) as u32,
    )?;
    Ok(Some(Mutex::new(injector)))
}

/// Release every held key/button when an injector exists; a capture-only
/// session (no virtual input) has nothing held.
fn release_all(injector: &Option<Mutex<InputInjector>>) -> Result<(), String> {
    match injector {
        Some(injector) => injector.lock().unwrap().release_all(),
        None => Ok(()),
    }
}

fn select_output(source_id: &str) -> Result<OutputInfo, String> {
    let expected = source_id
        .strip_prefix("linux:output:")
        .ok_or("source display is unavailable")?;
    let outputs = wayland::list_outputs()?;
    let mut matches = outputs.into_iter().filter(|output| output.name == expected);
    let output = matches.next().ok_or("source display is unavailable")?;
    if expected.is_empty() || matches.next().is_some() {
        return Err("source display identity is ambiguous".into());
    }
    Ok(output)
}

/// Identify which output a granted source_id points at, for the capture
/// worker's re-resolution at session start.
fn source_target_name(source_id: &str) -> Result<String, String> {
    select_output(source_id).map(|output| output.name)
}

/// Same proof as the Windows backend: sealed LCH1 challenge, 60 attempts,
/// 50 ms apart, exact echo required under the derived c2s key.
fn prove_udp_reachability(
    socket: &UdpSocket,
    sender: &MediaSender,
    crypto_rx: &DatagramSealer,
    challenge: &[u8],
    ip: &str,
    port: u16,
) -> Result<(), String> {
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|error| format!("configure reachability timeout: {error}"))?;
    let mut response = [0u8; 65_535];
    for attempt in 0..60 {
        if attempt % 4 == 0 {
            let _ = sender.send(challenge);
        }
        match socket.recv_from(&mut response) {
            Ok((size, source)) => {
                if size > 256 || Some(source) != sender.udp_peer() {
                    continue;
                }
                let opened = crypto_rx.open(&response[..size]).ok();
                if opened.as_deref() == Some(challenge) {
                    socket
                        .set_read_timeout(Some(Duration::from_millis(50)))
                        .map_err(|error| format!("configure input timeout: {error}"))?;
                    return Ok(());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(format!("UDP reachability proof receive failed: {error}")),
        }
    }
    Err(format!("UDP reachability proof failed for {ip}:{port}"))
}

fn run_udp_input(socket: UdpSocket, sender: MediaSender, session: Arc<LinuxSession>) {
    let mut sequencer = InputSequencer::default();
    sequencer.reset();
    // Consume complete datagrams so oversized unauthenticated UDP cannot grow
    // allocations. authenticate_udp_control rejects anything above the 640B
    // control envelope limit before AEAD allocation/work.
    let mut packet = [0u8; 65_535];
    let started = Instant::now();
    while !session.stop.load(Ordering::Acquire) {
        // An invalid datagram flood must not postpone the liveness deadline.
        check_viewer_health(&session, started);
        if session.stop.load(Ordering::Acquire) {
            break;
        }
        let (size, source) = match socket.recv_from(&mut packet) {
            Ok(received) => received,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                check_viewer_health(&session, started);
                continue;
            }
            Err(error) => {
                set_session_error(&session, format!("Linux input receive failed: {error}"));
                break;
            }
        };
        let received_ms = wall_clock_ms();
        let Some((message, reply_sender)) =
            sender.authenticate_udp_control(&session.crypto_rx, &packet[..size], source)
        else {
            continue;
        };
        process_authenticated_message(
            &message,
            &session,
            &mut sequencer,
            &reply_sender,
            started,
            received_ms,
        );
    }
    session.stop.store(true, Ordering::Release);
    session.input_gate.retire();
    let _ = release_all(&session.injector);
}

fn process_authenticated_message(
    message: &[u8],
    session: &Arc<LinuxSession>,
    sequencer: &mut InputSequencer,
    sender: &MediaSender,
    started: Instant,
    received_ms: u64,
) {
    let Some(_permission) = session.access.lease.enter() else {
        session.stop.store(true, Ordering::Release);
        return;
    };
    session
        .last_viewer_contact_ms
        .store(started.elapsed().as_millis() as u64, Ordering::Release);
    if message == b"BYE" {
        session.stop.store(true, Ordering::Release);
        return;
    }
    if message == b"IDR" {
        session.force_keyframe.store(true, Ordering::Release);
    }
    if session.input_gate.refresh_on_control(message) {
        return;
    }
    if let Some(response) = latency_probe_response(message, received_ms, wall_clock_ms()) {
        let _ = sender.send(&response);
        return;
    }
    if let Some(feedback) = ReceiverFeedback::parse(message) {
        let mut stats = session.stats.lock().unwrap();
        stats.receiver_frame_gaps = stats
            .receiver_frame_gaps
            .max(i64::from(feedback.frame_gaps));
        stats.receiver_input_drops = stats
            .receiver_input_drops
            .max(i64::from(feedback.input_drops));
        stats.receiver_incomplete_aus = stats
            .receiver_incomplete_aus
            .max(i64::from(feedback.incomplete_aus));
        stats.receiver_stale_frames = stats
            .receiver_stale_frames
            .max(i64::from(feedback.stale_frames));
        stats.receiver_stale_input_drops = feedback.stale_input_drops.map(i64::from);
        if let Some(discards) = feedback.output_burst_discards {
            stats.receiver_output_burst_discards = stats
                .receiver_output_burst_discards
                .max(i64::from(discards));
        }
        stats.receiver_rtt_ms = feedback.rtt_ms;
        stats.receiver_wire_ms = feedback.wire_ms;
        stats.rendered_fps = feedback.rendered_fps;
        *session.last_receiver_feedback.lock().unwrap() = Some(Instant::now());
        return;
    }
    match sequencer.accept(message) {
        InputDecision::Ignore => {}
        InputDecision::AckDuplicate(sequence) => {
            let enabled = session.input_gate.enabled();
            let _ = sender.send(&wire::input_ack(sequence, enabled));
        }
        InputDecision::Apply(event) => {
            if session.input_gate.enabled() {
                if let Some(injector) = &session.injector {
                    if let Err(error) = injector.lock().unwrap().apply(event) {
                        set_session_error(session, error);
                    }
                }
            }
        }
        InputDecision::ApplyAndAck { sequence, event } => {
            if session.input_gate.enabled() {
                if let Some(injector) = &session.injector {
                    if let Err(error) = injector.lock().unwrap().apply(event) {
                        set_session_error(session, error);
                    }
                }
            }
            let enabled = session.input_gate.enabled();
            let _ = sender.send(&wire::input_ack(sequence, enabled));
        }
    }
}

pub(crate) fn set_session_error(session: &LinuxSession, error: String) {
    session.stats.lock().unwrap().error = Some(error);
}

/// Terminate a running session whose viewer stopped emitting authenticated
/// traffic. Sends an LCT1 notice first so a still-live viewer closes its
/// window promptly, then flips the session to `error` so the control plane's
/// terminal-state GC releases the capture threads.
fn check_viewer_health(session: &Arc<LinuxSession>, started: Instant) {
    let last = session.last_viewer_contact_ms.load(Ordering::Acquire);
    let now_ms = started.elapsed().as_millis() as u64;
    if now_ms.saturating_sub(last) < VIEWER_CONTACT_TIMEOUT.as_millis() as u64 {
        return;
    }
    if session.stop.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = session
        .notice_sender
        .send_terminal(TerminationReason::Health);
    {
        let mut stats = session.stats.lock().unwrap();
        stats.state = "error".into();
        stats.error = Some("viewer connection lost (feedback timeout)".into());
    }
    let _ = release_all(&session.injector);
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Capture → encode → packetize → send worker
// ---------------------------------------------------------------------------

/// Per-interval fps/kbps counters, mirroring the Windows worker's rollup.
struct FrameInterval {
    started: Instant,
    frames: u32,
    bytes: u64,
}

impl FrameInterval {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            frames: 0,
            bytes: 0,
        }
    }
}

fn run_capture(
    sender: MediaSender,
    session: Arc<LinuxSession>,
    target_name: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<(), String> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err("H.264 dimensions must be even".into());
    }
    let _admission = session
        .access
        .lease
        .enter()
        .ok_or("source authorization revoked")?;
    // Re-resolve the granted output at the actual capture start so a stale
    // name from the control plane cannot stream the wrong screen (the same
    // identity re-check the WGC worker performs).
    let granted = session.access.source_id.clone();
    let resolved = source_target_name(&granted)?;
    if resolved != target_name {
        return Err("source display changed before capture; refresh sources".into());
    }
    let mut copier = ScreenCopier::new(target_name)?;
    drop(_admission);

    let bitrate = session.stats.lock().unwrap().current_bitrate;
    let selection = encoder::create_video_encoder(width, height, fps, bitrate)?;
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "linux_encoder_selected",
            "schema": 1,
            "session_thread": std::thread::current().name(),
            "encoder_mode": selection.meta.mode,
            "encoder_id": selection.meta.id,
            "hardware": selection.meta.hardware,
            "preset": selection.meta.preset,
            "profile": selection.meta.profile,
            "fallback_reason": selection.fallback_reason,
        })
    );
    {
        let mut stats = session.stats.lock().unwrap();
        stats.encoder_mode = selection.meta.mode.into();
        stats.encoder_id = selection.meta.id.into();
        stats.encoder_hardware_accelerated = Some(selection.meta.hardware);
        stats.encoder_preset = selection.meta.preset.into();
        stats.encoder_profile = selection.meta.profile.into();
    }
    let mut encoder = selection.encoder;
    let started = Instant::now();
    let mut pacing = CapturePacing::new(bitrate, fps, &sender, started);
    let frame_period = Duration::from_nanos(1_000_000_000 / u64::from(fps.max(1)));
    let result = (|| -> Result<(), String> {
        let mut au_id = 0u16;
        let mut config_sent = false;
        let mut last_frame: Option<Instant> = None;
        let mut interval = FrameInterval::new();
        let mut next_frame_at = Instant::now();

        while !session.stop.load(Ordering::Acquire) {
            let capture_started = Instant::now();
            let frame = copier.capture()?;
            let capture_to_encode_us = capture_started.elapsed().as_micros() as u64;
            if session.force_keyframe.swap(false, Ordering::AcqRel) {
                encoder.force_keyframe()?;
            }
            let encode_started = Instant::now();
            let source =
                convert::BgraFrame::new(&frame.bgra, frame.width, frame.height, frame.stride)?;
            let output = encoder.encode(&source)?;
            let encode_us = encode_started.elapsed().as_micros() as u64;
            send_encoded_output(
                &sender,
                &session,
                started,
                output,
                capture_to_encode_us,
                encode_us,
                &mut config_sent,
                &mut au_id,
                &mut last_frame,
                &mut interval,
                &mut pacing,
            )?;
            next_frame_at += frame_period;
            let now = Instant::now();
            if next_frame_at > now {
                std::thread::sleep(next_frame_at - now);
            } else {
                // Falling behind: drop the backlog so latency does not build.
                next_frame_at = now;
            }
        }
        Ok(())
    })();
    pacing.snapshot("final");
    if session.stop.load(Ordering::Acquire) {
        // A user/input-worker stop remains a normal stop, even if a pacing
        // boundary observed it while draining an AU.
        session.stats.lock().unwrap().state = "stopped".into();
        Ok(())
    } else {
        result
    }
}

/// Send one encoded access unit: config/SPS gating, fragmentation, parity,
/// pacing and per-AU stats. Mirrors the Windows `send_encoded_outputs`.
#[allow(clippy::too_many_arguments)]
fn send_encoded_output(
    sender: &MediaSender,
    session: &LinuxSession,
    started: Instant,
    output: encoder::EncodedAu,
    capture_to_encode_us: u64,
    encode_us: u64,
    config_sent: &mut bool,
    au_id: &mut u16,
    last_frame: &mut Option<Instant>,
    interval: &mut FrameInterval,
    pacing: &mut CapturePacing,
) -> Result<(), String> {
    if session.stats.lock().unwrap().first_capture_ms == 0 {
        session.stats.lock().unwrap().first_capture_ms = started.elapsed().as_millis() as u64;
    }
    if output.data.is_empty() {
        // openh264 emitted no AU for this frame (rate-control internal); the
        // capture loop continues and a forced keyframe re-arms if one was set.
        return Ok(());
    }
    let Some(annex_b) = wire::normalize_h264(&output.data) else {
        return Err("encoder emitted malformed H.264".into());
    };
    let mut config_packet = None;
    let parameter_sets = wire::h264_parameter_sets(&annex_b);
    let keyframe = output.keyframe || contains_idr(&annex_b);
    if (!*config_sent || keyframe) && !parameter_sets.is_empty() {
        if let Some(config) = wire::config_datagram(&parameter_sets) {
            config_packet = Some(config);
        }
    }
    // Do not send an undecodable access unit before SPS/PPS are known.
    if !*config_sent && config_packet.is_none() {
        session.force_keyframe.store(true, Ordering::Release);
        return Ok(());
    }
    *au_id = au_id.wrapping_add(1);
    let wall_ms = wall_clock_ms();
    let datagrams =
        wire::media_datagrams_with_limit(*au_id, wall_ms, &annex_b, sender.plaintext_limit());
    if datagrams.is_empty() {
        return Err("encoded H.264 access unit exceeds Leftcar fragment limit".into());
    }
    let fragment_count = datagrams.len();
    let parity_datagrams = crate::fec::parity_datagrams_for_media(*au_id, wall_ms, &datagrams);
    let parity_count = parity_datagrams.len();
    let send_started = Instant::now();
    let mut packets = Vec::with_capacity(
        datagrams.len() + parity_datagrams.len() + usize::from(config_packet.is_some()),
    );
    let has_config = config_packet.is_some();
    packets.extend(config_packet);
    packets.extend(datagrams);
    packets.extend(parity_datagrams);
    let before_bytes = pacing.policy.metrics().submitted_wire_bytes;
    pacing.drain(sender, session, &packets, true)?;
    *config_sent |= has_config;
    let bytes = (pacing.policy.metrics().submitted_wire_bytes - before_bytes) as usize;
    let send_us = send_started.elapsed().as_micros() as u64;
    interval.frames += 1;
    interval.bytes += bytes as u64;
    update_stats(
        session,
        started,
        last_frame.replace(Instant::now()),
        capture_to_encode_us,
        encode_us,
        send_us,
        bytes,
        *au_id,
        fragment_count,
        parity_count,
        packets.len(),
        bytes,
        keyframe,
    );
    if interval.started.elapsed() >= Duration::from_secs(1) {
        let elapsed = interval.started.elapsed().as_secs_f64();
        let mut stats = session.stats.lock().unwrap();
        stats.fps = (interval.frames as f64 / elapsed).round() as u32;
        stats.kbps = (interval.bytes as f64 * 8.0 / 1_000.0 / elapsed).round() as u32;
        interval.frames = 0;
        interval.bytes = 0;
        interval.started = Instant::now();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn update_stats(
    session: &LinuxSession,
    started: Instant,
    previous_frame: Option<Instant>,
    capture_us: u64,
    encode_us: u64,
    send_us: u64,
    bytes: usize,
    au_id: u16,
    fragments: usize,
    parity: usize,
    datagrams: usize,
    au_bytes: usize,
    keyframe: bool,
) {
    let mut stats = session.stats.lock().unwrap();
    stats.state = "running".into();
    stats.frames += 1;
    stats.bytes += bytes as i64;
    stats.capture_to_encode_us = capture_us;
    stats.max_capture_to_encode_us = stats.max_capture_to_encode_us.max(capture_us);
    stats.capture_to_encode_p95_us = stats.capture_to_encode_p95_us.max(capture_us);
    stats.encode_output_us = encode_us;
    stats.max_encode_output_us = stats.max_encode_output_us.max(encode_us);
    stats.encode_output_p95_us = stats.encode_output_p95_us.max(encode_us);
    stats.send_block_us = send_us;
    stats.max_send_block_us = stats.max_send_block_us.max(send_us);
    stats.send_block_p95_us = stats.send_block_p95_us.max(send_us);
    if let Some(previous) = previous_frame {
        let interval = previous.elapsed().as_micros() as u64;
        stats.capture_interval_p95_us = stats.capture_interval_p95_us.max(interval);
    }
    stats.last_au_bytes = au_bytes as u64;
    stats.last_au_fragments = fragments as u32;
    stats.last_au_parity = parity as u32;
    stats.last_au_datagrams = datagrams as u32;
    stats.last_au_is_keyframe = keyframe;
    stats.sent_datagrams += datagrams as i64;
    stats.sent_parity_datagrams += parity as i64;
    let _ = au_id;
    if stats.first_encode_ms == 0 {
        stats.first_encode_ms = started.elapsed().as_millis() as u64;
    }
    if stats.first_send_ms == 0 {
        stats.first_send_ms = started.elapsed().as_millis() as u64;
    }
}

/// The capture worker owns one pacer for its whole lifetime. Other socket
/// traffic (input acknowledgements/notices) is not video and keeps its sender.
struct CapturePacing {
    policy: MediaPacer,
    started: Instant,
    last_snapshot: Instant,
    capture_id: u128,
}

impl CapturePacing {
    fn new(bitrate: u32, fps: u32, sender: &MediaSender, started: Instant) -> Self {
        Self {
            policy: MediaPacer::with_plaintext_limit(
                bitrate,
                fps,
                sender.is_tcp(),
                started.elapsed(),
                sender.plaintext_limit(),
            ),
            started,
            last_snapshot: Instant::now(),
            capture_id: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        }
    }

    fn drain(
        &mut self,
        sender: &MediaSender,
        session: &LinuxSession,
        packets: &[Vec<u8>],
        access_unit: bool,
    ) -> Result<(), String> {
        let mut io = LinuxDrain {
            sender,
            session,
            started: self.started,
        };
        let result = self.policy.drain(packets, access_unit, &mut io);
        if result.is_err() {
            self.snapshot("terminal");
        } else if self.last_snapshot.elapsed() >= Duration::from_secs(1) {
            self.snapshot("periodic");
        }
        result.map_err(|error| {
            format!(
                "Linux media drain terminated: {error:?}; cause={:?}",
                self.policy.metrics().terminal_cause
            )
        })
    }

    fn snapshot(&mut self, reason: &str) {
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "linux_media_pacing",
                "schema": 1,
                "reason": reason,
                "capture_id": self.capture_id.to_string(),
                "session_thread": std::thread::current().name(),
                "counter_lifetime": "capture_worker_cumulative",
                "elapsed_us": self.started.elapsed().as_micros() as u64,
                "submission_mode": "bounded_ordinary",
                "transport_budget_bytes_per_second": self.policy.transport_bytes_per_second(),
                "terminal": self.policy.terminal(),
                "metrics": self.policy.metrics(),
            })
        );
        self.last_snapshot = Instant::now();
    }
}

struct LinuxDrain<'a> {
    sender: &'a MediaSender,
    session: &'a LinuxSession,
    started: Instant,
}

impl DrainIo for LinuxDrain<'_> {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn cancelled(&self) -> bool {
        self.session.stop.load(Ordering::Acquire)
    }
    fn prepare(&mut self, packet: &[u8]) -> std::io::Result<Vec<u8>> {
        self.sender.prepare(packet)
    }
    fn submit(&mut self, packet: &[u8]) -> std::io::Result<usize> {
        self.sender.submit_sealed(packet)
    }
}

fn initial_stats(width: u32, height: u32, fps: u32) -> StatsInfo {
    let bitrate = ((u64::from(width) * u64::from(height) * u64::from(fps)) / 10)
        .clamp(4_000_000, 50_000_000) as u32;
    StatsInfo {
        frames: 0,
        bytes: 0,
        state: "starting_capture".into(),
        fps: 0,
        kbps: 0,
        fps_target: fps,
        encoder_experiment_diagnostics_available: false,
        encoder_experiment_requested: "auto".into(),
        encoder_experiment_applied: "auto".into(),
        encoder_experiment_fallback_reason: None,
        encoder_mode: "software".into(),
        encoder_id: "openh264".into(),
        encoder_hardware_accelerated: Some(false),
        encoder_preset: "lowComplexity".into(),
        encoder_profile: "baseline".into(),
        current_bitrate: bitrate,
        capture_backend: CAPTURE_BACKEND_ID.into(),
        media_transport: "udp".into(),
        ..StatsInfo::default()
    }
}

fn contains_idr(annex_b: &[u8]) -> bool {
    let mut offset = 0;
    while offset + 4 < annex_b.len() {
        let prefix = if annex_b[offset..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if annex_b[offset..].starts_with(&[0, 0, 1]) {
            3
        } else {
            offset += 1;
            continue;
        };
        if annex_b
            .get(offset + prefix)
            .is_some_and(|byte| byte & 0x1f == 5)
        {
            return true;
        }
        offset += prefix;
    }
    false
}

#[cfg(all(test, target_os = "linux"))]
mod stream_smoke {
    //! End-to-end host-side stream smoke against the live compositor:
    //! backend.start → wlr-screencopy → OpenH264 → wire/FEC/pacing → a local
    //! fake viewer that proves reachability, heartbeats, receives and unseals
    //! media, and terminates with BYE. Needs a running Wayland session, so it
    //! is #[ignore]d from ordinary `cargo test`:
    //!
    //! `LEFTCAR_STREAM_SMOKE=1 cargo test stream_smoke -- --ignored --nocapture`

    use super::*;
    use crate::source_grants::{CaptureAccess, SourceLease};
    use secure_channel::DatagramSealer;
    use std::net::UdpSocket;
    use std::sync::atomic::AtomicUsize;

    struct SmokeViewer {
        proof_done: bool,
        cfg_count: usize,
        au_ids: Vec<u16>,
        fragments: usize,
        host_addr: Option<SocketAddr>,
        /// BYE fires once this many distinct access units arrived.
        bye_after_aus: usize,
        bye_sent: bool,
    }

    #[test]
    #[ignore = "needs a live Wayland compositor (run under the session)"]
    fn captures_encodes_and_streams_to_a_local_viewer() {
        let outputs = wayland::list_outputs().expect("list outputs");
        let target = outputs.first().expect("at least one output").clone();
        let width = target.width.max(2) as u32;
        let height = target.height.max(2) as u32;
        let fps = 60u32;

        let mut media_key = [0u8; 32];
        secure_channel::random_bytes(&mut media_key);
        let keys = secure_channel::media_keys(&media_key);
        let viewer_opener = DatagramSealer::new(keys.s2c);
        let viewer_sealer = DatagramSealer::new(keys.c2s);

        let viewer_socket = UdpSocket::bind(("127.0.0.1", 0)).expect("bind viewer socket");
        viewer_socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let viewer_port = viewer_socket.local_addr().unwrap().port();

        let viewer = Arc::new(Mutex::new(SmokeViewer {
            proof_done: false,
            cfg_count: 0,
            au_ids: Vec::new(),
            fragments: 0,
            host_addr: None,
            bye_after_aus: 65,
            bye_sent: false,
        }));
        let stop_viewer = Arc::new(AtomicBool::new(false));
        let heartbeats = Arc::new(AtomicUsize::new(0));

        let viewer_thread = {
            let viewer = viewer.clone();
            let stop = stop_viewer.clone();
            let heartbeats = heartbeats.clone();
            std::thread::spawn(move || {
                let mut packet = [0u8; 65_535];
                let mut last_heartbeat = Instant::now();
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    if last_heartbeat.elapsed() >= Duration::from_millis(500) {
                        if let Some(host) = viewer.lock().unwrap().host_addr {
                            let _ = viewer_socket.send_to(
                                &viewer_sealer.seal(b"LCK1").expect("seal heartbeat"),
                                host,
                            );
                            heartbeats.fetch_add(1, Ordering::Relaxed);
                        }
                        last_heartbeat = Instant::now();
                    }
                    let Ok((size, source)) = viewer_socket.recv_from(&mut packet) else {
                        continue;
                    };
                    let Some(plaintext) = viewer_opener.open(&packet[..size]).ok() else {
                        continue;
                    };
                    let mut state = viewer.lock().unwrap();
                    state.host_addr = Some(source);
                    if plaintext.starts_with(b"LCH1") {
                        let _ = viewer_socket
                            .send_to(&viewer_sealer.seal(&plaintext).expect("seal proof"), source);
                        state.proof_done = true;
                    } else if plaintext.starts_with(b"CFG") {
                        state.cfg_count += 1;
                    } else if plaintext.first() == Some(&b'G') {
                        // G | frag u16 BE | frag u16 BE | au_id u16 LE | magic
                        if plaintext.len() >= 7 {
                            let au_id = u16::from_le_bytes([plaintext[5], plaintext[6]]);
                            if state.au_ids.last() != Some(&au_id) {
                                state.au_ids.push(au_id);
                            }
                        }
                        state.fragments += 1;
                        if state.au_ids.len() >= state.bye_after_aus && !state.bye_sent {
                            let _ = viewer_socket
                                .send_to(&viewer_sealer.seal(b"BYE").expect("seal bye"), source);
                            state.bye_sent = true;
                        }
                    }
                }
            })
        };

        let backend = LinuxBackend::new().expect("backend construction");
        let access = CaptureAccess {
            revision: 1,
            source_id: format!("linux:output:{}", target.name),
            owner: "stream-smoke".into(),
            lease: Arc::new(SourceLease::default()),
        };
        let udp_stability = control_contract::udp_stability::AppliedUdpStability {
            requested: control_contract::udp_stability::UdpStabilityProfile::Auto,
            applied: control_contract::udp_stability::UdpStabilityProfile::Auto,
            burst_datagrams: 0,
            fec_parity_shards: 0,
            adaptive_pacing: false,
            fallback_reason: None,
        };
        let handle = backend
            .start(
                0,
                "127.0.0.1",
                viewer_port,
                width,
                height,
                fps,
                CAPTURE_BACKEND_ID,
                "udp",
                "interactive",
                EncoderExperiment::Auto,
                &udp_stability,
                &media_key,
                Some(&access),
            )
            .expect("backend start");

        // First frame must reach the wire quickly or the control plane would
        // fail the session (STARTUP_FIRST_FRAME_TIMEOUT is 5s).
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let stats = backend.stats(handle).expect("stats");
            if stats.first_send_ms > 0 && stats.frames >= 60 {
                break;
            }
            if Instant::now() > deadline {
                let viewer = viewer.lock().unwrap();
                panic!(
                    "stream did not produce 60 frames in time: frames={} state={:?} error={:?} \
                     proof={} cfg={} viewer_aus={} viewer_fragments={} heartbeats={}",
                    stats.frames,
                    stats.state,
                    stats.error,
                    viewer.proof_done,
                    viewer.cfg_count,
                    viewer.au_ids.len(),
                    viewer.fragments,
                    heartbeats.load(Ordering::Relaxed),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // Wait for the fake viewer's BYE, then the session must retire.
        let deadline = Instant::now() + Duration::from_secs(6);
        while !viewer.lock().unwrap().bye_sent && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(viewer.lock().unwrap().bye_sent, "viewer never reached BYE");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let stats = backend.stats(handle).expect("stats after bye");
            if stats.state == "stopped" {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "session did not stop after BYE: state={:?} error={:?}",
                stats.state,
                stats.error
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // Snapshot under the lock, release it, then stop the viewer: joining
        // while holding the mutex would deadlock the viewer's own lock().
        let summary = {
            let viewer = viewer.lock().unwrap();
            let stats = backend.stats(handle).expect("final stats");
            assert!(viewer.proof_done, "reachability proof never completed");
            assert!(viewer.cfg_count >= 1, "no CFG datagram reached the viewer");
            assert!(
                viewer.au_ids.len() >= 60,
                "viewer saw too few access units: {} ({} fragments)",
                viewer.au_ids.len(),
                viewer.fragments
            );
            // Access units must be sequential (wrap-tolerant): the viewer
            // decoder treats any other jump as reference-chain loss.
            for pair in viewer.au_ids.windows(2) {
                assert_eq!(
                    pair[0].wrapping_add(1),
                    pair[1],
                    "AU ids jumped: {:?}",
                    &viewer.au_ids[..viewer.au_ids.len().min(8)]
                );
            }
            assert!(stats.error.is_none(), "session errored: {:?}", stats.error);
            assert!(stats.fps > 0, "no fps rollup");
            (
                stats.frames,
                stats.fps,
                stats.kbps,
                viewer.cfg_count,
                viewer.au_ids.len(),
                stats.last_au_bytes,
            )
        };
        stop_viewer.store(true, Ordering::Release);
        viewer_thread.join().expect("viewer thread");
        eprintln!(
            "stream smoke ok: frames={} fps={} kbps={} cfg={} aus={} au_bytes_last={}",
            summary.0, summary.1, summary.2, summary.3, summary.4, summary.5
        );
    }
}
