use control_contract::host::DisplayInfo;
use leftcar_host_desktop::backend::{CaptureBackend, FakeBackend, SharedBackend};
use leftcar_host_desktop::control::{ControlServer, StatsInfo};
use leftcar_host_desktop::pairing::PairingServer;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn fake_backend() -> SharedBackend {
    Arc::new(FakeBackend {
        displays: vec![
            DisplayInfo {
                index: 0,
                name: "Main Display".into(),
                width: 1920,
                height: 1080,
            },
            DisplayInfo {
                index: 1,
                name: "Secondary Display".into(),
                width: 2560,
                height: 1440,
            },
        ],
    })
}

/// Backend that records the `ip` argument of every `start` call.
struct RecordingBackend {
    displays: Vec<DisplayInfo>,
    started_ips: Mutex<Vec<String>>,
}

impl CaptureBackend for RecordingBackend {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Ok(self.displays.clone())
    }
    fn start(
        &self,
        _source_index: u32,
        ip: &str,
        _port: u16,
        _w: u32,
        _h: u32,
        _fps: u32,
    ) -> Result<u32, String> {
        self.started_ips.lock().unwrap().push(ip.to_owned());
        Ok(7)
    }
    fn stop(&self, _handle: u32) -> Result<(), String> {
        Ok(())
    }
    fn stats(&self, _handle: u32) -> Result<StatsInfo, String> {
        Ok(StatsInfo {
            frames: 100,
            bytes: 1_000_000,
            state: "running".into(),
            fps: 90,
            kbps: 12000,
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
            error: None,
        })
    }
}

fn pairing() -> Arc<PairingServer> {
    Arc::new(PairingServer::new("leftcar-host".into(), None))
}

async fn spawn_test_server() -> (std::net::SocketAddr, Arc<PairingServer>) {
    let p = pairing();
    let server = Arc::new(ControlServer::new(fake_backend(), p.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        server.run(listener).await;
    });
    (addr, p)
}

async fn send_request(sock: &mut TcpStream, cmd: &str, args: &str, token: &str) -> String {
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

/// Run the real pair flow over the socket; returns the issued token.
async fn pair_over_socket(
    sock: &mut TcpStream,
    pairing: &PairingServer,
    device_id: &str,
) -> String {
    let view = pairing.begin_pairing("127.0.0.1", 7777);
    let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
    let resp = send_request(
        sock,
        "pair",
        &serde_json::json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": view.code,
            "deviceId": device_id,
            "deviceName": "Quest 3",
        })
        .to_string(),
        "",
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["ok"].as_bool().unwrap(), "pair failed: {resp}");
    v["result"]["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn test_catalog_query() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-1").await;

    let resp = send_request(&mut sock, "getCatalog", "{}", &token).await;
    assert!(resp.contains("\"ok\":true"), "{resp}");
    assert!(resp.contains("\"Main Display\""), "{resp}");
    assert!(resp.contains("\"Secondary Display\""), "{resp}");
    assert!(resp.contains("\"width\":1920"), "{resp}");
}

#[tokio::test]
async fn test_full_stream_lifecycle() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-1").await;

    // 1. Start stream on display 0
    let start_resp = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5000,"width":1920,"height":1080,"fps":90}"#,
        &token,
    )
    .await;
    assert!(start_resp.contains("\"ok\":true"), "{start_resp}");
    assert!(start_resp.contains("\"session\":1"), "{start_resp}");

    // 2. Query status while stream is running
    let status_resp = send_request(&mut sock, "getStatus", "{}", &token).await;
    assert!(status_resp.contains("\"ok\":true"), "{status_resp}");
    assert!(
        status_resp.contains("\"state\":\"running\""),
        "{status_resp}"
    );
    assert!(status_resp.contains("\"fps\":90"), "{status_resp}");
    assert!(
        status_resp.contains("\"sourceName\":\"Main Display\""),
        "{status_resp}"
    );

    // 3. Stop stream
    let stop_resp = send_request(&mut sock, "stopStream", r#"{"session":1}"#, &token).await;
    assert!(stop_resp.contains("\"ok\":true"), "{stop_resp}");

    // 4. Query status after stream stopped (should be empty sessions)
    let status_after = send_request(&mut sock, "getStatus", "{}", &token).await;
    assert!(status_after.contains("\"sessions\":[]"), "{status_after}");

    // 5. Start a second stream; session id should increment
    let start2_resp = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":1,"viewerPort":5001,"width":2560,"height":1440,"fps":120}"#,
        &token,
    )
    .await;
    assert!(start2_resp.contains("\"ok\":true"), "{start2_resp}");
    assert!(start2_resp.contains("\"session\":2"), "{start2_resp}");

    let stop2_resp = send_request(&mut sock, "stopStream", r#"{"session":2}"#, &token).await;
    assert!(stop2_resp.contains("\"ok\":true"), "{stop2_resp}");
}

