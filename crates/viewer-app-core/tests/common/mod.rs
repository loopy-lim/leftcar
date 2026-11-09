//! 제어 평면 스펙용 인메모리 duplex 전송 + 스크립트 서버.
//! TS control.test.ts가 react-native-tcp-socket을 모듈 경계에서 가짜로 갈아끼운
//! 것과 같은 역할을 한다 — 실제 소켓은 없다.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use secure_channel::{HostIdentity, StreamSealer, SessionKeys};
use viewer_app_core::b64;
use viewer_app_core::control::{
    ControlStream, ControlTransport, ControlWriter, ConnectOptions, ControlClient, ControlError,
    TokenProvider,
};
use viewer_app_core::pairing::{
    parse_recent_hosts_strict, CorePorts, CredentialStore, MemoryCredentialStore, NoUsb,
    RecentHostEntry, RecentHostsPort, StoreError, SystemClock,
};
use viewer_app_core::secure::{OsRandom, RandomSource};

// -- 파이프 -------------------------------------------------------------------

struct PipeInner {
    buf: VecDeque<u8>,
    shutdown: bool,
}

/// 단방향 바이트 파이프. 읽기는 데이터가 올 때까지 막히고, 종료 신호가 오면
/// 즉시 0(EOF)을 돌려준다.
pub struct Pipe {
    inner: Mutex<PipeInner>,
    signal: Condvar,
}

impl Pipe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(PipeInner {
                buf: VecDeque::new(),
                shutdown: false,
            }),
            signal: Condvar::new(),
        })
    }

    pub fn push(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.buf.extend(bytes.iter().copied());
        self.signal.notify_all();
    }

    /// 최소 1바이트를 읽거나 종료 신호에서 0을 돌려준다.
    pub fn read(&self, out: &mut [u8]) -> usize {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if !inner.buf.is_empty() {
                let n = out.len().min(inner.buf.len());
                for slot in out.iter_mut().take(n) {
                    *slot = inner.buf.pop_front().expect("len checked");
                }
                return n;
            }
            if inner.shutdown {
                return 0;
            }
            let (guard, _) = self
                .signal
                .wait_timeout(inner, Duration::from_millis(50))
                .unwrap_or_else(|p| p.into_inner());
            inner = guard;
        }
    }

    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.shutdown = true;
        self.signal.notify_all();
    }
}

// -- 쓰기 동작(오류 주입) ------------------------------------------------------

/// 클라이언트 쪽 쓰기 동작. TS fake의 `written` 배열과 실패 주입을 흉내 낸다.
#[derive(Default)]
pub struct WriteBehavior {
    written: Mutex<Vec<String>>,
    /// 다음 쓰기: 전송 줄을 기록한 뒤 오류(네이티브 실패 콜백 흉내).
    fail_after_queue: AtomicBool,
    /// 다음 쓰기: 기록 없이 즉시 오류(TS socket.write 동기 예외 흉내).
    fail_before_queue: AtomicBool,
}

impl WriteBehavior {
    fn write(&self, to_server: &Pipe, buf: &[u8]) -> io::Result<()> {
        if self.fail_before_queue.swap(false, Ordering::SeqCst) {
            return Err(io::Error::other("Socket is closed."));
        }
        let fail = self.fail_after_queue.swap(false, Ordering::SeqCst);
        self.written
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(String::from_utf8_lossy(buf).into_owned());
        to_server.push(buf);
        if fail {
            return Err(io::Error::other("Broken pipe"));
        }
        Ok(())
    }

    pub fn written(&self) -> Vec<String> {
        self.written
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn arm_failure_after_queue(&self) {
        self.fail_after_queue.store(true, Ordering::SeqCst);
    }

    pub fn arm_sync_failure(&self) {
        self.fail_before_queue.store(true, Ordering::SeqCst);
    }
}

// -- 연결 양단 ----------------------------------------------------------------

#[derive(Clone)]
pub struct ServerEnd {
    pub host: String,
    pub port: u16,
    to_server: Arc<Pipe>,
    from_server: Arc<Pipe>,
    behavior: Arc<WriteBehavior>,
    /// 클라이언트 destroy가 세트한다(TS socket.destroyed).
    client_destroyed: Arc<AtomicBool>,
}

pub struct FakeClientStream {
    to_server: Arc<Pipe>,
    from_server: Arc<Pipe>,
    behavior: Arc<WriteBehavior>,
    client_destroyed: Arc<AtomicBool>,
    remote: Option<String>,
}

impl ControlStream for FakeClientStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(self.from_server.read(buf))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.behavior.write(&self.to_server, buf)
    }

    fn remote_address(&self) -> Option<String> {
        self.remote.clone()
    }

    fn destroy(&mut self) {
        self.client_destroyed.store(true, Ordering::SeqCst);
        self.from_server.shutdown();
        self.to_server.shutdown();
    }

    fn writer(&self) -> Box<dyn ControlWriter> {
        Box::new(FakeWriter {
            to_server: Arc::clone(&self.to_server),
            from_server: Arc::clone(&self.from_server),
            behavior: Arc::clone(&self.behavior),
            client_destroyed: Arc::clone(&self.client_destroyed),
        })
    }
}

