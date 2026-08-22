//! Control server: viewers connect (pull) and exchange newline-delimited
//! JSON `{"command","args"}` / `{"ok":true,"result"}` over TCP (design §제어평면).
//!
//! `addNumbers` delegates to the real rustra host_package so the H02 proof
//! path stays intact; stateful v1 stream commands dispatch locally.

use crate::backend::SharedBackend;
use control_contract::host::{
    CatalogView, SessionView, StartStreamInput, StartStreamOutput, StatusView, StopStreamInput,
};

pub use control_contract::host::{StatsInfo, StatusView as StatusViewPublic};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

struct Session {
    handle: u32,
    source_index: u32,
    source_name: String,
    viewer_addr: String,
    input_enabled: bool,
    input_rate_hz: u32,
    terminal_since: Option<Instant>,
}

const TERMINAL_SESSION_RETENTION: Duration = Duration::from_secs(5);

pub struct ControlServer {
    backend: SharedBackend,
    pairing: std::sync::Arc<crate::pairing::PairingServer>,
    sessions: Mutex<State>,
}

struct State {
    next: u32,
    live: HashMap<u32, Session>,
}

impl ControlServer {
    pub fn new(
        backend: SharedBackend,
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
    ) -> Self {
        Self {
            backend,
            pairing,
            sessions: Mutex::new(State {
                next: 1,
                live: HashMap::new(),
            }),
        }
    }

    pub async fn bind(&self, addr: &str) -> std::io::Result<std::net::SocketAddr> {
        TcpListener::bind(addr).await?.local_addr()
    }