#[tokio::test]
async fn test_rustra_delegation() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-1").await;

    let resp = send_request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#, &token).await;
    assert!(resp.contains("\"ok\":true"), "{resp}");
    assert!(resp.contains("\"value\":42"), "{resp}");
}

#[tokio::test]
async fn test_error_handling() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-1").await;

    // Unknown command
    let unknown_resp = send_request(&mut sock, "unknownCmd", "{}", &token).await;
    assert!(unknown_resp.contains("\"ok\":false"), "{unknown_resp}");

    // Stop non-existent session
    let stop_err_resp = send_request(&mut sock, "stopStream", r#"{"session":999}"#, &token).await;
    assert!(stop_err_resp.contains("\"ok\":false"), "{stop_err_resp}");
    assert!(
        stop_err_resp.contains("no such session 999"),
        "{stop_err_resp}"
    );

    // Bad args for startStream
    let bad_args_resp = send_request(&mut sock, "startStream", r#"{"wrongKey":123}"#, &token).await;
    assert!(bad_args_resp.contains("\"ok\":false"), "{bad_args_resp}");
    assert!(bad_args_resp.contains("bad args"), "{bad_args_resp}");
}

#[tokio::test]
async fn unauthenticated_getcatalog_is_rejected() {
    let (addr, _p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    let resp = send_request(&mut sock, "getCatalog", "{}", "").await;
    assert!(resp.contains("\"ok\":false"), "{resp}");
    assert!(resp.contains("\"error\":\"unauthorized\""), "{resp}");

    // connection must be closed: next write gets EOF, not a response line
    sock.write_all(b"{\"command\":\"getCatalog\",\"args\":{}}\n")
        .await
        .unwrap();
    let mut buf = [0u8; 64];
    let n = sock.read(&mut buf).await.unwrap();
    assert_eq!(n, 0, "server must close the connection after unauthorized");
}

#[tokio::test]
async fn pair_then_catalog_works() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    // unpaired: rejected
    let denied = send_request(&mut sock, "getCatalog", "{}", "").await;
    assert!(denied.contains("\"error\":\"unauthorized\""), "{denied}");

    // reconnect (server closed the socket), pair, then use the token
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let view = p.begin_pairing("127.0.0.1", 7777);
    let payload: serde_json::Value = serde_json::from_str(&view.qr_payload).unwrap();
    let pair_resp = send_request(
        &mut sock,
        "pair",
        &serde_json::json!({
            "offerId": payload["id"],
            "secret": payload["s"],
            "code": view.code,
            "deviceId": "viewer-9",
            "deviceName": "Quest 3",
        })
        .to_string(),
        "",
    )
    .await;
    assert!(pair_resp.contains("\"ok\":true"), "{pair_resp}");
    let v: serde_json::Value = serde_json::from_str(&pair_resp).unwrap();
    let token = v["result"]["token"].as_str().unwrap();
    assert_eq!(token.len(), 64);

    let resp = send_request(&mut sock, "getCatalog", "{}", token).await;
    assert!(resp.contains("\"ok\":true"), "{resp}");
    assert!(resp.contains("\"Main Display\""), "{resp}");
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let (addr, _p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    let resp = send_request(&mut sock, "getCatalog", "{}", "0".repeat(64).as_str()).await;
    assert!(resp.contains("\"error\":\"unauthorized\""), "{resp}");
}

#[tokio::test]
async fn startstream_ignores_viewer_ips_uses_peer() {
    let p = pairing();
    let recorder = Arc::new(RecordingBackend {
        displays: vec![DisplayInfo {
            index: 0,
            name: "Main".into(),
            width: 1920,
            height: 1080,
        }],
        started_ips: Mutex::new(Vec::new()),
    });
    let server = Arc::new(ControlServer::new(recorder.clone(), p.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        server.run(listener).await;
    });

    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-1").await;

    let resp = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90,"viewerIps":["1.2.3.4"]}"#,
        &token,
    )
    .await;
    assert!(resp.contains("\"ok\":true"), "{resp}");

    let started = recorder.started_ips.lock().unwrap().clone();
    assert_eq!(started, vec!["127.0.0.1".to_owned()], "peer IP must win");
}
