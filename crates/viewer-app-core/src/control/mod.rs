//! 제어 평면 클라이언트 엔진 (design §제어평면): 뷰어가 호스트의 TCP JSON
//! 서버에서 당겨 쓴다. 개행 단위 `{"command","args"}` / `{"ok",...}` 줄을 오간다.
//!
//! TS 원본(apps/viewer-expo/src/control.ts)의 엔진 의미론을 그대로 옮긴다.
//!
//! - 응답 줄 한도: 핸드셰이크 16KiB / 제어 12MiB. FIFO 보류 한도: 32개 / 16MiB.
//! - 명령별 요청 데드라인은 FIFO **맨 앞**에만 무장한다 — 느린 resize 뒤의
//!   status 폴링은 데드라인 없이 기다린다(앞 항목이 끝나야 자기 시계가 시작).
//! - FIFO 채널에는 응답 ID가 없으므로 쓰기 실패·데드라인·오류는 보류 전체를
//!   폐기하고 소켓을 끊는다. 늦게 온 응답은 안전하게 재사용할 수 없다.
//! - 루프백이 아니면 secure 핸드셰이크를 먼저 하고, 봉인 모드에서 평문 줄이
//!   오면 연결을 끊는다(다운그레이드 거부).
//! - 토큰은 첫 요청 전에 한 번 조회해 캐시하고, unauthorized 응답이면 캐시를
//!   흘려 다음 요청이 다시 조회하게 한다.
//!
//! 동시성 모델은 std 전용이다: 읽기는 전용 스레드, 요청 대기는 채널, 데드라인은
//! 감시 스레드가 맨 앞 항목만 감시한다(tokio 금지 — design §2.2 원칙 3).

pub mod messages;
pub mod records;
pub mod transport;

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::b64;
use crate::secure::{
    self, create_client_hello, encode_client_hello, parse_sealed_line, parse_server_hello,
    ClientEphemeral, OsRandom, RandomSource,
};
use secure_channel::{StreamSealer, TAG_LEN};

pub use messages::{format_error_message, ErrorMessages};
pub use transport::{ControlStream, ControlTransport, ControlWriter};

/// 보류 중 응답 텍스트 상한 — 피어가 개행 없이 보낼 때를 대비한다. 명령 예산은
/// 호스트의 봉인된 8 MiB 클립보드 봉투를 수용한다.
pub const HANDSHAKE_LINE_LIMIT: usize = 16 * 1024;
pub const CONTROL_LINE_LIMIT: usize = 12 * 1024 * 1024;
pub const MAX_PENDING_REQUESTS: usize = 32;
pub const MAX_PENDING_REQUEST_BYTES: usize = 16 * 1024 * 1024;
/// REQUEST_TIMEOUT_MS에 없는 명령의 기본 데드라인.
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 5_000;
/// TS connect()의 기본 연결·핸드셰이크 상한.
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5_000;

/// 명령별 요청 타임아웃(ms): 스트림 시작은 첫 프레임 대기까지, 카탈로그는
/// 나열 조회까지의 실측 여유를 담는다. 파일 청크 1개는 최대 1 MiB(base64
/// ≈ 1.4 MB)라 느린 링크 대비 여유를 둔다. 클립보드는 256 KiB 텍스트 왕복이
/// 있어 기본 5초보다 여유를 둔다 — 실패 시 공용 제어 소켓이 파괴되므로 타임아웃이
/// 오탐이면 안 된다.
pub fn default_request_timeout_ms(command: &str) -> u64 {
    match command {
        "startStream" => 25_000,
        "createVirtualDisplay" => 30_000,
        "removeVirtualDisplay" => 20_000,
        "resizeVirtualDisplay" => 60_000,
        "arrangeVirtualDisplay" => 10_000,
        "getCatalog" => 15_000,
        "setClipboard" => 15_000,
        "getClipboard" => 15_000,
        "sendFileBegin" => 15_000,
        "sendFileChunk" => 30_000,
        "sendFileEnd" => 15_000,
        "sendFileCancel" => 10_000,
        "listShareQueue" => 15_000,
        "fetchFileBegin" => 15_000,
        "fetchFileChunk" => 30_000,
        "fetchFileEnd" => 15_000,
        "fetchFileCancel" => 10_000,
        _ => DEFAULT_REQUEST_TIMEOUT_MS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlErrorKind {
    Remote,
    Timeout,
    Transport,
    Unauthorized,
    Capacity,
}

impl std::fmt::Display for ControlErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ControlErrorKind::Remote => "remote",
            ControlErrorKind::Timeout => "timeout",
            ControlErrorKind::Transport => "transport",
            ControlErrorKind::Unauthorized => "unauthorized",
            ControlErrorKind::Capacity => "capacity",
        };
        f.write_str(name)
    }
}

