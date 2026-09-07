//! Virtual display provider abstraction for the tablet display feature.
//!
//! The rest of the pipeline (streaming, input, power) only sees this trait,
//! so the engine can be swapped without touching callers. Existing CLI argv
//! builders in `virtual_display.rs` remain the single source of truth for
//! BetterDisplay contracts.

/// What callers ask a provider to materialize.
pub struct DisplaySpec {
    pub name: String,
    /// Logical dimensions requested by the tablet. `scale` determines the
    /// backing pixel dimensions sent to the provider.
    pub width: u32,
    pub height: u32,
    pub scale: u8,
}

impl DisplaySpec {
    pub fn backing_dimensions(&self) -> Result<(u32, u32), ProviderError> {
        if !matches!(self.scale, 1 | 2) {
            return Err(ProviderError::EngineFailed(
                "HiDPI 배율은 1 또는 2여야 합니다.".into(),
            ));
        }
        let width = self.width.checked_mul(self.scale as u32).ok_or_else(|| {
            ProviderError::EngineFailed("가상 디스플레이 backing 폭이 너무 큽니다.".into())
        })?;
        let height = self.height.checked_mul(self.scale as u32).ok_or_else(|| {
            ProviderError::EngineFailed("가상 디스플레이 backing 높이가 너무 큽니다.".into())
        })?;
        super::virtual_display::validate_dimensions(width, height)
            .map_err(ProviderError::EngineFailed)?;
        Ok((width, height))
    }
}

/// Handle to a display a provider created.
pub struct VirtualDisplay {
    pub name: String,
    /// CGVD engine's numeric display id, keyed to its retained shim process.
    /// None for other engines.
    pub cgvd_display_id: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DisplayInspection {
    pub display_id: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub x: i32,
    pub y: i32,
}

/// Every non-macOS branch answers with this one message so the guidance is
/// identical whichever provider a caller holds. macOS builds cfg-gate every
/// use away, hence the lint escape.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const MACOS_ONLY_MESSAGE: &str = "가상 디스플레이는 macOS에서만 지원됩니다.";

const BUNDLED_SHIM_NAME: &str = "cgvd-shim";

/// Failure causes map 1:1 to UI guidance (spark 3-way classification).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ProviderError {
    /// Zero active displays — macOS cannot create a virtual display.
    /// UI must tell the user to open the lid or wake an external monitor.
    NoActiveDisplay,
    /// Engine missing: BetterDisplay not installed / CGVD shim absent.
    EngineUnavailable(String),
    /// Engine ran and failed (abort, displayID=0, CLI stderr...).
    EngineFailed(String),
}

impl ProviderError {
    pub fn message(&self) -> String {
        match self {
            ProviderError::NoActiveDisplay => {
                "활성 화면이 없습니다. 덮개를 열거나 외장 모니터를 켠 후 시작하세요.".into()
            }
            ProviderError::EngineUnavailable(detail) => {
                format!("가상 디스플레이 엔진을 사용할 수 없습니다: {detail}")
            }
            ProviderError::EngineFailed(detail) => format!("가상 디스플레이 생성 실패: {detail}"),
        }
    }
}

/// Observed result of an in-place mode switch, mirroring the shim's RESIZED
/// line: logical dimensions and the backing pixel dimensions actually reached.
pub struct ResizedDisplayMode {
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// The tablet-display pipeline depends only on this trait.
pub trait VirtualDisplayProvider: Send + Sync {
    fn name(&self) -> &'static str;
    /// True when the engine is installed/reachable. Must not create anything.
    fn available(&self) -> bool;
    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError>;
    fn place(&self, display: &VirtualDisplay, x: i32, y: i32) -> Result<(), ProviderError> {
        #[cfg(target_os = "macos")]
        {
            super::virtual_display::set_placement(&display.name, x, y)
                .map(|_| ())
                .map_err(ProviderError::EngineFailed)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (display, x, y);
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }
    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError>;
    /// Switch the live display's mode to (width, height, scale) without
    /// destroying it. Default: unsupported — engines with create-only CLIs
    /// (BetterDisplay) answer this way, and callers fall back to
    /// remove-and-recreate at the DisplayManager layer.
    fn resize(
        &self,
        display: &VirtualDisplay,
        width: u32,
        height: u32,
        scale: u8,
    ) -> Result<ResizedDisplayMode, ProviderError> {
        let _ = (display, width, height, scale);
        Err(ProviderError::EngineUnavailable(
            "이 엔진은 화면 제거 없이 리사이즈를 지원하지 않습니다. 화면을 제거한 뒤 새 크기로 다시 만들어야 합니다.".into(),
        ))
    }
}

/// Premise check shared by every provider: creation is impossible with zero
/// active displays (spark-verified on macOS 26.6.2). Must run BEFORE the
/// engine is invoked so headless attempts fail with actionable guidance.
pub fn ensure_active_display_premise(active_count: u32) -> Result<(), ProviderError> {
    if active_count == 0 {
        Err(ProviderError::NoActiveDisplay)
    } else {
        Ok(())
    }
}

/// Wraps the existing BetterDisplay CLI contracts in `virtual_display.rs`.
/// This module never bundles BetterDisplay — availability is probed, and the
/// user-facing error tells them what to install.
pub struct BetterDisplayProvider;

impl BetterDisplayProvider {
    pub fn new() -> Self {
        Self
    }
}

impl VirtualDisplayProvider for BetterDisplayProvider {
    fn name(&self) -> &'static str {
        "betterdisplay"
    }

