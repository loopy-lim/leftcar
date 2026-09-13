mod capture;
mod input;

use crate::backend::CaptureBackend;
use crate::wire::{self, InputDecision, InputSequencer};
use control_contract::host::{CaptureBackendInfo, DisplayInfo, EncoderExperiment, StatsInfo};
use control_contract::udp_stability::AppliedUdpStability;
use input::InputInjector;
use secure_channel::DatagramSealer;
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
    pub source_id: Option<String>,
    pub handle: isize,
    pub rect: RECT,
    pub name: String,
}

const MAX_TCP_MEDIA_FRAME: usize = 16 * 1024 * 1024;

/// 스레드 생존 중 절전을 막는 SetThreadExecutionState 가드. 스트리밍 중
/// 시스템·디스플레이가 유휴 절전에 들어가면 캡처가 끊긴다 — 세션 스레드
/// 시작 시 잡고 끝나면 원복한다(SetThreadExecutionState는 스레드 단위).
struct ExecutionGuard;

impl ExecutionGuard {
    fn acquire() -> ExecutionGuard {
        use windows::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
            EXECUTION_STATE,
        };
        unsafe {
            SetThreadExecutionState(EXECUTION_STATE(
                ES_CONTINUOUS.0 | ES_SYSTEM_REQUIRED.0 | ES_DISPLAY_REQUIRED.0,
            ));
        }
        ExecutionGuard
    }
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[derive(Clone)]
pub(super) enum MediaSocket {
    Udp(Arc<UdpSocket>),
    Tcp(Arc<Mutex<TcpStream>>),
}

/// Every host → viewer media send is sealed here — the lowest-level sender —
/// so capture output, input acks, and termination notices share one AEAD
/// boundary. TCP keeps its plaintext 4-byte length prefix; only the payload
/// is sealed.
#[derive(Clone)]
pub(super) struct MediaSender {
    socket: MediaSocket,
    /// s2c sealer under the HKDF-derived directional key — the raw session
    /// key is never used directly, so the two directions can never collide
    /// on a (key, nonce) pair. Counters start at a random point per instance
    /// (secure-channel), which also keeps the main sender and this notice
    /// sender from sharing a counter range.
    tx: Arc<DatagramSealer>,
    access: Option<Arc<crate::source_grants::SourceLease>>,
}

impl MediaSender {
    pub(super) fn new(socket: MediaSocket, media_key: &[u8; 32]) -> Self {
        let keys = secure_channel::media_keys(media_key);
        Self {
            socket,
            tx: Arc::new(DatagramSealer::new(keys.s2c)),
            access: None,
        }
    }

    fn with_access(mut self, access: &crate::source_grants::CaptureAccess) -> Self {
        self.access = Some(access.lease.clone());
        self
    }

    fn is_tcp(&self) -> bool {
        matches!(self.socket, MediaSocket::Tcp(_))
    }

    fn prepare(&self, packet: &[u8]) -> std::io::Result<Vec<u8>> {
        wire::seal_media_packet(&self.tx, packet, self.is_tcp())
    }

