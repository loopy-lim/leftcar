//! 앱 전역 제어 세션 상태머신(TS session.ts). 홈 화면이 한 번 연결하면 카탈로그·
//! 스트림 관리가 같은 클라이언트를 재사용한다.
//!
//! TS의 모듈 싱글턴 대신 [`ControlSession`] 인스턴스다. 핵심 의미론:
//! - **선택 세대** — disconnect·새 connectHost가 세대를 올리고, 늦게 끝나는 이전
//!   시도는 자기 세대가 최신이 아니면 소켓을 닫고 물러난다.
//! - **전송 손실 ≠ 사용자 선택** — 소켓이 저절로 닫히면 클라이언트·hostAddr만
//!   지운다(검증된 target·선택은 네이티브 창의 재연결을 위해 남긴다).
//! - **USB 액세서리 프록시** — 붙어 있으면 루프백 포트를 먼저 시도하고 실패하면
//!   네트워크로 폴백한다. 두 경로 모두 이 대상 호스트의 토큰을 쓴다.
//! - **자동 재접속 게이트** — [`auto_reconnect`]와 연결 성공 시 초기화.

pub mod auto_reconnect;
pub mod connect_flow;

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use crate::control::{
    connect, ControlClient, ControlError, ControlErrorKind, TokenProvider,
    DEFAULT_CONNECT_TIMEOUT_MS,
};
use crate::pairing::credential::{
    clear_stored_credential, get_stored_credential, remember_pinned_host_key, PairingError,
    StoredCredentialView,
};
use crate::pairing::ports::{CancelToken, CorePorts};
use crate::pairing::{HostEndpoint, DEFAULT_CONTROL_PORT};

pub use auto_reconnect::{
    should_auto_reconnect, should_reconnect_retained_context, AutoReconnectDecision,
    AutoReconnectGate, AutoReconnectGateSnapshot, ReconnectRouteTarget,
    AUTO_RECONNECT_MIN_INTERVAL_MS,
};
pub use connect_flow::{finish_pairing_attempt, PairingWorkflow, PairingWorkflowRun};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("leftcar:trustedHostError")]
    TrustedHost,
    #[error("host selection superseded")]
    Cancelled,
    #[error("leftcar:errNoReconnectTarget")]
    NoReconnectTarget,
    #[error("connection required")]
    ConnectionRequired,
    #[error("wifi required")]
    WifiRequired,
    #[error("pairing required")]
    PairingRequired,
    #[error("{0}")]
    Store(crate::pairing::StoreError),
    #[error("{0}")]
    Control(ControlError),
    #[error("{0}")]
    Message(String),
}

impl From<ControlError> for SessionError {
    fn from(e: ControlError) -> Self {
        SessionError::Control(e)
    }
}

impl From<crate::pairing::StoreError> for SessionError {
    fn from(e: crate::pairing::StoreError) -> Self {
        SessionError::Store(e)
    }
}

impl From<PairingError> for SessionError {
    fn from(e: PairingError) -> Self {
        match e {
            PairingError::Cancelled => SessionError::Cancelled,
            PairingError::Control(control) => SessionError::Control(control),
            PairingError::Store(store) => SessionError::Store(store),
            PairingError::Message(message) => SessionError::Message(message),
            other => SessionError::Message(other.to_string()),
        }
    }
}

/// 요청의 비동기 출발점에서 붙잡아 둔 컨텍스트(TS SessionRequestContext).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequestContext {
    pub client_id: u64,
    pub target: HostEndpoint,
    pub selection_generation: u64,
    /// Verified network identity; None for the endpoint-bound USB path.
    pub identity: Option<String>,
    /// Exact token incarnation cached by this socket.
    pub credential: Option<StoredCredentialView>,
}

/// Bind pending socket/storage work to both the user selection and its screen.
pub struct Cancellation {
    selection: Arc<CancelToken>,
    action: Option<Arc<CancelToken>>,
}