struct FakeWriter {
    to_server: Arc<Pipe>,
    from_server: Arc<Pipe>,
    behavior: Arc<WriteBehavior>,
    client_destroyed: Arc<AtomicBool>,
}

impl ControlWriter for FakeWriter {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.behavior.write(&self.to_server, buf)
    }

    /// TCP와 같은 의미다 — 쓰기 핸들의 파괴도 소켓을 죽여 읽기 쪽을 즉시 깨운다.
    fn destroy(&mut self) {
        self.client_destroyed.store(true, Ordering::SeqCst);
        self.from_server.shutdown();
        self.to_server.shutdown();
    }
}

impl ServerEnd {
    fn new(host: &str, port: u16, remote: Option<String>) -> (FakeClientStream, ServerEnd) {
        let to_server = Pipe::new();
        let from_server = Pipe::new();
        let behavior = Arc::new(WriteBehavior::default());
        let client_destroyed = Arc::new(AtomicBool::new(false));
        let client = FakeClientStream {
            to_server: Arc::clone(&to_server),
            from_server: Arc::clone(&from_server),
            behavior: Arc::clone(&behavior),
            client_destroyed: Arc::clone(&client_destroyed),
            remote,
        };

        let server = ServerEnd {
            host: host.to_string(),
            port,
            to_server,
            from_server,
            behavior,
            client_destroyed,
        };
        (client, server)
    }

    /// 클라이언트가 쓴 전송 줄들(성공·실패 모두 기록).
    pub fn written(&self) -> Vec<String> {
        self.behavior.written()
    }

    pub fn written_len(&self) -> usize {
        self.behavior.written().len()
    }

    pub fn last_written(&self) -> String {
        self.behavior
            .written()
            .pop()
            .expect("at least one written line")
    }

    /// TS socket.destroyed.
    pub fn destroyed(&self) -> bool {
        self.client_destroyed.load(Ordering::SeqCst)
    }

    /// 다음 쓰기가 큐에 넣은 뒤 실패한다.
    pub fn arm_write_failure(&self) {
        self.behavior.arm_failure_after_queue();
    }

    /// 다음 쓰기가 동기로 실패한다.
    pub fn arm_sync_write_failure(&self) {
        self.behavior.arm_sync_failure();
    }

    pub fn send_raw(&self, bytes: &[u8]) {
        self.from_server.push(bytes);
    }

    pub fn send(&self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        if !line.ends_with('\n') {
            bytes.push(b'\n');
        }
        self.from_server.push(&bytes);
    }

    /// 서버가 응답 줄 하나를 읽는다. 시간 초과 시 패닉(테스트 코드다).
    pub fn read_line(&self, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if Instant::now() >= deadline {
                panic!("server timed out reading a line");
            }
            let n = self.to_server.read(&mut byte);
            if n == 0 {
                panic!("server saw EOF while reading a line");
            }
            if byte[0] == b'\n' {
                return String::from_utf8_lossy(&line).into_owned();
            }
            line.push(byte[0]);
        }
    }

    /// 서버가 연결을 닫는다 — 클라이언트의 읽기가 EOF가 된다(TS socket.emit("close")).
    pub fn close(&self) {
        self.from_server.shutdown();
    }

    /// 서버 스레드에서 핸드셰이크를 완결하고 결과를 돌려줄 준비를 한다.
    pub fn auto_handshake(&self) -> SecureServerFuture {
        let server = self.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("spec-server-handshake".into())
            .spawn(move || {
                let result = handshake_as_server(&server);
                let _ = tx.send(result);
            })
            .expect("spawn handshake thread");
        SecureServerFuture { rx }
    }
}

