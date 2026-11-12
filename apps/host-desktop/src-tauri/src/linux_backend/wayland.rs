//! Wayland glue for the Linux Host: output enumeration, screen capture via
//! wlr-screencopy, and remote input injection via wlr virtual pointer /
//! keyboard. Each area owns its own compositor connection; multiple
//! connections from one process are ordinary Wayland clients (grim, wtype).
//!
//! Everything here fails with descriptive strings; the backend turns them
//! into session errors or startup errors (lib.rs fatal path).

use std::collections::HashMap;
use std::os::fd::{AsFd as _, AsRawFd as _, FromRawFd as _, OwnedFd};
use std::time::{Duration, Instant};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::{
    self, ZwlrVirtualPointerV1,
};

// zwlr_virtual_keyboard is not in wayland-protocols yet; generate bindings
// from the vendored upstream XML (wlroots MIT license, see protocols/).
pub mod virtual_keyboard {
    use wayland_client;
    use wayland_client::protocol::*;

    #[doc(hidden)]
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/wlr-virtual-keyboard-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/wlr-virtual-keyboard-unstable-v1.xml");
}
use virtual_keyboard::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use virtual_keyboard::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// What the current compositor advertises. Missing screencopy is fatal for
/// the backend; missing virtual input degrades to input-off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GlobalAvailability {
    pub outputs: usize,
    pub seats: usize,
    pub screencopy: bool,
    pub virtual_pointer: bool,
    pub virtual_keyboard: bool,
}

impl GlobalAvailability {
    pub(crate) fn supports_capture(&self) -> bool {
        self.outputs > 0 && self.screencopy
    }

    pub(crate) fn supports_input(&self) -> bool {
        self.seats > 0 && self.virtual_pointer && self.virtual_keyboard
    }
}

/// Probe the current compositor once: what can this host do?
pub(crate) fn probe_globals() -> Result<GlobalAvailability, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<RegistryOnly>(&conn).map_err(|e| format!("Wayland registry: {e}"))?;
    let mut state = RegistryOnly;
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    let mut availability = GlobalAvailability::default();
    globals.contents().with_list(|list| {
        for global in list {
            match global.interface.as_str() {
                "wl_output" => availability.outputs += 1,
                "wl_seat" => availability.seats += 1,
                "zwlr_screencopy_manager_v1" => availability.screencopy = true,
                "zwlr_virtual_pointer_manager_v1" => availability.virtual_pointer = true,
                "zwp_virtual_keyboard_manager_v1" => availability.virtual_keyboard = true,
                _ => {}
            }
        }
    });
    Ok(availability)
}

struct RegistryOnly;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for RegistryOnly {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// ---------------------------------------------------------------------------
// Output enumeration
// ---------------------------------------------------------------------------

/// One Wayland output, in compositor-global coordinates (wl_output geometry).
#[derive(Debug, Clone)]
pub(crate) struct OutputInfo {
    /// wl_output v4 name, e.g. "HDMI-A-3"; the source_id grant key derives
    /// from it. Without v4 the name is a non-stable placeholder.
    pub name: String,
    /// Whether `name` came from the wl_output v4 name event (stable across
    /// sessions and therefore grantable).
    pub stable_name: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Scan outputs with a short-lived connection. Stable names need wl_output
/// v4 (Hyprland ships v4; older compositors get the "output" placeholder and
/// their outputs stay ungrantable, mirroring the Windows mirrored-monitor
/// rule).
pub(crate) fn list_outputs() -> Result<Vec<OutputInfo>, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<ScanState>(&conn).map_err(|e| format!("Wayland registry: {e}"))?;
    let mut state = ScanState::default();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;

    // Bind AFTER with_list: GlobalList::bind locks the same globals mutex that
    // with_list holds, so binding inside the closure self-deadlocks.
    let output_globals = globals.contents().with_list(|list| {
        list.iter()
            .filter(|global| global.interface == "wl_output")
            .count()
    });
    for _ in 0..output_globals {
        let _ = globals.bind::<wl_output::WlOutput, ScanState, _>(&queue.handle(), 1..=4, ());
    }
    // Two roundtrips: bind delivery, then geometry/mode/name + Done.
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;