impl Cancellation {
    pub fn new(selection: &Arc<CancelToken>, action: Option<&Arc<CancelToken>>) -> Self {
        Self {
            selection: Arc::clone(selection),
            action: action.map(Arc::clone),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.selection.is_cancelled()
            || self
                .action
                .as_ref()
                .map(|a| a.is_cancelled())
                .unwrap_or(false)
    }

    pub fn selection(&self) -> &Arc<CancelToken> {
        &self.selection
    }

    pub fn action(&self) -> Option<&Arc<CancelToken>> {
        self.action.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportChoice {
    /// USB 액세서리가 붙어 있으면 루프백을 먼저 시도한다.
    Auto,
    /// Wi-Fi(네트워크) 경로만.
    Wifi,
}

/// Reserve a user-selection generation before any async discovery/storage work.
#[derive(Debug, Clone)]
pub struct HostSelection {
    pub generation: u64,
    pub cancel: Arc<CancelToken>,
}

struct SessionState {
    client: Option<Arc<ControlClient>>,
    host_addr: String,
    host_target: String,
    host_port: u16,
    /// Allocate `count` consecutive viewer-side UDP ports from 5001.
    next_port: u64,
    connect_generation: u64,
    selection_cancel: Arc<CancelToken>,
    active_context: Option<SessionRequestContext>,
    reconnect_in_flight: Option<Arc<ReconnectAttempt>>,
    closed_listeners: Vec<Arc<dyn Fn() + Send + Sync>>,
    notified_closed: bool,
}

impl SessionState {
    fn new() -> Self {
        Self {
            client: None,
            host_addr: String::new(),
            host_target: String::new(),
            host_port: DEFAULT_CONTROL_PORT,
            next_port: 5001,
            connect_generation: 0,
            selection_cancel: CancelToken::new(),
            active_context: None,
            reconnect_in_flight: None,
            closed_listeners: Vec::new(),
            notified_closed: false,
        }
    }
}

/// 공유 재접속 시도 — 같은 세대의 대기자는 한 번의 openControl을 함께 기다린다.
struct ReconnectAttempt {
    generation: u64,
    result: Mutex<Option<Result<Arc<ControlClient>, SessionError>>>,
    done: Condvar,
}

/// 제어 세션. 임베딩이 하나를 들고 다닌다(TS 모듈 싱글턴 대응).
pub struct ControlSession {
    ports: CorePorts,
    state: Mutex<SessionState>,
    gate: Mutex<AutoReconnectGate>,
}

fn lock<'a>(session: &'a ControlSession) -> MutexGuard<'a, SessionState> {
    session.state.lock().unwrap_or_else(|p| p.into_inner())
}

impl ControlSession {
    pub fn new(ports: CorePorts) -> Arc<Self> {
        Arc::new(Self {
            ports,
            state: Mutex::new(SessionState::new()),
            gate: Mutex::new(AutoReconnectGate::new()),
        })
    }

    pub fn ports(&self) -> &CorePorts {
        &self.ports
    }

    /// 게이트 스냅숏 — UI가 자동 재연결 조건을 표시할 때 읽는다.
    pub fn gate_snapshot(&self) -> auto_reconnect::AutoReconnectGateSnapshot {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .snapshot()
    }

    pub fn mark_user_disconnected(&self) {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .mark_user_disconnected();
    }

    pub fn mark_pairing_stale(&self) {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .mark_pairing_stale();
    }

    /// 어떤 경로로든 연결에 성공하면 게이트를 초기화한다.
    pub fn mark_connected(&self) {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .mark_connected();
    }

    pub fn note_auto_reconnect_attempt(&self, now: u64) {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .note_auto_reconnect_attempt(now);
    }

    /// 자동 재접속 단일 진입 — 게이트 판정이 통과하면 시도를 기록해 true.
    pub fn try_auto_entry(&self, has_client: bool, has_recent_host: bool, now: u64) -> bool {
        self.gate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_auto_entry(has_client, has_recent_host, now)
    }

