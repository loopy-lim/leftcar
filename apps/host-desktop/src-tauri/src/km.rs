//! Input-only native KM sessions. Capture/source/video lifetimes never enter this module.
use base64::Engine;
use control_contract::km::*;
use domain::km::{
    AndroidToMacTriggerSource, ArrangementSide, KmArrangement, KmCapabilities, KmConsent,
    KmDirection, KmEvent, KmState, KmStateMachine, MacDisplayUnion, TabletOrientation,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const HOST_WATCHDOG: Duration = Duration::from_millis(500);
// Peer cursor placement permits 1000ms/64 steps; reserve another 500ms for delivery.
const PLACEMENT_ACK_TIMEOUT: Duration = Duration::from_millis(1500);
const SEND_INTERVAL: Duration = Duration::from_millis(8);
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_QUEUE: usize = 256;
const MAX_FRAME: usize = MAX_KM_PLAINTEXT + secure_channel::COUNTER_LEN + secure_channel::TAG_LEN;

#[derive(Debug, Clone, Copy)]
pub struct MacReadiness {
    pub capture: bool,
    pub injection: bool,
    pub safe: bool,
}

/// An injectable OS boundary, separate from CaptureBackend and its handles.
pub trait KmBackend: Send + Sync {
    fn preflight(&self) -> MacReadiness;
    fn blocked_reason(&self) -> Option<String> {
        None
    }
    fn bounds(&self) -> Result<MacDisplayUnion, String>;
    fn start(&self, sink: Arc<NativeQueue>) -> Result<(), String>;
    fn owner(&self, owner: KmOwner) -> Result<(), String>;
    fn inject(&self, payload: &KmPayload) -> Result<(), String>;
    fn place(&self, x: f64, y: f64) -> Result<(), String>;
    fn release_all(&self);
    fn stop(&self);
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeEvent {
    Pointer {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        #[serde(default)]
        injected: bool,
        #[serde(default)]
        held: bool,
    },
    Button {
        button: u8,
        down: bool,
    },
    Wheel {
        dx: f64,
        dy: f64,
    },
    Key {
        code: u16,
        down: bool,
    },
    Stop,
}

/// Tap callbacks only try-send to a bounded channel; consumer work cannot block them.
pub struct NativeQueue {
    sender: std::sync::mpsc::SyncSender<NativeEvent>,
    state: Mutex<std::sync::mpsc::Receiver<NativeEvent>>,
    failed: std::sync::atomic::AtomicBool,
}
impl Default for NativeQueue {
    fn default() -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(MAX_QUEUE);
        Self {
            sender,
            state: Mutex::new(receiver),
            failed: std::sync::atomic::AtomicBool::new(false),
        }
    }
}
impl NativeQueue {
    pub fn push(&self, event: NativeEvent) {
        if self.sender.try_send(event).is_err() {
            self.failed
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    fn drain(&self) -> Result<Vec<NativeEvent>, String> {
        if self.failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Native input queue failed; sharing stopped".into());
        }
        let receiver = self.state.lock().unwrap();
        let mut events = Vec::new();
        let mut pointer = None;
        for _ in 0..MAX_QUEUE {
            let Ok(event) = receiver.try_recv() else {
                break;
            };
            if let NativeEvent::Pointer {
                x,
                y,
                dx,
                dy,
                injected,
                held,
            } = event
            {
                let (mut sum_x, mut sum_y) = (dx, dy);
                if let Some(previous) = pointer.take() {
                    match previous {
                        NativeEvent::Pointer {
                            dx,
                            dy,
                            injected: old,
                            ..
                        } if old == injected => {
                            sum_x += dx;
                            sum_y += dy;
                        }
                        previous => events.push(previous),
                    }
                }
                pointer = Some(NativeEvent::Pointer {
                    x,
                    y,
                    dx: sum_x,
                    dy: sum_y,
                    injected,
                    held,
                });
            } else {
                if let Some(pointer) = pointer.take() {
                    events.push(pointer);
                }
                events.push(event);
            }
        }
        if let Some(pointer) = pointer {
            events.push(pointer);
        }
        Ok(events)
    }
    fn clear(&self) {
        let receiver = self.state.lock().unwrap();
        for _ in 0..MAX_QUEUE {
            if receiver.try_recv().is_err() {
                break;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KmPeer {
    device: String,
    generation: u64,
}
impl KmPeer {
    pub(crate) fn from_authorization(auth: &crate::pairing::Authorization) -> Self {
        Self {
            device: auth.device_id().into(),
            generation: auth.generation(),
        }
    }
    pub(crate) fn unpaired(device: &str) -> Self {
        Self {
            device: device.into(),
            generation: u64::MAX,
        }
    }
}
struct Session {
    id: String,
    peer: KmPeer,
    connection: usize,
    input: KmStartInput,
    machine: KmStateMachine,
    epoch: u64,
    rx_seq: u64,
    tx_seq: u64,
    connected: bool,
    stop: Arc<tokio::sync::Notify>,
    queue: Arc<NativeQueue>,
    last_cursor_y: f64,
    return_ready: bool,
    pending_local: Option<KmOwner>,
    pending_since: Option<Instant>,
    placement_since: Option<Instant>,
}
struct ManagerState {
    approvals: HashMap<KmPeer, KmViewerConsent>,
    active: Option<Session>,
    reason: Option<String>,
}
pub struct KmManager {
    state: Mutex<ManagerState>,
    backend: Arc<dyn KmBackend>,
}
impl KmManager {
    pub fn new(backend: Arc<dyn KmBackend>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ManagerState {
                approvals: HashMap::new(),
                active: None,
                reason: None,
            }),
            backend,
        })
    }
    pub fn platform() -> Arc<Self> {
        Self::new(platform_backend())
    }
    pub(crate) fn consent(&self, peer: &KmPeer) -> KmViewerConsent {
        self.state
            .lock()
            .unwrap()
            .approvals
            .get(peer)
            .copied()
            .unwrap_or_default()
    }
    pub(crate) fn record_consent(&self, peer: &KmPeer, consent: KmViewerConsent) {
        self.state
            .lock()
            .unwrap()
            .approvals
            .insert(peer.clone(), consent);
    }
    #[cfg(test)]
    fn set_consent(&self, peer: &KmPeer, consent: KmViewerConsent) {
        self.state
            .lock()
            .unwrap()
            .approvals
            .insert(peer.clone(), consent);
        if !consent.mac_to_android || !consent.android_to_mac {
            self.stop_peer(peer, "Host revoked input-sharing consent", false);
        }
    }
    pub(crate) fn status(&self, peer: &KmPeer) -> KmStatus {
        let native = self.backend.preflight();
        let state = self.state.lock().unwrap();
        let active = state.active.as_ref().filter(|s| &s.peer == peer);
        let android = active
            .map(|s| s.input.android_capabilities)
            .unwrap_or_default();
        let approval = state.approvals.get(peer).copied().unwrap_or_default();
        let blocked = self
            .backend
            .blocked_reason()
            .or_else(|| readiness_reason(native, android, approval))
            .or_else(|| state.reason.clone())
            .or_else(|| {
                active
                    .filter(|s| !s.connected)
                    .map(|_| "Waiting for native input connection".into())
            });
        KmStatus {
            owner: active
                .map(|s| s.pending_local.unwrap_or_else(|| owner(s.machine.state())))
                .unwrap_or_default(),
            armed: active.is_some_and(|s| s.connected && !s.machine.state().is_off()),
            epoch: active.map(|s| s.epoch).unwrap_or(0),
            capabilities: KmReadiness {
                mac_capture: native.capture,
                mac_injection: native.injection,
                android,
            },
            blocked_reason: blocked,
        }
    }
    /// Called only after a sealed remote control command passed pairing authorization.
    pub(crate) async fn start(
        self: &Arc<Self>,
        peer_key: &KmPeer,
        connection: usize,
        peer: std::net::IpAddr,
        input: KmStartInput,
        authorized: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<KmStartResult, String> {
        if peer.is_loopback() || !authorized() {
            return Err("KM requires a secure authenticated Wi-Fi peer".into());
        }
        input.validate().map_err(|e| e.to_string())?;
        if input.tablet_width < 16 || input.tablet_height < 16 {
            return Err("KM display dimensions are too small for safe edge crossing".into());
        }
        if let Some(reason) = self.backend.blocked_reason() {
            return Err(reason);
        }
        let native = self.backend.preflight();
        if let Some(reason) =
            readiness_reason(native, input.android_capabilities, self.consent(peer_key))
        {
            return Err(reason);
        }
        if !input.viewer_consent.mac_to_android || !input.viewer_consent.android_to_mac {
            return Err("Approve both directions on the tablet before sharing input".into());
        }
        self.backend.bounds()?;
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let id = uuid::Uuid::new_v4().to_string();
        let mut seed = [0u8; 32];
        secure_channel::random_bytes(&mut seed);
        let session_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(seed);
        let keys = secure_channel::km_keys(&seed, &id);
        seed.fill(0);
        let stop = Arc::new(tokio::sync::Notify::new());
        let queue = Arc::new(NativeQueue::default());
        {
            let mut state = self.state.lock().unwrap();
            if state.active.is_some() {
                return Err(
                    "Input sharing is already active; stop it before starting another peer".into(),
                );
            }
            state.reason = None;
            state.active = Some(Session {
                id: id.clone(),
                peer: peer_key.clone(),
                connection,
                input: input.clone(),
                machine: KmStateMachine::new(KmCapabilities {
                    mac_to_android: native.capture && native.injection,
                    android_to_mac: input.android_capabilities.keyboard_capture,
                    android_cursor_injection: input.android_capabilities.cursor_injection,
                    android_mouse_capture: input.android_capabilities.mouse_capture,
                    android_observe_and_pass: input.android_capabilities.mouse_observation,
                    ..Default::default()
                }),
                epoch: 1,
                rx_seq: 0,
                tx_seq: 0,
                connected: false,
                stop: stop.clone(),
                queue,
                last_cursor_y: 0.5,
                return_ready: false,
                pending_local: None,
                pending_since: None,
                placement_since: None,
            });
        }
        if !authorized() {
            self.stop_session(&id, "KM peer authorization was revoked", true);
            return Err("KM peer authorization was revoked".into());
        }
        let manager = self.clone();
        let session_id = id.clone();
        tokio::spawn(async move {
            let result = manager
                .run(listener, peer, &session_id, keys, stop, authorized)
                .await;
            let reason = result
                .err()
                .unwrap_or_else(|| "Native input connection closed".into());
            manager.stop_session(&session_id, &reason, true);
        });
        Ok(KmStartResult {
            session_id: id,
            epoch: 1,
            port,
            session_key,
            status: self.status(peer_key),
        })
    }
    pub(crate) fn stop_peer(&self, peer: &KmPeer, reason: &str, expire_consent: bool) {
        let id = self
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .filter(|s| &s.peer == peer)
            .map(|s| s.id.clone());
        if let Some(id) = id {
            self.stop_session(&id, reason, expire_consent);
        }
        if expire_consent {
            self.state.lock().unwrap().approvals.remove(peer);
        }
    }
    pub fn stop_all(&self, reason: &str) {
        let id = self
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .map(|s| s.id.clone());
        if let Some(id) = id {
            self.stop_session(&id, reason, true);
        }
        self.state.lock().unwrap().approvals.clear();
    }
    pub fn control_closed(&self, device: &str, connection: usize) {
        let id = self
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .filter(|s| s.peer.device == device && s.connection == connection && !s.connected)
            .map(|s| s.id.clone());
        if let Some(id) = id {
            self.stop_session(&id, "Control closed before native key proof", true);
        }
    }
    fn stop_session(&self, id: &str, reason: &str, expire_consent: bool) {
        let mut state = self.state.lock().unwrap();
        if state.active.as_ref().is_none_or(|s| s.id != id) {
            return;
        }
        let session = state.active.take().unwrap();
        self.backend.release_all();
        let _ = self.backend.owner(KmOwner::Local);
        self.backend.stop();
        session.stop.notify_one();
        if expire_consent {
            state.approvals.remove(&session.peer);
        }
        state.reason = Some(reason.into());
    }
    fn envelope(session: &mut Session, payload: KmPayload) -> KmEnvelope {
        session.tx_seq += 1;
        KmEnvelope {
            version: 1,
            session_id: session.id.clone(),
            epoch: session.epoch,
            seq: session.tx_seq,
            origin: KmOrigin::Mac,
            payload,
        }
    }
    fn transition(&self, session: &mut Session, event: KmEvent) -> Result<Vec<KmEnvelope>, String> {
        let before = session.machine.state();
        session.machine.on(event);
        if session.machine.state() == before {
            return Ok(Vec::new());
        }
        // Send release in the old epoch before publishing the next owner/epoch.
        self.backend.release_all();
        let release = Self::envelope(session, KmPayload::ReleaseAll);
        let return_cursor = if session.machine.state() == KmState::Armed
            && owner(before) == KmOwner::AndroidToMac
        {
            let width = session.input.tablet_width;
            let height = session.input.tablet_height;
            Some(Self::envelope(
                session,
                KmPayload::Cursor {
                    x: if session.input.arrangement_side == KmArrangementSide::Right {
                        10.0
                    } else {
                        f64::from(width) - 11.0
                    },
                    y: session.last_cursor_y.clamp(0.0, 1.0) * f64::from(height - 1),
                    width,
                    height,
                },
            ))
        } else {
            None
        };
        session.epoch = session.epoch.checked_add(1).ok_or("KM epoch exhausted")?;
        session.placement_since = Some(Instant::now());
        session.queue.clear();
        session.return_ready = false;
        let target = owner(session.machine.state());
        if target == KmOwner::Local {
            // TCP ordering does not prove the peer applied release. Keep old physical
            // suppression until its immediate new-epoch heartbeat confirms state.
            session.pending_local = Some(owner(before));
            session.pending_since = Some(Instant::now());
            session.placement_since = session.pending_since;
        } else {
            self.backend.owner(target)?;
        }
        let mut messages = vec![release];
        if let Some(cursor) = return_cursor {
            messages.push(cursor);
        }
        messages.push(Self::envelope(session, KmPayload::State { owner: target }));
        Ok(messages)
    }
    fn confirm_local(&self, session: &mut Session) -> Result<(), String> {
        if session.pending_local.take().is_none() {
            return Ok(());
        }
        session.pending_since = None;
        session.placement_since = None;
        self.backend.owner(KmOwner::Local)?;
        let bounds = self.backend.bounds()?;
        let x = if session.input.arrangement_side == KmArrangementSide::Left {
            bounds.min_x + 10.0
        } else {
            bounds.min_x + bounds.width - 11.0
        };
        let y = f64::from(bounds.min_y)
            + session.last_cursor_y.clamp(0.0, 1.0) * f64::from(bounds.height - 1.0);
        self.backend.place(f64::from(x), y)?;
        session.queue.clear();
        Ok(())
    }
    fn incoming(&self, id: &str, message: KmEnvelope) -> Result<Vec<KmEnvelope>, String> {
        let mut state = self.state.lock().unwrap();
        let session = state
            .active
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or("KM session stopped")?;
        // Old packets already queued before a host epoch change cannot act on the new owner.
        if message.session_id == id
            && message.origin == KmOrigin::Android
            && message.epoch < session.epoch
        {
            return Ok(Vec::new());
        }
        message
            .validate_for(id, session.epoch, KmOrigin::Android)
            .map_err(|e| e.to_string())?;
        if message.seq <= session.rx_seq {
            return Err("KM sequence replay".into());
        }
        session.rx_seq = message.seq;
        match message.payload {
            KmPayload::Heartbeat => {
                session.placement_since = None;
                self.confirm_local(session)?;
                Ok(Vec::new())
            }
            KmPayload::Stop => Err("Tablet stopped input sharing".into()),
            KmPayload::Cursor {
                x,
                y,
                width,
                height,
            } => {
                if width != session.input.tablet_width || height != session.input.tablet_height {
                    return Err("Tablet cursor dimensions changed; re-arm sharing".into());
                }
                session.last_cursor_y = y / f64::from(height);
                let left = session.input.arrangement_side == KmArrangementSide::Right;
                let inward = if left {
                    x >= 8.0
                } else {
                    x <= f64::from(width) - 9.0
                };
                session.return_ready |= inward;
                Ok(Vec::new())
            }
            KmPayload::Edge { returning: true }
                if owner(session.machine.state()) == KmOwner::MacToAndroid
                    && session.return_ready =>
            {
                self.transition(session, KmEvent::ReturnToLocal)
            }
            KmPayload::Edge { returning: false } if session.machine.state() == KmState::Armed => {
                let result = self.transition(
                    session,
                    KmEvent::AndroidToMacTrigger {
                        source: AndroidToMacTriggerSource::MotionObservationEdge,
                    },
                )?;
                let bounds = self.backend.bounds()?;
                let x = if session.input.arrangement_side == KmArrangementSide::Left {
                    bounds.min_x + 10.0
                } else {
                    bounds.min_x + bounds.width - 11.0
                };
                self.backend.place(
                    f64::from(x),
                    f64::from(bounds.min_y)
                        + session.last_cursor_y * f64::from(bounds.height - 1.0),
                )?;
                Ok(result)
            }
            KmPayload::Edge { .. } => Ok(Vec::new()),
            KmPayload::ReleaseAll => {
                self.backend.release_all();
                Ok(Vec::new())
            }
            payload @ (KmPayload::Move { .. }
            | KmPayload::Button { .. }
            | KmPayload::Wheel { .. }
            | KmPayload::Key { .. }) => {
                if owner(session.machine.state()) != KmOwner::AndroidToMac {
                    return Err("KM input has no current Android ownership".into());
                }
                self.backend.inject(&payload)?;
                Ok(Vec::new())
            }
            _ => Err("Unexpected Android KM payload".into()),
        }
    }
    fn native(&self, id: &str, event: NativeEvent) -> Result<Vec<KmEnvelope>, String> {
        let mut state = self.state.lock().unwrap();
        let session = state
            .active
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or("KM session stopped")?;
        if session.pending_local.is_some() && !matches!(event, NativeEvent::Stop) {
            return Ok(Vec::new());
        }
        match event {
            NativeEvent::Stop => {
                Err("Mac locked, secure input, event tap failed, or emergency escape".into())
            }
            NativeEvent::Pointer {
                x,
                y,
                dx,
                dy,
                injected,
                held,
            } => {
                if !x.is_finite() || !y.is_finite() || !dx.is_finite() || !dy.is_finite() {
                    return Err("Invalid native pointer".into());
                }
                let bounds = self.backend.bounds()?;
                let side = if session.input.arrangement_side == KmArrangementSide::Left {
                    ArrangementSide::Left
                } else {
                    ArrangementSide::Right
                };
                let arrangement =
                    KmArrangement::new(side, 0.0, 1.0, 1.0, TabletOrientation::Portrait)
                        .map_err(|e| format!("{e:?}"))?;
                if session.machine.state() == KmState::Armed && !injected && !held {
                    if let Ok(entry) = arrangement.try_enter(&bounds, x as f32, y as f32) {
                        session.last_cursor_y = f64::from(entry.tablet_y_norm);
                        let mut messages = self
                            .transition(session, KmEvent::MacPointerCrossedSharedEdge { entry })?;
                        if owner(session.machine.state()) == KmOwner::MacToAndroid {
                            let width = session.input.tablet_width;
                            let height = session.input.tablet_height;
                            messages.push(Self::envelope(
                                session,
                                KmPayload::Cursor {
                                    x: if side == ArrangementSide::Right {
                                        2.0
                                    } else {
                                        f64::from(width) - 3.0
                                    },
                                    y: session.last_cursor_y * f64::from(height - 1),
                                    width,
                                    height,
                                },
                            ));
                        }
                        return Ok(messages);
                    }
                } else if owner(session.machine.state()) == KmOwner::AndroidToMac && injected {
                    let distance = if side == ArrangementSide::Left {
                        x - f64::from(bounds.min_x)
                    } else {
                        f64::from(bounds.min_x + bounds.width) - 1.0 - x
                    };
                    session.return_ready |= distance >= 8.0;
                    if distance <= 2.0 && session.return_ready {
                        session.last_cursor_y =
                            (y - f64::from(bounds.min_y)) / f64::from(bounds.height);
                        return self.transition(session, KmEvent::ReturnToLocal);
                    }
                } else if owner(session.machine.state()) == KmOwner::MacToAndroid
                    && !injected
                    && (dx != 0.0 || dy != 0.0)
                {
                    return Ok(vec![Self::envelope(session, KmPayload::Move { dx, dy })]);
                }
                Ok(Vec::new())
            }
            NativeEvent::Button { button, down }
                if owner(session.machine.state()) == KmOwner::MacToAndroid =>
            {
                Ok(vec![Self::envelope(
                    session,
                    KmPayload::Button { button, down },
                )])
            }
            NativeEvent::Wheel { dx, dy }
                if owner(session.machine.state()) == KmOwner::MacToAndroid =>
            {
                Ok(vec![Self::envelope(session, KmPayload::Wheel { dx, dy })])
            }
            NativeEvent::Key { code, down }
                if owner(session.machine.state()) == KmOwner::MacToAndroid =>
            {
                Ok(vec![Self::envelope(session, KmPayload::Key { code, down })])
            }
            _ => Ok(Vec::new()),
        }
    }
    async fn run(
        &self,
        listener: TcpListener,
        peer: std::net::IpAddr,
        id: &str,
        keys: secure_channel::SessionKeys,
        stop: Arc<tokio::sync::Notify>,
        authorized: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(), String> {
        let (mut socket, remote) = tokio::select! {
            _ = stop.notified() => return Err("KM cancelled before native connection".into()),
            result = tokio::time::timeout(Duration::from_secs(3),listener.accept()) => result.map_err(|_|"Native input connect timeout")?.map_err(|e|e.to_string())?,
        };
        if remote.ip() != peer {
            return Err("Native KM peer address mismatch".into());
        }
        socket.set_nodelay(true).map_err(|e| e.to_string())?;
        let mut rx = secure_channel::StreamSealer::new(keys.c2s);
        let mut tx = secure_channel::StreamSealer::new(keys.s2c);
        let hello = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut socket, &mut rx))
            .await
            .map_err(|_| "Native key proof timeout")??;
        if !authorized() {
            return Err("KM peer revoked".into());
        }
        let (queue, initial) = {
            let mut state = self.state.lock().unwrap();
            let peer = state
                .active
                .as_ref()
                .filter(|s| s.id == id)
                .ok_or("KM session stopped")?
                .peer
                .clone();
            let consent = state.approvals.get(&peer).copied().unwrap_or_default();
            let session = state.active.as_mut().unwrap();
            hello
                .validate_for(id, session.epoch, KmOrigin::Android)
                .map_err(|e| e.to_string())?;
            if hello.payload != KmPayload::Hello {
                return Err("Native KM must begin with key proof hello".into());
            }
            let native = self.backend.preflight();
            if let Some(reason) =
                readiness_reason(native, session.input.android_capabilities, consent)
            {
                return Err(reason);
            }
            self.backend.start(session.queue.clone())?;
            session.machine.on(KmEvent::Arm {
                consent: KmConsent {
                    mac_to_android: true,
                    android_to_mac: true,
                },
            });
            if session.machine.state() != KmState::Armed {
                return Err("KM preflight rejected arm".into());
            }
            session.rx_seq = hello.seq;
            session.connected = true;
            (
                session.queue.clone(),
                Self::envelope(
                    session,
                    KmPayload::State {
                        owner: KmOwner::Local,
                    },
                ),
            )
        };
        write_frame(&mut socket, &mut tx, &initial).await?;
        let (mut rd, mut wr) = socket.into_split();
        let mut tick = tokio::time::interval(SEND_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_peer = Instant::now();
        let mut last_heartbeat = Instant::now();
        // Preserve a partial frame read across timer ticks; cancellation would lose framing.
        let (incoming_tx, mut incoming_rx) = tokio::sync::mpsc::channel(32);
        let reader = tokio::spawn(async move {
            loop {
                let result = read_frame(&mut rd, &mut rx).await;
                let failed = result.is_err();
                if incoming_tx.send(result).await.is_err() || failed {
                    break;
                }
            }
        });
        let result = async {
            loop {
                let messages = tokio::select! {
                    biased;
                    _ = stop.notified() => return Err("KM stopped".into()),
                    _ = tick.tick() => {
                        if !authorized() {return Err("KM peer authorization revoked".into())}
                        let placement = self.state.lock().unwrap().active.as_ref().filter(|s|s.id==id).and_then(|s|s.placement_since);
                        if let Some(since) = placement {
                            if since.elapsed() >= PLACEMENT_ACK_TIMEOUT { return Err("Peer did not confirm bounded cursor placement and release".into()) }
                        } else if last_peer.elapsed() >= HOST_WATCHDOG {return Err("KM peer heartbeat timeout".into())}
                        let preflight = self.backend.preflight();
                        if !preflight.capture || !preflight.injection || !preflight.safe {return Err("Mac OS input authority lost".into())}
                        let mut messages = Vec::new();
                        for event in queue.drain()? {messages.extend(self.native(id,event)?)}
                        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                            let mut state = self.state.lock().unwrap();
                            let session = state.active.as_mut().filter(|s|s.id == id).ok_or("KM stopped")?;
                            messages.push(Self::envelope(session,KmPayload::Heartbeat)); last_heartbeat = Instant::now();
                        }
                        messages
                    }
                    received = incoming_rx.recv() => {
                        let message = received.ok_or("Native KM socket closed")??;
                        if !authorized() { return Err("KM peer authorization revoked".into()) }
                        let fresh = self.state.lock().unwrap().active.as_ref().filter(|s|s.id==id).is_some_and(|s|s.epoch==message.epoch);
                        let messages = self.incoming(id,message)?; if fresh {last_peer = Instant::now();} messages
                    }
                };
                tokio::time::timeout(HOST_WATCHDOG, async {
                    for message in messages { write_frame(&mut wr,&mut tx,&message).await?; }
                    Ok::<(),String>(())
                }).await.map_err(|_|"KM bounded batch send timeout")??;
            }
        }.await;
        reader.abort();
        result
    }
}
fn owner(state: KmState) -> KmOwner {
    match state {
        KmState::Active(KmDirection::MacToAndroid) => KmOwner::MacToAndroid,
        KmState::Active(KmDirection::AndroidToMac) => KmOwner::AndroidToMac,
        _ => KmOwner::Local,
    }
}
fn readiness_reason(
    mac: MacReadiness,
    android: KmAndroidCapabilities,
    consent: KmViewerConsent,
) -> Option<String> {
    if !mac.capture {
        Some("Enable Input Monitoring for Leftcar Host in macOS System Settings".into())
    } else if !mac.injection {
        Some("Enable Accessibility for Leftcar Host in macOS System Settings".into())
    } else if !mac.safe {
        Some("Unlock the Mac and leave secure password input before sharing".into())
    } else if !consent.mac_to_android || !consent.android_to_mac {
        Some("Approve both input-sharing directions for this peer on the Mac".into())
    } else if !android.cursor_injection {
        Some("Start Shizuku by USB and approve Leftcar's real mouse input helper".into())
    } else if !android.mouse_observation {
        Some("Android physical mouse observe-and-pass capability is unavailable".into())
    } else if !android.mouse_capture {
        Some("Android physical mouse capture capability is unavailable".into())
    } else if !android.keyboard_capture {
        Some("Enable Leftcar accessibility keyboard capture on the tablet".into())
    } else {
        None
    }
}
async fn read_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    rx: &mut secure_channel::StreamSealer,
) -> Result<KmEnvelope, String> {
    let length = reader.read_u32().await.map_err(|e| e.to_string())? as usize;
    if !(secure_channel::COUNTER_LEN + secure_channel::TAG_LEN..=MAX_FRAME).contains(&length) {
        return Err("KM frame length out of bounds".into());
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    let plaintext = rx.open(&bytes).map_err(|e| e.to_string())?;
    KmEnvelope::decode(&plaintext).map_err(|e| e.to_string())
}
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    tx: &mut secure_channel::StreamSealer,
    message: &KmEnvelope,
) -> Result<(), String> {
    let plaintext = message.encode().map_err(|e| e.to_string())?;
    let frame = tx.seal(&plaintext).map_err(|e| e.to_string())?;
    tokio::time::timeout(HOST_WATCHDOG, async {
        writer.write_u32(frame.len() as u32).await?;
        writer.write_all(&frame).await
    })
    .await
    .map_err(|_| "KM send backpressure timeout")?
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    type Callback = extern "C" fn(*mut c_void, *const c_char);
    struct Sink {
        queue: Arc<NativeQueue>,
    }
    struct NativeBackend {
        library: Result<libloading::Library, String>,
        sink: Mutex<Option<Box<Sink>>>,
    }
    impl NativeBackend {
        fn new() -> Self {
            let mut library = Err("Rebuild the macOS shim with independent KM support".into());
            for path in crate::ffi::dylib_candidates() {
                if path.exists() {
                    library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string());
                    break;
                }
            }
            if let Ok(lib) = &library {
                for symbol in [
                    b"leftcar_km_preflight_v1\0".as_slice(),
                    b"leftcar_km_bounds_v1\0",
                    b"leftcar_km_start_v1\0",
                    b"leftcar_km_owner_v1\0",
                    b"leftcar_km_inject_v1\0",
                    b"leftcar_km_release_v1\0",
                    b"leftcar_km_stop_v2\0",
                ] {
                    if unsafe { lib.get::<unsafe extern "C" fn()>(symbol) }.is_err() {
                        library = Err(
                            "Rebuild the macOS shim with current independent KM support".into()
                        );
                        break;
                    }
                }
            }
            Self {
                library,
                sink: Mutex::new(None),
            }
        }
        unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
            Ok(*self
                .library
                .as_ref()
                .map_err(Clone::clone)?
                .get::<T>(name)
                .map_err(|e| e.to_string())?)
        }
    }
    extern "C" fn callback(context: *mut c_void, json: *const c_char) {
        if context.is_null() || json.is_null() {
            return;
        }
        // Every Swift emission owns a fence lease; stop returns 0 only after all producers drain.
        let sink = unsafe { &*(context as *const Sink) };
        let parsed = unsafe { CStr::from_ptr(json) }.to_bytes();
        match serde_json::from_slice(parsed) {
            Ok(event) => sink.queue.push(event),
            Err(_) => sink.queue.push(NativeEvent::Stop),
        }
    }
    impl KmBackend for NativeBackend {
        fn blocked_reason(&self) -> Option<String> {
            self.library.as_ref().err().cloned()
        }
        fn preflight(&self) -> MacReadiness {
            let flags = unsafe {
                self.symbol::<unsafe extern "C" fn() -> i32>(b"leftcar_km_preflight_v1\0")
                    .map(|f| f())
                    .unwrap_or(0)
            };
            MacReadiness {
                capture: flags & 1 != 0,
                injection: flags & 2 != 0,
                safe: flags & 4 != 0,
            }
        }
        fn bounds(&self) -> Result<MacDisplayUnion, String> {
            let mut bounds = [0.0; 4];
            let result = unsafe {
                self.symbol::<unsafe extern "C" fn(*mut f64) -> i32>(b"leftcar_km_bounds_v1\0")?(
                    bounds.as_mut_ptr(),
                )
            };
            if result != 0 {
                return Err("Mac desktop bounds unavailable".into());
            }
            MacDisplayUnion::new(
                bounds[0] as f32,
                bounds[1] as f32,
                bounds[2] as f32,
                bounds[3] as f32,
            )
            .map_err(|e| format!("{e:?}"))
        }
        fn start(&self, queue: Arc<NativeQueue>) -> Result<(), String> {
            let mut sink = self.sink.lock().unwrap();
            if sink.is_some() {
                return Err("Native KM tap already active".into());
            }
            let mut value = Box::new(Sink { queue });
            let raw = &mut *value as *mut Sink as *mut c_void;
            let start = unsafe {
                self.symbol::<unsafe extern "C" fn(*mut c_void, Callback) -> i32>(
                    b"leftcar_km_start_v1\0",
                )?
            };
            *sink = Some(value);
            let result = unsafe { start(raw, callback) };
            if result != 0 {
                let stopped = unsafe {
                    self.symbol::<unsafe extern "C" fn() -> i32>(b"leftcar_km_stop_v2\0")
                        .map(|stop| stop())
                        .unwrap_or(1)
                };
                if stopped == 0 {
                    sink.take();
                }
                return Err(format!("Mac KM tap could not start (code {result}); check Input Monitoring/Accessibility"));
            }
            Ok(())
        }
        fn owner(&self, owner: KmOwner) -> Result<(), String> {
            let result = unsafe {
                self.symbol::<unsafe extern "C" fn(i32) -> i32>(b"leftcar_km_owner_v1\0")?(
                    match owner {
                        KmOwner::Local => 0,
                        KmOwner::MacToAndroid => 1,
                        KmOwner::AndroidToMac => 2,
                    },
                )
            };
            match result {
                0 => Ok(()),
                -3 => Err(
                    "Release locally held keys and mouse buttons, then re-arm input sharing".into(),
                ),
                _ => Err("Native KM owner update failed".into()),
            }
        }
        fn inject(&self, payload: &KmPayload) -> Result<(), String> {
            self.inject_json(serde_json::to_string(payload).map_err(|e| e.to_string())?)
        }
        fn place(&self, x: f64, y: f64) -> Result<(), String> {
            self.inject_json(serde_json::json!({"kind":"cursor","x":x,"y":y}).to_string())
        }
        fn release_all(&self) {
            if let Ok(f) =
                unsafe { self.symbol::<unsafe extern "C" fn()>(b"leftcar_km_release_v1\0") }
            {
                unsafe { f() }
            }
        }
        fn stop(&self) {
            let stopped = unsafe {
                self.symbol::<unsafe extern "C" fn() -> i32>(b"leftcar_km_stop_v2\0")
                    .map(|stop| stop())
                    .unwrap_or(1)
            };
            // Timeout is an explicit retained lifetime, never permission to free a live sink.
            if stopped == 0 {
                self.sink.lock().unwrap().take();
            }
        }
    }
    impl Drop for NativeBackend {
        fn drop(&mut self) {
            KmBackend::stop(self);
            if let Some(sink) = self.sink.get_mut().unwrap().take() {
                // A stuck callback owns a retained context until process exit.
                let _ = Box::into_raw(sink);
            }
            // Swift/Objective-C classes and captured notification closures remain registered
            // with the runtime. Keep their code loaded even after callback retirement.
            if let Ok(library) =
                std::mem::replace(&mut self.library, Err("KM backend retired".into()))
            {
                std::mem::forget(library);
            }
        }
    }
    impl NativeBackend {
        fn inject_json(&self, json: String) -> Result<(), String> {
            let json = CString::new(json).map_err(|e| e.to_string())?;
            let result = unsafe {
                self.symbol::<unsafe extern "C" fn(*const c_char) -> i32>(
                    b"leftcar_km_inject_v1\0",
                )?(json.as_ptr())
            };
            if result == 0 {
                Ok(())
            } else {
                Err("Mac native injection rejected input".into())
            }
        }
    }
    pub(super) fn backend() -> Arc<dyn KmBackend> {
        Arc::new(NativeBackend::new())
    }
}
#[cfg(target_os = "macos")]
fn platform_backend() -> Arc<dyn KmBackend> {
    native::backend()
}
#[cfg(not(target_os = "macos"))]
fn platform_backend() -> Arc<dyn KmBackend> {
    struct Unsupported;
    impl KmBackend for Unsupported {
        fn preflight(&self) -> MacReadiness {
            MacReadiness {
                capture: false,
                injection: false,
                safe: false,
            }
        }
        fn bounds(&self) -> Result<MacDisplayUnion, String> {
            Err("Independent KM backend is currently macOS only".into())
        }
        fn start(&self, _: Arc<NativeQueue>) -> Result<(), String> {
            Err("KM backend unsupported".into())
        }
        fn owner(&self, _: KmOwner) -> Result<(), String> {
            Err("KM backend unsupported".into())
        }
        fn inject(&self, _: &KmPayload) -> Result<(), String> {
            Err("KM backend unsupported".into())
        }
        fn place(&self, _: f64, _: f64) -> Result<(), String> {
            Err("KM backend unsupported".into())
        }
        fn release_all(&self) {}
        fn stop(&self) {}
    }
    Arc::new(Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;
    #[derive(Default)]
    struct FakeBackend {
        log: Mutex<Vec<String>>,
        queue: Mutex<Option<Arc<NativeQueue>>>,
    }
    impl KmBackend for FakeBackend {
        fn preflight(&self) -> MacReadiness {
            MacReadiness {
                capture: true,
                injection: true,
                safe: true,
            }
        }
        fn bounds(&self) -> Result<MacDisplayUnion, String> {
            Ok(MacDisplayUnion::new(-1000.0, -200.0, 2000.0, 1200.0).unwrap())
        }
        fn start(&self, q: Arc<NativeQueue>) -> Result<(), String> {
            self.log.lock().unwrap().push("start".into());
            *self.queue.lock().unwrap() = Some(q);
            Ok(())
        }
        fn owner(&self, o: KmOwner) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("owner:{o:?}"));
            Ok(())
        }
        fn inject(&self, p: &KmPayload) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("inject:{p:?}"));
            Ok(())
        }
        fn place(&self, x: f64, y: f64) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("place:{x}:{y}"));
            Ok(())
        }
        fn release_all(&self) {
            self.log.lock().unwrap().push("release".into());
        }
        fn stop(&self) {
            self.log.lock().unwrap().push("stop".into());
        }
    }
    fn input() -> KmStartInput {
        KmStartInput {
            arrangement_side: KmArrangementSide::Right,
            tablet_width: 1600,
            tablet_height: 2560,
            viewer_consent: KmViewerConsent {
                mac_to_android: true,
                android_to_mac: true,
            },
            android_capabilities: KmAndroidCapabilities {
                cursor_injection: true,
                mouse_observation: true,
                mouse_capture: true,
                keyboard_capture: true,
            },
        }
    }
    fn test_peer(generation: u64) -> KmPeer {
        KmPeer {
            device: "peer".into(),
            generation,
        }
    }
    fn fixture() -> (Arc<KmManager>, Arc<FakeBackend>) {
        let backend = Arc::new(FakeBackend::default());
        let manager = KmManager::new(backend.clone());
        manager.set_consent(&test_peer(1), input().viewer_consent);
        let mut machine = KmStateMachine::new(KmCapabilities {
            mac_to_android: true,
            android_to_mac: true,
            android_cursor_injection: true,
            android_observe_and_pass: true,
            android_mouse_capture: true,
            ..Default::default()
        });
        machine.on(KmEvent::Arm {
            consent: KmConsent {
                mac_to_android: true,
                android_to_mac: true,
            },
        });
        manager.state.lock().unwrap().active = Some(Session {
            id: "session".into(),
            peer: test_peer(1),
            connection: 42,
            input: input(),
            machine,
            epoch: 1,
            rx_seq: 0,
            tx_seq: 0,
            connected: true,
            stop: Arc::new(tokio::sync::Notify::new()),
            queue: Arc::new(NativeQueue::default()),
            last_cursor_y: 0.5,
            return_ready: false,
            pending_local: None,
            pending_since: None,
            placement_since: None,
        });
        (manager, backend)
    }
    fn incoming(epoch: u64, seq: u64, payload: KmPayload) -> KmEnvelope {
        KmEnvelope {
            version: 1,
            session_id: "session".into(),
            epoch,
            seq,
            origin: KmOrigin::Android,
            payload,
        }
    }
    #[test]
    fn native_producer_never_fails_because_consumer_is_busy() {
        let q = NativeQueue::default();
        let consumer = q.state.lock().unwrap();
        q.push(NativeEvent::Key {
            code: 4,
            down: true,
        });
        drop(consumer);
        assert!(matches!(
            q.drain().unwrap()[0],
            NativeEvent::Key {
                code: 4,
                down: true
            }
        ));
    }

    #[test]
    fn queue_is_bounded_and_does_not_merge_physical_with_injected_motion() {
        let q = NativeQueue::default();
        for injected in [false, true] {
            q.push(NativeEvent::Pointer {
                x: 4.0,
                y: 5.0,
                dx: 3.0,
                dy: 4.0,
                injected,
                held: false,
            });
        }
        let events = q.drain().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            NativeEvent::Pointer {
                injected: false,
                dx: 3.0,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            NativeEvent::Pointer {
                injected: true,
                dx: 3.0,
                ..
            }
        ));
        for _ in 0..MAX_QUEUE {
            q.push(NativeEvent::Key {
                code: 4,
                down: true,
            });
        }
        assert_eq!(q.drain().unwrap().len(), MAX_QUEUE);
        for _ in 0..=MAX_QUEUE {
            q.push(NativeEvent::Key {
                code: 4,
                down: true,
            });
        }
        assert!(q.drain().is_err(), "actual overflow must fail closed");
    }

    #[test]
    fn motion_coalescing_preserves_all_deltas_and_transition_order() {
        let q = NativeQueue::default();
        q.push(NativeEvent::Pointer {
            x: 1.0,
            y: 1.0,
            dx: 2.0,
            dy: -1.0,
            injected: false,
            held: false,
        });
        q.push(NativeEvent::Pointer {
            x: 4.0,
            y: 5.0,
            dx: 3.0,
            dy: 4.0,
            injected: false,
            held: false,
        });
        q.push(NativeEvent::Button {
            button: 0,
            down: true,
        });
        let events = q.drain().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            NativeEvent::Pointer {
                x: 4.0,
                y: 5.0,
                dx: 5.0,
                dy: 3.0,
                ..
            }
        ));
        assert!(matches!(events[1], NativeEvent::Button { down: true, .. }));
    }
    #[test]
    fn held_local_modifier_defers_crossing_and_return_releases_before_restoration() {
        let (manager, backend) = fixture();
        let held = manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 999.0,
                    y: 100.0,
                    dx: 2.0,
                    dy: 0.0,
                    injected: false,
                    held: true,
                },
            )
            .unwrap();
        assert!(held.is_empty());
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::Local);
        let crossed = manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 999.0,
                    y: 100.0,
                    dx: 2.0,
                    dy: 0.0,
                    injected: false,
                    held: false,
                },
            )
            .unwrap();
        assert!(matches!(crossed[0].payload, KmPayload::ReleaseAll));
        assert_eq!(crossed[0].epoch, 1);
        assert!(matches!(
            crossed[1].payload,
            KmPayload::State {
                owner: KmOwner::MacToAndroid
            }
        ));
        assert_eq!(crossed[1].epoch, 2);
        manager
            .incoming(
                "session",
                incoming(
                    2,
                    1,
                    KmPayload::Cursor {
                        x: 100.0,
                        y: 500.0,
                        width: 1600,
                        height: 2560,
                    },
                ),
            )
            .unwrap();
        backend.log.lock().unwrap().clear();
        let returned = manager
            .incoming(
                "session",
                incoming(2, 2, KmPayload::Edge { returning: true }),
            )
            .unwrap();
        assert_eq!(returned[0].epoch, 2);
        assert_eq!(returned[1].epoch, 3);
        assert_eq!(
            manager.status(&test_peer(1)).owner,
            KmOwner::MacToAndroid,
            "suppression must remain until peer applied release"
        );
        assert!(manager.status(&test_peer(1)).armed);
        assert_eq!(*backend.log.lock().unwrap(), vec!["release"]);
        manager
            .incoming("session", incoming(2, 3, KmPayload::Heartbeat))
            .unwrap();
        assert_eq!(
            manager.status(&test_peer(1)).owner,
            KmOwner::MacToAndroid,
            "stale heartbeat cannot confirm release"
        );
        assert!(
            manager
                .native(
                    "session",
                    NativeEvent::Key {
                        code: 4,
                        down: true
                    }
                )
                .unwrap()
                .is_empty(),
            "new physical input must wait for return commit"
        );
        manager
            .incoming("session", incoming(3, 4, KmPayload::Heartbeat))
            .unwrap();
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::Local);
        let log = backend.log.lock().unwrap();
        assert_eq!(log[0], "release");
        assert_eq!(log[1], "owner:Local");
        assert!(log[2].starts_with("place:"));
    }
    #[test]
    fn android_origin_return_sends_mapped_cursor_before_local_state_and_confirmation() {
        let (manager, backend) = fixture();
        manager
            .incoming(
                "session",
                incoming(1, 1, KmPayload::Edge { returning: false }),
            )
            .unwrap();
        manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 900.0,
                    y: 700.0,
                    dx: 0.0,
                    dy: 0.0,
                    injected: true,
                    held: false,
                },
            )
            .unwrap();
        backend.log.lock().unwrap().clear();
        let messages = manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 999.0,
                    y: 700.0,
                    dx: 0.0,
                    dy: 0.0,
                    injected: true,
                    held: false,
                },
            )
            .unwrap();
        assert_eq!(
            messages.len(),
            3,
            "return must carry actual Android cursor placement"
        );
        assert!(matches!(messages[0].payload, KmPayload::ReleaseAll));
        assert_eq!(messages[0].epoch, 2);
        assert!(matches!(
            messages[1].payload,
            KmPayload::Cursor {
                x: 10.0,
                y: 1919.25,
                width: 1600,
                height: 2560
            }
        ));
        assert_eq!(messages[1].epoch, 2);
        assert!(matches!(
            messages[2].payload,
            KmPayload::State {
                owner: KmOwner::Local
            }
        ));
        assert_eq!(messages[2].epoch, 3);
        assert_eq!(*backend.log.lock().unwrap(), vec!["release"]);
        manager
            .incoming("session", incoming(3, 2, KmPayload::Heartbeat))
            .unwrap();
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::Local);
    }

    #[test]
    fn stale_epoch_and_duplicate_transition_cannot_inject_or_steal_current_owner() {
        let (manager, backend) = fixture();
        manager
            .incoming(
                "session",
                incoming(1, 1, KmPayload::Edge { returning: false }),
            )
            .unwrap();
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::AndroidToMac);
        backend.log.lock().unwrap().clear();
        assert!(manager
            .incoming(
                "session",
                incoming(
                    1,
                    2,
                    KmPayload::Key {
                        code: 4,
                        down: true
                    }
                )
            )
            .unwrap()
            .is_empty());
        assert!(backend.log.lock().unwrap().is_empty());
        manager
            .incoming(
                "session",
                incoming(
                    2,
                    3,
                    KmPayload::Key {
                        code: 4,
                        down: true,
                    },
                ),
            )
            .unwrap();
        assert!(manager
            .incoming(
                "session",
                incoming(
                    2,
                    3,
                    KmPayload::Key {
                        code: 4,
                        down: true
                    }
                )
            )
            .is_err());
        assert_eq!(backend.log.lock().unwrap().len(), 1);
    }
    #[test]
    fn revoke_releases_before_local_owner_and_closes_native_lifetime() {
        let (manager, backend) = fixture();
        manager.stop_peer(&test_peer(1), "revoked", true);
        assert_eq!(
            *backend.log.lock().unwrap(),
            vec!["release", "owner:Local", "stop"]
        );
        assert!(!manager.status(&test_peer(1)).armed);
        assert_eq!(manager.consent(&test_peer(1)), KmViewerConsent::default());
    }
    #[test]
    fn native_key_proof_transfers_lifetime_out_of_idle_js_control() {
        let (manager, backend) = fixture();
        manager.control_closed("peer", 42);
        assert!(manager.status(&test_peer(1)).armed);
        assert!(backend.log.lock().unwrap().is_empty());
        manager
            .state
            .lock()
            .unwrap()
            .active
            .as_mut()
            .unwrap()
            .connected = false;
        manager.control_closed("peer", 42);
        assert!(!manager.status(&test_peer(1)).armed);
    }
    #[tokio::test]
    async fn start_denies_unapproved_peer_and_android_authority_before_native_start() {
        let backend = Arc::new(FakeBackend::default());
        let manager = KmManager::new(backend.clone());
        let peer = "192.0.2.1".parse().unwrap();
        assert!(manager
            .start(&test_peer(1), 42, peer, input(), Arc::new(|| true))
            .await
            .is_err());
        manager.set_consent(&test_peer(1), input().viewer_consent);
        let mut denied = input();
        denied.android_capabilities.cursor_injection = false;
        assert!(manager
            .start(&test_peer(1), 42, peer, denied, Arc::new(|| true))
            .await
            .is_err());
        assert!(manager
            .start(&test_peer(1), 42, peer, input(), Arc::new(|| false))
            .await
            .is_err());
        assert!(manager
            .start(
                &test_peer(1),
                42,
                "127.0.0.1".parse().unwrap(),
                input(),
                Arc::new(|| true)
            )
            .await
            .is_err());
        assert!(backend.log.lock().unwrap().is_empty());
        assert!(manager.state.lock().unwrap().active.is_none());
    }
    #[tokio::test]
    async fn sealed_framing_rejects_oversize_and_aead_replay() {
        let key = [7; 32];
        let mut tx = secure_channel::StreamSealer::new(key);
        let mut rx = secure_channel::StreamSealer::new(key);
        let message = incoming(1, 1, KmPayload::Hello);
        let bytes = tx.seal(&message.encode().unwrap()).unwrap();
        let mut framed = Vec::new();
        framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        framed.extend_from_slice(&bytes);
        let mut data = framed.as_slice();
        assert_eq!(read_frame(&mut data, &mut rx).await.unwrap(), message);
        let mut duplicate = framed.as_slice();
        assert!(read_frame(&mut duplicate, &mut rx).await.is_err());
        let invalid = ((MAX_FRAME + 1) as u32).to_be_bytes();
        let mut data = invalid.as_slice();
        assert!(read_frame(&mut data, &mut rx).await.is_err());
    }
    #[tokio::test]
    async fn valid_peer_placement_can_confirm_after_six_hundred_milliseconds() {
        let (manager, backend) = fixture();
        {
            let mut state = manager.state.lock().unwrap();
            let session = state.active.as_mut().unwrap();
            session.connected = false;
            session.machine.on(KmEvent::Disarm);
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let keys = secure_channel::km_keys(&[9; 32], "session");
        let client_keys = secure_channel::km_keys(&[9; 32], "session");
        let stop = manager
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .stop
            .clone();
        let run_manager = manager.clone();
        let task = tokio::spawn(async move {
            let result = run_manager
                .run(
                    listener,
                    "127.0.0.1".parse().unwrap(),
                    "session",
                    keys,
                    stop,
                    Arc::new(|| true),
                )
                .await;
            run_manager.stop_session("session", result.as_ref().err().unwrap(), true);
            result
        });
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let mut tx = secure_channel::StreamSealer::new(client_keys.c2s);
        let mut rx = secure_channel::StreamSealer::new(client_keys.s2c);
        write_frame(&mut socket, &mut tx, &incoming(1, 1, KmPayload::Hello))
            .await
            .unwrap();
        read_frame(&mut socket, &mut rx).await.unwrap();
        manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 999.0,
                    y: 100.0,
                    dx: 2.0,
                    dy: 0.0,
                    injected: false,
                    held: false,
                },
            )
            .unwrap();
        manager
            .incoming(
                "session",
                incoming(
                    2,
                    2,
                    KmPayload::Cursor {
                        x: 100.0,
                        y: 500.0,
                        width: 1600,
                        height: 2560,
                    },
                ),
            )
            .unwrap();
        manager
            .incoming(
                "session",
                incoming(2, 3, KmPayload::Edge { returning: true }),
            )
            .unwrap();
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::MacToAndroid);
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(
            manager.status(&test_peer(1)).armed,
            "valid Android placement may take more than 500ms"
        );
        write_frame(&mut socket, &mut tx, &incoming(3, 4, KmPayload::Heartbeat))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::Local);
        manager.stop_session("session", "test finished", false);
        assert!(task.await.unwrap().is_err());
        assert!(backend
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|item| item == "owner:Local"));
    }

    #[tokio::test]
    async fn pending_release_confirmation_times_out_despite_stale_heartbeats() {
        let (manager, backend) = fixture();
        {
            let mut state = manager.state.lock().unwrap();
            let session = state.active.as_mut().unwrap();
            session.connected = false;
            session.machine.on(KmEvent::Disarm);
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let keys = secure_channel::km_keys(&[9; 32], "session");
        let client_keys = secure_channel::km_keys(&[9; 32], "session");
        let stop = manager
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .stop
            .clone();
        let run_manager = manager.clone();
        let task = tokio::spawn(async move {
            let result = run_manager
                .run(
                    listener,
                    "127.0.0.1".parse().unwrap(),
                    "session",
                    keys,
                    stop,
                    Arc::new(|| true),
                )
                .await;
            run_manager.stop_session("session", result.as_ref().err().unwrap(), true);
            result
        });
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let mut tx = secure_channel::StreamSealer::new(client_keys.c2s);
        let mut rx = secure_channel::StreamSealer::new(client_keys.s2c);
        write_frame(&mut socket, &mut tx, &incoming(1, 1, KmPayload::Hello))
            .await
            .unwrap();
        read_frame(&mut socket, &mut rx).await.unwrap();
        manager
            .native(
                "session",
                NativeEvent::Pointer {
                    x: 999.0,
                    y: 100.0,
                    dx: 2.0,
                    dy: 0.0,
                    injected: false,
                    held: false,
                },
            )
            .unwrap();
        manager
            .incoming(
                "session",
                incoming(
                    2,
                    2,
                    KmPayload::Cursor {
                        x: 100.0,
                        y: 500.0,
                        width: 1600,
                        height: 2560,
                    },
                ),
            )
            .unwrap();
        manager
            .incoming(
                "session",
                incoming(2, 3, KmPayload::Edge { returning: true }),
            )
            .unwrap();
        assert_eq!(manager.status(&test_peer(1)).owner, KmOwner::MacToAndroid);
        for seq in 4..21 {
            if write_frame(
                &mut socket,
                &mut tx,
                &incoming(2, seq, KmPayload::Heartbeat),
            )
            .await
            .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let result = tokio::time::timeout(Duration::from_millis(200), task)
            .await
            .expect("stale epochs must not extend release deadline")
            .unwrap();
        assert!(result.is_err());
        assert!(!manager.status(&test_peer(1)).armed);
        let log = backend.log.lock().unwrap();
        assert_eq!(&log[log.len() - 3..], ["release", "owner:Local", "stop"]);
    }

    #[tokio::test]
    async fn native_hello_and_watchdog_operate_without_decoder_or_surface() {
        let (manager, backend) = fixture();
        {
            let mut state = manager.state.lock().unwrap();
            let session = state.active.as_mut().unwrap();
            session.connected = false;
            session.machine.on(KmEvent::Disarm);
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let keys = secure_channel::km_keys(&[9; 32], "session");
        let client_keys = secure_channel::km_keys(&[9; 32], "session");
        let stop = manager
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .stop
            .clone();
        let run_manager = manager.clone();
        let task = tokio::spawn(async move {
            let result = run_manager
                .run(
                    listener,
                    "127.0.0.1".parse().unwrap(),
                    "session",
                    keys,
                    stop,
                    Arc::new(|| true),
                )
                .await;
            run_manager.stop_session("session", result.as_ref().err().unwrap(), true);
            result
        });
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let mut tx = secure_channel::StreamSealer::new(client_keys.c2s);
        let mut rx = secure_channel::StreamSealer::new(client_keys.s2c);
        write_frame(&mut socket, &mut tx, &incoming(1, 1, KmPayload::Hello))
            .await
            .unwrap();
        let response = read_frame(&mut socket, &mut rx).await.unwrap();
        assert!(matches!(
            response.payload,
            KmPayload::State {
                owner: KmOwner::Local
            }
        ));
        assert!(manager.status(&test_peer(1)).armed);
        manager.control_closed("peer", 42);
        assert!(manager.status(&test_peer(1)).armed);
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.unwrap_err().contains("heartbeat timeout"));
        let log = backend.log.lock().unwrap();
        assert_eq!(log[0], "start");
        assert_eq!(&log[log.len() - 3..], ["release", "owner:Local", "stop"]);
        assert!(!manager.status(&test_peer(1)).armed);
    }
}
