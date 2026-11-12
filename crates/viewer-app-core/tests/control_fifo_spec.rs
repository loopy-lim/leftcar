//! bounded control response lifetime 스펙 이전(apps/viewer-expo/src/control.test.ts의
//! "bounded control response lifetime"·"FIFO request deadlines"·
//! "FIFO write failure isolation").

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{
    connect_plain, issue, issue_with_payload, read_plain_request, wait_until, ConnectOutcome,
    GateHandle, ScriptedTransport,
};
use viewer_app_core::control::{
    connect, ConnectOptions, ControlErrorKind, CONTROL_LINE_LIMIT, HANDSHAKE_LINE_LIMIT,
    MAX_PENDING_REQUESTS, MAX_PENDING_REQUEST_BYTES,
};

fn kind_of(error: &viewer_app_core::control::ControlError) -> ControlErrorKind {
    error.kind
}

#[test]
fn rejects_only_unadmitted_work_when_the_fifo_is_full_and_admits_again_after_a_reply() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();

    // TS는 단일 스레드에서 순서대로 발화한다 — 등록 순서가 FIFO 순서와 일치하게
    // 각 쓰기 경계를 기다린다.
    let mut pending = Vec::new();
    for index in 0..MAX_PENDING_REQUESTS {
        pending.push(issue(&client, "getStatus"));
        let expected = index + 1;
        assert!(
            wait_until(|| server.written_len() == expected, Duration::from_secs(5)),
            "request {index} written"
        );
    }

    // 33번째는 즉시 거부된다(용량) — 아무것도 쓰지 않는다.
    let rejected = client
        .request_simple("getStatus", None)
        .expect_err("queue full");
    assert_eq!(rejected.kind, ControlErrorKind::Capacity);
    assert_eq!(rejected.message, "control request queue full");
    assert_eq!(server.written_len(), MAX_PENDING_REQUESTS);
    assert!(!server.destroyed());

    // 첫 응답이 FIFO에서 하나를 은퇴시키면 재입장이 열린다.
    server.send(&serde_json::json!({"ok": true, "result": "first"}).to_string());
    let first = pending[0]
        .recv_timeout(Duration::from_secs(5))
        .expect("first resolves")
        .expect("ok");
    assert_eq!(first, serde_json::Value::String("first".into()));

    let admitted = issue(&client, "getStatus");
    assert!(
        wait_until(
            || server.written_len() == MAX_PENDING_REQUESTS + 1,
            Duration::from_secs(5)
        ),
        "re-admitted after a reply"
    );

    // 남은 요청을 전부 회수한다(채널 닫힘 후 실패로 정리된다).
    server.close();
    let _ = admitted.recv_timeout(Duration::from_secs(5));
    for receiver in pending {
        let _ = receiver.recv_timeout(Duration::from_secs(5));
    }
}

#[test]
fn bounds_outstanding_payload_bytes_without_retiring_requests_already_on_the_wire() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();

    let half = MAX_PENDING_REQUEST_BYTES / 2;
    let first = issue_with_payload(
        &client,
        "setClipboard",
        serde_json::json!({ "text": "a".repeat(half) }),
    );
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(10)
    ));

    // 두 번째 8MiB는 바이트 예산 초과로 거부되며, 전송 중인 첫 요청은 그대로 둔다.
    let rejected = client
        .request_simple(
            "setClipboard",
            Some(&serde_json::json!({ "text": "a".repeat(half) })),
        )
        .expect_err("budget exceeded");
    assert_eq!(rejected.kind, ControlErrorKind::Capacity);
    assert_eq!(rejected.message, "control request byte budget exceeded");
    assert_eq!(server.written_len(), 1);
    assert!(!server.destroyed());

    server.send(&serde_json::json!({"ok": true, "result": "accepted"}).to_string());
    let accepted = first
        .recv_timeout(Duration::from_secs(10))
        .expect("first resolves")
        .expect("ok");
    assert_eq!(accepted, serde_json::Value::String("accepted".into()));

    // 예산이 비워졌으므로 다음 요청은 들어간다.
    let next = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 2,
        Duration::from_secs(5)
    ));
    server.send(&serde_json::json!({"ok": true, "result": "next"}).to_string());
    let next_value = next
        .recv_timeout(Duration::from_secs(5))
        .expect("next resolves")
        .expect("ok");
    assert_eq!(next_value, serde_json::Value::String("next".into()));
    client.close();
}

