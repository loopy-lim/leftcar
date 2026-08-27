mod capture;
mod input;

use crate::backend::CaptureBackend;
use crate::wire::{self, InputDecision, InputSequencer};
use control_contract::host::{CaptureBackendInfo, DisplayInfo, StatsInfo};
use input::InputInjector;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};

#[derive(Clone)]
pub(super) struct Monitor {
    pub handle: isize,
    pub rect: RECT,
    pub name: String,
}

const MAX_TCP_MEDIA_FRAME: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(super) enum MediaSender {
    Udp(Arc<UdpSocket>),
    Tcp(Arc<Mutex<TcpStream>>),
}

impl MediaSender {
    fn send(&self, packet: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Udp(socket) => socket.send(packet),
            Self::Tcp(stream) => {
                if packet.is_empty() || packet.len() > MAX_TCP_MEDIA_FRAME {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "TCP media frame is outside the bounded range",
                    ));
                }
                let mut frame = Vec::with_capacity(4 + packet.len());
                frame.extend_from_slice(&(packet.len() as u32).to_be_bytes());
                frame.extend_from_slice(packet);
                stream
                    .lock()
                    .map_err(|_| std::io::Error::other("TCP media writer lock poisoned"))?
                    .write_all(&frame)
                    .map(|_| packet.len())
            }
        }
    }
}

pub(super) enum MediaReceiver {
    Udp(UdpSocket),
    Tcp(TcpStream),
}

pub struct WindowsBackend {
    next_handle: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<WindowsSession>>>,
}

pub(super) struct WindowsSession {
    stop: AtomicBool,
    input_enabled: AtomicBool,
    force_keyframe: AtomicBool,
    /// Milliseconds since process start of the last authenticated viewer
    /// datagram (feedback, probe, input, or BYE). Zero until first contact.
    last_viewer_contact_ms: AtomicU64,
    pub stats: Mutex<StatsInfo>,
    injector: Mutex<InputInjector>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    /// Clone of the media socket used only for termination notices, so the
    /// input receiver can notify a dying viewer before the capture thread's
    /// socket is torn down. `None` on Windows builds where cloning failed.
    notice_sender: Mutex<Option<MediaSender>>,
    /// Session nonce shared with the viewer; authenticates LCT1 notices.
    termination_token: Vec<u8>,
}

impl WindowsSession {
    fn termination_token(&self) -> &[u8] {
        &self.termination_token
    }
}

/// The viewer emits authenticated traffic at 1 Hz (latency probes and
/// receiver feedback). Silence beyond this budget while a session is running
/// means the peer is gone: the connected UDP socket may never error, so the
/// input receiver reaps the session itself.
const VIEWER_CONTACT_TIMEOUT: Duration = Duration::from_secs(6);

impl WindowsBackend {
    pub fn new() -> Result<Self, String> {
        // Fail early on unsupported Windows editions instead of displaying a
        // catalog whose sources can never be opened by WGC.
        let supported = std::thread::spawn(|| {
            use windows::Win32::System::WinRT::{
                RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
            };
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
                .map_err(|error| format!("initialize WinRT for WGC probe: {error}"))?;
            let result = windows::Graphics::Capture::GraphicsCaptureSession::IsSupported()
                .map_err(|error| format!("Windows Graphics Capture probe failed: {error}"));
            unsafe { RoUninitialize() };
            result
        })
        .join()
        .map_err(|_| "Windows Graphics Capture probe panicked".to_string())??;
        if !supported {
            return Err(
                "Windows Graphics Capture monitor interop is unavailable (Windows 10 1903+ required)".into(),
            );
        }
        Ok(Self {
            next_handle: AtomicU32::new(1),
            sessions: Mutex::new(HashMap::new()),
        })
    }
}

