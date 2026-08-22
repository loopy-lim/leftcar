mod capture;
mod input;

use crate::backend::CaptureBackend;
use crate::wire::{self, InputDecision, InputSequencer};
use control_contract::host::{CaptureBackendInfo, DisplayInfo, StatsInfo};
use input::InputInjector;
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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

pub struct WindowsBackend {
    next_handle: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<WindowsSession>>>,
}

pub(super) struct WindowsSession {
    stop: AtomicBool,
    input_enabled: AtomicBool,
    force_keyframe: AtomicBool,
    pub stats: Mutex<StatsInfo>,
    injector: Mutex<InputInjector>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

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
    ) -> Result<u32, String> {
        if capture_backend != "windowsGraphicsCapture" {
            return Err(format!(
                "unsupported Windows capture backend: {capture_backend}"
            ));
        }
        let monitor = monitors()?
            .get(source_index as usize)
            .cloned()
            .ok_or_else(|| format!("display index {source_index} no longer exists"))?;
        let socket = UdpSocket::bind(("0.0.0.0", 0))
            .map_err(|error| format!("bind media socket: {error}"))?;
        socket
            .connect((ip, port))
            .map_err(|error| format!("connect viewer UDP {ip}:{port}: {error}"))?;
        socket
            .set_write_timeout(Some(Duration::from_millis(20)))
            .map_err(|error| format!("configure media socket: {error}"))?;
        let token = uuid::Uuid::new_v4().to_string().into_bytes();
        prove_udp_reachability(&socket, &token, ip, port)?;

        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed).max(1);
        let session = Arc::new(WindowsSession {
            stop: AtomicBool::new(false),
            input_enabled: AtomicBool::new(false),
            force_keyframe: AtomicBool::new(true),
            stats: Mutex::new(initial_stats(width, height, fps)),
            injector: Mutex::new(InputInjector::new(monitor.rect)),
            threads: Mutex::new(Vec::new()),
        });

        let input_socket = socket
            .try_clone()
            .map_err(|error| format!("clone input socket: {error}"))?;
        let input_session = session.clone();
        let input_token = token.clone();
        let input_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-input-{handle}"))
            .spawn(move || run_input(input_socket, input_token, input_session))
            .map_err(|error| format!("spawn Windows input receiver: {error}"))?;

        let capture_session = session.clone();
        let capture_thread = std::thread::Builder::new()
            .name(format!("leftcar-windows-capture-{handle}"))
            .spawn(move || {
                if let Err(error) =
                    capture::run(monitor, width, height, fps, socket, capture_session.clone())
                {
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

fn run_input(socket: UdpSocket, token: Vec<u8>, session: Arc<WindowsSession>) {
    let mut sequencer = InputSequencer::default();
    sequencer.reset();
    let mut packet = [0u8; 512];
    while !session.stop.load(Ordering::Acquire) {
        let size = match socket.recv(&mut packet) {
            Ok(size) => size,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(error) => {
                set_session_error(&session, format!("Windows input receive failed: {error}"));
                break;
            }
        };
        let Some(message) = wire::authenticated(&packet[..size], &token) else {
            continue;
        };
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
                let _ = socket.send(&wire::input_ack(sequence, &token));
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
                let _ = socket.send(&wire::input_ack(sequence, &token));
            }
        }
    }
    let _ = session.injector.lock().unwrap().release_all();
}

pub(super) fn set_session_error(session: &WindowsSession, error: String) {
    session.stats.lock().unwrap().error = Some(error);
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
        capture_backend: "windowsGraphicsCapture".into(),
        media_transport: "udp".into(),
        first_capture_ms: 0,
        first_encode_ms: 0,
        first_send_ms: 0,
        current_bitrate: bitrate,
        capture_interval_p95_us: 0,
        capture_to_encode_p95_us: 0,
        capture_queue_wait_p95_us: 0,
        encode_output_p95_us: 0,
        send_block_p95_us: 0,
        error: None,
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