    if state.done.is_empty() {
        return Err("no Wayland outputs are advertised".into());
    }
    let mut infos: Vec<OutputInfo> = state
        .done
        .values()
        .map(|scan| OutputInfo {
            name: scan.name.clone().unwrap_or_else(|| "output".into()),
            stable_name: scan.name.is_some(),
            x: scan.x,
            y: scan.y,
            width: scan.width,
            height: scan.height,
        })
        .collect();
    infos.sort_by_key(|info| (info.x, info.y));
    Ok(infos)
}

#[derive(Default, Clone)]
struct OutputScan {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    name: Option<String>,
}

#[derive(Default)]
struct ScanState {
    /// Per-output snapshot, keyed by wl_output protocol id, updated on Done.
    done: HashMap<u32, OutputScan>,
    /// In-progress events before the roundtrip's Done.
    pending: HashMap<u32, OutputScan>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ScanState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

fn output_event(scan: &mut OutputScan, event: wl_output::Event) {
    match event {
        wl_output::Event::Geometry { x, y, .. } => {
            scan.x = x;
            scan.y = y;
        }
        wl_output::Event::Mode {
            flags,
            width,
            height,
            ..
        } => match flags {
            WEnum::Value(flags) if flags.contains(wl_output::Mode::Current) => {
                scan.width = width;
                scan.height = height;
            }
            _ => {}
        },
        wl_output::Event::Name { name } => scan.name = Some(name),
        _ => {}
    }
}

impl Dispatch<wl_output::WlOutput, ()> for ScanState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let scan = state.pending.entry(output.id().protocol_id()).or_default();
        if let wl_output::Event::Done = event {
            let scan = scan.clone();
            state.done.insert(output.id().protocol_id(), scan);
        } else {
            output_event(scan, event);
        }
    }
}

// ---------------------------------------------------------------------------
// Screen capture (wlr-screencopy)
// ---------------------------------------------------------------------------