impl CaptureBackend for WindowsBackend {
    fn platform(&self) -> &'static str {
        "windows"
    }

    fn stop_with_reason(&self, handle: u32, reason_code: u8) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(&handle)
            .ok_or_else(|| format!("no such Windows capture handle {handle}"))?;
        let notice = wire::termination(reason_code, session.termination_token());
        let _ = socket_attempt_send(&session, &notice);
        session.stop.store(true, Ordering::Release);
        session.input_enabled.store(false, Ordering::Release);
        session.injector.lock().unwrap().release_all()?;
        let threads = std::mem::take(&mut *session.threads.lock().unwrap());
        for thread in threads {
            let _ = thread.join();
        }
        session.stats.lock().unwrap().state = "stopped".into();
        Ok(())
    }

    fn capture_backends(&self) -> Vec<CaptureBackendInfo> {
        vec![CaptureBackendInfo {
            id: "windowsGraphicsCapture".into(),
            label: "Windows Graphics Capture".into(),
            hint: "권장 · Media Foundation 하드웨어 H.264".into(),
        }]
    }

    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        monitors().map(|monitors| {
            monitors
                .into_iter()
                .enumerate()
                .map(|(index, monitor)| DisplayInfo {
                    index: index as u32,
                    name: monitor.name,
                    width: (monitor.rect.right - monitor.rect.left).max(0) as u32,
                    height: (monitor.rect.bottom - monitor.rect.top).max(0) as u32,
                })
                .collect()
        })
    }

    fn start(
        &self,
        source_index: u32,
        ip: &str,
        port: u16,
        width: u32,
        height: u32,
        fps: u32,
        capture_backend: &str,
        media_transport: &str,
        _content_mode: &str,
    ) -> Result<u32, String> {
        if !matches!(media_transport, "udp" | "usb") {
            return Err("Windows backend supports Wi-Fi UDP or USB AOAP media".into());
        }
        if capture_backend != "windowsGraphicsCapture" {
            return Err(format!(
                "unsupported Windows capture backend: {capture_backend}"
            ));
        }
        let monitor = monitors()?
            .get(source_index as usize)
            .cloned()
            .ok_or_else(|| format!("display index {source_index} no longer exists"))?;
        let token = uuid::Uuid::new_v4().to_string().into_bytes();
        let (media_sender, media_receiver, notice_sender) = if media_transport == "usb" {
            let stream = TcpStream::connect(("127.0.0.1", port))
                .map_err(|error| format!("connect USB media proxy {port}: {error}"))?;
            stream
                .set_nodelay(true)
                .map_err(|error| format!("configure USB media TCP_NODELAY: {error}"))?;
            stream
                .set_write_timeout(Some(Duration::from_millis(20)))
                .map_err(|error| format!("configure USB media write timeout: {error}"))?;
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .map_err(|error| format!("configure USB media read timeout: {error}"))?;
            prove_tcp_reachability(&stream, &token)?;
            let sender = MediaSender::Tcp(Arc::new(Mutex::new(
                stream
                    .try_clone()
                    .map_err(|error| format!("clone USB media writer: {error}"))?,
            )));
            (sender, MediaReceiver::Tcp(stream), None)
        } else {
            let socket = UdpSocket::bind(("0.0.0.0", 0))
                .map_err(|error| format!("bind media socket: {error}"))?;
            socket
                .connect((ip, port))
                .map_err(|error| format!("connect viewer UDP {ip}:{port}: {error}"))?;
            socket
                .set_write_timeout(Some(Duration::from_millis(20)))
                .map_err(|error| format!("configure media socket: {error}"))?;
            prove_udp_reachability(&socket, &token, ip, port)?;
            let input_socket = socket
                .try_clone()
                .map_err(|error| format!("clone Windows input socket: {error}"))?;
            let notice_socket = socket
                .try_clone()
                .map_err(|error| format!("clone Windows notice socket: {error}"))?;
            (
                MediaSender::Udp(Arc::new(socket)),
                MediaReceiver::Udp(input_socket),
                Some(MediaSender::Udp(Arc::new(notice_socket))),
            )
        };

        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
        let mut initial_stats = initial_stats(width, height, fps);
        initial_stats.media_transport = media_transport.into();
        let session = Arc::new(WindowsSession {
            stop: AtomicBool::new(false),
            input_enabled: AtomicBool::new(false),
            force_keyframe: AtomicBool::new(true),
            last_viewer_contact_ms: AtomicU64::new(0),
            stats: Mutex::new(initial_stats),
            injector: Mutex::new(InputInjector::new(monitor.rect)),
            threads: Mutex::new(Vec::new()),
            notice_sender: Mutex::new(notice_sender),
            termination_token: token.clone(),
        });

        let input_session = session.clone();
        let input_token = token.clone();
        let input_sender = media_sender.clone();
        let input_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-input-{handle}"))
            .spawn(move || run_input(media_receiver, input_sender, input_token, input_session))
            .map_err(|error| format!("spawn Windows input receiver: {error}"))?;

        let capture_session = session.clone();
        let capture_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-capture-{handle}"))
            .spawn(move || {
                if let Err(error) = capture::run(
                    monitor,
                    width,
                    height,
                    fps,
                    media_sender,
                    capture_session.clone(),
                ) {
                    let mut stats = capture_session.stats.lock().unwrap();
                    stats.state = "error".into();
                    stats.error = Some(error);
                    capture_session.stop.store(true, Ordering::Release);
                }
            })
            .map_err(|error| format!("spawn Windows capture worker: {error}"))?;
        session
            .threads
            .lock()
            .unwrap()
            .extend([input_thread, capture_thread]);
        self.sessions.lock().unwrap().insert(handle, session);
        Ok(handle)
    }

    fn stop(&self, handle: u32) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(&handle)
            .ok_or_else(|| format!("no such Windows capture handle {handle}"))?;
        // Let a live viewer close its window immediately; a dead one never
        // receives this and the terminal-state GC reaps the session anyway.
        let notice = wire::termination(wire::TERMINATION_STOPPED, session.termination_token());
        let _ = socket_attempt_send(&session, &notice);
        session.stop.store(true, Ordering::Release);
        session.input_enabled.store(false, Ordering::Release);
        session.injector.lock().unwrap().release_all()?;
        let threads = std::mem::take(&mut *session.threads.lock().unwrap());
        for thread in threads {
            let _ = thread.join();
        }
        session.stats.lock().unwrap().state = "stopped".into();
        Ok(())
    }

    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(&handle)
            .map(|session| session.stats.lock().unwrap().clone())
            .ok_or_else(|| format!("no such Windows capture handle {handle}"))
    }

    fn input_permission(&self) -> Result<bool, String> {
        // SendInput has no promptable permission. UIPI is enforced per target:
        // an ordinary process controls equal/lower-integrity applications.
        Ok(true)
    }

    fn request_input_permission(&self) -> Result<bool, String> {
        Ok(true)
    }

    fn set_input_enabled(&self, handle: u32, enabled: bool) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&handle)
            .cloned()
            .ok_or_else(|| format!("no such Windows capture handle {handle}"))?;
        session.input_enabled.store(enabled, Ordering::Release);
        if !enabled {
            session.injector.lock().unwrap().release_all()?;
        }
        Ok(())
    }
}