pub struct SecureServerFuture {
    rx: Receiver<SecureServerContext>,
}

impl SecureServerFuture {
    pub fn wait(self) -> SecureServerContext {
        match self.rx.recv_timeout(Duration::from_secs(5)) {
            Ok(context) => context,
            Err(RecvTimeoutError::Timeout) => panic!("server handshake did not finish in time"),
            Err(RecvTimeoutError::Disconnected) => panic!("server handshake thread panicked"),
        }
    }
}

// -- 전송 스크립트 ------------------------------------------------------------

/// connect() 호출마다 소비되는 결과. 비어 있으면 항상 연결된다.
#[derive(Clone)]
pub enum ConnectOutcome {
    Open,
    Fail(&'static str),
    /// release될 때까지 connect가 막힌다(늦게 끝나는 시도 흉내).
    Hold(Arc<GateHandle>),
}

pub struct GateHandle {
    open: Mutex<bool>,
    signal: Condvar,
    waiters: AtomicUsize,
}

impl GateHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(false),
            signal: Condvar::new(),
            waiters: AtomicUsize::new(0),
        })
    }

    /// connect가 게이트에 막혀 있는 호출자 수 — 흐름이 connect에 도달했는지
    /// 확인하는 신호다.
    pub fn waiters(&self) -> usize {
        self.waiters.load(Ordering::SeqCst)
    }

    pub fn release(&self) {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        *open = true;
        self.signal.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        self.waiters.fetch_add(1, Ordering::SeqCst);
        let result = self.wait_inner(timeout);
        self.waiters.fetch_sub(1, Ordering::SeqCst);
        result
    }

    fn wait_inner(&self, timeout: Duration) -> bool {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let deadline = Instant::now() + timeout;
        while !*open {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, _) = self
                .signal
                .wait_timeout(open, deadline - now)
                .unwrap_or_else(|p| p.into_inner());
            open = guard;
        }
        true
    }
}

#[derive(Clone, Default)]
pub struct ScriptedTransport {
    servers: Arc<Mutex<Vec<ServerEnd>>>,
    script: Arc<Mutex<VecDeque<ConnectOutcome>>>,
}

impl ScriptedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_outcome(&self, outcome: ConnectOutcome) {
        self.script
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(outcome);
    }

    /// 지금까지 열린 연결들(호출 순서).
    pub fn servers(&self) -> Vec<ServerEnd> {
        self.servers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn last_server(&self) -> ServerEnd {
        self.servers()
            .last()
            .cloned()
            .expect("a connection was opened")
    }

    pub fn connections_created(&self) -> usize {
        self.servers().len()
    }
}

impl ControlTransport for ScriptedTransport {
    fn connect(&self, host: &str, port: u16, timeout: Duration) -> io::Result<Box<dyn ControlStream>> {
        let outcome = self
            .script
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front();
        match outcome.unwrap_or(ConnectOutcome::Open) {
            ConnectOutcome::Fail(message) => Err(io::Error::other(message)),
            ConnectOutcome::Hold(gate) => {
                if !gate.wait(timeout.max(Duration::from_millis(100))) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "connect gate never opened",
                    ));
                }
                self.open(host, port)
            }
            ConnectOutcome::Open => self.open(host, port),
        }
    }
}

impl ScriptedTransport {
    fn open(&self, host: &str, port: u16) -> io::Result<Box<dyn ControlStream>> {
        let (client, server) = ServerEnd::new(host, port, Some("127.0.0.1".to_string()));
        self.servers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(server);
        Ok(Box::new(client))
    }
}

// -- 스크립트 서버 ------------------------------------------------------------

pub struct SecureServerContext {
    pub keys: SessionKeys,
    pub spk_b64: String,
    /// ClientHello의 nc(b64url) — 확인 프레임 검증용. 확인 프레임을 읽지 않는
    /// 경로(핀 불일치)는 None이다.
    pub client_nc_b64: Option<String>,
    /// s2c 방향 응답 봉인기 — 카운터가 폴링·응답마다 단조 증가해야 한다.
    pub tx: Arc<Mutex<StreamSealer>>,
    /// c2s 방향 요청 열기 — 카운터 검사를 유지한다.
    pub rx_opener: Arc<Mutex<StreamSealer>>,
}