    /// Accept loop — runs until the process exits.
    pub async fn run(self: std::sync::Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((sock, _)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        let peer = sock
                            .peer_addr()
                            .map(|a| a.ip().to_string())
                            .unwrap_or_default();
                        handle_conn(sock, &server, &peer).await;
                    });
                }
                Err(e) => {
                    eprintln!("control accept error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Snapshot for the Tauri UI (`get_status` command reuses this).
    pub fn snapshot(&self) -> StatusView {
        let now = Instant::now();
        let (sessions, expired) = {
            let mut state = self.sessions.lock().unwrap();
            let mut sessions = Vec::with_capacity(state.live.len());
            let mut expired_ids = Vec::new();

            for (id, s) in &mut state.live {
                let metrics = self.backend.stats(s.handle).unwrap_or_else(|_| StatsInfo {
                    frames: 0,
                    bytes: 0,
                    state: "stopped".into(),
                    fps: 0,
                    kbps: 0,
                    fps_target: 0,
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
                    capture_backend: "unknown".into(),
                    media_transport: "unknown".into(),
                    first_capture_ms: 0,
                    first_encode_ms: 0,
                    first_send_ms: 0,
                    current_bitrate: 0,
                    capture_interval_p95_us: 0,
                    capture_to_encode_p95_us: 0,
                    capture_queue_wait_p95_us: 0,
                    encode_output_p95_us: 0,
                    send_block_p95_us: 0,
                    error: Some("backend stats unavailable".into()),
                });

                let terminal = matches!(metrics.state.as_str(), "error" | "stopped" | "unknown");
                if terminal {
                    let terminal_since = s.terminal_since.get_or_insert(now);
                    if now.duration_since(*terminal_since) >= TERMINAL_SESSION_RETENTION {
                        expired_ids.push(*id);
                        continue;
                    }
                } else {
                    s.terminal_since = None;
                }

                sessions.push(SessionView {
                    session: *id,
                    source_index: s.source_index,
                    source_name: s.source_name.clone(),
                    viewer_addr: s.viewer_addr.clone(),
                    state: metrics.state,
                    fps: metrics.fps,
                    kbps: metrics.kbps,
                    fps_target: metrics.fps_target,
                    input_enabled: s.input_enabled,
                    input_rate_hz: s.input_rate_hz,
                    dropped: metrics.dropped,
                    network_dropped: metrics.network_dropped,
                    capture_queue_dropped: metrics.capture_queue_dropped,
                    capture_to_encode_us: metrics.capture_to_encode_us,
                    max_capture_to_encode_us: metrics.max_capture_to_encode_us,
                    capture_queue_wait_us: metrics.capture_queue_wait_us,
                    max_capture_queue_wait_us: metrics.max_capture_queue_wait_us,
                    encode_output_us: metrics.encode_output_us,
                    max_encode_output_us: metrics.max_encode_output_us,
                    send_block_us: metrics.send_block_us,
                    max_send_block_us: metrics.max_send_block_us,
                    pending_frame: metrics.pending_frame,
                    frames: metrics.frames,
                    bytes: metrics.bytes,
                    capture_backend: metrics.capture_backend,
                    media_transport: metrics.media_transport,
                    first_capture_ms: metrics.first_capture_ms,
                    first_encode_ms: metrics.first_encode_ms,
                    first_send_ms: metrics.first_send_ms,
                    current_bitrate: metrics.current_bitrate,
                    capture_interval_p95_us: metrics.capture_interval_p95_us,
                    capture_to_encode_p95_us: metrics.capture_to_encode_p95_us,
                    capture_queue_wait_p95_us: metrics.capture_queue_wait_p95_us,
                    encode_output_p95_us: metrics.encode_output_p95_us,
                    send_block_p95_us: metrics.send_block_p95_us,
                    error: metrics.error,
                });
            }

            let expired = expired_ids
                .into_iter()
                .filter_map(|id| state.live.remove(&id))
                .collect::<Vec<_>>();
            (sessions, expired)
        };

        for session in expired {
            if let Err(error) = self.backend.stop(session.handle) {
                eprintln!(
                    "failed to release terminal session {}: {error}",
                    session.handle
                );
            }
        }

        StatusView { sessions }
    }

    pub fn input_permission(&self) -> Result<bool, String> {
        self.backend.input_permission()
    }

    pub fn request_input_permission(&self) -> Result<bool, String> {
        self.backend.request_input_permission()
    }

    pub fn set_session_input(&self, session_id: u32, enabled: bool) -> Result<(), String> {
        let handle = {
            let state = self.sessions.lock().unwrap();
            state
                .live
                .get(&session_id)
                .map(|session| session.handle)
                .ok_or_else(|| format!("no such session {session_id}"))?
        };
        self.backend.set_input_enabled(handle, enabled)?;
        let mut state = self.sessions.lock().unwrap();
        let session = state
            .live
            .get_mut(&session_id)
            .ok_or_else(|| format!("session {session_id} ended while changing input"))?;
        session.input_enabled = enabled;
        Ok(())
    }

    /// A viewer restart can leave the old capture handle alive until its TCP
    /// write notices the closed socket. Do not allow two hosts to push into
    /// the same viewer endpoint: their H.264 AU ids would interleave and the
    /// receiver would correctly enter keyframe recovery over and over.
    fn stop_sessions_for_viewer(&self, viewer_addr: &str) {
        let stale = {
            let mut state = self.sessions.lock().unwrap();
            let ids: Vec<u32> = state
                .live
                .iter()
                .filter_map(|(id, session)| (session.viewer_addr == viewer_addr).then_some(*id))
                .collect();
            ids.into_iter()
                .filter_map(|id| state.live.remove(&id))
                .collect::<Vec<_>>()
        };

        for session in stale {
            if let Err(error) = self.backend.stop(session.handle) {
                eprintln!(
                    "failed to stop stale viewer session {}: {error}",
                    session.handle
                );
            }
        }
    }

    async fn dispatch(
        &self,
        command: &str,
        args: serde_json::Value,
        viewer_ip: &str,
    ) -> serde_json::Value {
        match command {
            "beginPairing" => {
                // Local operator/diagnostic entry point. This exposes the same
                // short-lived two-factor offer as the Tauri pairing window,
                // but it is never reachable from a LAN or tailnet peer.
                if !is_loopback_peer(viewer_ip) {
                    return err("unauthorized");
                }
                let Some(host_ip) = crate::local_lan_ip() else {
                    return err("no LAN interface found");
                };
                ok(self.pairing.begin_pairing(&host_ip, crate::CONTROL_PORT))
            }
            "pair" => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct PairArgs {
                    offer_id: String,
                    secret: String,
                    code: String,
                    device_id: String,
                    #[serde(default)]
                    device_name: String,
                }
                let input: PairArgs = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                // The error is deliberately generic: never reveal whether the
                // secret or the human code was the wrong factor.
                match self.pairing.pair(
                    &input.offer_id,
                    &input.secret,
                    &input.code,
                    &input.device_id,
                    &input.device_name,
                ) {
                    Ok(token) => ok(json!({ "token": token })),
                    Err(_) => err("pairing failed"),
                }
            }
            "getCatalog" => match self.backend.list_displays() {
                Ok(displays) => ok(CatalogView { displays }),
                Err(e) => err(&e),
            },
            "startStream" => {
                let input: StartStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                if input.width == 0
                    || input.height == 0
                    || input.width > 8192
                    || input.height > 8192
                    || input.fps == 0
                    || input.fps > 90
                {
                    return err("unsupported stream dimensions or fps");
                }
                if !matches!(
                    input.capture_backend.as_str(),
                    "screenCaptureKit" | "cgDisplayStream"
                ) {
                    return err("unsupported capture backend");
                }
                let name = self
                    .backend
                    .list_displays()
                    .ok()
                    .and_then(|d| d.get(input.source_index as usize).cloned())
                    .map(|d| d.name)
                    .unwrap_or_else(|| format!("display {}", input.source_index));
                // A non-bypassable VPN can route a local control connection
                // through a LAN subnet router, so its TCP peer is not always
                // the viewer's physical Wi-Fi address. Consider claimed
                // addresses only when they are private and on the peer's /24.
                // The production UDP backend performs a nonce reachability
                // proof before capture, preventing arbitrary redirection.
                let mut candidates = input
                    .viewer_ips
                    .iter()
                    .take(4)
                    .filter(|candidate| same_private_lan_candidate(candidate, viewer_ip))
                    .cloned()
                    .collect::<Vec<_>>();
                if !candidates.iter().any(|candidate| candidate == viewer_ip) {
                    candidates.push(viewer_ip.to_owned());
                }
                let mut last_error = None;
                let mut started = None;
                for candidate in candidates {
                    let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                    self.stop_sessions_for_viewer(&viewer_addr);
                    match self.backend.start(
                        input.source_index,
                        &candidate,
                        input.viewer_port,
                        input.width,
                        input.height,
                        input.fps,
                        &input.capture_backend,
                    ) {
                        Ok(handle) => {
                            started = Some((handle, candidate));
                            break;
                        }
                        Err(e) => last_error = Some(format!("{candidate}: {e}")),
                    }
                }
                match started {
                    Some((handle, candidate)) => {
                        let viewer_addr = format!("{candidate}:{}", input.viewer_port);
                        let session_id = {
                            let mut st = self.sessions.lock().unwrap();
                            let id = st.next;
                            st.next += 1;
                            st.live.insert(
                                id,
                                Session {
                                    handle,
                                    source_index: input.source_index,
                                    source_name: name,
                                    viewer_addr,
                                    input_enabled: false,
                                    input_rate_hz: input.fps.saturating_mul(2).clamp(30, 240),
                                    terminal_since: None,
                                },
                            );
                            id
                        };
                        ok(StartStreamOutput {
                            session: session_id,
                        })
                    }
                    None => err(&format!(
                        "all viewer addresses failed: {}",
                        last_error.unwrap_or_else(|| "no viewer addresses".into())
                    )),
                }
            }
            "stopStream" => {
                let input: StopStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                let removed = {
                    let mut st = self.sessions.lock().unwrap();
                    st.live.remove(&input.session)
                };
                match removed {
                    Some(s) => match self.backend.stop(s.handle) {
                        Ok(()) => ok(json!({})),
                        Err(e) => err(&e),
                    },
                    None => err(&format!("no such session {}", input.session)),
                }
            }
            "getStatus" => ok(self.snapshot()),
            _ => {
                // delegate stateless commands to the real rustra package (H02 path)
                match control_contract::host::host_package().invoke_json(command, args) {
                    Ok(v) => ok(v),
                    Err(e) => err(&e.to_string()),
                }
            }
        }
    }
}