/// TS ControlRequestError 대응. kind가 의미의 원천이고 message는 진단용이다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ControlError {
    pub kind: ControlErrorKind,
    pub message: String,
}

impl ControlError {
    pub fn new(kind: ControlErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// The host rejected our token (or we never paired) — pairing is required.
pub fn is_unauthorized_error(error: &ControlError) -> bool {
    error.kind == ControlErrorKind::Unauthorized
}

/// A transport error cannot prove a mutating command failed on the Host.
pub fn is_control_transport_error(error: &ControlError) -> bool {
    error.kind == ControlErrorKind::Transport
}

/// 루프백 제어 경로(USB 네이티브 프록시, 진단 도구)는 평문 JSON을 유지한다.
pub fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().to_lowercase();
    normalized == "127.0.0.1" || normalized == "::1" || normalized == "localhost"
}

/// 요청 봉투에 주입할 페어링 토큰 공급자. 요청마다 polling해 새로 끝낸 페어링을
/// 재연결 없이 반영한다(TS TokenProvider). 실패는 요청 거절로 이어지며 아무것도
/// 쓰지 않는다.
pub trait TokenProvider: Send + Sync {
    fn token(&self) -> Result<Option<String>, ControlError>;
}

/// 이미 알고 있는 토큰을 그대로 주입하는 공급자(페어링 연결, USB 프록시 경로).
pub struct StaticTokenProvider(pub Option<String>);

impl TokenProvider for StaticTokenProvider {
    fn token(&self) -> Result<Option<String>, ControlError> {
        Ok(self.0.clone())
    }
}

pub type HostKeyCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct ConnectOptions {
    /// QR(v2)에서 핀한 호스트 Ed25519 공개키(b64url 32B). ServerHello의 서명
    /// 키가 이것과 다르면 중간자로 간주하고 연결을 끊는다. 없으면 TOFU.
    pub pinned_host_key: Option<String>,
    /// 요청 봉투에 주입할 토큰 공급자.
    pub token_provider: Option<Arc<dyn TokenProvider>>,
    /// 핸드셰이크로 증명된 호스트 공개키(b64url) — TOFU 핀 저장용.
    pub on_host_key: Option<HostKeyCallback>,
    /// 사용자 안내 문구 표(오류 포맷 경계).
    pub messages: ErrorMessages,
    /// 명령별 데드라인 재정의(테스트·임베딩용 — 기본 표는
    /// [`default_request_timeout_ms`]).
    pub timeout_overrides: BTreeMap<String, u64>,
    /// ClientHello 난수 원천. 기본 OS CSPRNG.
    pub random: Arc<dyn RandomSource>,
}

impl ConnectOptions {
    pub fn new() -> Self {
        Self {
            pinned_host_key: None,
            token_provider: None,
            on_host_key: None,
            messages: ErrorMessages::KOREAN,
            timeout_overrides: BTreeMap::new(),
            random: Arc::new(OsRandom),
        }
    }

    pub fn pinned_host_key(mut self, key: Option<String>) -> Self {
        self.pinned_host_key = key;
        self
    }

    pub fn token_provider(mut self, provider: Arc<dyn TokenProvider>) -> Self {
        self.token_provider = Some(provider);
        self
    }

    pub fn on_host_key(mut self, callback: HostKeyCallback) -> Self {
        self.on_host_key = Some(callback);
        self
    }

    pub fn messages(mut self, messages: ErrorMessages) -> Self {
        self.messages = messages;
        self
    }

    pub fn timeout_overrides(mut self, overrides: BTreeMap<String, u64>) -> Self {
        self.timeout_overrides = overrides;
        self
    }

    fn timeout_for(&self, command: &str) -> u64 {
        self.timeout_overrides
            .get(command)
            .copied()
            .unwrap_or_else(|| default_request_timeout_ms(command))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Handshake,
    Plain,
    Sealed,
}

type ResponseSink = Sender<Result<Value, ControlError>>;
type ClosedListener = Arc<dyn Fn() + Send + Sync>;

struct PendingEntry {
    id: u64,
    command: String,
    timeout_ms: u64,
    /// FIFO 바이트 예산에 세는 평문 바이트 수(봉인 오버헤드 제외).
    bytes: usize,
    sink: ResponseSink,
}

struct FrontDeadline {
    id: u64,
    at: Instant,
}

struct EngineState {
    mode: Mode,
    pending: VecDeque<PendingEntry>,
    pending_bytes: usize,
    next_id: u64,
    terminal: Option<ControlError>,
    front_deadline: Option<FrontDeadline>,
    /// None은 아직 조회 전, Some(None)은 조회 결과가 없음(TS cachedToken).
    token_cache: Option<Option<String>>,
    client_host_key: Option<String>,
    ephemeral: Option<ClientEphemeral>,
    tx: Option<StreamSealer>,
    rx: Option<StreamSealer>,
    closed_listeners: Vec<ClosedListener>,
    notified_closed: bool,
    destroyed: bool,
}

impl EngineState {
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            pending: VecDeque::new(),
            pending_bytes: 0,
            next_id: 1,
            terminal: None,
            front_deadline: None,
            token_cache: None,
            client_host_key: None,
            ephemeral: None,
            tx: None,
            rx: None,
            closed_listeners: Vec::new(),
            notified_closed: false,
            destroyed: false,
        }
    }
}