/// ClientHello를 읽어 ServerHello로 답한다. 키 확인 프레임은 읽지 않는다 —
/// 핀 불일치처럼 클라이언트가 확인 프레임 없이 끊는 경로도 검증하게 한다.
pub fn read_hello_and_reply(server: &ServerEnd) -> SecureServerContext {
    let hello_line = server.read_line(Duration::from_secs(5));
    let hello: serde_json::Value =
        serde_json::from_str(hello_line.trim()).expect("client hello json");
    assert_eq!(hello.get("hello").and_then(|v| v.as_str()), Some("c"));
    let nc_b64 = hello.get("nc").and_then(|v| v.as_str()).expect("nc").to_string();
    let nc: [u8; 32] = b64::decode(&nc_b64)
        .expect("nc decodes")
        .try_into()
        .expect("32B");
    let client_xk: [u8; 32] = b64::decode(hello.get("xk").and_then(|v| v.as_str()).expect("xk"))
        .expect("xk decodes")
        .try_into()
        .expect("32B");
    let server_secret = [11u8; 32];
    let server_seed = [12u8; 32];
    let ns = [13u8; 32];
    let identity = HostIdentity::from_seed(server_seed);
    let hello_struct = secure_channel::server_hello(
        &identity,
        &secure_channel::ClientHello {
            nc,
            xk: client_xk,
        },
        ns,
        &x25519_dalek::StaticSecret::from(server_secret),
    );
    server.send(&serde_json::json!({
        "v": 1,
        "hello": "s",
        "ns": b64::encode(&hello_struct.ns),
        "xk": b64::encode(&hello_struct.xk),
        "spk": b64::encode(&hello_struct.spk),
        "sig": b64::encode(&hello_struct.sig),
    }).to_string());
    let keys = secure_channel::server_keys(
        &x25519_dalek::StaticSecret::from(server_secret),
        &secure_channel::ClientHello {
            nc,
            xk: client_xk,
        },
        &ns,
    )
    .expect("server keys");
    SecureServerContext {
        rx_opener: Arc::new(Mutex::new(StreamSealer::new(keys.c2s))),
        tx: Arc::new(Mutex::new(StreamSealer::new(keys.s2c))),
        keys,
        spk_b64: b64::encode(&hello_struct.spk),
        client_nc_b64: Some(nc_b64),
    }
}

/// ClientHello에 답하고 키 확인 프레임을 파이프에서 소비해 검증한다
/// (TS control.test.ts의 handshakeAsServer). 서버 시크릿은 테스트 고정값이다.
pub fn handshake_as_server(server: &ServerEnd) -> SecureServerContext {
    let context = read_hello_and_reply(server);
    // 키 확인 프레임(두 번째 줄)을 서버 키로 연다 — 파이프에서 소비한다.
    let confirm_line = server.read_line(Duration::from_secs(5));
    let confirm = viewer_app_core::secure::parse_sealed_line(confirm_line.trim())
        .unwrap_or_else(|| panic!("confirm frame is a sealed line: {confirm_line:?}"));
    let mut opener = StreamSealer::new(context.keys.c2s);
    let plaintext = opener.open(&confirm).expect("confirm opens with c2s");
    let confirm_json: serde_json::Value =
        serde_json::from_slice(&plaintext).expect("confirm json");
    assert_eq!(
        confirm_json.get("hello").and_then(|v| v.as_str()),
        Some("ok")
    );
    let expected_nc = context
        .client_nc_b64
        .clone()
        .expect("handshake recorded nc");
    assert_eq!(confirm_json.get("nc").and_then(|v| v.as_str()), Some(expected_nc.as_str()));
    context
}

/// 봉인 응답 줄을 보낸다(s2c 방향 — 카운터는 컨텍스트에서 단조 증가한다).
pub fn reply_sealed(server: &ServerEnd, context: &SecureServerContext, line: &str) {
    let mut sealer = context.tx.lock().unwrap_or_else(|p| p.into_inner());
    let frame = sealer.seal(line.as_bytes()).expect("seal reply");
    server.send(&viewer_app_core::secure::sealed_line(&frame));
}