    pub fn begin_host_selection(&self) -> HostSelection {
        let mut st = lock(self);
        st.selection_cancel.cancel();
        st.selection_cancel = CancelToken::new();
        st.connect_generation += 1;
        HostSelection {
            generation: st.connect_generation,
            cancel: st.selection_cancel.clone(),
        }
    }

    pub fn is_host_selection_current(&self, selection: &HostSelection) -> bool {
        lock(self).connect_generation == selection.generation
    }

    pub fn control_client(&self) -> Option<Arc<ControlClient>> {
        lock(self).client.clone()
    }

    pub fn control_host(&self) -> String {
        lock(self).host_addr.clone()
    }

    /// 현재 제어 세션의 대상 엔드포인트(세션이 없으면 None) — 401 시 그 대상의
    /// 토큰만 치운다.
    pub fn control_target(&self) -> Option<HostEndpoint> {
        let st = lock(self);
        if st.host_target.is_empty() {
            None
        } else {
            Some(HostEndpoint {
                host: st.host_target.clone(),
                port: st.host_port,
            })
        }
    }

    /// Capture client, target, selection, and credential at the request's
    /// async origin.
    pub fn capture_request_context(&self) -> Option<SessionRequestContext> {
        lock(self).active_context.clone()
    }

    pub fn is_request_context_current(&self, context: &SessionRequestContext) -> bool {
        let st = lock(self);
        st.active_context.as_ref() == Some(context)
            && (st.client.as_ref().map(|c| c.id()) == Some(context.client_id)
                || st.client.is_none())
            && st.connect_generation == context.selection_generation
    }

    /// 연결 상태가 바뀔 때(성립·끊김·해제) 발화한다. 홈 배지가 이 통지로 진실을
    /// 유지한다 — 죽은 연결을 "연결됨"으로 표시하지 않게 하는 계기.
    pub fn subscribe_connection_changed(self: &Arc<Self>, listener: Arc<dyn Fn() + Send + Sync>) {
        lock(self).closed_listeners.push(listener);
    }

    fn notify_connection_changed(&self) {
        let listeners = lock(self).closed_listeners.clone();
        for listener in listeners {
            listener();
        }
    }

    /// Allocate `count` consecutive viewer-side UDP ports for a new stream
    /// window. Streams reserve their full wire footprint up front: a
    /// splitVertical stream occupies base AND base+1, so the next openDisplay
    /// never receives the split's right-tile port.
    pub fn alloc_ports(&self, count: u32) -> u16 {
        let mut st = lock(self);
        let base = st.next_port;
        st.next_port += count.max(1) as u64;
        base as u16
    }

    /// Allocate the next viewer-side UDP port for a single-socket stream.
    pub fn alloc_port(&self) -> u16 {
        self.alloc_ports(1)
    }