struct EngineShared {
    state: Mutex<EngineState>,
    /// FIFO 변화·터미널 오류를 데드라인 감시 스레드에 알린다.
    wakeup: Condvar,
    /// 요청 스레드가 쓰는 핸들. 읽기 스레드가 스트림을 소유하므로 분리한다.
    writer: Mutex<Option<Box<dyn ControlWriter>>>,
    options: ConnectOptions,
    /// 핀한 호스트 키(b64url → 32B). 파싱 실패는 무시하고 TOFU로 둔다.
    pinned: Option<[u8; 32]>,
    connect_tx: Mutex<Option<Sender<Result<(), ControlError>>>>,
    remote_address: Option<String>,
    id: u64,
}

static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

fn lock<'a>(shared: &'a EngineShared) -> MutexGuard<'a, EngineState> {
    shared.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// TS formatErrorMessage 경계 — 엔진 안의 실패 문구는 문구 표를 통과한다.
fn fmt_message(shared: &EngineShared, message: &str) -> String {
    format_error_message(message, &shared.options.messages)
}

/// 터미널 오류를 세우고 보류 전체를 폐기하며(오류로 완료) 닫힘 리스너를
/// 발화한다. 멱등 — 이미 터미널이 있으면 그 오류를 유지한다(TS fail/close).
fn teardown(shared: &Arc<EngineShared>, error: ControlError) {
    let (pending, listeners, notify) = {
        let mut s = lock(shared);
        if s.terminal.is_none() {
            s.terminal = Some(error.clone());
        }
        s.ephemeral = None; // helloSecret.fill(0) 대응 — drop에서 0화된다
        let pending = std::mem::take(&mut s.pending);
        s.pending_bytes = 0;
        s.front_deadline = None;
        s.destroyed = true;
        let notify = !s.notified_closed;
        s.notified_closed = true;
        (pending, s.closed_listeners.clone(), notify)
    };
    let terminal = lock(shared).terminal.clone().unwrap_or(error);
    for entry in pending {
        let _ = entry.sink.send(Err(terminal.clone()));
    }
    {
        let mut writer = shared.writer.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut w) = writer.take() {
            w.destroy();
        }
    }
    // connect()를 아직 기다리는 호출자가 있다면 터미널 오류로 확정한다.
    let connect_sender = shared
        .connect_tx
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(sender) = connect_sender {
        let _ = sender.send(Err(terminal.clone()));
    }
    shared.wakeup.notify_all();
    if notify {
        for listener in listeners {
            listener();
        }
    }
}

fn fail(shared: &Arc<EngineShared>, message: &str) {
    teardown(shared, ControlError::new(ControlErrorKind::Transport, message.to_string()));
}

/// connect() 결과를 확정한다. 이미 터미널이 있으면 Err로 확정한다.
fn settle_connect(shared: &Arc<EngineShared>, result: Result<(), ControlError>) {
    let terminal = {
        let s = lock(shared);
        match (&s.terminal, &result) {
            (Some(t), Ok(())) => Some(t.clone()),
            _ => None,
        }
    };
    let sender = shared
        .connect_tx
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(sender) = sender {
        let _ = sender.send(match (terminal, result) {
            (Some(t), _) => Err(t),
            (None, r) => r,
        });
    }
}

