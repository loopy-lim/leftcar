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
    terminal_since: Option<Instant>,
}

const TERMINAL_SESSION_RETENTION: Duration = Duration::from_secs(5);

pub struct ControlServer {
    backend: SharedBackend,
    sessions: Mutex<State>,
}

struct State {
    next: u32,
    live: HashMap<u32, Session>,
}

impl ControlServer {
    pub fn new(backend: SharedBackend) -> Self {
        Self {
            backend,
            sessions: Mutex::new(State { next: 1, live: HashMap::new() }),
        }
    }

    pub async fn bind(&self, addr: &str) -> std::io::Result<std::net::SocketAddr> {
        let listener = TcpListener::bind(addr).await?;
        Ok(listener.local_addr()?)
    }

    /// Accept loop — runs until the process exits.
    pub async fn run(self: std::sync::Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((sock, _)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        let peer =
                            sock.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
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
                    error: Some("backend stats unavailable".into()),
                });

                if metrics.state == "running" {
                    s.terminal_since = None;
                } else {
                    let terminal_since = s.terminal_since.get_or_insert(now);
                    if now.duration_since(*terminal_since) >= TERMINAL_SESSION_RETENTION {
                        expired_ids.push(*id);
                        continue;
                    }
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
                eprintln!("failed to release terminal session {}: {error}", session.handle);
            }
        }

        StatusView { sessions }
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
                .filter_map(|(id, session)| {
                    (session.viewer_addr == viewer_addr).then_some(*id)
                })
                .collect();
            ids.into_iter()
                .filter_map(|id| state.live.remove(&id))
                .collect::<Vec<_>>()
        };

        for session in stale {
            if let Err(error) = self.backend.stop(session.handle) {
                eprintln!("failed to stop stale viewer session {}: {error}", session.handle);
            }
        }
    }

    async fn dispatch(&self, command: &str, args: serde_json::Value, viewer_ip: &str) -> serde_json::Value {
        match command {
            "getCatalog" => {
                match self.backend.list_displays() {
                    Ok(displays) => ok(CatalogView { displays }),
                    Err(e) => err(&e),
                }
            }
            "startStream" => {
                let input: StartStreamInput = match serde_json::from_value(args) {
                    Ok(v) => v,
                    Err(e) => return err(&format!("bad args: {e}")),
                };
                let name = self
                    .backend
                    .list_displays()
                    .ok()
                    .and_then(|d| d.get(input.source_index as usize).cloned())
                    .map(|d| d.name)
                    .unwrap_or_else(|| format!("display {}", input.source_index));
                let candidates = if input.viewer_ips.is_empty() {
                    vec![viewer_ip.to_owned()]
                } else {
                    input.viewer_ips
                };
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
                                    terminal_since: None,
                                },
                            );
                            id
                        };
                        ok(StartStreamOutput { session: session_id })
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
        let parsed: Result<(String, serde_json::Value), String> = serde_json::from_str(&line)
            .map(|v: Envelope| (v.command, v.args))
            .map_err(|e| format!("bad request: {e}"));
        let resp = match parsed {
            Ok((cmd, args)) => {
                let out = server.dispatch(&cmd, args, peer).await;
                serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
            }
            Err(e) => format!("{{\"ok\":false,\"error\":{}}}", json!(e)),
        };
        if wr.write_all(resp.as_bytes()).await.is_err()
            || wr.write_all(b"\n").await.is_err()
        {
            break;
        }
    }
}

#[derive(serde::Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
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
                error: Some("viewer closed stream".into()),
            })
        }
    }

    async fn spawn_server() -> std::net::SocketAddr {
        let server = std::sync::Arc::new(ControlServer::new(backend()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { server.run(listener).await });
        addr
    }

    async fn request(sock: &mut tokio::net::TcpStream, cmd: &str, args: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(format!("{{\"command\":\"{cmd}\",\"args\":{args}}}\n").as_bytes())
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

    #[tokio::test]
    async fn catalog_start_status_stop_roundtrip() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "getCatalog", "{}").await;
        assert!(line.contains("\"displays\""), "{line}");

        let line = request(
            &mut sock,
            "startStream",
            r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
        )
        .await;
        assert!(line.contains("\"session\":1"), "{line}");

        let line = request(&mut sock, "getStatus", "{}").await;
        assert!(line.contains("\"state\":\"running\""), "{line}");

        let line = request(&mut sock, "stopStream", r#"{"session":1}"#).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    #[tokio::test]
    async fn add_numbers_delegates_to_rustra() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let line = request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#).await;
        assert!(line.contains("\"value\":42"), "{line}");
    }

    #[tokio::test]
    async fn unknown_command_and_restart_session_ids() {
        let addr = spawn_server().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        let line = request(&mut sock, "nope", "{}").await;
        assert!(line.contains("\"ok\":false"), "{line}");

        for i in 1..=2 {
            let line = request(
                &mut sock,
                "startStream",
                r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#,
            )
            .await;
            assert!(line.contains(&format!("\"session\":{i}")), "{line}");
            let line = request(&mut sock, "stopStream", &format!("{{\"session\":{i}}}")).await;
            assert!(line.contains("\"ok\":true"), "{line}");
        }
    }

    #[test]
    fn terminal_sessions_are_retained_briefly_then_released() {
        let stopped = Arc::new(AtomicUsize::new(0));
        let server = ControlServer::new(Arc::new(TerminalBackend {
            stopped: stopped.clone(),
        }));
        server.sessions.lock().unwrap().live.insert(
            1,
            Session {
                handle: 7,
                source_index: 0,
                source_name: "Main".into(),
                viewer_addr: "192.168.0.2:5001".into(),
                terminal_since: None,
            },
        );

        let first = server.snapshot();
        assert_eq!(first.sessions.len(), 1);
        assert_eq!(first.sessions[0].error.as_deref(), Some("viewer closed stream"));

        server.sessions.lock().unwrap().live.get_mut(&1).unwrap().terminal_since =
            Some(Instant::now() - TERMINAL_SESSION_RETENTION - Duration::from_millis(1));

        let second = server.snapshot();
        assert!(second.sessions.is_empty());
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }
}