fn prove_udp_reachability(
    socket: &UdpSocket,
    token: &[u8],
    ip: &str,
    port: u16,
) -> Result<(), String> {
    let challenge = wire::challenge(token);
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|error| format!("configure reachability timeout: {error}"))?;
    let mut response = [0u8; 256];
    for attempt in 0..60 {
        if attempt % 4 == 0 {
            let _ = socket.send(&challenge);
        }
        match socket.recv(&mut response) {
            Ok(size) if response[..size] == challenge => {
                socket
                    .set_read_timeout(Some(Duration::from_millis(50)))
                    .map_err(|error| format!("configure input timeout: {error}"))?;
                return Ok(());
            }
            Ok(_) => {}
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

fn prove_tcp_reachability(stream: &TcpStream, token: &[u8]) -> Result<(), String> {
    let challenge = wire::challenge(token);
    let mut writer = stream
        .try_clone()
        .map_err(|error| format!("clone USB challenge writer: {error}"))?;
    write_tcp_frame(&mut writer, &challenge)
        .map_err(|error| format!("send USB media reachability proof: {error}"))?;
    for _ in 0..20 {
        match read_tcp_frame(
            &mut stream
                .try_clone()
                .map_err(|error| format!("clone USB challenge reader: {error}"))?,
        ) {
            Ok(Some(response)) if response == challenge => return Ok(()),
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(format!("read USB media reachability proof: {error}")),
        }
    }
    Err("USB media reachability proof failed".into())
}

fn write_tcp_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    if payload.is_empty() || payload.len() > MAX_TCP_MEDIA_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "TCP media frame is outside the bounded range",
        ));
    }
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)
}