/// FIFO 맨 앞 데드라인 감시 스레드. 앞 항목이 바뀌면 시계를 다시 무장한다.
fn watchdog_loop(shared: Arc<EngineShared>) {
    let mut s = lock(&shared);
    loop {
        if s.terminal.is_some() {
            return;
        }
        let armed = s.front_deadline.as_ref().map(|d| (d.id, d.at));
        match armed {
            None => {
                if let Some(front) = s.pending.front() {
                    let at = Instant::now() + Duration::from_millis(front.timeout_ms);
                    s.front_deadline = Some(FrontDeadline {
                        id: front.id,
                        at,
                    });
                    continue;
                }
                s = shared
                    .wakeup
                    .wait(s)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            Some((deadline_id, deadline_at)) => {
                let now = Instant::now();
                if now < deadline_at {
                    let wait = deadline_at - now;
                    let (guard, _) = shared
                        .wakeup
                        .wait_timeout(s, wait)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    s = guard;
                    continue;
                }
                let front = s.pending.front();
                let expires_front = front.map(|e| e.id) == Some(deadline_id);
                if expires_front {
                    // FIFO 맨 앞이 응답 없이 데드라인을 넘겼다 — 채널 전체가
                    // 죽은 것이다. 늦은 응답은 재사용할 수 없다(TS armFrontDeadline).
                    let error = ControlError::new(
                        ControlErrorKind::Timeout,
                        format!(
                            "control request timeout: {}",
                            front.map(|e| e.command.as_str()).unwrap_or("")
                        ),
                    );
                    drop(s);
                    teardown(&shared, error);
                    return;
                }
                // 앞 항목이 바뀌었다 — 시계를 다시 무장한다.
                s.front_deadline = None;
            }
        }
    }
}

/// 읽기 스레드. 줄 경계를 나누고 한도를 지키며 drain으로 넘긴다.
fn reader_loop(shared: Arc<EngineShared>, mut stream: Box<dyn ControlStream>) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        {
            let s = lock(&shared);
            if s.terminal.is_some() || s.destroyed {
                break;
            }
        }
        let read = stream.read(&mut chunk);
        match read {
            Ok(0) => break,
            Ok(n) => process_chunk(&shared, &mut buffer, &chunk[..n]),
            Err(e) => {
                let message = format!("control connection error: {}", fmt_message(&shared, &e.to_string()));
                teardown(&shared, ControlError::new(ControlErrorKind::Transport, message));
                break;
            }
        }
    }
    // TS socket 'close' 이벤트 대응: 어떤 이유로든 읽기가 끝나면 연결은 닫힌다.
    stream.destroy();
    teardown(
        &shared,
        ControlError::new(ControlErrorKind::Transport, "control connection closed"),
    );
}

/// TS socket.on("data") — 새 텍스트만 훑어 부분 줄이 자라지 않게 한다.
fn process_chunk(shared: &Arc<EngineShared>, buffer: &mut Vec<u8>, chunk: &[u8]) {
    let mut start = 0usize;
    while start < chunk.len() {
        if lock(shared).terminal.is_some() {
            return;
        }
        let newline = chunk[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| i + start);
        let end = newline.unwrap_or(chunk.len());
        let limit = {
            let s = lock(shared);
            match s.mode {
                Mode::Handshake => HANDSHAKE_LINE_LIMIT,
                _ => CONTROL_LINE_LIMIT,
            }
        };
        if buffer.len() + (end - start) > limit {
            fail(shared, "control response exceeds line limit");
            return;
        }
        buffer.extend_from_slice(&chunk[start..end]);
        let Some(nl) = newline else { return };
        buffer.push(b'\n');
        drain(shared, buffer);
        start = nl + 1;
    }
}

/// 완결된 줄들을 순서대로 처리한다(TS drain).
fn drain(shared: &Arc<EngineShared>, buffer: &mut Vec<u8>) {
    loop {
        let mode = {
            let s = lock(shared);
            if s.terminal.is_some() {
                return;
            }
            s.mode
        };
        let Some(nl) = buffer.iter().position(|&b| b == b'\n') else {
            return;
        };
        let raw: Vec<u8> = buffer[..nl].to_vec();
        buffer.drain(..=nl);
        match mode {
            Mode::Handshake => handle_handshake_line(shared, &raw),
            Mode::Sealed => {
                let line = String::from_utf8_lossy(&raw).into_owned();
                let Some(frame) = parse_sealed_line(&line) else {
                    fail(shared, "control channel expected a sealed frame");
                    return;
                };
                let opened = {
                    let mut s = lock(shared);
                    match s.rx.as_mut() {
                        Some(rx) => rx.open(&frame).map_err(|e| e.to_string()),
                        // 봉인기 없이 봉인 모드에 있을 수 없다(내부 불변).
                        None => Err("control channel expected a sealed frame".to_string()),
                    }
                };
                match opened {
                    Ok(plaintext) => handle_plain_text_line(shared, &plaintext),
                    Err(message) => {
                        fail(shared, &format!("sealed frame rejected: {message}"));
                        return;
                    }
                }
            }
            Mode::Plain => handle_plain_text_line(shared, &raw),
        }
    }
}