#[test]
fn rejects_an_unterminated_oversized_handshake_immediately() {
    let transport = ScriptedTransport::new();
    let connect_transport = transport.clone();
    let handle = std::thread::spawn(move || {
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
    // ClientHello 쓰기를 기다린다.
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    // 핸드셰이크 한도(16KiB) 딱까지는 버퍼에 쌓인다.
    server.send_raw(&vec![b'x'; HANDSHAKE_LINE_LIMIT]);
    assert!(wait_until(|| !server.destroyed(), Duration::from_secs(5)));
    // 한 바이트 더 오면 즉시 거부된다.
    server.send_raw(b"x");
    let error = handle
        .join()
        .expect("connect thread")
        .expect_err("rejected");
    assert_eq!(error.kind, ControlErrorKind::Transport);
    assert!(server.destroyed());
}

#[test]
fn bounds_accumulated_command_text_across_chunks_and_retires_pending_replies() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();
    let result = issue(&client, "getClipboard");
    let chunk = CONTROL_LINE_LIMIT / 12;
    for _ in 0..12 {
        server.send_raw(&vec![b'x'; chunk]);
    }
    assert!(wait_until(|| !server.destroyed(), Duration::from_secs(5)));
    // 12조각까지는 버퍼 안이다 — 한 바이트 더 오면 초과.
    server.send_raw(b"x");
    let error = result
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect_err("line limit");
    assert_eq!(kind_of(&error), ControlErrorKind::Transport);
    assert!(server.destroyed());
}

#[test]
fn rejects_malformed_responses_without_throwing_in_the_reader_thread() {
    for line in ["null", "[]", "garbage", "{\"ok\":\"yes\"}"] {
        let transport = ScriptedTransport::new();
        let client = connect_plain(&transport, "localhost", ConnectOptions::new());
        let server = transport.last_server();
        let result = issue(&client, "getStatus");
        assert!(wait_until(
            || server.written_len() == 1,
            Duration::from_secs(5)
        ));
        server.send(line);
        let error = result
            .recv_timeout(Duration::from_secs(5))
            .expect("settles")
            .expect_err(line);
        assert_eq!(error.kind, ControlErrorKind::Transport, "{line}");
        assert!(server.destroyed(), "{line}");
    }
}

#[test]
fn matches_split_and_coalesced_valid_responses_in_fifo_order() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();
    let first = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let second = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 2,
        Duration::from_secs(5)
    ));
    server.send_raw(b"{\"ok\":tr");
    server.send_raw(b"ue,\"result\":1}\n{\"ok\":true,\"result\":2}\n");
    let values = [
        first
            .recv_timeout(Duration::from_secs(5))
            .expect("first")
            .expect("ok"),
        second
            .recv_timeout(Duration::from_secs(5))
            .expect("second")
            .expect("ok"),
    ];
    assert_eq!(
        values,
        [
            serde_json::Value::Number(1.into()),
            serde_json::Value::Number(2.into())
        ]
    );
    assert!(!server.destroyed());
    client.close();
}

#[test]
fn settles_a_connection_closed_before_its_handshake_without_waiting_for_timeout() {
    let transport = ScriptedTransport::new();
    let connect_transport = transport.clone();
    let handle = std::thread::spawn(move || {
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
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    server.close();
    let error = handle.join().expect("connect thread").expect_err("closed");
    assert_eq!(error.kind, ControlErrorKind::Transport);
    assert_eq!(error.message, "control connection closed");
}

#[test]
fn retains_the_sockets_numeric_peer_for_native_media_admission() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    assert_eq!(client.remote_address().as_deref(), Some("127.0.0.1"));
    client.close();
}

#[test]
fn allows_status_polls_to_wait_behind_a_slow_display_resize() {
    let transport = ScriptedTransport::new();
    let mut overrides = std::collections::BTreeMap::new();
    // 느린 resize는 앞의 데드라인이고, status 폴링은 데드라인이 뒤에서 무장하지
    // 않아야 한다(TS fake timers 12s advance).
    overrides.insert("resizeVirtualDisplay".to_string(), 1_200);
    overrides.insert("getVirtualDisplay".to_string(), 300);
    overrides.insert("getStatus".to_string(), 300);
    let options = ConnectOptions::new().timeout_overrides(overrides);
    let client = connect_plain(&transport, "localhost", options);
    let server = transport.last_server();

    let resize = issue_with_payload(
        &client,
        "resizeVirtualDisplay",
        serde_json::json!({"width": 1440, "height": 900, "scale": 2}),
    );
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let status = issue(&client, "getVirtualDisplay");
    assert!(wait_until(
        || server.written_len() == 2,
        Duration::from_secs(5)
    ));
    let stream_status = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 3,
        Duration::from_secs(5)
    ));

    // resize 데드라인(1.2s)의 절반이 지나도 아무도 죽지 않는다 — 뒤의 status는
    // 아직 시계가 시작하지 않았다.
    std::thread::sleep(Duration::from_millis(600));
    assert!(!server.destroyed());

    server.send(
        &serde_json::json!({"ok": true, "result": {"sourceId": "display:resized"}}).to_string(),
    );
    server.send(&serde_json::json!({"ok": true, "result": {"removalPending": false}}).to_string());
    server.send(&serde_json::json!({"ok": true, "result": {"sessions": []}}).to_string());
    for receiver in [resize, status, stream_status] {
        let outcome = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("settles");
        assert!(outcome.is_ok(), "all three fulfil: {outcome:?}");
    }
    assert!(!server.destroyed());
    client.close();
}