    fn available(&self) -> bool {
        // `CLI` and the spawn helpers are macOS-only in `virtual_display.rs`,
        // and Windows CI compiles and tests this crate, so the engine probe is
        // cfg-gated: on other platforms the engine is simply absent.
        #[cfg(target_os = "macos")]
        {
            super::virtual_display::cli_available()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        ensure_engine_premise()?;
        #[cfg(target_os = "macos")]
        {
            // The name is validated BEFORE the handle is built so `VirtualDisplay`
            // always carries a non-empty name — `remove` resolves by `-namelike=`,
            // and a blank name could discard ALL discardable devices.
            let name = super::virtual_display::validate_name(&spec.name)
                .map_err(ProviderError::EngineFailed)?;
            let _ = spec.backing_dimensions()?;
            super::virtual_display::create_virtual_display_hidpi(
                &name,
                spec.width,
                spec.height,
                spec.scale,
            )
            .map_err(ProviderError::EngineFailed)?;
            Ok(VirtualDisplay {
                name,
                cgvd_display_id: None,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        // No active-display premise here: the premise doc scopes it to
        // creation, and by teardown time the lid may be closed (count == 0)
        // — requiring it would misclassify cleanup as NoActiveDisplay.
        // There is nothing to discard when count == 0 anyway.
        #[cfg(target_os = "macos")]
        {
            super::virtual_display::remove_virtual_display(&display.name)
                .map(|_| ())
                .map_err(ProviderError::EngineFailed)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = display;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }
}

fn ensure_engine_premise() -> Result<(), ProviderError> {
    #[cfg(target_os = "macos")]
    {
        let count = active_display_count();
        ensure_active_display_premise(count)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
    }
}

#[cfg(target_os = "macos")]
fn active_display_count() -> u32 {
    use std::os::raw::c_uint;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(
            max_displays: c_uint,
            active_displays: *mut c_uint,
            display_count: *mut c_uint,
        ) -> i32;
    }
    let mut count: c_uint = 0;
    // Safe: count pointer is valid; null list with max=0 only writes count.
    unsafe {
        CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count);
    }
    count
}

impl Default for BetterDisplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// EXPERIMENT-ONLY provider behind the CGVD opt-in flag (R-015 논골 유지).
/// Calls the Swift shim built from `tools/cgvd-shim/`; never promotes to the
/// default provider without a separate ADR.
pub struct CgvdProvider {
    // Only macOS-cfg'd methods read the path; on other platforms it is
    // written but never read, which would fail Windows `clippy -D warnings`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    binary_path: String,
    #[cfg(target_os = "macos")]
    sessions: std::sync::Mutex<std::collections::HashMap<u32, CgvdSession>>,
}

#[cfg(target_os = "macos")]
struct CgvdSession {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    responses: std::sync::mpsc::Receiver<std::io::Result<String>>,
    next_request_id: u64,
    generation: u64,
}

#[cfg(target_os = "macos")]
impl Drop for CgvdSession {
    fn drop(&mut self) {
        // Child does not terminate or reap itself on drop. Cover early returns
        // (including a poisoned registry lock), not only normal removal.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl CgvdProvider {
    pub fn with_binary_path(binary_path: &str) -> Self {
        Self {
            binary_path: binary_path.into(),
            #[cfg(target_os = "macos")]
            sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Dev-built shim location; built via `swift build -c release` per
    /// tools/cgvd-shim/README.md. `LEFTCAR_CGVD_SHIM` overrides the path so
    /// a debug build or an alternate checkout can be tested without moving
    /// files — the same pattern as `LEFTCAR_CAPTURE_DYLIB` in ffi.rs.
    pub fn new() -> Self {
        if let Ok(path) = std::env::var("LEFTCAR_CGVD_SHIM") {
            // An explicit override is authoritative, even when the path does
            // not exist — availability then fails loudly instead of silently
            // probing the default.
            return Self::with_binary_path(&path);
        }
        Self::with_binary_path(&default_cgvd_shim_path())
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn inspect_display(display_id: u32) -> Result<DisplayInspection, ProviderError> {
        use std::time::{Duration, Instant};
        let path = default_cgvd_shim_path();
        let mut child = std::process::Command::new(path)
            .arg("inspect")
            .arg(format!("--display-id={display_id}"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| {
                ProviderError::EngineUnavailable(format!("shim 실행 불가: {error}"))
            })?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while child
            .try_wait()
            .map_err(|error| ProviderError::EngineFailed(format!("inspect 상태 실패: {error}")))?
            .is_none()
        {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProviderError::EngineFailed("inspect 응답 시간 초과".into()));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child
            .wait_with_output()
            .map_err(|error| ProviderError::EngineFailed(format!("inspect 읽기 실패: {error}")))?;
        let line = String::from_utf8_lossy(&output.stdout);
        parse_cgvd_inspection(line.trim())
    }

    /// Spawns the shim and returns its (status, stdout, stderr) verbatim.
    /// `probe` speaks a different one-line contract (`EXISTS`/`MISSING`) than
    /// `create`, so both callers share the spawn but classify separately.
    #[cfg(target_os = "macos")]
    fn spawn_shim(
        &self,
        args: &[&str],
    ) -> std::io::Result<(std::process::ExitStatus, String, String)> {
        let output = std::process::Command::new(&self.binary_path)
            .args(args)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((output.status, stdout, stderr))
    }

    #[cfg(target_os = "macos")]
    fn spawn_session(
        &self,
        args: &[String],
    ) -> Result<(CgvdSession, String, String), ProviderError> {
        use std::io::BufRead;
        use std::sync::mpsc;
        use std::time::Duration;
        let mut child = std::process::Command::new(&self.binary_path)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| {
                ProviderError::EngineUnavailable(format!("shim 실행 불가: {error}"))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::EngineFailed("shim stdin을 열 수 없습니다.".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::EngineFailed("shim stdout을 열 수 없습니다.".into()))?;
        let stderr = child.stderr.take();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let result = reader.read_line(&mut line).map(|_| line);
                let done = matches!(&result, Ok(line) if line.is_empty());
                if tx.send(result).is_err() || done {
                    break;
                }
            }
        });
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut sink = String::new();
                while reader.read_line(&mut sink).unwrap_or(0) != 0 {
                    sink.clear();
                }
            });
        }
        // Registration and mode selection each allow two seconds in the shim.
        let line = rx.recv_timeout(Duration::from_secs(6)).map_err(|_| {
            let _ = child.kill();
            let _ = child.wait();
            ProviderError::EngineFailed("shim READY 응답 시간 초과".into())
        })?;
        let session = CgvdSession {
            child,
            stdin,
            responses: rx,
            next_request_id: 1,
            generation: args
                .iter()
                .find_map(|argument| argument.strip_prefix("--serial="))
                .and_then(|serial| serial.parse().ok())
                .unwrap_or(0),
        };
        let line = line.map_err(|error| {
            ProviderError::EngineFailed(format!("shim READY 읽기 실패: {error}"))
        })?;
        Ok((session, line, String::new()))
    }

    #[cfg(target_os = "macos")]
    fn stop_session(&self, display_id: u32) -> Result<(), ProviderError> {
        use std::io::Write;
        use std::time::{Duration, Instant};
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ProviderError::EngineFailed("CGVD 세션 잠금 실패".into()))?;
        let session = sessions.get_mut(&display_id).ok_or_else(|| {
            ProviderError::EngineFailed(format!("관리 중인 displayID가 아닙니다: {display_id}"))
        })?;
        if matches!(session.child.try_wait(), Ok(Some(_))) {
            let generation = session.generation;
            sessions.remove(&display_id);
            return crate::ffi::clear_managed_display_mode(display_id, generation)
                .map_err(ProviderError::EngineFailed);
        }
        let stop_result = session
            .stdin
            .write_all(b"stop\n")
            .and_then(|_| session.stdin.flush());
        if stop_result.is_err() {
            // Keep ownership if termination cannot be confirmed, allowing retry.
            session
                .child
                .kill()
                .map_err(|error| ProviderError::EngineFailed(format!("shim 종료 실패: {error}")))?;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match session.child.try_wait() {
                Ok(Some(_)) => {
                    let generation = session.generation;
                    sessions.remove(&display_id);
                    return crate::ffi::clear_managed_display_mode(display_id, generation)
                        .map_err(ProviderError::EngineFailed);
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                Ok(None) => {
                    session.child.kill().map_err(|error| {
                        ProviderError::EngineFailed(format!("shim 종료 실패: {error}"))
                    })?;
                    session.child.wait().map_err(|error| {
                        ProviderError::EngineFailed(format!("shim 종료 대기 실패: {error}"))
                    })?;
                    let generation = session.generation;
                    sessions.remove(&display_id);
                    return crate::ffi::clear_managed_display_mode(display_id, generation)
                        .map_err(ProviderError::EngineFailed);
                }
                Err(error) => {
                    return Err(ProviderError::EngineFailed(format!(
                        "shim 상태 확인 실패: {error}"
                    )))
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn place_session(&self, display_id: u32, x: i32, y: i32) -> Result<(), ProviderError> {
        use std::io::Write;
        use std::sync::mpsc::RecvTimeoutError;
        use std::time::{Duration, Instant};

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ProviderError::EngineFailed("CGVD 세션 잠금 실패".into()))?;
        let session = sessions.get_mut(&display_id).ok_or_else(|| {
            ProviderError::EngineFailed(format!("관리 중인 displayID가 아닙니다: {display_id}"))
        })?;
        let request_id = session.next_request_id;
        session.next_request_id = session.next_request_id.wrapping_add(1).max(1);
        writeln!(session.stdin, "PLACE {request_id} {x} {y}")
            .and_then(|_| session.stdin.flush())
            .map_err(|error| {
                ProviderError::EngineFailed(format!("shim 배치 요청 실패: {error}"))
            })?;

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProviderError::EngineFailed(
                    "shim PLACED 응답 시간 초과".into(),
                ));
            }
            let line = match session.responses.recv_timeout(remaining) {
                Ok(Ok(line)) if !line.is_empty() => line,
                Ok(Ok(_)) => {
                    return Err(ProviderError::EngineFailed("shim 응답 스트림 종료".into()))
                }
                Ok(Err(error)) => {
                    return Err(ProviderError::EngineFailed(format!(
                        "shim PLACED 읽기 실패: {error}"
                    )))
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(ProviderError::EngineFailed(
                        "shim PLACED 응답 시간 초과".into(),
                    ))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ProviderError::EngineFailed("shim 응답 스트림 종료".into()))
                }
            };
            match parse_cgvd_placed(line.trim(), request_id, display_id, x, y) {
                Err(ProviderError::EngineFailed(detail))
                    if detail.starts_with("stale request ") =>
                {
                    continue
                }
                result => return result,
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn abort_session(mut session: CgvdSession) {
        use std::io::Write;
        let _ = session.stdin.write_all(b"stop\n");
        let _ = session.child.kill();
        let _ = session.child.wait();
    }

    /// Sends `RESIZE <w> <h> <scale>` to the retained shim and waits for its
    /// `RESIZED` line (see tools/cgvd-shim). The sessions lock is held for the
    /// whole handshake, serializing this against PLACE — the shim's tracked
    /// current logical size is never racing a concurrent place request, and
    /// the next reply line on the channel is guaranteed to be this resize's.
    #[cfg(target_os = "macos")]
    fn resize_session(
        &self,
        display_id: u32,
        width: u32,
        height: u32,
        scale: u8,
    ) -> Result<ResizedCgvdMode, ProviderError> {
        use std::io::Write;
        use std::sync::mpsc::RecvTimeoutError;
        use std::time::Duration;

        if let Err(error) = super::virtual_display::validate_dimensions(width, height) {
            return Err(ProviderError::EngineFailed(error));
        }
        if let Err(error) = super::virtual_display::validate_scale(scale) {
            return Err(ProviderError::EngineFailed(error));
        }
        let expected_pixel_width = width.checked_mul(u32::from(scale)).ok_or_else(|| {
            ProviderError::EngineFailed("검증할 backing 폭이 너무 큽니다.".into())
        })?;
        let expected_pixel_height = height.checked_mul(u32::from(scale)).ok_or_else(|| {
            ProviderError::EngineFailed("검증할 backing 높이가 너무 큽니다.".into())
        })?;

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ProviderError::EngineFailed("CGVD 세션 잠금 실패".into()))?;
        let session = sessions.get_mut(&display_id).ok_or_else(|| {
            ProviderError::EngineFailed(format!("관리 중인 displayID가 아닙니다: {display_id}"))
        })?;
        writeln!(session.stdin, "RESIZE {width} {height} {scale}")
            .and_then(|_| session.stdin.flush())
            .map_err(|error| {
                ProviderError::EngineFailed(format!("shim 리사이즈 요청 실패: {error}"))
            })?;

        // RESIZED 응답에는 PLACE처럼 식별 가능한 requestID가 없다. 대신 이
        // sessions 잠금을 든 채로 응답을 기다린다 — PLACE·RESIZE·stop이 모두
        // 같은 잠금을 쓰므로 요청 자체가 직렬화되고, 다음 도착 줄이 곧 이
        // RESIZE의 응답임이 보장된다. 잠금이 최대 5초 묶이는 건 그 직렬화의
        // 대가다.
        const RESIZE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
        let line = match session.responses.recv_timeout(RESIZE_RESPONSE_TIMEOUT) {
            Ok(Ok(line)) if !line.is_empty() => line,
            Ok(Ok(_)) | Err(RecvTimeoutError::Disconnected) => {
                return Err(ProviderError::EngineFailed("shim 응답 스트림 종료".into()))
            }
            Ok(Err(error)) => {
                return Err(ProviderError::EngineFailed(format!(
                    "shim RESIZED 읽기 실패: {error}"
                )))
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(ProviderError::EngineFailed(
                    "shim RESIZED 응답 시간 초과".into(),
                ))
            }
        };
        // READY와 동일하게 관측값이 요청과 일치하는지 확인한다 — shim이 다른
        // 모드로 떨어졌는데 성공처럼 기록되는 것을 막는다.
        let mode = parse_cgvd_resized(line.trim())?;
        if (mode.logical_width, mode.logical_height) != (width, height)
            || (mode.pixel_width, mode.pixel_height)
                != (expected_pixel_width, expected_pixel_height)
        {
            return Err(ProviderError::EngineFailed(format!(
                "shim 모드 불일치: logical={}x{}, pixel={}x{}",
                mode.logical_width, mode.logical_height, mode.pixel_width, mode.pixel_height
            )));
        }
        Ok(mode)
    }
}

/// One-line RESIZED contract of the retained shim: the logical and pixel
/// dimensions actually reached after the mode switch. Kept compiled on every
/// platform for contract tests (same rationale as parse_cgvd_line).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_cgvd_resized(line: &str) -> Result<ResizedCgvdMode, ProviderError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.first().copied() == Some("FAILED") {
        return Err(ProviderError::EngineFailed(
            fields[1..].join(" ").trim_end().to_string(),
        ));
    }
    if fields.len() != 5 || fields[0] != "RESIZED" {
        return Err(ProviderError::EngineFailed(format!(
            "shim RESIZED 응답 파싱 실패: {line}"
        )));
    }
    let values: Vec<u32> = fields[1..]
        .iter()
        .map(|value| value.parse().ok())
        .collect::<Option<_>>()
        .ok_or_else(|| {
            ProviderError::EngineFailed(format!("shim RESIZED 응답 파싱 실패: {line}"))
        })?;
    if values[0] == 0 || values[1] == 0 || values[2] == 0 || values[3] == 0 {
        return Err(ProviderError::EngineFailed(format!(
            "shim 리사이즈 모드 불일치: logical={}x{}, pixel={}x{}",
            values[0], values[1], values[2], values[3]
        )));
    }
    Ok(ResizedCgvdMode {
        logical_width: values[0],
        logical_height: values[1],
        pixel_width: values[2],
        pixel_height: values[3],
    })
}

/// RESIZE가 실제로 도달한 모드. PLACE와 달리 requestID가 없는 계약이라 늦은
/// 응답 구분은 세션 직렬화(sessions 잠금)에 의존한다.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
struct ResizedCgvdMode {
    logical_width: u32,
    logical_height: u32,
    pixel_width: u32,
    pixel_height: u32,
}

impl Default for CgvdProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl Drop for CgvdProvider {
    fn drop(&mut self) {
        let ids: Vec<u32> = self
            .sessions
            .lock()
            .ok()
            .map(|sessions| sessions.keys().copied().collect())
            .unwrap_or_default();
        for id in ids {
            let _ = self.stop_session(id);
        }
    }
}

/// One-line stdout contract of the shim (see tools/cgvd-shim/README.md).
/// Kept compiled on every platform so the contract tests run on Windows CI;
/// the non-macOS build never calls it (the shim is macOS-only).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_cgvd_line(line: &str) -> Result<u32, ProviderError> {
    let mut parts = line.splitn(2, ' ');
    match parts.next() {
        Some("OK") | Some("READY") => line
            .split_whitespace()
            .nth(1)
            .and_then(|rest| rest.parse::<u32>().ok())
            .ok_or_else(|| ProviderError::EngineFailed(format!("shim 출력 파싱 실패: {line}"))),
        Some("NOACTIVE") => Err(ProviderError::NoActiveDisplay),
        Some("UNAVAILABLE") => Err(ProviderError::EngineUnavailable(
            parts.next().unwrap_or("").into(),
        )),
        Some("FAILED") => Err(ProviderError::EngineFailed(
            parts.next().unwrap_or("").into(),
        )),
        _ => Err(ProviderError::EngineFailed(format!(
            "shim 출력 파싱 실패: {line}"
        ))),
    }
}

fn parse_cgvd_placed(
    line: &str,
    request_id: u64,
    display_id: u32,
    x: i32,
    y: i32,
) -> Result<(), ProviderError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.first().copied() == Some("FAILED") {
        let response_id = fields.get(1).and_then(|value| value.parse::<u64>().ok());
        if response_id != Some(request_id) {
            return Err(ProviderError::EngineFailed(format!("stale request {line}")));
        }
        return Err(ProviderError::EngineFailed(fields[2..].join(" ")));
    }
    if fields.len() != 9 || fields[0] != "PLACED" {
        return Err(ProviderError::EngineFailed(format!(
            "shim PLACED 응답 파싱 실패: {line}"
        )));
    }
    let response_request = fields[1].parse::<u64>().ok();
    if response_request != Some(request_id) {
        return Err(ProviderError::EngineFailed(format!("stale request {line}")));
    }
    let actual_id = fields[2].parse::<u32>().ok();
    let actual_x = fields[3].parse::<i32>().ok();
    let actual_y = fields[4].parse::<i32>().ok();
    let actual_width = fields[5].parse::<u32>().ok();
    let actual_height = fields[6].parse::<u32>().ok();
    let primary_before = fields[7].parse::<u32>().ok();
    let primary_after = fields[8].parse::<u32>().ok();
    if actual_id != Some(display_id)
        || actual_x != Some(x)
        || actual_y != Some(y)
        || actual_width == Some(0)
        || actual_width.is_none()
        || actual_height == Some(0)
        || actual_height.is_none()
    {
        return Err(ProviderError::EngineFailed(format!(
            "shim 배치 검증 불일치: {line}"
        )));
    }
    if primary_before.is_none() || primary_before != primary_after {
        return Err(ProviderError::EngineFailed(format!(
            "shim 주 디스플레이 변경 감지: {line}"
        )));
    }
    Ok(())
}

fn parse_cgvd_inspection(line: &str) -> Result<DisplayInspection, ProviderError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.first().copied() == Some("FAILED") {
        return Err(ProviderError::EngineFailed(fields[1..].join(" ")));
    }
    if fields.len() != 8 || fields[0] != "INSPECT" {
        return Err(ProviderError::EngineFailed(format!(
            "shim INSPECT 응답 파싱 실패: {line}"
        )));
    }
    Ok(DisplayInspection {
        display_id: fields[1].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        logical_width: fields[2].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        logical_height: fields[3].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        pixel_width: fields[4].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        pixel_height: fields[5].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        x: fields[6].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
        y: fields[7].parse().map_err(|_| {
            ProviderError::EngineFailed(format!("shim INSPECT 응답 파싱 실패: {line}"))
        })?,
    })
}