/// ClientHello에 답한 ServerHello 줄을 검증하고 봉인 모드로 넘어간다.
fn handle_handshake_line(shared: &Arc<EngineShared>, raw: &[u8]) {
    let line = String::from_utf8_lossy(raw).into_owned();
    enum Outcome {
        Sealed { confirm: Vec<u8>, host_key: String },
        Failure(String),
    }
    let outcome = {
        let mut s = lock(shared);
        let server = match parse_server_hello(&line) {
            Ok(server) => server,
            Err(e) => return fail(shared, &format!("secure handshake failed: {e}")),
        };
        let Some(ephemeral) = s.ephemeral.as_ref() else {
            return fail(shared, "secure handshake failed: no client hello");
        };
        match secure::client_finish(ephemeral, &server, shared.pinned.as_ref()) {
            Ok(result) => {
                let mut tx = StreamSealer::new(result.keys.c2s);
                let rx = StreamSealer::new(result.keys.s2c);
                let confirm_text = format!(
                    "{{\"hello\":\"ok\",\"nc\":\"{}\"}}",
                    b64::encode(&ephemeral.hello.nc)
                );
                let confirm = match tx.seal(confirm_text.as_bytes()) {
                    Ok(frame) => frame,
                    Err(_) => {
                        return fail(shared, "secure handshake failed: confirmation seal failed")
                    }
                };
                s.tx = Some(tx);
                s.rx = Some(rx);
                s.ephemeral = None;
                s.client_host_key = Some(b64::encode(&result.spk));
                s.mode = Mode::Sealed;
                Outcome::Sealed {
                    confirm,
                    host_key: s.client_host_key.clone().unwrap_or_default(),
                }
            }
            Err(e) => Outcome::Failure(e.to_string()),
        }
    };
    match outcome {
        Outcome::Failure(message) => {
            fail(shared, &format!("secure handshake failed: {}", fmt_message(shared, &message)))
        }
        Outcome::Sealed { confirm, host_key } => {
            // TS와 같이 봉인 프레임을 전송 줄({"e":"<b64url>"})로 감싼다.
            let mut wire = secure::sealed_line(&confirm).into_bytes();
            wire.push(b'\n');
            if let Err(message) = write_wire(shared, &wire) {
                fail(
                    shared,
                    &format!("secure handshake write error: {}", fmt_message(shared, &message)),
                );
                return;
            }
            if let Some(callback) = &shared.options.on_host_key {
                callback(&host_key);
            }
            settle_connect(shared, Ok(()));
        }
    }
}

/// 한 줄의 평문 JSON을 응답 매칭으로 처리한다(TS handlePlainTextLine).
fn handle_plain_text_line(shared: &Arc<EngineShared>, raw: &[u8]) {
    let line = String::from_utf8_lossy(raw);
    if line.trim().is_empty() {
        return;
    }
    let parsed: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => return fail(shared, "malformed control response"),
    };
    let ok_flag = parsed.get("ok").and_then(|v| v.as_bool());
    if !parsed.is_object() || ok_flag.is_none() {
        return fail(shared, "malformed control response");
    }
    // FIFO match: server replies in request order
    let entry = {
        let mut s = lock(shared);
        let entry = s.pending.pop_front();
        if entry.is_some() {
            s.front_deadline = None; // 다음 항목의 시계가 시작된다
        }
        if let Some(entry) = &entry {
            s.pending_bytes = s.pending_bytes.saturating_sub(entry.bytes);
        }
        shared.wakeup.notify_all();
        entry
    };
    let Some(entry) = entry else { return };
    if ok_flag == Some(true) {
        let result = parsed.get("result").cloned().unwrap_or(Value::Null);
        let _ = entry.sink.send(Ok(result));
        return;
    }
    let error_text = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("control error");
    if error_text == "unauthorized" {
        // The host closes the connection right after "unauthorized"; this
        // rejection is registered before the close handler runs, so the
        // specific error wins over the generic one.
        lock(shared).token_cache = None;
        let _ = entry.sink.send(Err(ControlError::new(
            ControlErrorKind::Unauthorized,
            error_text,
        )));
        return;
    }
    let _ = entry.sink.send(Err(ControlError::new(
        ControlErrorKind::Remote,
        error_text,
    )));
}

/// 전송 줄을 쓴다. 상태 잠금 밖에서 쓰기 핸들만 잠근다 — 읽기 루프와 간섭하지
/// 않는다(transport 트레잇이 읽기/쓰기를 분리한다).
fn write_wire(shared: &Arc<EngineShared>, wire: &[u8]) -> Result<(), String> {
    let mut writer = shared.writer.lock().unwrap_or_else(|p| p.into_inner());
    match writer.as_mut() {
        Some(writer) => writer.write_all(wire).map_err(|e| e.to_string()),
        None => Err("socket destroyed".to_string()),
    }
}