async fn handle_conn(sock: TcpStream, server: &ControlServer, peer: &str) {
    let (rd, mut wr) = sock.into_split();
    let mut lines = BufReader::new(rd).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Result<(String, serde_json::Value, Option<String>), String> =
            serde_json::from_str(&line)
                .map(|v: Envelope| (v.command, v.args, v.token))
                .map_err(|e| format!("bad request: {e}"));
        let resp = match parsed {
            Ok((cmd, args, token)) => {
                // Auth gate: `pair` is the only command reachable without a
                // token over the network. The loopback-only beginPairing
                // command is an operator diagnostic equivalent to opening the
                // local Tauri pairing panel. Anything else requires a token
                // issued by a completed pairing; on failure the connection is
                // closed, not just the request rejected (design §2).
                let local_pairing = cmd == "beginPairing" && is_loopback_peer(peer);
                if cmd != "pair"
                    && !local_pairing
                    && !server.pairing.authorize(token.as_deref().unwrap_or(""))
                {
                    write_line(
                        &mut wr,
                        &serde_json::to_string(&err("unauthorized")).unwrap_or_default(),
                    )
                    .await;
                    break;
                }
                let out = server.dispatch(&cmd, args, peer).await;
                serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
            }
            Err(e) => format!("{{\"ok\":false,\"error\":{}}}", json!(e)),
        };
        if wr.write_all(resp.as_bytes()).await.is_err() || wr.write_all(b"\n").await.is_err() {
            break;
        }
    }
}