fn read_tcp_frame(stream: &mut TcpStream) -> std::io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_TCP_MEDIA_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid TCP media frame length",
        ));
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload)?;
    Ok(Some(payload))
}

fn run_input(
    receiver: MediaReceiver,
    sender: MediaSender,
    token: Vec<u8>,
    session: Arc<WindowsSession>,
) {
    match receiver {
        MediaReceiver::Udp(socket) => run_udp_input(socket, sender, token, session),
        MediaReceiver::Tcp(stream) => run_tcp_input(stream, sender, token, session),
    }
}

fn run_udp_input(
    socket: UdpSocket,
    sender: MediaSender,
    token: Vec<u8>,
    session: Arc<WindowsSession>,
) {
    let mut sequencer = InputSequencer::default();
    sequencer.reset();
    let mut packet = [0u8; 512];
    let started = std::time::Instant::now();
    while !session.stop.load(Ordering::Acquire) {
        let size = match socket.recv(&mut packet) {
            Ok(size) => size,
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
                set_session_error(&session, format!("Windows input receive failed: {error}"));
                break;
            }
        };
        let Some(message) = wire::authenticated(&packet[..size], &token) else {
            continue;
        };
        session
            .last_viewer_contact_ms
            .store(started.elapsed().as_millis() as u64, Ordering::Release);
        if message == b"BYE" {
            session.stop.store(true, Ordering::Release);
            break;
        }
        if message == b"IDR" {
            session.force_keyframe.store(true, Ordering::Release);
            continue;
        }
        match sequencer.accept(message) {
            InputDecision::Ignore => {}
            InputDecision::AckDuplicate(sequence) => {
                let enabled = session.input_enabled.load(Ordering::Acquire);
                let _ = sender.send(&wire::input_ack(sequence, enabled, &token));
            }
            InputDecision::Apply(event) => {
                if session.input_enabled.load(Ordering::Acquire) {
                    if let Err(error) = session.injector.lock().unwrap().apply(event) {
                        set_session_error(&session, error);
                    }
                }
            }
            InputDecision::ApplyAndAck { sequence, event } => {
                if session.input_enabled.load(Ordering::Acquire) {
                    if let Err(error) = session.injector.lock().unwrap().apply(event) {
                        set_session_error(&session, error);
                    }
                }
                let enabled = session.input_enabled.load(Ordering::Acquire);
                let _ = sender.send(&wire::input_ack(sequence, enabled, &token));
            }
        }
    }
    let _ = session.injector.lock().unwrap().release_all();
}

fn run_tcp_input(
    mut stream: TcpStream,
    sender: MediaSender,
    token: Vec<u8>,
    session: Arc<WindowsSession>,
) {
    let mut sequencer = InputSequencer::default();
    sequencer.reset();
    let started = std::time::Instant::now();
    while !session.stop.load(Ordering::Acquire) {
        let packet = match read_tcp_frame(&mut stream) {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
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
                set_session_error(
                    &session,
                    format!("Windows USB input receive failed: {error}"),
                );
                break;
            }
        };
        process_input_packet(&packet, &token, &session, &mut sequencer, &sender, started);
    }
    let _ = session.injector.lock().unwrap().release_all();
}