/// 봉인 요청을 열어 (command, args)를 돌려준다.
pub fn read_sealed_request(
    server: &ServerEnd,
    context: &SecureServerContext,
) -> (String, serde_json::Value) {
    let line = server.read_line(Duration::from_secs(5));
    let frame = viewer_app_core::secure::parse_sealed_line(line.trim())
        .expect("request is a sealed line");
    let mut opener = StreamSealer::new(context.keys.c2s);
    let plaintext = opener.open(&frame).expect("request opens with c2s");
    let request: serde_json::Value = serde_json::from_slice(&plaintext).expect("request json");
    (
        request
            .get("command")
            .and_then(|v| v.as_str())
            .expect("command")
            .to_string(),
        request.get("args").cloned().unwrap_or(serde_json::Value::Null),
    )
}

/// 봉인 요청의 `token` 필드까지 돌려준다.
pub fn read_sealed_envelope(
    server: &ServerEnd,
    context: &SecureServerContext,
) -> serde_json::Value {
    let line = server.read_line(Duration::from_secs(5));
    let frame = viewer_app_core::secure::parse_sealed_line(line.trim())
        .unwrap_or_else(|| panic!("request is a sealed line: {line:?}"));
    let mut opener = context.rx_opener.lock().unwrap_or_else(|p| p.into_inner());
    let plaintext = opener.open(&frame).expect("request opens with c2s");
    serde_json::from_slice(&plaintext).expect("request json")
}

/// 평문(루프백) 요청을 읽어 (command, 전체 봉투)를 돌려준다.
pub fn read_plain_request(server: &ServerEnd) -> (String, serde_json::Value) {
    let line = server.read_line(Duration::from_secs(5));
    let request: serde_json::Value = serde_json::from_str(line.trim()).expect("request json");
    (
        request
            .get("command")
            .and_then(|v| v.as_str())
            .expect("command")
            .to_string(),
        request,
    )
}

// -- 저장·시계·난수 -----------------------------------------------------------

/// 결정적 난수 — secure-channel 벡터 테스트의 fixed(start) 시퀀스.
pub struct SeqRandom(pub u8);

impl RandomSource for SeqRandom {
    fn fill(&self, buf: &mut [u8]) {
        for (index, byte) in buf.iter_mut().enumerate() {
            *byte = self.0.wrapping_add(index as u8);
        }
    }
}

/// 수동 시계 — 승인 데드라인 계산용.
pub struct ManualClock(pub Mutex<u64>);

impl ManualClock {
    pub fn at(ms: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(ms)))
    }

    pub fn advance(&self, ms: u64) {
        let mut value = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *value += ms;
    }
}

impl viewer_app_core::pairing::Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// 최근 호스트 저장소의 메모리 구현 — 자격증명 저장소의
/// `leftcar.recent_hosts` 블롭을 읽고 쓴다(TS와 같은 저장 경로).
pub struct MemoryRecentHosts {
    store: Arc<dyn CredentialStore>,
}

impl MemoryRecentHosts {
    pub fn new(store: Arc<dyn CredentialStore>) -> Arc<Self> {
        Arc::new(Self { store })
    }

    pub fn set_raw(&self, raw: Option<String>) {
        match raw {
            Some(raw) => self.store.set(&crate_key(), raw).expect("recent blob write"),
            None => self.store.delete(&crate_key()).expect("recent blob delete"),
        }
    }

    pub fn raw(&self) -> Option<String> {
        self.store.get(&crate_key()).expect("recent blob read")
    }

    fn write_entries(&self, entries: &[RecentHostEntry]) -> Result<(), StoreError> {
        let items: Vec<serde_json::Value> = entries
            .iter()
            .map(|entry| {
                let mut item = serde_json::json!({
                    "host": entry.host,
                    "port": entry.port,
                    "lastConnected": 1u64,
                });
                if let Some(name) = &entry.name {
                    item["name"] = serde_json::Value::String(name.clone());
                }
                if let Some(host_key) = &entry.host_key {
                    item["hostKey"] = serde_json::Value::String(host_key.clone());
                }
                if !entry.aliases.is_empty() {
                    item["aliases"] = serde_json::json!(entry.aliases);
                }
                item
            })
            .collect();
        self.store
            .set(&crate_key(), serde_json::Value::Array(items).to_string())
    }
}

/// leftcar.recent_hosts 키 — common이 pairing 상수를 그대로 쓴다.
fn crate_key() -> String {
    use viewer_app_core::pairing::RECENT_HOSTS_KEY;
    RECENT_HOSTS_KEY.to_string()
}

impl RecentHostsPort for MemoryRecentHosts {
    fn entries(&self) -> Result<Vec<RecentHostEntry>, StoreError> {
        parse_recent_hosts_strict(self.raw().as_deref())
    }

