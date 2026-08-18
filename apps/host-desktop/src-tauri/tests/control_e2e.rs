use control_contract::host::DisplayInfo;
use leftcar_host_desktop::backend::{FakeBackend, SharedBackend};
use leftcar_host_desktop::control::ControlServer;
use std::sync::Arc;
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

async fn spawn_test_server() -> std::net::SocketAddr {
    let server = Arc::new(ControlServer::new(fake_backend()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        server.run(listener).await;
    });
    addr
}

async fn send_request(sock: &mut TcpStream, cmd: &str, args: &str) -> String {
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
async fn test_catalog_query() {
    let addr = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    let resp = send_request(&mut sock, "getCatalog", "{}").await;
    assert!(resp.contains("\"ok\":true"), "{resp}");
    assert!(resp.contains("\"Main Display\""), "{resp}");
    assert!(resp.contains("\"Secondary Display\""), "{resp}");
    assert!(resp.contains("\"width\":1920"), "{resp}");
}

#[tokio::test]
async fn test_full_stream_lifecycle() {
    let addr = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    // 1. Start stream on display 0
    let start_resp = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5000,"width":1920,"height":1080,"fps":90}"#,
    )
    .await;
    assert!(start_resp.contains("\"ok\":true"), "{start_resp}");
    assert!(start_resp.contains("\"session\":1"), "{start_resp}");

    // 2. Query status while stream is running
    let status_resp = send_request(&mut sock, "getStatus", "{}").await;
    assert!(status_resp.contains("\"ok\":true"), "{status_resp}");
    assert!(status_resp.contains("\"state\":\"running\""), "{status_resp}");
    assert!(status_resp.contains("\"fps\":90"), "{status_resp}");
    assert!(status_resp.contains("\"sourceName\":\"Main Display\""), "{status_resp}");

    // 3. Stop stream
    let stop_resp = send_request(&mut sock, "stopStream", r#"{"session":1}"#).await;
    assert!(stop_resp.contains("\"ok\":true"), "{stop_resp}");

    // 4. Query status after stream stopped (should be empty sessions)
    let status_after = send_request(&mut sock, "getStatus", "{}").await;
    assert!(status_after.contains("\"sessions\":[]"), "{status_after}");

    // 5. Start a second stream; session id should increment
    let start2_resp = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":1,"viewerPort":5001,"width":2560,"height":1440,"fps":120}"#,
    )
    .await;
    assert!(start2_resp.contains("\"ok\":true"), "{start2_resp}");
    assert!(start2_resp.contains("\"session\":2"), "{start2_resp}");

    let stop2_resp = send_request(&mut sock, "stopStream", r#"{"session":2}"#).await;
    assert!(stop2_resp.contains("\"ok\":true"), "{stop2_resp}");
}

#[tokio::test]
async fn test_rustra_delegation() {
    let addr = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    let resp = send_request(&mut sock, "addNumbers", r#"{"a":20,"b":22}"#).await;
    assert!(resp.contains("\"ok\":true"), "{resp}");
    assert!(resp.contains("\"value\":42"), "{resp}");
}

#[tokio::test]
async fn test_error_handling() {
    let addr = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();

    // Unknown command
    let unknown_resp = send_request(&mut sock, "unknownCmd", "{}").await;
    assert!(unknown_resp.contains("\"ok\":false"), "{unknown_resp}");

    // Stop non-existent session
    let stop_err_resp = send_request(&mut sock, "stopStream", r#"{"session":999}"#).await;
    assert!(stop_err_resp.contains("\"ok\":false"), "{stop_err_resp}");
    assert!(stop_err_resp.contains("no such session 999"), "{stop_err_resp}");

    // Bad args for startStream
    let bad_args_resp = send_request(&mut sock, "startStream", r#"{"wrongKey":123}"#).await;
    assert!(bad_args_resp.contains("\"ok\":false"), "{bad_args_resp}");
    assert!(bad_args_resp.contains("bad args"), "{bad_args_resp}");
}