fn process_input_packet(
    packet: &[u8],
    token: &[u8],
    session: &Arc<WindowsSession>,
    sequencer: &mut InputSequencer,
    sender: &MediaSender,
    started: std::time::Instant,
) {
    let Some(message) = wire::authenticated(packet, token) else {
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
        return;
    }
    match sequencer.accept(message) {
        InputDecision::Ignore => {}
        InputDecision::AckDuplicate(sequence) => {
            let enabled = session.input_enabled.load(Ordering::Acquire);
            let _ = sender.send(&wire::input_ack(sequence, enabled, token));
        }
        InputDecision::Apply(event) => {
            if session.input_enabled.load(Ordering::Acquire) {
                if let Err(error) = session.injector.lock().unwrap().apply(event) {
                    set_session_error(session, error);
                }
            }
        }
        InputDecision::ApplyAndAck { sequence, event } => {
            if session.input_enabled.load(Ordering::Acquire) {
                if let Err(error) = session.injector.lock().unwrap().apply(event) {
                    set_session_error(session, error);
                }
            }
            let enabled = session.input_enabled.load(Ordering::Acquire);
            let _ = sender.send(&wire::input_ack(sequence, enabled, token));
        }
    }
}

pub(super) fn set_session_error(session: &WindowsSession, error: String) {
    session.stats.lock().unwrap().error = Some(error);
}

/// Terminate a running session whose viewer stopped emitting authenticated
/// traffic. Sends an LCT1 notice first so a still-live viewer closes its
/// window promptly, then flips the session to `error` so the control plane's
/// terminal-state GC releases the capture threads.
fn check_viewer_health(session: &Arc<WindowsSession>, started: std::time::Instant) {
    let last = session.last_viewer_contact_ms.load(Ordering::Acquire);
    let now_ms = started.elapsed().as_millis() as u64;
    if now_ms.saturating_sub(last) < VIEWER_CONTACT_TIMEOUT.as_millis() as u64 {
        return;
    }
    if session.stop.swap(true, Ordering::AcqRel) {
        return;
    }
    let notice = wire::termination(wire::TERMINATION_HEALTH, session.termination_token());
    // Two attempts: the loss that killed the feedback stream may also take
    // the notice itself.
    let _ = socket_attempt_send(session, &notice);
    std::thread::sleep(Duration::from_millis(20));
    let _ = socket_attempt_send(session, &notice);
    {
        let mut stats = session.stats.lock().unwrap();
        stats.state = "error".into();
        stats.error = Some("viewer connection lost (feedback timeout)".into());
    }
    let _ = session.injector.lock().unwrap().release_all();
}

fn socket_attempt_send(session: &Arc<WindowsSession>, notice: &[u8]) -> std::io::Result<()> {
    let sender = session.notice_sender.lock().unwrap();
    match sender.as_ref() {
        Some(sender) => sender.send(notice).map(|_| ()),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "no notice socket",
        )),
    }
}

fn initial_stats(width: u32, height: u32, fps: u32) -> StatsInfo {
    let bitrate =
        ((width as u64 * height as u64 * fps as u64) / 10).clamp(4_000_000, 50_000_000) as u32;
    StatsInfo {
        frames: 0,
        bytes: 0,
        state: "starting_capture".into(),
        fps: 0,
        kbps: 0,
        fps_target: fps,
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
        capture_backend: "windowsGraphicsCapture".into(),
        media_transport: "udp".into(),
        first_capture_ms: 0,
        first_encode_ms: 0,
        first_send_ms: 0,
        current_bitrate: bitrate,
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
    }
}

fn monitors() -> Result<Vec<Monitor>, String> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let output = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut MONITORINFO) }.as_bool()
        {
            let length = info
                .szDevice
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(info.szDevice.len());
            output.push(Monitor {
                handle: monitor.0 as isize,
                rect: info.monitorInfo.rcMonitor,
                name: String::from_utf16_lossy(&info.szDevice[..length]),
            });
        }
        BOOL(1)
    }

    let mut output: Vec<Monitor> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut output as *mut Vec<Monitor> as isize),
        )
    };
    if !ok.as_bool() {
        return Err("EnumDisplayMonitors failed".into());
    }
    output.sort_by_key(|monitor| (monitor.rect.left, monitor.rect.top));
    if output.is_empty() {
        return Err("Windows reported no active display monitors".into());
    }
    Ok(output)
}