/// A captured output frame in BGRA byte order (wl_shm ARGB8888 is
/// little-endian XRGB, i.e. B,G,R,X in memory).
pub(crate) struct CapturedFrame {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

struct ShmBuffer {
    _fd: OwnedFd,
    map: *mut u8,
    map_len: usize,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: u32,
    height: u32,
    stride: u32,
}

// SAFETY: the mapping is owned exclusively by this struct and only touched
// from the capture thread that owns the ScreenCopier.
unsafe impl Send for ShmBuffer {}

impl ShmBuffer {
    fn create(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<CaptureState>,
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<Self, String> {
        let map_len = stride as usize * height as usize;
        let fd = make_memfd(map_len)?;
        // SAFETY: mmap of a private memfd we just sized; unmapped in Drop.
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err("mmap for capture buffer failed".into());
        }
        let pool = shm.create_pool(fd.as_fd(), map_len as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        Ok(Self {
            _fd: fd,
            map: map.cast(),
            map_len,
            _pool: pool,
            buffer,
            width,
            height,
            stride,
        })
    }

    fn read_out(&self, y_invert: bool) -> Vec<u8> {
        let len = self.map_len;
        // SAFETY: mapped above, valid for map_len bytes.
        let mapped = unsafe { std::slice::from_raw_parts(self.map, len) };
        if y_invert {
            let mut out = mapped.to_vec();
            let stride = self.stride as usize;
            let rows = self.height as usize;
            for row in 0..rows {
                let src = (rows - 1 - row) * stride;
                out[row * stride..(row + 1) * stride].copy_from_slice(&mapped[src..src + stride]);
            }
            out
        } else {
            mapped.to_vec()
        }
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        // SAFETY: paired with the mmap in create().
        unsafe {
            let _ = libc::munmap(self.map.cast(), self.map_len);
        }
    }
}

fn make_memfd(size: usize) -> Result<OwnedFd, String> {
    let name = std::ffi::CString::new("leftcar-capture").map_err(|e| e.to_string())?;
    // SAFETY: memfd_create with a C name; the fd is owned and closed on drop.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), 0) };
    if fd < 0 {
        return Err("memfd_create failed".into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe { libc::ftruncate(fd.as_raw_fd(), size as libc::off_t) } != 0 {
        return Err("ftruncate for capture buffer failed".into());
    }
    Ok(fd)
}

#[derive(Default)]
struct CaptureState {
    // (unsafe impl Send below: ShmBuffer owns a raw mapping used only from
    // the thread that owns the ScreenCopier.)
    manager: Option<ZwlrScreencopyManagerV1>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    outputs: HashMap<u32, wl_output::WlOutput>,
    output_scans: HashMap<u32, OutputScan>,
    output_pending: HashMap<u32, OutputScan>,
    /// The resolved capture target.
    target: Option<wl_output::WlOutput>,
    buffer: Option<ShmBuffer>,
    buffer_meta: Option<(u32, u32, u32)>,
    ready: Option<Result<CapturedFrame, String>>,
    failed: Option<String>,
    y_invert: bool,
}

// SAFETY: the raw mapping in ShmBuffer never leaves the capture thread that
// owns the ScreenCopier; the compositor shares the contents via the memfd.
unsafe impl Send for CaptureState {}

pub(crate) struct ScreenCopier {
    conn: Connection,
    queue: wayland_client::EventQueue<CaptureState>,
    state: CaptureState,
}

impl ScreenCopier {
    /// Bind globals and resolve `output_name` as the capture target.
    pub(crate) fn new(output_name: &str) -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("Wayland connect: {e}"))?;
        let (globals, queue) = registry_queue_init::<CaptureState>(&conn)
            .map_err(|e| format!("Wayland registry: {e}"))?;
        let mut state = CaptureState::default();
        let qh = queue.handle();
        // Collect what to bind first (the with_list borrow ends), then bind.
        #[derive(Clone, Copy, PartialEq)]
        enum Bind {
            Shm,
            Screencopy,
            Seat,
            Output,
        }
        let mut plan: Vec<Bind> = Vec::new();
        globals.contents().with_list(|list| {
            for global in list {
                let kind = match global.interface.as_str() {
                    "wl_shm" => Some(Bind::Shm),
                    "zwlr_screencopy_manager_v1" => Some(Bind::Screencopy),
                    "wl_seat" => Some(Bind::Seat),
                    "wl_output" => Some(Bind::Output),
                    _ => None,
                };
                if let Some(kind) = kind {
                    if !plan.contains(&kind) || kind == Bind::Output {
                        plan.push(kind);
                    }
                }
            }
        });
        for kind in plan {
            match kind {
                Bind::Shm => {
                    state.shm = Some(globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?);
                }
                Bind::Screencopy => {
                    state.manager = Some(globals.bind(&qh, 1..=3, ()).map_err(|e| e.to_string())?);
                }
                Bind::Seat => {
                    state.seat = Some(
                        globals
                            .bind::<wl_seat::WlSeat, CaptureState, _>(&qh, 1..=1, ())
                            .map_err(|e| e.to_string())?,
                    );
                }
                Bind::Output => {
                    let output = globals
                        .bind::<wl_output::WlOutput, CaptureState, _>(&qh, 1..=4, ())
                        .map_err(|e| e.to_string())?;
                    state.outputs.insert(output.id().protocol_id(), output);
                }
            }
        }
        if state.manager.is_none() {
            return Err("compositor does not advertise zwlr_screencopy_manager_v1 (a wlroots-based compositor like Hyprland is required)".into());
        }
        if state.shm.is_none() {
            return Err("compositor does not advertise wl_shm".into());
        }

        let mut copier = Self { conn, queue, state };
        copier.roundtrip()?;
        copier.roundtrip()?;
        copier.resolve_target(output_name)?;
        Ok(copier)
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn resolve_target(&mut self, name: &str) -> Result<(), String> {
        let matches: Vec<u32> = self
            .state
            .output_scans
            .iter()
            .filter(|(_, scan)| scan.name.as_deref() == Some(name))
            .map(|(pid, _)| *pid)
            .collect();
        let pid = match matches.len() {
            1 => matches[0],
            0 => {
                let known: Vec<String> = self
                    .state
                    .output_scans
                    .values()
                    .filter_map(|scan| scan.name.clone())
                    .collect();
                return Err(format!("output {name} not found (present: {known:?})"));
            }
            _ => return Err(format!("output name {name} is ambiguous")),
        };
        let target = self
            .state
            .outputs
            .get(&pid)
            .cloned()
            .ok_or("resolved output lost its proxy")?;
        self.state.target = Some(target);
        Ok(())
    }

    /// Capture one frame. Blocks until the compositor delivers it (typically
    /// a few ms at 1080p) or a deadline passes.
    pub(crate) fn capture(&mut self) -> Result<CapturedFrame, String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        self.state.ready = None;
        self.state.buffer_meta = None;
        self.state.failed = None;
        self.state.y_invert = false;

        let manager = self
            .state
            .manager
            .clone()
            .ok_or("screencopy manager disappeared")?;
        let output = self.state.target.clone().ok_or("no capture target")?;
        // overlay_cursor = 0: the viewer draws its own pointer; the macOS and
        // Windows paths likewise stream content without a baked-in cursor.
        let frame = manager.capture_output(0, &output, &self.queue.handle(), ());
        // Requests sit in the outbound buffer until an explicit flush; without
        // this the compositor never sees the capture request.
        self.conn.flush().map_err(|e| e.to_string())?;

        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|e| e.to_string())?;
            if let Some(result) = self.state.ready.take() {
                return result;
            }
            if let Some(error) = self.state.failed.take() {
                return Err(error);
            }
            // The frame's buffer event names the required dimensions; allocate
            // once and submit the copy request for exactly one buffer.
            if let Some((width, height, stride)) = self.state.buffer_meta.take() {
                let reallocate = !matches!(&self.state.buffer, Some(buffer) if
                    buffer.width == width && buffer.height == height && buffer.stride == stride);
                if reallocate {
                    let shm = self.state.shm.clone().ok_or("wl_shm disappeared")?;
                    self.state.buffer = Some(ShmBuffer::create(
                        &shm,
                        &self.queue.handle(),
                        width,
                        height,
                        stride,
                    )?);
                }
                let buffer = self
                    .state
                    .buffer
                    .as_ref()
                    .map(|b| &b.buffer)
                    .ok_or("capture buffer missing")?;
                frame.copy(buffer);
                self.conn.flush().map_err(|e| e.to_string())?;
            }
            if Instant::now() >= deadline {
                return Err("screencopy frame did not arrive within 2s".into());
            }
            match self.conn.prepare_read() {
                Some(guard) => {
                    // Bounded wait: a compositor that never answers the copy
                    // must not leave the session thread stuck in read().
                    let fd = guard.connection_fd();
                    let mut poll_fd = libc::pollfd {
                        fd: fd.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let readable = unsafe { libc::poll(&mut poll_fd, 1, 50) };
                    if readable > 0 {
                        guard.read().map_err(|e| format!("Wayland read: {e}"))?;
                    }
                }
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for CaptureState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for CaptureState {
    fn event(
        _state: &mut Self,
        _shm: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for CaptureState {
    fn event(
        _state: &mut Self,
        _pool: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for CaptureState {
    fn event(
        _state: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for CaptureState {
    fn event(
        _state: &mut Self,
        _seat: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for CaptureState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let scan = state
            .output_pending
            .entry(output.id().protocol_id())
            .or_default();
        if let wl_output::Event::Done = event {
            let scan = scan.clone();
            state.output_scans.insert(output.id().protocol_id(), scan);
        } else {
            output_event(scan, event);
        }
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for CaptureState {
    fn event(
        _state: &mut Self,
        _manager: &ZwlrScreencopyManagerV1,
        _event: wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _frame: &ZwlrScreencopyFrameV1,
        event: wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event::Buffer {
                width,
                height,
                stride,
                ..
            } => {
                state.buffer_meta = Some((width, height, stride));
            }
            wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event::Flags {
                flags,
            } => match flags {
                WEnum::Value(flags) => {
                    state.y_invert = flags.contains(
                        wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Flags::YInvert,
                    );
                }
                WEnum::Unknown(_) => {}
            },
            wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                // The copy target's dimensions live in the ShmBuffer; the
                // buffer_metadata slot was already consumed by the allocation
                // loop by the time Ready is processed.
                let result = match &state.buffer {
                    Some(buffer) => {
                        let bgra = buffer.read_out(state.y_invert);
                        Ok(CapturedFrame {
                            bgra,
                            width: buffer.width,
                            height: buffer.height,
                            stride: buffer.stride,
                        })
                    }
                    None => Err("screencopy finished without a buffer".into()),
                };
                state.ready = Some(result);
            }
            wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event::Failed => {
                state.failed = Some("compositor refused the capture (output gone or blocked)".into());
            }
            _ => {}
        }
        let _ = qh;
    }
}

// ---------------------------------------------------------------------------
// Virtual input devices (wlr-virtual-pointer + virtual keyboard)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct InputState {
    seat: Option<wl_seat::WlSeat>,
    pointer_manager: Option<ZwlrVirtualPointerManagerV1>,
    keyboard_manager: Option<ZwpVirtualKeyboardManagerV1>,
    pointer: Option<ZwlrVirtualPointerV1>,
    keyboard: Option<ZwpVirtualKeyboardV1>,
    start: Option<Instant>,
}

/// One connection owning a virtual pointer and a virtual keyboard. Both are
/// created against the first advertised wl_seat.
pub(crate) struct VirtualInput {
    conn: Connection,
    queue: wayland_client::EventQueue<InputState>,
    state: InputState,
}

impl VirtualInput {
    pub(crate) fn new() -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("Wayland connect: {e}"))?;
        let (globals, queue) = registry_queue_init::<InputState>(&conn)
            .map_err(|e| format!("Wayland registry: {e}"))?;
        let mut state = InputState::default();
        let qh = queue.handle();
        // Record what to bind inside with_list and bind after it returns:
        // GlobalList::bind locks the same globals mutex the closure holds.
        let (has_seat, has_pointer, has_keyboard) = globals.contents().with_list(|list| {
            let mut seat = false;
            let mut pointer = false;
            let mut keyboard = false;
            for global in list {
                match global.interface.as_str() {
                    "wl_seat" => seat = true,
                    "zwlr_virtual_pointer_manager_v1" => pointer = true,
                    "zwp_virtual_keyboard_manager_v1" => keyboard = true,
                    _ => {}
                }
            }
            (seat, pointer, keyboard)
        });
        if has_seat {
            state.seat = globals.bind(&qh, 1..=1, ()).ok();
        }
        if has_pointer {
            state.pointer_manager = globals.bind(&qh, 1..=2, ()).ok();
        }
        if has_keyboard {
            state.keyboard_manager = globals.bind(&qh, 1..=1, ()).ok();
        }
        if state.pointer_manager.is_none() || state.keyboard_manager.is_none() {
            return Err(
                "compositor does not advertise virtual input protocols (zwlr_virtual_pointer / zwp_virtual_keyboard)"
                    .into(),
            );
        }
        let Some(seat) = state.seat.clone() else {
            return Err("compositor does not advertise a wl_seat".into());
        };
        state.pointer = Some(
            state
                .pointer_manager
                .as_ref()
                .expect("checked above")
                .create_virtual_pointer(Some(&seat), &qh, ()),
        );
        state.keyboard = Some(
            state
                .keyboard_manager
                .as_ref()
                .expect("checked above")
                .create_virtual_keyboard(&seat, &qh, ()),
        );
        state.start = Some(Instant::now());
        let mut devices = Self { conn, queue, state };
        devices.flush()?;
        Ok(devices)
    }

    fn now_ms(&self) -> u32 {
        self.state
            .start
            .map(|start| start.elapsed().as_millis() as u32)
            .unwrap_or(0)
    }

    fn flush(&mut self) -> Result<(), String> {
        self.conn.flush().map_err(|e| e.to_string())
    }

    /// Dispatch pending events briefly so errors (protocol violations) surface
    /// instead of silently queueing. Non-blocking.
    fn drain(&mut self) -> Result<(), String> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Absolute motion in the given coordinate extents, followed by a frame.
    pub(crate) fn move_absolute(
        &mut self,
        x: u32,
        y: u32,
        extent_w: u32,
        extent_h: u32,
    ) -> Result<(), String> {
        let pointer = self.state.pointer.as_ref().ok_or("virtual pointer gone")?;
        let time = self.now_ms();
        pointer.motion_absolute(time, x, y, extent_w, extent_h);
        pointer.frame();
        self.flush()?;
        self.drain()
    }

    /// Evdev button code (BTN_LEFT 0x110 ...) with pressed state.
    pub(crate) fn button(&mut self, code: u32, pressed: bool) -> Result<(), String> {
        let pointer = self.state.pointer.as_ref().ok_or("virtual pointer gone")?;
        let time = self.now_ms();
        let state = if pressed {
            wayland_client::protocol::wl_pointer::ButtonState::Pressed
        } else {
            wayland_client::protocol::wl_pointer::ButtonState::Released
        };
        pointer.button(time, code, state);
        pointer.frame();
        self.flush()?;
        self.drain()
    }

    /// Wheel motion in lines; positive vertical means "scroll content up"
    /// (the wire's sign convention), Wayland's axis grows downward.
    pub(crate) fn wheel(
        &mut self,
        vertical_lines: f64,
        horizontal_lines: f64,
    ) -> Result<(), String> {
        const PIXELS_PER_LINE: f64 = 40.0;
        let pointer = self.state.pointer.as_ref().ok_or("virtual pointer gone")?;
        let time = self.now_ms();
        if vertical_lines != 0.0 {
            pointer.axis_source(wayland_client::protocol::wl_pointer::AxisSource::Wheel);
            pointer.axis(
                time,
                wayland_client::protocol::wl_pointer::Axis::VerticalScroll,
                -vertical_lines * PIXELS_PER_LINE,
            );
            pointer.frame();
        }
        if horizontal_lines != 0.0 {
            pointer.axis_source(wayland_client::protocol::wl_pointer::AxisSource::Wheel);
            pointer.axis(
                time,
                wayland_client::protocol::wl_pointer::Axis::HorizontalScroll,
                -horizontal_lines * PIXELS_PER_LINE,
            );
            pointer.frame();
        }
        self.flush()?;
        self.drain()
    }

    /// Upload a fresh xkb keymap (xkb_v1 text). Needed before any key event
    /// and whenever dynamically assigned unicode keycodes change.
    pub(crate) fn set_keymap(&mut self, keymap: &str) -> Result<(), String> {
        let keyboard = self
            .state
            .keyboard
            .as_ref()
            .ok_or("virtual keyboard gone")?;
        let bytes = keymap.as_bytes();
        let fd = make_memfd(bytes.len())?;
        let mut written = 0usize;
        while written < bytes.len() {
            let chunk = unsafe {
                libc::write(
                    fd.as_raw_fd(),
                    bytes[written..].as_ptr().cast(),
                    bytes.len() - written,
                )
            };
            if chunk <= 0 {
                return Err("keymap write failed".into());
            }
            written += chunk as usize;
        }
        // wl_keyboard keymap format 1 = xkb_v1; the compositor maps it read-only.
        keyboard.keymap(
            wayland_client::protocol::wl_keyboard::KeymapFormat::XkbV1,
            fd.as_fd(),
            bytes.len() as u32,
        );
        self.flush()?;
        self.drain()
    }

    /// Press or release a key. `evdev_code` is the Linux input.h keycode;
    /// the wire-level XKB keycode is evdev + 8.
    pub(crate) fn key(&mut self, evdev_code: u16, pressed: bool) -> Result<(), String> {
        let keyboard = self
            .state
            .keyboard
            .as_ref()
            .ok_or("virtual keyboard gone")?;
        let time = self.now_ms();
        // The wlr XML types `state` as a plain uint: 1 = pressed, 0 = released
        // (wl_keyboard.key_state values).
        let state: u32 = u32::from(pressed);
        keyboard.key(time, u32::from(evdev_code) + 8, state);
        self.flush()?;
        self.drain()
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for InputState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for InputState {
    fn event(
        _state: &mut Self,
        _seat: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for InputState {
    fn event(
        _state: &mut Self,
        _manager: &ZwlrVirtualPointerManagerV1,
        _event: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for InputState {
    fn event(
        _state: &mut Self,
        _pointer: &ZwlrVirtualPointerV1,
        _event: zwlr_virtual_pointer_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for InputState {
    fn event(
        _state: &mut Self,
        _manager: &ZwpVirtualKeyboardManagerV1,
        _event: virtual_keyboard::zwp_virtual_keyboard_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for InputState {
    fn event(
        _state: &mut Self,
        _keyboard: &ZwpVirtualKeyboardV1,
        _event: virtual_keyboard::zwp_virtual_keyboard_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}
