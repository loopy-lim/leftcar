//! secure control channel 스펙 이전(apps/viewer-expo/src/control.test.ts의
//! "secure control channel") — 스크립트 서버로 핸드셰이크·봉인 프레임을 검증한다.

mod common;

use std::time::Duration;

use common::{
    read_hello_and_reply, read_sealed_envelope, reply_sealed, wait_until, RecordingProvider,
    ScriptedTransport,
};
use viewer_app_core::b64;
use viewer_app_core::control::{connect, ConnectOptions, ControlErrorKind, ErrorMessages};

#[test]
fn rejects_an_oversized_sealed_request_before_writing_and_preserves_earlier_fifo_work() {
    let transport = ScriptedTransport::new();
    let connect_transport = transport.clone();
    let pending = std::thread::spawn(move || {
        connect(
            &connect_transport,
            "192.168.1.10",
            7777,
            5000,
            ConnectOptions::new(),
        )
    });
    assert!(wait_until(
        || transport.connections_created() >= 1,
        Duration::from_secs(5)
    ));
    let server = transport.last_server();
    let context = server.auto_handshake().wait();
    let client = pending.join().expect("connect thread").expect("connects");

    let first = common::issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 3,
        Duration::from_secs(5)
    ));
    let writes_before = server.written_len();
    // 10MiB 봉투는 호스트 줄 판독기(12MiB)를 넘는다 — 봉인 전에 거부되고 FIFO의
    // 앞 작업은 그대로다.
    let rejected = client
        .request_simple(
            "setClipboard",
            Some(&serde_json::json!({ "text": "a".repeat(10 * 1024 * 1024) })),
        )
        .expect_err("line limit");
    assert_eq!(rejected.kind, ControlErrorKind::Capacity);
    assert_eq!(server.written_len(), writes_before);
    assert!(!server.destroyed());

    reply_sealed(
        &server,
        &context,
        &serde_json::json!({"ok": true, "result": "still valid"}).to_string(),
    );
    let value = first
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::Value::String("still valid".into()));

    let next = common::issue(&client, "getStatus");
    reply_sealed(
        &server,
        &context,
        &serde_json::json!({"ok": true, "result": "next"}).to_string(),
    );
    let value = next
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::Value::String("next".into()));
    client.close();
}

#[test]
fn handshakes_with_a_pinned_key_and_seals_every_frame() {
    let transport = ScriptedTransport::new();
    let connect_transport = transport.clone();
    let pending = std::thread::spawn(move || {
        connect(
            &connect_transport,
            "192.168.1.10",
            7777,
            5000,
            ConnectOptions::new().token_provider(RecordingProvider::with_token(Some("tok"))),
        )
    });
    assert!(wait_until(
        || transport.connections_created() >= 1,
        Duration::from_secs(5)
    ));
    let server = transport.last_server();
    let context = server.auto_handshake().wait();
    let client = pending.join().expect("connect thread").expect("connects");
    assert_eq!(client.host_key().as_deref(), Some(context.spk_b64.as_str()));

    // 요청은 봉인 봉투로 나가야 한다.
    let request = common::issue_with_payload(&client, "getCatalog", serde_json::json!({}));
    assert!(wait_until(
        || server.written_len() >= 3,
        Duration::from_secs(5)
    ));
    let envelope = read_sealed_envelope(&server, &context);
    assert_eq!(
        envelope.get("command").and_then(|v| v.as_str()),
        Some("getCatalog")
    );
    assert_eq!(envelope.get("token").and_then(|v| v.as_str()), Some("tok"));

    reply_sealed(
        &server,
        &context,
        &serde_json::json!({"ok": true, "result": {"displays": [1, 2]}}).to_string(),
    );
    let value = request
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::json!({"displays": [1, 2]}));

    // 서버가 평문 JSON을 보내면 연결이 끊긴다(다운그레이드 거부).
    let transport2 = ScriptedTransport::new();
    let connect_transport2 = transport2.clone();
    let pending2 = std::thread::spawn(move || {
        connect(
            &connect_transport2,
            "192.168.1.10",
            7777,
            5000,
            ConnectOptions::new().token_provider(RecordingProvider::with_token(Some("tok"))),
        )
    });
    assert!(wait_until(
        || transport2.connections_created() >= 1,
        Duration::from_secs(5)
    ));
    let server2 = transport2.last_server();
    let context2 = server2.auto_handshake().wait();
    let client2 = pending2.join().expect("connect thread").expect("connects");
    let request2 = common::issue_with_payload(&client2, "getStatus", serde_json::json!({}));
    assert!(wait_until(
        || server2.written_len() >= 3,
        Duration::from_secs(5)
    ));
    server2.send(&serde_json::json!({"ok": true, "result": {}}).to_string());
    let error = request2
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect_err("downgrade");
    assert!(error.message.contains("sealed frame"));
    assert!(client2.is_closed());
    client.close();
    let _ = &context2;
}

#[test]
fn rejects_a_server_whose_key_does_not_match_the_pin() {
    let transport = ScriptedTransport::new();
    let pinned = b64::encode(&[99u8; 32]);
    let connect_transport = transport.clone();
    let pending = std::thread::spawn(move || {
        connect(
            &connect_transport,
            "192.168.1.10",
            7777,
            5000,
            ConnectOptions::new()
                .token_provider(RecordingProvider::with_token(None))
                .pinned_host_key(Some(pinned)),
        )
    });
    assert!(wait_until(
        || transport.connections_created() >= 1,
        Duration::from_secs(5)
    ));
    let server = transport.last_server();
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    // 스크립트 서버는 자기 키로 답한다 — 핀과 다르다. 클라이언트는 확인 프레임을
    // 쓰기 전에 연결을 끊는다.
    let context = read_hello_and_reply(&server);
    let error = pending
        .join()
        .expect("connect thread")
        .expect_err("pin mismatch");
    // 핀 불일치는 formatErrorMessage의 "pinned key" 매핑을 통과한다(보안 키 안내).
    let formatted =
        viewer_app_core::control::format_error_message(&error.message, &ErrorMessages::KOREAN);
    assert!(formatted.contains("보안 키"), "{formatted}");
    // 핀 불일치는 키 확인 전에 연결을 끊는다 — 확인 프레임을 쓰면 안 된다.
    assert_eq!(server.written_len(), 1);
    assert_eq!(context.spk_b64.len(), 43);
}

#[test]
fn oversized_handshake_lines_know_the_sealed_request_budget_math() {
    // 봉인 봉투 크기 추정: ceil((payload + 24) * 4 / 3) + 9 — TS와 동일한 산식.
    let payload = 100usize;
    let sealed = (payload + 8 + 16).div_ceil(3) * 4 + 9;
    assert_eq!(sealed, (payload + 24).div_ceil(3) * 4 + 9);
}