    fn persist_host_key(&self, host: &str, port: u16, host_key: &str) -> Result<(), StoreError> {
        let mut entries = self.entries()?;
        let normalized = host.trim().to_lowercase();
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.host.trim().to_lowercase() == normalized && e.port == port)
        {
            entry.host_key = Some(host_key.to_string());
        } else {
            entries.push(RecentHostEntry {
                host: host.to_string(),
                port,
                name: None,
                host_key: Some(host_key.to_string()),
                aliases: Vec::new(),
            });
        }
        self.write_entries(&entries)
    }

    fn persist_alias(&self, host: &str, port: u16, alias: &str) -> Result<(), StoreError> {
        let mut entries = self.entries()?;
        let normalized = host.trim().to_lowercase();
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.host.trim().to_lowercase() == normalized && e.port == port)
        {
            if !entry.aliases.iter().any(|a| a == alias) {
                entry.aliases.push(alias.to_string());
            }
        } else {
            entries.push(RecentHostEntry {
                host: host.to_string(),
                port,
                name: None,
                host_key: None,
                aliases: vec![alias.to_string()],
            });
        }
        self.write_entries(&entries)
    }
}

/// 붙어 있는 USB 액세서리 흉내.
pub struct FixedUsb(pub u16);

impl viewer_app_core::pairing::UsbPort for FixedUsb {
    fn control_port(&self) -> Option<u16> {
        Some(self.0)
    }
}

/// 스펙용 포트 묶음 — 결정적 난수·수동 시계·메모리 저장.
pub fn test_ports(transport: Arc<ScriptedTransport>) -> (CorePorts, Arc<MemoryCredentialStore>, Arc<MemoryRecentHosts>) {
    let store = Arc::new(MemoryCredentialStore::new());
    let recent = MemoryRecentHosts::new(Arc::clone(&store) as Arc<dyn CredentialStore>);
    let ports = CorePorts {
        transport,
        store: store.clone(),
        pins: Arc::new(viewer_app_core::pairing::HostKeyPins::new()),
        recent: recent.clone(),
        random: Arc::new(OsRandom),
        clock: Arc::new(SystemClock),
        usb: Arc::new(NoUsb),
        messages: viewer_app_core::control::ErrorMessages::KOREAN,
        device_name: "스펙 뷰어".to_string(),
        credential_lock: Arc::new(Mutex::new(())),
    };
    (ports, store, recent)
}

/// 조건이 참이 될 때까지 기다린다(비동기 엔진 스레드와의 동기화 지점).
pub fn wait_until(mut condition: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    condition()
}

/// 토큰 공급자가 호출될 때마다 기록하는 스파이.
pub struct RecordingProvider {
    pub result: Mutex<Result<Option<String>, String>>,
    pub calls: AtomicUsize,
}

impl RecordingProvider {
    pub fn with_token(token: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(token.map(str::to_string))),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn failing(message: &str) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Err(message.to_string())),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

impl TokenProvider for RecordingProvider {
    fn token(&self) -> Result<Option<String>, ControlError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &*self.result.lock().unwrap_or_else(|p| p.into_inner()) {
            Ok(token) => Ok(token.clone()),
            Err(message) => Err(ControlError::new(
                viewer_app_core::control::ControlErrorKind::Transport,
                message.clone(),
            )),
        }
    }
}

/// 루프백(평문) 연결을 연다.
pub fn connect_plain(
    transport: &ScriptedTransport,
    host: &str,
    options: ConnectOptions,
) -> ControlClient {
    viewer_app_core::control::connect(transport, host, 7777, 2000, options).expect("connects")
}

/// 응답 없는 요청을 별도 스레드에서 발화한다(TS는 프라미스 배열). 결과 수신기만
/// 돌려준다 — 응답은 나중에 읽는다.
pub fn issue(
    client: &ControlClient,
    command: &str,
) -> Receiver<Result<serde_json::Value, ControlError>> {
    let client = client.clone();
    let command = command.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(client.request_simple(&command, None));
    });
    rx
}

/// 인자를 싣는 변형.
pub fn issue_with_payload(
    client: &ControlClient,
    command: &str,
    args: serde_json::Value,
) -> Receiver<Result<serde_json::Value, ControlError>> {
    let client = client.clone();
    let command = command.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(client.request_simple(&command, Some(&args)));
    });
    rx
}