fn default_cgvd_shim_path() -> String {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(path) = bundled_cgvd_shim_path(&executable) {
            return path.to_string_lossy().into_owned();
        }
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/cgvd-shim/.build/release")
        .join(BUNDLED_SHIM_NAME)
        .to_string_lossy()
        .into_owned()
}

fn bundled_cgvd_shim_path(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    let macos_dir = executable.parent()?;
    (macos_dir.file_name().and_then(|name| name.to_str()) == Some("MacOS"))
        .then(|| {
            macos_dir
                .parent()
                .map(|contents| contents.join("Resources").join(BUNDLED_SHIM_NAME))
        })
        .flatten()
}

#[cfg(target_os = "macos")]
fn parse_cgvd_ready(line: &str, width: u32, height: u32, scale: u8) -> Result<u32, ProviderError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.first().copied() != Some("READY") {
        // Preserve actionable failure classes emitted before a session is ready.
        parse_cgvd_line(line)?;
    }
    if fields.len() != 6 || fields[0] != "READY" {
        return Err(ProviderError::EngineFailed(format!(
            "shim READY 응답 파싱 실패: {line}"
        )));
    }
    let values: Vec<u32> = fields[1..]
        .iter()
        .map(|value| value.parse().ok())
        .collect::<Option<_>>()
        .ok_or_else(|| ProviderError::EngineFailed(format!("shim READY 응답 파싱 실패: {line}")))?;
    let expected_pixel_width = width
        .checked_mul(scale as u32)
        .ok_or_else(|| ProviderError::EngineFailed("검증할 backing 폭이 너무 큽니다.".into()))?;
    let expected_pixel_height = height
        .checked_mul(scale as u32)
        .ok_or_else(|| ProviderError::EngineFailed("검증할 backing 높이가 너무 큽니다.".into()))?;
    if values[1] != width
        || values[2] != height
        || values[3] != expected_pixel_width
        || values[4] != expected_pixel_height
    {
        return Err(ProviderError::EngineFailed(format!(
            "shim 모드 불일치: logical={}x{}, pixel={}x{}",
            values[1], values[2], values[3], values[4]
        )));
    }
    if values[0] == 0 {
        return Err(ProviderError::EngineFailed("shim displayID=0".into()));
    }
    Ok(values[0])
}