/// 요청 봉투 JSON. TS와 같은 필드 순서(command, args, token)를 유지하고
/// 토큰이 없으면 필드를 생략한다.
#[derive(Serialize)]
struct RequestEnvelope<'a> {
    command: &'a str,
    args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
}

fn is_io_message(message: String) -> String {
    message
}

/// 클라이언트 핸들. 소켓 하나의 FIFO 채널을 나타내며 복제해 공유할 수 있다.
/// 마지막 핸들이 drop되면 TS GC와 마찬가지로 연결을 닫는다.
#[derive(Clone)]
pub struct ControlClient {
    shared: Arc<EngineShared>,
    id: u64,
}

impl Drop for ControlClient {
    fn drop(&mut self) {
        // 모든 핸들이 사라졌고 아직 닫히지 않았다면 소켓을 유출하지 않는다.
        if Arc::strong_count(&self.shared) == 1 {
            let terminal = lock(&self.shared).terminal.is_some();
            if !terminal {
                teardown(
                    &self.shared,
                    ControlError::new(ControlErrorKind::Transport, "control connection closed"),
                );
            }
        }
    }
}

impl std::fmt::Debug for ControlClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlClient")
            .field("id", &self.id)
            .finish()
    }
}

impl ControlClient {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Numeric peer from the connected socket, including MagicDNS resolution.
    pub fn remote_address(&self) -> Option<String> {
        self.shared.remote_address.clone()
    }

    /// 서버가 서명으로 증명한 호스트 공개키(b64url). 평문(루프백) 모드면 없다.
    pub fn host_key(&self) -> Option<String> {
        lock(&self.shared).client_host_key.clone()
    }

    /// 연결이 죽었는지(요청이 전부 실패하는 상태) — 세션·테스트 보조.
    pub fn is_closed(&self) -> bool {
        lock(&self.shared).terminal.is_some()
    }

    /// 소켓이 닫히면(오류·상대 종료·close() 모두) 발화한다. 호출 후 이
    /// 클라이언트의 요청은 전부 실패다.
    pub fn when_closed(&self, listener: ClosedListener) {
        let mut s = lock(&self.shared);
        if s.notified_closed {
            // 이미 닫힌 소켓이라면 즉시 발화한다(TS는 등록 이전 이벤트를
            // 재생하지 않지만, 상태 진실 유지를 위해 등록 시점의 사실을 알린다).
            drop(s);
            listener();
            return;
        }
        s.closed_listeners.push(listener);
    }

    fn resolve_token(&self) -> Result<Option<String>, ControlError> {
        let cached = lock(&self.shared).token_cache.clone();
        if let Some(token) = cached {
            return Ok(token);
        }
        let fetched = match &self.shared.options.token_provider {
            None => None,
            Some(provider) => provider.token()?,
        };
        lock(&self.shared).token_cache = Some(fetched.clone());
        Ok(fetched)
    }