#[test]
fn still_times_out_an_unanswered_status_once_it_reaches_the_front() {
    let transport = ScriptedTransport::new();
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("resizeVirtualDisplay".to_string(), 5_000);
    overrides.insert("getStatus".to_string(), 200);
    let options = ConnectOptions::new().timeout_overrides(overrides);
    let client = connect_plain(&transport, "localhost", options);
    let server = transport.last_server();

    let resize = issue(&client, "resizeVirtualDisplay");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let status = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 2,
        Duration::from_secs(5)
    ));
    // resize가 끝나면 status가 맨 앞이 되고 그때부터 200ms 시계가 돈다.
    server.send(&serde_json::json!({"ok": true, "result": {}}).to_string());
    resize
        .recv_timeout(Duration::from_secs(5))
        .expect("resize resolves")
        .expect("ok");
    let error = status
        .recv_timeout(Duration::from_secs(5))
        .expect("status settles")
        .expect_err("timeout");
    assert_eq!(kind_of(&error), ControlErrorKind::Timeout);
    assert!(error
        .message
        .starts_with("control request timeout: getStatus"));
    assert!(server.destroyed());
}

#[test]
fn rejects_every_pending_request_before_a_late_failed_write_reply_can_reach_the_next_request() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();

    let first = issue(&client, "startStream");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    // 다음 쓰기는 큐에 넣은 뒤 실패한다(TS 지연 실패 콜백 흉내) — 이미 전송 중인
    // 첫 요청도 함께 은퇴해야 한다(FIFO 채널에는 응답 ID가 없다).
    server.arm_write_failure();
    let second = issue(&client, "getStatus");
    // TS는 실패 콜백을 응답 이벤트보다 먼저 발화한다 — 쓰기 실패가 보류 전체를
    // 은퇴시킨 뒤 늦은 응답이 도착하는 순서를 맞춘다.
    assert!(wait_until(|| server.destroyed(), Duration::from_secs(5)));

    server.send(&serde_json::json!({"ok": true, "result": {"session": 99}}).to_string());
    server.send(&serde_json::json!({"ok": true, "result": {"running": true}}).to_string());

    let first_error = first
        .recv_timeout(Duration::from_secs(5))
        .expect("first settles")
        .expect_err("transport");
    let second_error = second
        .recv_timeout(Duration::from_secs(5))
        .expect("second settles")
        .expect_err("transport");
    assert_eq!(kind_of(&first_error), ControlErrorKind::Transport);
    assert_eq!(kind_of(&second_error), ControlErrorKind::Transport);
    assert!(server.destroyed());

    // 이후 요청은 터미널 오류로 즉시 거부되고 아무것도 쓰지 않는다.
    let blocked = client
        .request_simple("getStatus", None)
        .expect_err("terminal");
    assert_eq!(kind_of(&blocked), ControlErrorKind::Transport);
    assert_eq!(server.written_len(), 2);
}

#[test]
fn terminates_the_fifo_channel_when_a_later_socket_write_throws_synchronously() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();

    let first = issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    server.arm_sync_write_failure();
    let second = issue(&client, "startStream");

    server.send(&serde_json::json!({"ok": true, "result": {"running": true}}).to_string());

    let second_error = second
        .recv_timeout(Duration::from_secs(5))
        .expect("second settles")
        .expect_err("transport");
    let first_error = first
        .recv_timeout(Duration::from_secs(5))
        .expect("first settles")
        .expect_err("transport");
    assert_eq!(kind_of(&second_error), ControlErrorKind::Transport);
    assert_eq!(kind_of(&first_error), ControlErrorKind::Transport);
    assert!(server.destroyed());
}

#[test]
fn rejects_the_connect_promise_when_the_transport_never_opens() {
    let transport = ScriptedTransport::new();
    let gate = GateHandle::new();
    transport.push_outcome(ConnectOutcome::Hold(gate.clone()));
    let started = Instant::now();
    let error =
        connect(&transport, "192.168.1.10", 7777, 150, ConnectOptions::new()).expect_err("timeout");
    assert_eq!(error.kind, ControlErrorKind::Timeout);
    assert_eq!(error.message, "connect timeout to 192.168.1.10:7777");
    assert!(started.elapsed() >= Duration::from_millis(140));
    gate.release();
}

#[test]
fn acknowledges_the_write_before_the_server_response() {
    let transport = ScriptedTransport::new();
    let client = connect_plain(&transport, "localhost", ConnectOptions::new());
    let server = transport.last_server();
    let written_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag_for_callback = Arc::clone(&written_flag);
    let (tx, rx) = std::sync::mpsc::channel();
    let client_for_thread = client.clone();
    std::thread::spawn(move || {
        let mut on_written = || {
            flag_for_callback.store(true, std::sync::atomic::Ordering::SeqCst);
        };
        let _ = tx.send(client_for_thread.request(
            "startStream",
            Some(&serde_json::json!({"viewerPort": 5001})),
            Some(&mut on_written),
        ));
    });
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    // 쓰기 성공 시점에 콜백이 이미 발화했다(서버 응답 전).
    assert!(written_flag.load(std::sync::atomic::Ordering::SeqCst));
    server.send(&serde_json::json!({"ok": true, "result": {"session": 1}}).to_string());
    let value = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("request settles")
        .expect("ok");
    assert_eq!(value, serde_json::json!({"session": 1}));
    client.close();
    let _ = read_plain_request(&server);
}