impl VirtualDisplayProvider for CgvdProvider {
    fn name(&self) -> &'static str {
        "cgvirtualdisplay"
    }

    fn available(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Cheap disk check first — spawning a missing binary on every
            // availability poll is wasted work.
            if !std::path::Path::new(&self.binary_path).exists() {
                return false;
            }
            // probe answers EXISTS/MISSING (always exit 0) — its own contract,
            // matched literally instead of through parse_cgvd_line.
            matches!(
                self.spawn_shim(&["probe"]),
                Ok((_status, stdout, _stderr))
                    if stdout.lines().next().map(str::trim) == Some("EXISTS")
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn create(&self, spec: &DisplaySpec) -> Result<VirtualDisplay, ProviderError> {
        #[cfg(target_os = "macos")]
        {
            let count = active_display_count();
            ensure_active_display_premise(count)?;
            // Same shared validators as BetterDisplayProvider: the trait
            // promises engine-swap transparency, so ratio-style dimensions
            // and blank names must be rejected with Korean guidance before
            // the shim ever runs — not as an English shim usage line.
            let name = super::virtual_display::validate_name(&spec.name)
                .map_err(ProviderError::EngineFailed)?;
            super::virtual_display::validate_dimensions(spec.width, spec.height)
                .map_err(ProviderError::EngineFailed)?;
            spec.backing_dimensions()?;
            let serial = next_cgvd_serial();
            let args = vec![
                "create".into(),
                format!("--name={name}"),
                format!("--width={}", spec.width),
                format!("--height={}", spec.height),
                format!("--scale={}", spec.scale),
                format!("--serial={serial}"),
            ];
            let (session, line, _) = self.spawn_session(&args)?;
            let display_id =
                match parse_cgvd_ready(line.trim(), spec.width, spec.height, spec.scale) {
                    Ok(display_id) => display_id,
                    Err(error) => {
                        Self::abort_session(session);
                        return Err(error);
                    }
                };
            let (pixel_width, pixel_height) = spec.backing_dimensions()?;
            if let Err(error) = crate::ffi::register_managed_display_mode(
                display_id,
                serial as u64,
                spec.width,
                spec.height,
                pixel_width,
                pixel_height,
            ) {
                Self::abort_session(session);
                return Err(ProviderError::EngineFailed(format!(
                    "capture HiDPI mode 등록 실패: {error}"
                )));
            }
            let mut sessions = match self.sessions.lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    let _ = crate::ffi::clear_managed_display_mode(display_id, serial as u64);
                    Self::abort_session(session);
                    return Err(ProviderError::EngineFailed("CGVD 세션 잠금 실패".into()));
                }
            };
            sessions.insert(display_id, session);
            Ok(VirtualDisplay {
                name,
                cgvd_display_id: Some(display_id),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn remove(&self, display: &VirtualDisplay) -> Result<(), ProviderError> {
        // No active-display premise here: cleanup must not require an active
        // display (BetterDisplayProvider lesson — the lid may be closed by
        // teardown time).
        #[cfg(target_os = "macos")]
        {
            let id = display
                .cgvd_display_id
                .ok_or_else(|| ProviderError::EngineFailed("CGVD displayID가 없습니다.".into()))?;
            self.stop_session(id)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = display;
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn place(&self, display: &VirtualDisplay, x: i32, y: i32) -> Result<(), ProviderError> {
        #[cfg(target_os = "macos")]
        {
            let id = display
                .cgvd_display_id
                .ok_or_else(|| ProviderError::EngineFailed("CGVD displayID가 없습니다.".into()))?;
            self.place_session(id, x, y)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (display, x, y);
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }

    fn resize(
        &self,
        display: &VirtualDisplay,
        width: u32,
        height: u32,
        scale: u8,
    ) -> Result<ResizedDisplayMode, ProviderError> {
        #[cfg(target_os = "macos")]
        {
            let id = display
                .cgvd_display_id
                .ok_or_else(|| ProviderError::EngineFailed("CGVD displayID가 없습니다.".into()))?;
            let generation = {
                let sessions = self
                    .sessions
                    .lock()
                    .map_err(|_| ProviderError::EngineFailed("CGVD 세션 잠금 실패".into()))?;
                sessions
                    .get(&id)
                    .ok_or_else(|| {
                        ProviderError::EngineFailed(format!("관리 중인 displayID가 아닙니다: {id}"))
                    })?
                    .generation
            };
            let mode = self.resize_session(id, width, height, scale)?;
            // Same (displayID, serial) key the capture side registered at
            // create time — the mode table is updated in place, keeping the
            // session and its display alive even if re-registration fails so
            // a retry remains possible.
            if let Err(error) = crate::ffi::register_managed_display_mode(
                id,
                generation,
                mode.logical_width,
                mode.logical_height,
                mode.pixel_width,
                mode.pixel_height,
            ) {
                return Err(ProviderError::EngineFailed(format!(
                    "capture HiDPI mode 재등록 실패: {error}"
                )));
            }
            Ok(ResizedDisplayMode {
                logical_width: mode.logical_width,
                logical_height: mode.logical_height,
                pixel_width: mode.pixel_width,
                pixel_height: mode.pixel_height,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (display, width, height, scale);
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        }
    }
}

#[cfg(target_os = "macos")]
fn next_cgvd_serial() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SERIAL: std::sync::OnceLock<AtomicU32> = std::sync::OnceLock::new();
    let serial = SERIAL.get_or_init(|| {
        let bytes = *uuid::Uuid::new_v4().as_bytes();
        AtomicU32::new(u32::from_ne_bytes(bytes[..4].try_into().unwrap()))
    });
    loop {
        let candidate = serial.fetch_add(1, Ordering::Relaxed);
        if candidate != 0 {
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_active_displays_is_rejected_before_engine_call() {
        assert_eq!(
            ensure_active_display_premise(0),
            Err(ProviderError::NoActiveDisplay)
        );
    }

    #[test]
    fn any_positive_display_count_passes_the_premise() {
        assert_eq!(ensure_active_display_premise(1), Ok(()));
        assert_eq!(ensure_active_display_premise(3), Ok(()));
    }

    #[test]
    fn error_messages_are_user_actionable() {
        assert!(ProviderError::NoActiveDisplay
            .message()
            .contains("덮개를 열거나"));
        assert!(ProviderError::EngineUnavailable("BD 미설치".into())
            .message()
            .contains("BD 미설치"));
    }

    #[test]
    fn betterdisplay_provider_reports_engine_unavailable_without_cli() {
        // No mocking of PATH here: betterdisplaycli is not on CI's PATH, so
        // available() must be false there — that IS the contract under test.
        let provider = BetterDisplayProvider::new();
        #[cfg(target_os = "macos")]
        assert_eq!(
            provider.available(),
            crate::virtual_display::cli_available()
        );
        #[cfg(not(target_os = "macos"))]
        assert!(!provider.available());
    }

    #[test]
    fn providers_are_object_safe_for_the_session_registry() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(BetterDisplayProvider::new());
        assert_eq!(provider.name(), "betterdisplay");
    }

    #[test]
    fn cgvd_output_contract_is_parsed_into_provider_errors() {
        assert_eq!(
            parse_cgvd_line("NOACTIVE"),
            Err(ProviderError::NoActiveDisplay)
        );
        assert_eq!(
            parse_cgvd_line("UNAVAILABLE shim missing"),
            Err(ProviderError::EngineUnavailable("shim missing".into()))
        );
        assert_eq!(
            parse_cgvd_line("FAILED displayID=0"),
            Err(ProviderError::EngineFailed("displayID=0".into()))
        );
        assert_eq!(parse_cgvd_line("OK 42"), Ok(42));
        assert_eq!(parse_cgvd_line("READY 42 1600 1000 3200 2000"), Ok(42));
        assert!(parse_cgvd_line("garbage").is_err());
    }

    #[test]
    fn cgvd_parser_pins_multiformat_and_malformed_lines() {
        // Multi-word FAILED details are part of the shim contract.
        assert_eq!(
            parse_cgvd_line("FAILED registration timeout displayID=42"),
            Err(ProviderError::EngineFailed(
                "registration timeout displayID=42".into()
            ))
        );
        // A non-numeric OK payload is a contract violation, not a display id.
        assert!(parse_cgvd_line("OK abc").is_err());
        // Empty stdout (crash before print) must parse as failure, never as
        // a silently successful creation.
        assert!(parse_cgvd_line("").is_err());
    }

    #[test]
    fn cgvd_placed_ack_requires_matching_request_display_bounds_and_primary() {
        assert_eq!(
            parse_cgvd_placed("PLACED 9 42 -1600 0 1600 1000 1 1", 9, 42, -1600, 0),
            Ok(())
        );
        assert!(matches!(
            parse_cgvd_placed("PLACED 8 42 -1600 0 1600 1000 1 1", 9, 42, -1600, 0),
            Err(ProviderError::EngineFailed(detail)) if detail.starts_with("stale request ")
        ));
        assert!(parse_cgvd_placed("PLACED 9 42 -1500 0 1600 1000 1 1", 9, 42, -1600, 0).is_err());
        assert!(parse_cgvd_placed("PLACED 9 42 -1600 0 1600 1000 1 2", 9, 42, -1600, 0).is_err());
        assert_eq!(
            parse_cgvd_placed("FAILED 9 configure code=1001", 9, 42, -1600, 0),
            Err(ProviderError::EngineFailed("configure code=1001".into()))
        );
    }

    #[test]
    fn cgvd_inspection_contract_includes_logical_pixel_dimensions_and_bounds() {
        assert_eq!(
            parse_cgvd_inspection("INSPECT 42 1600 1000 3200 2000 -1600 0"),
            Ok(DisplayInspection {
                display_id: 42,
                logical_width: 1600,
                logical_height: 1000,
                pixel_width: 3200,
                pixel_height: 2000,
                x: -1600,
                y: 0,
            })
        );
        assert!(parse_cgvd_inspection("INSPECT 42 malformed").is_err());
        assert_eq!(
            parse_cgvd_inspection("FAILED mode unavailable displayID=42"),
            Err(ProviderError::EngineFailed(
                "mode unavailable displayID=42".into()
            ))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cgvd_ready_parser_requires_logical_and_pixel_dimensions() {
        assert_eq!(
            parse_cgvd_ready("READY 42 1600 1000 3200 2000", 1600, 1000, 2),
            Ok(42)
        );
        assert!(parse_cgvd_ready("READY 42 3200 2000 6400 4000", 1600, 1000, 2).is_err());
        assert!(parse_cgvd_ready("READY 0 1600 1000 3200 2000", 1600, 1000, 2).is_err());
    }

    #[test]
    fn cgvd_resized_parser_pins_contract_and_late_place_replies() {
        assert_eq!(
            parse_cgvd_resized("RESIZED 1280 800 1280 800"),
            Ok(ResizedCgvdMode {
                logical_width: 1280,
                logical_height: 800,
                pixel_width: 1280,
                pixel_height: 800,
            })
        );
        // The shim keeps the session alive on failure — FAILED detail only.
        assert_eq!(
            parse_cgvd_resized("FAILED resize displayID=42 mode timeout"),
            Err(ProviderError::EngineFailed(
                "resize displayID=42 mode timeout".into()
            ))
        );
        // Malformed, wrong shape, and zero dimensions are contract violations.
        assert!(parse_cgvd_resized("").is_err());
        assert!(parse_cgvd_resized("RESIZED 1280 800").is_err());
        assert!(parse_cgvd_resized("RESIZED 0 800 0 800").is_err());
        assert!(parse_cgvd_resized("RESIZED a b c d").is_err());
        // Late PLACE replies (multi-word FAILED <requestID> detail included)
        // are recognized so the resize wait can skip them.
        assert!(parse_cgvd_resized("PLACED 9 42 -1600 0 1600 1000 1 1").is_err());
        assert_eq!(
            parse_cgvd_resized("FAILED 9 configure code=1001"),
            Err(ProviderError::EngineFailed("9 configure code=1001".into()))
        );
    }

    #[test]
    fn cgvd_remove_rejects_an_unmanaged_display_id() {
        let provider = CgvdProvider::with_binary_path("missing-binary");
        let display = VirtualDisplay {
            name: "tablets".into(),
            cgvd_display_id: Some(7),
        };
        #[cfg(target_os = "macos")]
        assert_eq!(
            provider.remove(&display),
            Err(ProviderError::EngineFailed(
                "관리 중인 displayID가 아닙니다: 7".into()
            ))
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            provider.remove(&display),
            Err(ProviderError::EngineUnavailable(MACOS_ONLY_MESSAGE.into()))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cgvd_session_drop_terminates_a_live_child() {
        let mut child = std::process::Command::new("/bin/cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let stdin = child.stdin.take().unwrap();
        let (_tx, responses) = std::sync::mpsc::channel();
        drop(CgvdSession {
            child,
            stdin,
            responses,
            next_request_id: 1,
            generation: 1,
        });
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "session drop left a child running");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cgvd_remove_reaps_an_already_exited_child() {
        let provider = CgvdProvider::with_binary_path("unused");
        let mut child = std::process::Command::new("/usr/bin/true")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        child.wait().unwrap();
        provider.sessions.lock().unwrap().insert(
            99,
            CgvdSession {
                child,
                stdin,
                responses: std::sync::mpsc::channel().1,
                next_request_id: 1,
                generation: 1,
            },
        );
        assert_eq!(provider.stop_session(99), Ok(()));
        assert!(provider.sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn cgvd_provider_is_object_safe_and_named() {
        let provider: std::sync::Arc<dyn VirtualDisplayProvider> =
            std::sync::Arc::new(CgvdProvider::with_binary_path("missing-binary"));
        assert_eq!(provider.name(), "cgvirtualdisplay");
        // The injected path does not exist, so availability must be false.
        assert!(!provider.available());
    }
}
#[test]
fn packaged_cgvd_path_resolves_from_contents_macos() {
    let executable =
        std::path::Path::new("/Applications/Leftcar Host.app/Contents/MacOS/leftcar-host-desktop");
    assert_eq!(
        bundled_cgvd_shim_path(executable).unwrap(),
        std::path::Path::new("/Applications/Leftcar Host.app/Contents/Resources/cgvd-shim")
    );
}