    /// One ordinary datagram, or one existing length-prefixed TCP frame.
    /// No GSO, scatter/gather-as-batch or uncertain automatic resend.
    fn submit_sealed(&self, envelope: &[u8]) -> std::io::Result<usize> {
        let _permission = self
            .access
            .as_ref()
            .map(|lease| {
                lease.enter().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "source authorization revoked",
                    )
                })
            })
            .transpose()?;
        match &self.socket {
            MediaSocket::Udp(socket) => socket.send(envelope),
            MediaSocket::Tcp(stream) => stream
                .lock()
                .map_err(|_| std::io::Error::other("TCP media writer lock poisoned"))?
                .write_all(envelope)
                .map(|_| envelope.len()),
        }
    }

    /// Input acknowledgements/notices retain the plaintext return contract.
    fn send(&self, packet: &[u8]) -> std::io::Result<usize> {
        let envelope = self.prepare(packet)?;
        let submitted = self.submit_sealed(&envelope)?;
        wire::complete_plaintext_send(packet.len(), envelope.len(), submitted)
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
    access: crate::source_grants::CaptureAccess,
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
    /// c2s opener for every viewer datagram (input, feedback, probes, BYE).
    /// Same key as the sender's sealer; only the replay window is separate.
    crypto_rx: DatagramSealer,
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
        if let Some(sender) = session.notice_sender.lock().unwrap().as_ref() {
            let _ = sender.send(&wire::termination(reason_code));
        }
        session.stop.store(true, Ordering::Release);
        session.input_enabled.store(false, Ordering::Release);
        let released_input = session.injector.lock().unwrap().release_all();
        let threads = std::mem::take(&mut *session.threads.lock().unwrap());
        for thread in threads {
            let _ = thread.join();
        }
        session.stats.lock().unwrap().state = "stopped".into();
        released_input
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
                    source_id: monitor.source_id.clone(),
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
        if !matches!(media_transport, "udp" | "usb") {
            return Err("Windows backend supports Wi-Fi UDP or USB AOAP media".into());
        }
        if capture_backend != "windowsGraphicsCapture" {
            return Err(format!(
                "unsupported Windows capture backend: {capture_backend}"
            ));
        }
        let access = access.ok_or("Host source authorization is required")?;
        let _admission = access.lease.enter().ok_or("source authorization revoked")?;
        let monitor = select_monitor(monitors()?, &access.source_id)?;
        // Random LCH1 nonce: freshness for the reachability proof. The media
        // key — possession of it — is the actual authentication. The receive
        // side opens viewer frames under the derived c2s key.
        let mut nonce = [0u8; 32];
        secure_channel::random_bytes(&mut nonce);
        let challenge = wire::challenge(&nonce);
        let crypto_rx = DatagramSealer::new(secure_channel::media_keys(media_key).c2s);
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
            let sender = MediaSender::new(
                MediaSocket::Tcp(Arc::new(Mutex::new(
                    stream
                        .try_clone()
                        .map_err(|error| format!("clone USB media writer: {error}"))?,
                ))),
                media_key,
            )
            .with_access(access);
            prove_tcp_reachability(&stream, &sender, &crypto_rx, &challenge)?;
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
            let socket = Arc::new(socket);
            let sender =
                MediaSender::new(MediaSocket::Udp(socket.clone()), media_key).with_access(access);
            prove_udp_reachability(&socket, &sender, &crypto_rx, &challenge, ip, port)?;
            let input_socket = socket
                .try_clone()
                .map_err(|error| format!("clone Windows input socket: {error}"))?;
            let notice_socket = socket
                .try_clone()
                .map_err(|error| format!("clone Windows notice socket: {error}"))?;
            (
                sender,
                MediaReceiver::Udp(input_socket),
                Some(
                    MediaSender::new(MediaSocket::Udp(Arc::new(notice_socket)), media_key)
                        .with_access(access),
                ),
            )
        };

        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
        let mut initial_stats = initial_stats(width, height, fps);
        initial_stats.media_transport = media_transport.into();
        let session = Arc::new(WindowsSession {
            access: access.clone(),
            stop: AtomicBool::new(false),
            input_enabled: AtomicBool::new(false),
            force_keyframe: AtomicBool::new(true),
            last_viewer_contact_ms: AtomicU64::new(0),
            stats: Mutex::new(initial_stats),
            injector: Mutex::new(InputInjector::new(monitor.rect)),
            threads: Mutex::new(Vec::new()),
            notice_sender: Mutex::new(notice_sender),
            crypto_rx,
        });

        let input_session = session.clone();
        let input_sender = media_sender.clone();
        let input_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-input-{handle}"))
            .spawn(move || run_input(media_receiver, input_sender, input_session))
            .map_err(|error| format!("spawn Windows input receiver: {error}"))?;

        let capture_session = session.clone();
        let capture_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-capture-{handle}"))
            .spawn(move || {
                let _execution = ExecutionGuard::acquire();
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
        if let Some(sender) = session.notice_sender.lock().unwrap().as_ref() {
            let _ = sender.send(&wire::termination(wire::TERMINATION_STOPPED));
        }
        session.stop.store(true, Ordering::Release);
        session.input_enabled.store(false, Ordering::Release);
        let released_input = session.injector.lock().unwrap().release_all();
        let threads = std::mem::take(&mut *session.threads.lock().unwrap());
        for thread in threads {
            let _ = thread.join();
        }
        session.stats.lock().unwrap().state = "stopped".into();
        released_input
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
        session.input_enabled.store(enabled, Ordering::Release);
        if !enabled {
            session.injector.lock().unwrap().release_all()?;
        }
        Ok(())
    }
}

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
    // Sealed challenge + 24B AEAD overhead; genuine replies fit comfortably.
    let mut response = [0u8; 256];
    for attempt in 0..60 {
        if attempt % 4 == 0 {
            let _ = sender.send(challenge);
        }
        match socket.recv(&mut response) {
            Ok(size) => {
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

fn prove_tcp_reachability(
    stream: &TcpStream,
    sender: &MediaSender,
    crypto_rx: &DatagramSealer,
    challenge: &[u8],
) -> Result<(), String> {
    sender
        .send(challenge)
        .map_err(|error| format!("send USB media reachability proof: {error}"))?;
    for _ in 0..20 {
        match read_tcp_frame(
            &mut stream
                .try_clone()
                .map_err(|error| format!("clone USB challenge reader: {error}"))?,
        ) {
            Ok(Some(response)) => {
                if crypto_rx
                    .open(&response)
                    .is_ok_and(|opened| opened == challenge)
                {
                    return Ok(());
                }
            }
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

fn run_input(receiver: MediaReceiver, sender: MediaSender, session: Arc<WindowsSession>) {
    let _execution = ExecutionGuard::acquire();
    match receiver {
        MediaReceiver::Udp(socket) => run_udp_input(socket, sender, session),
        MediaReceiver::Tcp(stream) => run_tcp_input(stream, sender, session),
    }
}

fn run_udp_input(socket: UdpSocket, sender: MediaSender, session: Arc<WindowsSession>) {
    let mut sequencer = InputSequencer::default();
    sequencer.reset();
    // Sealed input datagrams carry +24B AEAD overhead over the largest
    // plaintext input event.
    let mut packet = [0u8; 640];
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
        // AEAD possession replaces the old token-suffix authentication:
        // anything that does not open under the session key is dropped here.
        let Ok(message) = session.crypto_rx.open(&packet[..size]) else {
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
        let Some(_permission) = session.access.lease.enter() else {
            break;
        };
        match sequencer.accept(&message) {
            InputDecision::Ignore => {}
            InputDecision::AckDuplicate(sequence) => {
                let enabled = session.input_enabled.load(Ordering::Acquire);
                let _ = sender.send(&wire::input_ack(sequence, enabled));
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
                let _ = sender.send(&wire::input_ack(sequence, enabled));
            }
        }
    }
    let _ = session.injector.lock().unwrap().release_all();
}

fn run_tcp_input(mut stream: TcpStream, sender: MediaSender, session: Arc<WindowsSession>) {
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
        process_input_packet(&packet, &session, &mut sequencer, &sender, started);
    }
    let _ = session.injector.lock().unwrap().release_all();
}

fn process_input_packet(
    packet: &[u8],
    session: &Arc<WindowsSession>,
    sequencer: &mut InputSequencer,
    sender: &MediaSender,
    started: std::time::Instant,
) {
    let Some(_permission) = session.access.lease.enter() else {
        return;
    };
    let Ok(message) = session.crypto_rx.open(packet) else {
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
    match sequencer.accept(&message) {
        InputDecision::Ignore => {}
        InputDecision::AckDuplicate(sequence) => {
            let enabled = session.input_enabled.load(Ordering::Acquire);
            let _ = sender.send(&wire::input_ack(sequence, enabled));
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
            let _ = sender.send(&wire::input_ack(sequence, enabled));
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
    let notice = wire::termination(wire::TERMINATION_HEALTH);
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
        ..StatsInfo::default()
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
                source_id: None,
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
    let identities = active_monitor_identities()?;
    for monitor in &mut output {
        let matches: Vec<_> = identities
            .iter()
            .filter(|(name, _)| name == &monitor.name)
            .collect();
        if let [(_, source)] = matches.as_slice() {
            monitor.source_id = Some(source.clone());
        }
    }
    output.sort_by_key(|monitor| (monitor.rect.left, monitor.rect.top));
    if output.is_empty() {
        return Err("Windows reported no active display monitors".into());
    }
    Ok(output)
}

fn select_monitor(monitors: Vec<Monitor>, source: &str) -> Result<Monitor, String> {
    let mut matches = monitors
        .into_iter()
        .filter(|monitor| monitor.source_id.as_deref() == Some(source));
    let monitor = matches.next().ok_or("source display is unavailable")?;
    if source.is_empty() || matches.next().is_some() {
        return Err("source display identity is ambiguous".into());
    }
    Ok(monitor)
}

/// Join active display configuration source names to target interface paths.
/// Mirrored/multiple targets for one source are intentionally ungrantable.
fn active_monitor_identities() -> Result<Vec<(String, String)>, String> {
    use windows::Win32::Devices::Display::*;
    let mut path_count = 0;
    let mut mode_count = 0;
    let status = unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    };
    if status.0 != 0 {
        return Err(format!("GetDisplayConfigBufferSizes: {}", status.0));
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    let status = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if status.0 != 0 {
        return Err(format!("QueryDisplayConfig: {}", status.0));
    }
    let string = |units: &[u16]| {
        String::from_utf16_lossy(
            &units[..units.iter().position(|v| *v == 0).unwrap_or(units.len())],
        )
    };
    let mut output = Vec::new();
    for path in paths.into_iter().take(path_count as usize) {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = std::mem::size_of_val(&source) as u32;
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        target.header.size = std::mem::size_of_val(&target) as u32;
        target.header.adapterId = path.targetInfo.adapterId;
        target.header.id = path.targetInfo.id;
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0
            || unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0
        {
            continue;
        }
        let name = string(&source.viewGdiDeviceName);
        let id = string(&target.monitorDevicePath);
        if !name.is_empty() && !id.is_empty() {
            output.push((name, format!("windows:display:{}", id.to_lowercase())));
        }
    }
    Ok(output)
}