fn is_loopback_peer(peer: &str) -> bool {
    peer.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn same_private_lan_candidate(candidate: &str, peer: &str) -> bool {
    let Ok(candidate) = candidate.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let Ok(peer) = peer.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let candidate_octets = candidate.octets();
    let peer_octets = peer.octets();
    candidate.is_private()
        && peer.is_private()
        && candidate_octets[..3] == peer_octets[..3]
}

async fn write_line(wr: &mut tokio::net::tcp::OwnedWriteHalf, body: &str) {
    let _ = wr.write_all(body.as_bytes()).await;
    let _ = wr.write_all(b"\n").await;
}

#[derive(serde::Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    token: Option<String>,
}

fn ok<T: serde::Serialize>(result: T) -> serde_json::Value {
    json!({ "ok": true, "result": result })
}

fn err(error: &str) -> serde_json::Value {
    json!({ "ok": false, "error": error })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CaptureBackend, FakeBackend};
    use control_contract::host::DisplayInfo;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn backend() -> SharedBackend {
        Arc::new(FakeBackend {
            displays: vec![DisplayInfo {
                index: 0,
                name: "Main".into(),
                width: 1920,
                height: 1080,
            }],
        })
    }

    struct TerminalBackend {
        stopped: Arc<AtomicUsize>,
    }

    impl CaptureBackend for TerminalBackend {
        fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
            Ok(Vec::new())
        }

        fn start(
            &self,
            _source_index: u32,
            _ip: &str,
            _port: u16,
            _w: u32,
            _h: u32,
            _fps: u32,
            _capture_backend: &str,
        ) -> Result<u32, String> {
            Ok(7)
        }

        fn stop(&self, _handle: u32) -> Result<(), String> {
            self.stopped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
            Ok(StatsInfo {
                frames: 10,
                bytes: 1_000,
                state: "error".into(),
                fps: 0,
                kbps: 0,
                fps_target: 60,
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
                capture_backend: "screenCaptureKit".into(),
                media_transport: "udp".into(),
                first_capture_ms: 20,
                first_encode_ms: 25,
                first_send_ms: 26,
                current_bitrate: 12_000_000,
                capture_interval_p95_us: 16_667,
                capture_to_encode_p95_us: 8_000,
                capture_queue_wait_p95_us: 1_000,
                encode_output_p95_us: 7_000,
                send_block_p95_us: 1_000,
                error: Some("viewer closed stream".into()),
            })
        }
    }

    async fn spawn_server() -> std::net::SocketAddr {
        spawn_server_with_pairing(test_pairing()).await
    }

    fn test_pairing() -> std::sync::Arc<crate::pairing::PairingServer> {
        std::sync::Arc::new(crate::pairing::PairingServer::new(
            "leftcar-host".into(),
            None,
        ))
    }

    async fn spawn_server_with_pairing(
        pairing: std::sync::Arc<crate::pairing::PairingServer>,
    ) -> std::net::SocketAddr {
        let server = std::sync::Arc::new(ControlServer::new(backend(), pairing));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { server.run(listener).await });
        addr
    }

    async fn request(
        sock: &mut tokio::net::TcpStream,
        cmd: &str,
        args: &str,
        token: &str,
    ) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(
            format!("{{\"command\":\"{cmd}\",\"args\":{args},\"token\":\"{token}\"}}\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            sock.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        String::from_utf8(buf).unwrap()
    }

    /// Pair via the real pairing flow and return the issued token.
    async fn pair_token(
        sock: &mut tokio::net::TcpStream,
        pairing: &crate::pairing::PairingServer,
    ) -> String {
        let view = pairing.begin_pairing("127.0.0.1", 7777);
        let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
        let args = json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": view.code,
            "deviceId": "test-viewer",
            "deviceName": "Test Viewer",
        });
        let line = request(sock, "pair", &args.to_string(), "").await;
        let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        resp["result"]["token"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn catalog_start_status_stop_roundtrip() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "getCatalog", "{}", &token).await;
        assert!(line.contains("\"displays\""), "{line}");

        let line = request(
            &mut sock,
            "startStream",
            r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
            &token,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");

        let line = request(&mut sock, "getStatus", "{}", &token).await;
        assert!(line.contains("\"state\":\"running\""), "{line}");
        assert!(line.contains("\"inputEnabled\":false"), "{line}");
        assert!(line.contains("\"inputRateHz\":180"), "{line}");

        let line = request(&mut sock, "stopStream", r#"{"session":1}"#, &token).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    #[tokio::test]
    async fn add_numbers_delegates_to_rustra() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;
        let line = request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#, &token).await;
        assert!(line.contains("\"value\":42"), "{line}");
    }

    #[tokio::test]
    async fn unknown_command_and_restart_session_ids() {
        let pairing = test_pairing();
        let addr = spawn_server_with_pairing(pairing.clone()).await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let token = pair_token(&mut sock, &pairing).await;

        let line = request(&mut sock, "nope", "{}", &token).await;
        assert!(line.contains("\"ok\":false"), "{line}");

        for i in 1..=2 {
            let line = request(
                &mut sock,
                "startStream",
                r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
                &token,
            )
            .await;
            assert!(line.contains(&format!("\"session\":{i}")), "{line}");
            let line = request(
                &mut sock,
                "stopStream",
                &format!("{{\"session\":{i}}}"),
                &token,
            )
            .await;
            assert!(line.contains("\"ok\":true"), "{line}");
        }
    }

    #[tokio::test]
    async fn unauthenticated_getcatalog_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
        assert!(line.contains("\"ok\":false"), "{line}");

        // connection is closed: the next request hits EOF
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(b"{\"command\":\"getCatalog\",\"args\":{}}\n")
            .await
            .unwrap();
        let mut buf = [0u8; 16];
        let n = sock.read(&mut buf).await.unwrap();
        assert_eq!(n, 0, "connection must be closed after unauthorized");
    }

    #[tokio::test]
    async fn loopback_operator_can_begin_pairing_without_a_token() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "beginPairing", "{}", "").await;
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], true, "{line}");
        assert_eq!(response["result"]["expires_in_secs"], 120, "{line}");
        assert_eq!(response["result"]["code"].as_str().unwrap().len(), 6);
        let payload: serde_json::Value =
            serde_json::from_str(response["result"]["qr_payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["v"], 1);
        assert_eq!(payload["p"], crate::CONTROL_PORT);
    }

    #[test]
    fn operator_pairing_is_strictly_loopback_only() {
        assert!(is_loopback_peer("127.0.0.1"));
        assert!(is_loopback_peer("::1"));
        assert!(!is_loopback_peer("192.168.0.18"));
        assert!(!is_loopback_peer("100.77.109.50"));
        assert!(!is_loopback_peer("localhost"));
    }

    #[test]
    fn media_candidate_must_be_private_and_on_the_control_peers_lan() {
        assert!(same_private_lan_candidate(
            "192.168.0.18",
            "192.168.0.170"
        ));
        assert!(!same_private_lan_candidate(
            "192.168.1.18",
            "192.168.0.170"
        ));
        assert!(!same_private_lan_candidate("1.2.3.4", "192.168.0.170"));
        assert!(!same_private_lan_candidate(
            "192.168.0.18",
            "100.77.109.50"
        ));
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}", "deadbeef").await;
        assert!(line.contains("\"error\":\"unauthorized\""), "{line}");
    }

    #[test]
    fn terminal_sessions_are_retained_briefly_then_released() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(
            Arc::new(TerminalBackend {
                stopped: stopped.clone(),
            }),
            test_pairing(),
        );
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                viewer_addr: "192.168.0.2:5001".into(),
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
            },
        );

        let first = server.snapshot();
        assert_eq!(first.sessions.len(), 1);
        assert_eq!(
            first.sessions[0].error.as_deref(),
            Some("viewer closed stream")
        );

        server
            .sessions
            .lock()
            .unwrap()
            .live
            .get_mut(&1)
            .unwrap()
            .terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        let second = server.snapshot();
        assert!(second.sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn remote_input_is_host_opt_in_per_session() {
        let server = ControlServer::new(backend(), test_pairing());
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                viewer_addr: "192.168.0.2:5001".into(),
                input_enabled: false,
                input_rate_hz: 120,
                terminal_since: None,
            },
        );

        assert!(server.input_permission().unwrap());
        assert!(!server.snapshot().sessions[0].input_enabled);
        server.set_session_input(1, true).unwrap();
        let session = server.snapshot().sessions.remove(0);
        assert!(session.input_enabled);
        assert_eq!(session.input_rate_hz, 120);
    }
}