    /// 명령을 FIFO에 넣고 응답을 기다린다. `on_written`은 쓰기가 성공한 직후
    /// (서버 응답 전) 한 번 발화한다 — 네이티브 활동을 연 뒤 소켓 쓰기가
    /// 대기열에 들었음을 호출부에 알리는 계기다.
    pub fn request(
        &self,
        command: &str,
        args: Option<&Value>,
        mut on_written: Option<&mut dyn FnMut()>,
    ) -> Result<Value, ControlError> {
        let token = self.resolve_token()?;
        let (rx, wire) = {
            let mut s = lock(&self.shared);
            if let Some(terminal) = &s.terminal {
                return Err(terminal.clone());
            }
            if s.pending.len() >= MAX_PENDING_REQUESTS {
                return Err(ControlError::new(
                    ControlErrorKind::Capacity,
                    "control request queue full",
                ));
            }
            let envelope = RequestEnvelope {
                command,
                args: args.cloned().unwrap_or(Value::Object(Default::default())),
                token: token.as_deref(),
            };
            let payload = match serde_json::to_string(&envelope) {
                Ok(payload) => payload,
                Err(_) => {
                    return Err(ControlError::new(
                        ControlErrorKind::Transport,
                        "control request encoding failed",
                    ))
                }
            };
            // Unpadded base64url plus {"e":""} and the delimiter must fit the
            // Host's line reader. Reject before sealing or admitting into FIFO:
            // an oversized write would otherwise retire unrelated requests.
            let wire_bytes = match s.mode {
                Mode::Sealed => {
                    (payload.len() + secure::COUNTER_LEN + TAG_LEN).div_ceil(3) * 4 + 9
                }
                _ => payload.len(),
            };
            if wire_bytes > CONTROL_LINE_LIMIT {
                return Err(ControlError::new(
                    ControlErrorKind::Capacity,
                    "control request line limit exceeded",
                ));
            }
            if payload.len() > MAX_PENDING_REQUEST_BYTES.saturating_sub(s.pending_bytes) {
                return Err(ControlError::new(
                    ControlErrorKind::Capacity,
                    "control request byte budget exceeded",
                ));
            }
            let (sink, rx) = std::sync::mpsc::channel();
            let timeout_ms = self.shared.options.timeout_for(command);
            let id = s.next_id;
            s.next_id += 1;
            s.pending.push_back(PendingEntry {
                id,
                command: command.to_string(),
                timeout_ms,
                bytes: payload.len(),
                sink,
            });
            s.pending_bytes += payload.len();
            if s.pending.len() == 1 {
                s.front_deadline = None; // 새 맨 앞 — 시계는 감시 스레드가 무장
            }
            self.shared.wakeup.notify_all();
            let sealed = matches!(s.mode, Mode::Sealed) && s.tx.is_some();
            let wire = if sealed {
                let tx = s.tx.as_mut().expect("sealed mode checked above");
                match tx.seal(payload.as_bytes()) {
                    Ok(frame) => {
                        // 평문 요청을 전송 줄({"e":"<b64url>"})로 감싼다(TS sealedLine).
                        let mut wire = secure::sealed_line(&frame).into_bytes();
                        wire.push(b'\n');
                        wire
                    }
                    Err(_) => {
                        // 봉인은 평문 상한(16MiB) 안에서만 실패하지 않는다 —
                        // 위 줄 한도 검사가 이미 걸러낸다. 도달하면 거부한다.
                        return Err(ControlError::new(
                            ControlErrorKind::Capacity,
                            "control request line limit exceeded",
                        ));
                    }
                }
            } else {
                let mut wire = payload.into_bytes();
                wire.push(b'\n');
                wire
            };
            (rx, wire)
        };
        let write_error = match write_wire(&self.shared, &wire) {
            Ok(()) => {
                if lock(&self.shared).terminal.is_none() {
                    if let Some(callback) = on_written.as_mut() {
                        callback();
                    }
                }
                None
            }
            Err(message) => Some(is_io_message(message)),
        };
        if let Some(message) = write_error {
            // No response IDs exist on this FIFO channel. Even a failed write
            // may have reached the Host; retire every pending reply.
            if lock(&self.shared).terminal.is_none() {
                fail(
                    &self.shared,
                    &format!("control write error: {}", fmt_message(&self.shared, &message)),
                );
            }
        }
        match rx.recv() {
            Ok(result) => result,
            Err(_) => {
                let terminal = lock(&self.shared)
                    .terminal
                    .clone()
                    .unwrap_or_else(|| {
                        ControlError::new(
                            ControlErrorKind::Transport,
                            "control connection closed",
                        )
                    });
                Err(terminal)
            }
        }
    }

    /// 콜백 없는 요청 편의 함수.
    pub fn request_simple(&self, command: &str, args: Option<&Value>) -> Result<Value, ControlError> {
        self.request(command, args, None)
    }

    /// Terminate the active control channel. 등록된 닫힘 리스너가 발화한다.
    pub fn close(&self) {
        teardown(
            &self.shared,
            ControlError::new(ControlErrorKind::Transport, "control connection closed"),
        );
    }
}