    /// 제어 소켓 하나를 연다(TS openControl). USB 프록시 → 네트워크 폴백.
    fn open_control(
        &self,
        target: &str,
        port: u16,
        cancellation: &Cancellation,
        transport: TransportChoice,
    ) -> Result<OpenedControl, SessionError> {
        let endpoint = HostEndpoint {
            host: target.to_string(),
            port,
        };
        let usb_control_port = match transport {
            TransportChoice::Auto => self.ports.usb.control_port(),
            TransportChoice::Wifi => None,
        };
        if cancellation.is_cancelled() {
            return Err(SessionError::Cancelled);
        }
        if let Some(control_port) = usb_control_port.filter(|&p| p > 0) {
            let credential = get_stored_credential(
                &self.ports,
                &endpoint,
                None,
                Some(cancellation.selection().as_ref()),
                true,
            )?;
            let token = credential.as_ref().map(|c| c.token.clone());
            let result = connect(
                self.ports.transport.as_ref(),
                "127.0.0.1",
                control_port,
                DEFAULT_CONNECT_TIMEOUT_MS,
                crate::control::ConnectOptions::new()
                    .messages(self.ports.messages.clone())
                    .token_provider(Arc::new(crate::control::StaticTokenProvider(token))),
            );
            match result {
                Ok(client) => {
                    return Ok(OpenedControl {
                        client: Arc::new(client),
                        credential,
                        identity: None,
                    })
                }
                Err(_) => {
                    if cancellation.is_cancelled() {
                        return Err(SessionError::Cancelled);
                    }
                    // 루프백 실패는 네트워크 경로로 이어 시도한다.
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(SessionError::Cancelled);
        }
        // Read persistent trust before opening a network socket — 핀 저장이
        // 읽히지 않으면 소켓도 열지 않는다(실패 닫힘).
        let pinned_host_key =
            crate::pairing::credential::get_stored_pinned_host_key(&self.ports, target, port)?;
        if cancellation.is_cancelled() {
            return Err(SessionError::Cancelled);
        }
        let allow_endpoint_credential =
            pinned_host_key.is_some() || target == "127.0.0.1" || target == "localhost";
        let verified: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let verified_for_callback = Arc::clone(&verified);
        let pins = self.ports.pins.clone();
        let target_owned = target.to_string();
        let on_host_key: crate::control::HostKeyCallback = Arc::new(move |key: &str| {
            *verified_for_callback
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = Some(key.to_string());
            // registerPinnedHostKey — 메모리 핀만 등록한다(지속은 자격증명
            // 마이그레이션이 소유한다).
            pins.register(&target_owned, port, key);
        });
        let provider = LazyCredentialProvider {
            ports: self.ports.clone(),
            endpoint: endpoint.clone(),
            verified: Arc::clone(&verified),
            allow_endpoint_credential,
            cache: Mutex::new(None),
            cancel: cancellation
                .action()
                .cloned()
                .or_else(|| Some(cancellation.selection().clone())),
        };
        let network_client = Arc::new(connect(
            self.ports.transport.as_ref(),
            target,
            port,
            DEFAULT_CONNECT_TIMEOUT_MS,
            crate::control::ConnectOptions::new()
                .messages(self.ports.messages.clone())
                .pinned_host_key(pinned_host_key)
                .on_host_key(on_host_key)
                .token_provider(Arc::new(provider)),
        )?);
        let result = (|| -> Result<OpenedControl, SessionError> {
            if cancellation.is_cancelled() {
                return Err(SessionError::Cancelled);
            }
            let verified_host_key = network_client
                .host_key()
                .or_else(|| verified.lock().unwrap_or_else(|p| p.into_inner()).clone());
            let credential = get_stored_credential(
                &self.ports,
                &endpoint,
                verified_host_key.as_deref(),
                Some(cancellation.selection().as_ref()),
                allow_endpoint_credential,
            )?;
            // Credential migration owns the endpoint/identity/recent-host
            // transaction. Only an unpaired connection needs this standalone
            // TOFU persistence; doing it before migration would leave the
            // alias committed if token migration later rolled back.
            if let Some(key) = &verified_host_key {
                if credential.is_none() {
                    remember_pinned_host_key(
                        &self.ports,
                        target,
                        port,
                        key,
                        Some(cancellation.selection().as_ref()),
                    )?;
                }
            }
            Ok(OpenedControl {
                client: Arc::clone(&network_client),
                credential,
                identity: verified_host_key,
            })
        })();
        match result {
            Ok(opened) => Ok(opened),
            Err(error) => {
                // 자격증명 단계가 실패하면 새 소켓을 닫고 오류를 올린다.
                network_client.close();
                Err(error)
            }
        }
    }

    /// 제어 소켓 하나를 열고 세션 상태로 확정한다. 이전 연결은 교체가 성공한 뒤
    /// 끝낸다 — 여러 컴퓨터 사이를 옮겨도 소켓이 새지 않는다.
    pub fn connect_host(
        self: &Arc<Self>,
        host: &str,
        port: u16,
        selection: Option<HostSelection>,
        action_cancel: Option<Arc<CancelToken>>,
    ) -> Result<Arc<ControlClient>, SessionError> {
        if !crate::pairing::is_trusted_host(host) {
            return Err(SessionError::TrustedHost);
        }
        let selection = selection.unwrap_or_else(|| self.begin_host_selection());
        let cancellation = Cancellation::new(&selection.cancel, action_cancel.as_ref());
        if cancellation.is_cancelled() || !self.is_host_selection_current(&selection) {
            return Err(SessionError::Cancelled);
        }
        let opened = self.open_control(host, port, &cancellation, TransportChoice::Auto)?;
        let client = opened.client;
        // 대기 중에 disconnect나 더 새로운 connectHost가 이겼다 — 늦게 도착한
        // 이 소켓은 닫고 상태는 그대로 둔다.
        if cancellation.is_cancelled() || !self.is_host_selection_current(&selection) {
            client.close();
            return Err(SessionError::Cancelled);
        }
        self.commit_connection(
            &client,
            host,
            port,
            &selection,
            opened.identity,
            opened.credential,
        );
        Ok(client)
    }

    /// 후보 주소를 순서대로 시도해 처음 열리는 제어 소켓을 반환한다. 같은 호스트의
    /// LAN 대표 주소와 테일넷 별칭을 하나의 엔트리로 들고 다니기 때문에, 집·밖
    /// 어디서든 호출부는 후보 목록만 넘기면 된다. 취소(선택 교체·화면 종료)는
    /// 즉시 위로 던지고, 순수 네트워크 실패만 다음 후보로 넘어간다. 성공 시 실제로
    /// 연결된 주소를 돌려준다.
    pub fn connect_host_with_fallback(
        self: &Arc<Self>,
        hosts: &[&str],
        port: u16,
        selection: Option<HostSelection>,
        action_cancel: Option<Arc<CancelToken>>,
    ) -> Result<String, SessionError> {
        let mut last_error = SessionError::TrustedHost;
        // All aliases belong to one user selection. A failed older candidate
        // must not create a fresh selection after the user has chosen another
        // Host.
        let selection = selection.unwrap_or_else(|| self.begin_host_selection());
        for host in hosts {
            match self.connect_host(host, port, Some(selection.clone()), action_cancel.clone()) {
                Ok(_) => return Ok((*host).to_string()),
                Err(error) => {
                    // 사용자가 이미 다른 곳으로 떠났다면 남은 후보를 시도하지 않는다.
                    if action_cancel
                        .as_ref()
                        .map(|c| c.is_cancelled())
                        .unwrap_or(false)
                    {
                        return Err(error);
                    }
                    if !self.is_host_selection_current(&selection) {
                        return Err(error);
                    }
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    fn commit_connection(
        self: &Arc<Self>,
        client: &Arc<ControlClient>,
        host: &str,
        port: u16,
        selection: &HostSelection,
        identity: Option<String>,
        credential: Option<StoredCredentialView>,
    ) {
        let previous = {
            let mut st = lock(self);
            let previous = st.client.clone();
            st.client = Some(Arc::clone(client));
            st.host_addr = format!("{host}:{port}");
            st.host_target = host.to_string();
            st.host_port = port;
            st.active_context = Some(SessionRequestContext {
                client_id: client.id(),
                target: HostEndpoint {
                    host: host.to_string(),
                    port,
                },
                selection_generation: selection.generation,
                identity,
                credential,
            });
            previous
        };
        if let Some(previous) = previous {
            if previous.id() != client.id() {
                previous.close();
            }
        }
        self.mark_connected();
        // 소켓이 저절로 닫히면(호스트 재시작·네트워크 전환) 죽은 클라이언트를
        // "연결됨"으로 표시하지 않도록 상태를 즉시 무효화한다. 우리가 close()한
        // 경우엔 disconnectHost가 이미 정리했으므로 이 클라이언트가 아닐 때는
        // 무시한다.
        self.watch_control_close(client);
        self.notify_connection_changed();
    }

    fn watch_control_close(self: &Arc<Self>, client: &Arc<ControlClient>) {
        let weak = Arc::downgrade(self);
        let watched_id = client.id();
        client.when_closed(Arc::new(move || {
            let Some(session) = weak.upgrade() else {
                return;
            };
            {
                let mut st = lock(&session);
                if st.client.as_ref().map(|c| c.id()) != Some(watched_id) {
                    return;
                }
                st.client = None;
                st.host_addr = String::new();
            }
            // Transport loss is not a user selection change. Native windows
            // retain the verified target/selection so they can reconnect
            // without MainActivity.
            session.notify_connection_changed();
        }));
    }

    /// Reopen the control socket after the host app was restarted. 같은 세대의
    /// 대기자는 하나의 시도를 공유한다(TS reconnectInFlight).
    pub fn reconnect_host(
        self: &Arc<Self>,
        origin: Option<SessionRequestContext>,
        cancel: Option<Arc<CancelToken>>,
    ) -> Result<Arc<ControlClient>, SessionError> {
        if cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false) {
            return Err(SessionError::Cancelled);
        }
        let Some(origin) = origin else {
            return Err(SessionError::NoReconnectTarget);
        };
        if !self.is_request_context_current(&origin) {
            return Err(SessionError::Cancelled);
        }
        // join or create the single in-flight attempt for this selection
        let (attempt, is_creator) = {
            let mut st = lock(self);
            match &st.reconnect_in_flight {
                Some(existing) if existing.generation == origin.selection_generation => {
                    (Arc::clone(existing), false)
                }
                _ => {
                    let attempt = Arc::new(ReconnectAttempt {
                        generation: origin.selection_generation,
                        result: Mutex::new(None),
                        done: Condvar::new(),
                    });
                    st.reconnect_in_flight = Some(Arc::clone(&attempt));
                    (attempt, true)
                }
            }
        };
        if is_creator {
            let session = Arc::clone(self);
            let attempt_for_thread = Arc::clone(&attempt);
            let origin_for_thread = origin.clone();
            let selection_cancel = {
                let st = lock(self);
                st.selection_cancel.clone()
            };
            let _ = std::thread::Builder::new()
                .name("viewer-reconnect".into())
                .spawn(move || {
                    let result = session.run_reconnect(&origin_for_thread, &selection_cancel);
                    {
                        let mut slot = attempt_for_thread
                            .result
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        *slot = Some(result);
                    }
                    attempt_for_thread.done.notify_all();
                    // Retirement belongs to the attempt, never an individual
                    // cancelled waiter.
                    let mut st = lock(&session);
                    if st
                        .reconnect_in_flight
                        .as_ref()
                        .map(|a| Arc::ptr_eq(a, &attempt_for_thread))
                        .unwrap_or(false)
                    {
                        st.reconnect_in_flight = None;
                    }
                });
        }
        // A caller owns only its wait, while the selected Host owns the shared
        // socket.
        loop {
            {
                let slot = attempt.result.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(result) = slot.as_ref() {
                    return result.clone();
                }
            }
            if cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false) {
                return Err(SessionError::Cancelled);
            }
            let slot = attempt.result.lock().unwrap_or_else(|p| p.into_inner());
            if slot.is_none() {
                let _ = attempt
                    .done
                    .wait_timeout(slot, Duration::from_millis(5))
                    .unwrap_or_else(|p| p.into_inner());
            }
        }
    }

    fn run_reconnect(
        self: &Arc<Self>,
        origin: &SessionRequestContext,
        selection_cancel: &Arc<CancelToken>,
    ) -> Result<Arc<ControlClient>, SessionError> {
        let cancellation = Cancellation::new(selection_cancel, None);
        let opened = self.open_control(
            &origin.target.host,
            origin.target.port,
            &cancellation,
            TransportChoice::Auto,
        )?;
        let client = opened.client;
        if cancellation.is_cancelled() || !self.is_request_context_current(origin) {
            client.close();
            return Err(SessionError::Cancelled);
        }
        let previous = {
            let mut st = lock(self);
            let previous = st.client.clone();
            st.client = Some(Arc::clone(&client));
            st.host_addr = format!("{}:{}", origin.target.host, origin.target.port);
            st.active_context = Some(SessionRequestContext {
                client_id: client.id(),
                target: origin.target.clone(),
                selection_generation: origin.selection_generation,
                identity: opened.identity,
                credential: opened.credential,
            });
            previous
        };
        if let Some(previous) = previous {
            if previous.id() != client.id() {
                previous.close();
            }
        }
        self.watch_control_close(&client);
        self.mark_connected();
        self.notify_connection_changed();
        Ok(client)
    }

    /// Terminate the active control session and clear client state. 대기 중인
    /// connect/reconnect를 무효화한다 — 늦게 끝나는 시도가 상태를 되살리지
    /// 못하게 세대를 올린다.
    pub fn disconnect_host(&self, context: Option<&SessionRequestContext>) -> bool {
        if let Some(context) = context {
            if !self.is_request_context_current(context) {
                return false;
            }
        }
        let client = {
            let mut st = lock(self);
            st.selection_cancel.cancel();
            st.connect_generation += 1;
            let client = st.client.take();
            st.host_addr = String::new();
            st.host_target = String::new();
            st.active_context = None;
            st.reconnect_in_flight = None;
            let notify = !st.notified_closed;
            st.notified_closed = true;
            let _ = notify;
            client
        };
        if let Some(client) = client {
            client.close();
        }
        self.notify_connection_changed();
        true
    }

    /// Dedicated sealed, pinned, paired Wi-Fi control lease for native input
    /// only(TS openKmControl의 접속 부분 — KM 세션 래퍼는 T-7 km 모듈).
    pub fn open_km_control(
        self: &Arc<Self>,
        cancel: Arc<CancelToken>,
    ) -> Result<Arc<ControlClient>, SessionError> {
        let origin = self.capture_request_context();
        if cancel.is_cancelled() {
            return Err(SessionError::Cancelled);
        }
        let Some(origin) = origin else {
            return Err(SessionError::ConnectionRequired);
        };
        if !self.is_request_context_current(&origin) {
            return Err(SessionError::Cancelled);
        }
        let (host, port) = (origin.target.host.clone(), origin.target.port);
        if host == "127.0.0.1" || host == "localhost" || host == "::1" {
            return Err(SessionError::WifiRequired);
        }
        let pinned =
            crate::pairing::credential::get_stored_pinned_host_key(&self.ports, &host, port)?;
        let Some(pinned) = pinned else {
            return Err(SessionError::PairingRequired);
        };
        let selection_guard = lock(self);
        let cancellation = Cancellation::new(&selection_guard.selection_cancel, Some(&cancel));
        let opened = self.open_control(&host, port, &cancellation, TransportChoice::Wifi)?;
        drop(selection_guard);
        if cancellation.is_cancelled() || !self.is_request_context_current(&origin) {
            opened.client.close();
            return Err(SessionError::Cancelled);
        }
        if opened.identity.as_deref() != Some(pinned.as_str()) || opened.credential.is_none() {
            opened.client.close();
            return Err(SessionError::PairingRequired);
        }
        Ok(opened.client)
    }

    /// 401(토큰 만료)의 자격증명 폐기 절차(TS handleUnauthorized의 코어).
    /// UI 탐항(Alert·router)은 임베딩이 navigate 콜백으로 대신한다.
    pub fn handle_unauthorized(
        self: &Arc<Self>,
        context: HandleUnauthorizedContext<'_>,
    ) -> Result<bool, PairingError> {
        let HandleUnauthorizedContext {
            context: origin,
            mark_stale,
            cancel,
        } = context;
        // Explicit None means the error arrived without a provable request
        // origin. Resolving a mutable global target here could retire a newer
        // host.
        let Some(origin) = origin else {
            return Ok(false);
        };
        if let Some(credential) = &origin.credential {
            clear_stored_credential(&self.ports, credential)?;
        }
        // A late 401 still retires only its own credential incarnation, but
        // it must not tear down or navigate away from a newer user selection.
        if cancel.as_ref().map(|c| c.is_cancelled()).unwrap_or(false)
            || !self.is_request_context_current(origin)
        {
            return Ok(false);
        }
        self.disconnect_host(Some(origin));
        if mark_stale {
            self.mark_pairing_stale();
        }
        Ok(true)
    }
}

/// handle_unauthorized의 인자 묶음. `context: None`은 요청 출발을 증명할 수 없는
/// 401이다 — 전역 target을 해석하지 않고 조용히 끝낸다(TS options.context === null).
#[derive(Default)]
pub struct HandleUnauthorizedContext<'a> {
    pub context: Option<&'a SessionRequestContext>,
    /// 재연결 게이트를 페어링 만료로 막는다 — 조용한 자동 재연결 경로용.
    pub mark_stale: bool,
    /// 화면 이탈(포커스 상실) 취소.
    pub cancel: Option<&'a CancelToken>,
}

struct OpenedControl {
    client: Arc<ControlClient>,
    credential: Option<StoredCredentialView>,
    identity: Option<String>,
}

/// connect()의 토큰 공급자 — 첫 요청 전에 자격증명을 한 번만 해석하고 실패도
/// 기억한다(TS credentialPromise ??= getStoredCredential(...)).
struct LazyCredentialProvider {
    ports: CorePorts,
    endpoint: HostEndpoint,
    verified: Arc<Mutex<Option<String>>>,
    allow_endpoint_credential: bool,
    cache: Mutex<Option<Result<Option<String>, String>>>,
    cancel: Option<Arc<CancelToken>>,
}

impl TokenProvider for LazyCredentialProvider {
    fn token(&self) -> Result<Option<String>, ControlError> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = cache.as_ref() {
            return cached
                .clone()
                .map_err(|message| ControlError::new(ControlErrorKind::Transport, message));
        }
        let verified = self
            .verified
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let result = get_stored_credential(
            &self.ports,
            &self.endpoint,
            verified.as_deref(),
            self.cancel.as_deref(),
            self.allow_endpoint_credential,
        )
        .map(|credential| credential.map(|c| c.token));
        match &result {
            Ok(_) => *cache = Some(result.clone().map_err(|e| e.to_string())),
            Err(error) => *cache = Some(Err(error.to_string())),
        }
        result.map_err(|error| ControlError::new(ControlErrorKind::Transport, error.to_string()))
    }
}

/// 세션 테스트·임베딩이 데드라인을 재정의할 때 쓰는 편의 형태.
pub type TimeoutOverrides = BTreeMap<String, u64>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_allocation_reserves_the_split_neighbor() {
        let ports = CorePorts::new(
            Arc::new(crate::control::transport::TcpTransport),
            Arc::new(crate::pairing::MemoryCredentialStore::new()),
        );
        let session = ControlSession::new(ports);
        // allocPort hands out sequential ports from 5001
        assert_eq!(session.alloc_port(), 5001);
        assert_eq!(session.alloc_port(), 5002);
        assert_eq!(session.alloc_port(), 5003);
    }

    #[test]
    fn port_allocation_clamps_degenerate_counts_to_one_port() {
        let ports = CorePorts::new(
            Arc::new(crate::control::transport::TcpTransport),
            Arc::new(crate::pairing::MemoryCredentialStore::new()),
        );
        let session = ControlSession::new(ports);
        assert_eq!(session.alloc_ports(0), 5001);
        assert_eq!(session.alloc_port(), 5002);
    }
}