/// 제어 소켓 하나를 열고, 루프백이 아니면 secure 핸드셰이크를 마친 뒤
/// 클라이언트를 돌려준다(TS connect). `timeout_ms`는 소켓 연결과 핸드셰이크
/// 전체에 적용된다.
pub fn connect(
    transport: &dyn ControlTransport,
    host: &str,
    port: u16,
    timeout_ms: u64,
    options: ConnectOptions,
) -> Result<ControlClient, ControlError> {
    // 루프백이 아닌 피어는 secure-channel 핸드셰이크가 필수다.
    let secure = !is_loopback_host(host);
    let io_timeout = Duration::from_millis(timeout_ms.max(1));
    let connect_started = Instant::now();
    let stream = transport.connect(host, port, io_timeout).map_err(|e| {
        // 전송이 상한까지 막힌 뒤 실패했다면 TS의 전체 connect 타임아웃과 같은
        // 관찰 결과다 — timeout 종류로 보고한다(빠른 연결 거부는 transport).
        if connect_started.elapsed() >= io_timeout {
            return ControlError::new(
                ControlErrorKind::Timeout,
                format!("connect timeout to {host}:{port}"),
            );
        }
        ControlError::new(
            ControlErrorKind::Transport,
            format!("control connection error: {}", format_error_message(&e.to_string(), &options.messages)),
        )
    })?;
    let writer = stream.writer();
    let remote_address = stream.remote_address();
    let pinned = options
        .pinned_host_key
        .as_deref()
        .and_then(b64::decode)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    let shared = Arc::new(EngineShared {
        state: Mutex::new(EngineState::new(if secure { Mode::Handshake } else { Mode::Plain })),
        wakeup: Condvar::new(),
        writer: Mutex::new(Some(writer)),
        options,
        pinned,
        connect_tx: Mutex::new(None),
        remote_address,
        id: NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed),
    });
    let (connect_tx, connect_rx) = std::sync::mpsc::channel();
    *shared.connect_tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(connect_tx);

    if secure {
        let handshake_result = (|| -> Result<(), ControlError> {
            let ephemeral = create_client_hello(shared.options.random.as_ref());
            let line = encode_client_hello(&ephemeral.hello);
            {
                let mut s = lock(&shared);
                s.ephemeral = Some(ephemeral);
            }
            let mut wire = line.into_bytes();
            wire.push(b'\n');
            write_wire(&shared, &wire).map_err(|message| {
                ControlError::new(
                    ControlErrorKind::Transport,
                    format!("secure handshake write error: {}", format_error_message(&message, &shared.options.messages)),
                )
            })
        })();
        if let Err(e) = handshake_result {
            teardown(&shared, e);
            return Err(lock(&shared).terminal.clone().unwrap_or_else(|| {
                ControlError::new(ControlErrorKind::Transport, "secure handshake failed")
            }));
        }
    }

    // 평문(루프백) 모드는 연결 콜백 즉시 확정된다(TS makeClient resolve).
    if !secure {
        settle_connect(&shared, Ok(()));
    }

    let reader_shared = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("viewer-control-rx".into())
        .spawn(move || reader_loop(reader_shared, stream))
        .map_err(|_| {
            ControlError::new(ControlErrorKind::Transport, "control reader thread unavailable")
        })?;
    let watchdog_shared = Arc::clone(&shared);
    let _ = std::thread::Builder::new()
        .name("viewer-control-deadline".into())
        .spawn(move || watchdog_loop(watchdog_shared));

    let deadline = Instant::now() + io_timeout;
    let client_id = shared.id;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match connect_rx.recv_timeout(deadline - now) {
            Ok(Ok(())) => return Ok(ControlClient { shared, id: client_id }),
            Ok(Err(e)) => {
                // 확정 실패 — teardown이 이미 소켓을 정리했다.
                return Err(e);
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let error = ControlError::new(
        ControlErrorKind::Timeout,
        format!("connect timeout to {host}:{port}"),
    );
    teardown(&shared, error.clone());
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::transport::TcpTransport;

    #[test]
    fn loopback_detection_matches_ts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host(" ::1 "));
        assert!(is_loopback_host("LocalHost"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("100.80.133.120"));
    }

    #[test]
    fn timeout_table_matches_ts() {
        assert_eq!(default_request_timeout_ms("resizeVirtualDisplay"), 60_000);
        assert_eq!(default_request_timeout_ms("getCatalog"), 15_000);
        assert_eq!(default_request_timeout_ms("sendFileChunk"), 30_000);
        assert_eq!(default_request_timeout_ms("fetchFileCancel"), 10_000);
        assert_eq!(default_request_timeout_ms("pair"), DEFAULT_REQUEST_TIMEOUT_MS);
    }

    #[test]
    fn error_helpers_read_kind_only() {
        let transport = ControlError::new(ControlErrorKind::Transport, "x");
        let unauthorized = ControlError::new(ControlErrorKind::Unauthorized, "unauthorized");
        assert!(is_control_transport_error(&transport));
        assert!(!is_control_transport_error(&unauthorized));
        assert!(is_unauthorized_error(&unauthorized));
        assert_eq!(unauthorized.to_string(), "unauthorized");
    }

    /// 루프백 TCP로도 connect()가 실제 소켓을 다룬다는 사실만 잠근다(나머지
    /// 엔진 동작은 tests/의 인메모리 duplex 스펙이 검증한다).
    #[test]
    #[ignore = "loopback smoke: requires localhost TCP; duplex specs cover the engine"]
    fn tcp_smoke_connect_and_close() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().expect("accept");
            // 아무 응답 없이 유지 → 클라이언트가 닫을 때까지 대기
            std::thread::sleep(Duration::from_millis(200));
        });
        let client = connect(
            &TcpTransport,
            "127.0.0.1",
            addr.port(),
            1000,
            ConnectOptions::new(),
        )
        .expect("loopback connects in plain mode");
        assert!(client.remote_address().is_some());
        assert_eq!(client.host_key(), None);
        client.close();
        let _ = handle.join();
    }
}
