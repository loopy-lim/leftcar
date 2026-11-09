//! 제어 세션 수명 스펙 이전(apps/viewer-expo/src/session.test.ts의 connect
//! lifecycle·connection-lost invalidation 부분). 실물 엔진 + 인메모리 duplex로
//! 검증한다 — secure 호스트는 스크립트 서버가 ServerHello로 답한다.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{read_sealed_envelope, test_ports, wait_until, FixedUsb, GateHandle, ScriptedTransport};
use secure_channel::HostIdentity;
use common::{SecureServerContext, ServerEnd};
use viewer_app_core::b64;
use viewer_app_core::control::ControlClient;
use viewer_app_core::pairing::{
    identity_token_key, token_key, HostEndpoint, RecentHostsPort, RECENT_HOSTS_KEY,
};
use viewer_app_core::session::{ControlSession, SessionError};

/// 서버(시드 12)의 공개키 — 스크립트 서버가 항상 제시하는 정체다.
fn server_spk_b64() -> String {
    b64::encode(&HostIdentity::from_seed([12u8; 32]).public_key())
}

/// 핸드셰이크 응답 방식.
enum HandshakeReply {
    /// ServerHello에 답하고 확인 프레임까지 소비한다(성공 경로).
    Full,
    /// ServerHello에만 답한다(핀 불일치처럼 클라이언트가 확인 전에 끊는 경로).
    ReplyOnly,
}

/// secure 호스트 연결을 열고 스크립트 서버의 핸드셰이크를 마친다.
fn connect_secure(
    session: &Arc<ControlSession>,
    transport: &ScriptedTransport,
    host: &str,
    port: u16,
) -> (Result<Arc<ControlClient>, SessionError>, SecureServerContext, ServerEnd) {
    connect_secure_with(session, transport, host, port, HandshakeReply::Full)
}

fn connect_secure_with(
    session: &Arc<ControlSession>,
    transport: &ScriptedTransport,
    host: &str,
    port: u16,
    mode: HandshakeReply,
) -> (Result<Arc<ControlClient>, SessionError>, SecureServerContext, ServerEnd) {
    let session_for_thread = Arc::clone(session);
    let transport_for_thread = transport.clone();
    let host_owned = host.to_string();
    let baseline = transport.connections_created();
    let handle = std::thread::spawn(move || {
        session_for_thread.connect_host(&host_owned, port, None, None)
    });
    assert!(
        wait_until(|| transport.connections_created() > baseline, Duration::from_secs(5)),
        "connection opened"
    );
    let server = transport.last_server();
    assert!(wait_until(|| server.written_len() == 1, Duration::from_secs(5)));
    let context = match mode {
        HandshakeReply::Full => server.auto_handshake().wait(),
        HandshakeReply::ReplyOnly => common::read_hello_and_reply(&server),
    };
    let result = handle.join().expect("connect thread");
    (result, context, server)
}

#[test]
fn an_old_alias_fallback_cannot_overtake_a_newer_host_selection() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let gate = GateHandle::new();
    transport.push_outcome(common::ConnectOutcome::Hold(Arc::clone(&gate)));
    let session_for_thread = Arc::clone(&session);
    let transport_for_thread = transport.clone();
    let fallback = std::thread::spawn(move || {
        session_for_thread.connect_host_with_fallback(&["10.0.0.1", "100.64.0.1"], 7777, None, None)
    });
    // 첫 후보의 connect가 게이트에 막혀 있다.
    assert!(wait_until(|| gate.waiters() >= 1, Duration::from_secs(5)));

    // 사용자가 다른 호스트를 선택한다 — 새 세대가 열린다.
    let won = connect_secure(&session, &transport, "10.0.0.2", 7777).0.expect("B wins");
    assert_eq!(session.control_host(), "10.0.0.2:7777");
    assert_eq!(session.control_client().map(|c| c.id()), Some(won.id()));

    gate.release();
    assert!(
        wait_until(|| transport.connections_created() >= 2, Duration::from_secs(5)),
        "late candidate opens"
    );
    transport.last_server().auto_handshake().wait();
    // 늦게 도착한 시도는 세대가 옛것이라 거부되고, 남은 후보도 시도하지 않는다.
    let error = fallback.join().expect("fallback thread").expect_err("superseded");
    assert!(matches!(error, SessionError::Cancelled), "{error}");
    assert_eq!(
        transport.servers().iter().map(|s| s.host.as_str()).collect::<Vec<_>>(),
        ["10.0.0.2", "10.0.0.1"]
    );
    assert_eq!(session.control_target().map(|t| t.host), Some("10.0.0.2".to_string()));
    let _ = transport_for_thread;
}

#[test]
fn an_unavailable_candidate_still_falls_back_within_the_same_selection() {
    let transport = ScriptedTransport::new();
    transport.push_outcome(common::ConnectOutcome::Fail("connection timeout"));
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let session_for_thread = Arc::clone(&session);
    let transport_for_thread = transport.clone();
    let handle = std::thread::spawn(move || {
        session_for_thread.connect_host_with_fallback(&["10.0.0.1", "100.64.0.1"], 7777, None, None)
    });
    // 두 번째 후보는 secure 연결이므로 핸드셰이크를 답한다.
    assert!(wait_until(|| transport.connections_created() >= 1, Duration::from_secs(5)));
    transport.last_server().auto_handshake().wait();
    let host = handle.join().expect("fallback thread").expect("fallback");
    assert_eq!(host, "100.64.0.1");
    assert_eq!(session.control_target().map(|t| t.host), Some("100.64.0.1".to_string()));
    let _ = transport_for_thread;
}

#[test]
fn reads_the_persisted_pin_before_any_handshake_or_credential_delivery() {
    let transport = ScriptedTransport::new();
    let (ports, store, recent) = test_ports(Arc::new(transport.clone()));
    let endpoint_token = "a".repeat(64);
    let saved_pin = "B".repeat(43); // 서버 키와 다르다
    recent.set_raw(Some(
        serde_json::json!([{
            "host": "10.0.0.5",
            "port": 7777,
            "hostKey": saved_pin,
            "lastConnected": 1,
        }])
        .to_string(),
    ));
    store.put(&token_key("10.0.0.5", 7777), &endpoint_token);
    let session = ControlSession::new(ports);

    let (result, _context, _server) =
        connect_secure_with(&session, &transport, "10.0.0.5", 7777, HandshakeReply::ReplyOnly);
    let error = result.expect_err("pin mismatch");
    let message = error.to_string();
    assert!(message.contains("보안 키"), "{message}");
    // 핀 불일치는 확인 프레임 전에 끊는다 — 키 확인 쓰기가 없다.
    assert_eq!(transport.last_server().written_len(), 1);
    // 핸드셰이크가 실패했으므로 자격증명은 아무것도 전달되지 않았다.
    assert_eq!(
        store.snapshot().get(&token_key("10.0.0.5", 7777)).map(String::as_str),
        Some(endpoint_token.as_str())
    );
    // 정체 저장은 그대로다.
    assert!(recent.raw().as_deref().map(|raw| raw.contains(&saved_pin)).unwrap_or(false));
}

#[test]
fn does_not_open_a_socket_when_persisted_pin_storage_is_unreadable() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    store.put(RECENT_HOSTS_KEY, "broken");
    let session = ControlSession::new(ports);

    let error = session
        .connect_host("10.0.0.5", 7777, None, None)
        .expect_err("fail closed");
    assert!(matches!(error, SessionError::Store(_)), "{error}");
    assert_eq!(transport.connections_created(), 0);
}

#[test]
fn fails_closed_before_a_handshake_on_corrupt_persisted_identities() {
    for raw in [
        "broken".to_string(),
        "{}".to_string(),
        r#"[{"host":"10.0.0.5","port":7777,"hostKey":3,"lastConnected":1}]"#.to_string(),
    ] {
        let transport = ScriptedTransport::new();
        let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
        store.put(RECENT_HOSTS_KEY, &raw);
        let session = ControlSession::new(ports);
        let error = session
            .connect_host("10.0.0.5", 7777, None, None)
            .expect_err("corrupt identities");
        assert!(matches!(error, SessionError::Store(_)), "{raw}: {error}");
        assert_eq!(transport.connections_created(), 0, "{raw}");
    }
}

#[test]
fn never_sends_an_unpinned_endpoint_credential_to_a_newly_proved_peer() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let endpoint_token = "a".repeat(64);
    store.put(&token_key("10.0.0.5", 7777), &endpoint_token);
    let session = ControlSession::new(ports);

    let (result, context, server) = connect_secure(&session, &transport, "10.0.0.5", 7777);
    let client = result.expect("connects via TOFU");
    // 검증된 정체로 세션 컨텍스트가 채워진다.
    assert_eq!(
        session.capture_request_context().map(|ctx| ctx.identity),
        Some(Some(server_spk_b64()))
    );
    // 첫 요청은 토큰 없이 나간다 — 핀 없는 엔드포인트 자격증명은 새 피어에
    // 보내지 않는다.
    let request = common::issue(&client, "getStatus");
    let envelope = read_sealed_envelope(&server, &context);
    assert!(envelope.get("token").is_none(), "{envelope}");
    // TOFU 핀은 이 거래에서 저장된다(정체 자격증명이 없었으므로).
    let _ = request;
    client.close();
}

#[test]
fn pins_a_saved_alternate_route_before_the_handshake() {
    let transport = ScriptedTransport::new();
    let (ports, _store, recent) = test_ports(Arc::new(transport.clone()));
    // 별칭 경로로 저장된 핀 — 서버가 제시하는 키와 같다.
    recent.set_raw(Some(
        serde_json::json!([{
            "host": "10.0.0.5",
            "aliases": ["mac.tail.ts.net"],
            "port": 7777,
            "hostKey": server_spk_b64(),
            "lastConnected": 1,
        }])
        .to_string(),
    ));
    let session = ControlSession::new(ports);

    let (result, context, _server) = connect_secure(&session, &transport, "mac.tail.ts.net", 7777);
    let client = result.expect("alias pin matched the handshake");
    assert_eq!(client.host_key().as_deref(), Some(context.spk_b64.as_str()));
    assert_eq!(
        session.capture_request_context().map(|ctx| ctx.identity),
        Some(Some(server_spk_b64()))
    );
    client.close();
}

#[test]
fn a_stale_older_socket_closing_never_tears_down_the_newer_session() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let (first, _c1, server_a) = connect_secure(&session, &transport, "10.0.0.1", 7777);
    let first = first.expect("A");
    let (second, _c2, _server_b) = connect_secure(&session, &transport, "10.0.0.2", 7777);
    let second = second.expect("B");
    assert_eq!(session.control_client().map(|c| c.id()), Some(second.id()));

    // 옛 소켓이 닫혀도 새 세션은 그대로다.
    server_a.close();
    assert!(wait_until(|| first.is_closed(), Duration::from_secs(5)));
    assert_eq!(session.control_client().map(|c| c.id()), Some(second.id()));
    second.close();
}

#[test]
fn a_closed_socket_invalidates_the_session_so_the_home_badge_stops_claiming_connected() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let (client, _context, server) = connect_secure(&session, &transport, "10.0.0.1", 7777);
    let client = client.expect("connects");
    assert!(session.control_client().is_some());

    // 호스트 재시작·네트워크 전환 등으로 소켓이 닫히면 연결 객체가 남아
    // "컴퓨터 연결됨" 배지가 거짓말을 하던 회귀를 잠근다.
    server.close();
    assert!(wait_until(|| session.control_client().is_none(), Duration::from_secs(5)));
    assert_eq!(session.control_host(), "");
    assert!(client.is_closed());
    // target·선택 세대는 남는다 — 네이티브 창이 같은 대상으로 재접속할 수 있게.
    assert_eq!(
        session.capture_request_context().map(|ctx| ctx.target.host),
        Some("10.0.0.1".to_string())
    );
}

#[test]
fn reconnects_the_retained_context_after_transport_loss() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let (first, _context, server) = connect_secure(&session, &transport, "10.0.0.1", 7777);
    let first = first.expect("connects");
    let origin = session.capture_request_context().expect("origin");
    server.close();
    assert!(wait_until(|| session.control_client().is_none(), Duration::from_secs(5)));

    // 같은 대상으로 직접 되살린다(선택 세대를 올리지 않는다).
    let session_for_thread = Arc::clone(&session);
    let transport_for_thread = transport.clone();
    let reconnect = std::thread::spawn(move || session_for_thread.reconnect_host(Some(origin), None));
    assert!(wait_until(|| transport_for_thread.connections_created() >= 2, Duration::from_secs(5)));
    transport_for_thread.last_server().auto_handshake().wait();
    let replacement = reconnect.join().expect("reconnect thread").expect("reconnected");
    assert_eq!(session.control_host(), "10.0.0.1:7777");
    assert_eq!(session.control_client().map(|c| c.id()), Some(replacement.id()));
    let _ = first;
}

#[test]
fn transport_loss_recovery_cannot_undo_an_explicit_disconnect() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let (first, _context, server) = connect_secure(&session, &transport, "10.0.0.1", 7777);
    let _first = first.expect("connects");
    let origin = session.capture_request_context().expect("origin");
    server.close();
    assert!(wait_until(|| session.control_client().is_none(), Duration::from_secs(5)));

    session.disconnect_host(None);
    // 해제가 이미 요청 컨텍스트를 비웠다 — 재접속은 소켓을 열기 전에 거부된다.
    let error = session
        .reconnect_host(Some(origin), None)
        .expect_err("superseded");
    assert!(matches!(error, SessionError::Cancelled), "{error}");
    assert!(session.control_client().is_none());
    assert_eq!(transport.connections_created(), 1);
}

#[test]
fn a_connect_that_settles_after_disconnect_host_does_not_resurrect_the_session() {
    let transport = ScriptedTransport::new();
    let gate = GateHandle::new();
    transport.push_outcome(common::ConnectOutcome::Hold(Arc::clone(&gate)));
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let session = ControlSession::new(ports);

    let session_for_thread = Arc::clone(&session);
    let transport_for_thread = transport.clone();
    let pending = std::thread::spawn(move || session_for_thread.connect_host("10.0.0.1", 7777, None, None));
    assert!(wait_until(|| gate.waiters() >= 1, Duration::from_secs(5)));
    session.disconnect_host(None);
    gate.release();
    assert!(wait_until(|| transport_for_thread.connections_created() >= 1, Duration::from_secs(5)));
    transport_for_thread.last_server().auto_handshake().wait();
    let error = pending.join().expect("connect thread").expect_err("superseded");
    assert!(matches!(error, SessionError::Cancelled), "{error}");
    assert!(session.control_client().is_none());
    assert_eq!(session.control_host(), "");
    assert!(session.control_target().is_none());
    // 늦게 도착한 소켓은 닫혔다.
    assert!(wait_until(|| {
        transport.servers().first().map(|s| s.destroyed()).unwrap_or(false)
    }, Duration::from_secs(5)));
}

#[test]
fn the_usb_loopback_path_still_reads_the_selected_network_targets_token() {
    let transport = ScriptedTransport::new();
    let (mut ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let endpoint_token = "e".repeat(64);
    store.put(&token_key("10.0.0.5", 7777), &endpoint_token);
    ports.usb = Arc::new(FixedUsb(6200));
    let session = ControlSession::new(ports);

    let session_for_thread = Arc::clone(&session);
    let transport_for_thread = transport.clone();
    let pending = std::thread::spawn(move || session_for_thread.connect_host("10.0.0.5", 7777, None, None));
    assert!(wait_until(|| transport_for_thread.connections_created() >= 1, Duration::from_secs(5)));
    let server = transport.last_server();
    // 루프백 프록시로 연결됐다(평문 모드 — 핸드셰이크 없음).
    assert_eq!(server.host, "127.0.0.1");
    assert_eq!(server.port, 6200);
    let client = pending.join().expect("connect thread").expect("connects");
    // 선택된 네트워크 대상의 토큰이 프록시 요청에 실린다.
    let request = common::issue(&client, "getStatus");
    assert!(wait_until(|| server.written_len() >= 1, Duration::from_secs(5)));
    let (_command, envelope) = common::read_plain_request(&server);
    assert_eq!(
        envelope.get("token").and_then(|v| v.as_str()),
        Some(endpoint_token.as_str())
    );
    let _ = request;
    client.close();
}

#[test]
fn handle_unauthorized_retires_only_its_own_credential_incarnation() {
    let transport = ScriptedTransport::new();
    let (ports, store, recent) = test_ports(Arc::new(transport.clone()));
    // 정체 자격증명을 프리셋한다 — 연결이 이 교체값을 컨텍스트로 캡처한다.
    let endpoint = HostEndpoint { host: "10.0.0.1".into(), port: 7777 };
    let token = "a".repeat(64);
    store.put(&token_key(&endpoint.host, endpoint.port), &token);
    store.put(&identity_token_key(&server_spk_b64()), &token);
    let session = ControlSession::new(ports);

    let (result, _context, server) = connect_secure(&session, &transport, "10.0.0.1", 7777);
    let client = result.expect("connects with the pinned identity");
    let origin = session.capture_request_context().expect("origin");
    assert!(origin.credential.is_some(), "credential captured");

    session
        .handle_unauthorized(viewer_app_core::session::HandleUnauthorizedContext {
            context: Some(&origin),
            mark_stale: true,
            cancel: None,
        })
        .expect("unauthorized handled");
    // 같은 교체값의 키만 지워진다.
    assert!(!store.snapshot().contains_key(&token_key(&endpoint.host, endpoint.port)));
    assert!(!store.snapshot().contains_key(&identity_token_key(&server_spk_b64())));
    let raw = recent.raw().unwrap_or_default();
    assert!(!raw.contains(&server_spk_b64()), "{raw}");
    // 세션이 해제되고 게이트가 페어링 만료로 막혔다.
    assert!(wait_until(|| session.control_client().is_none(), Duration::from_secs(5)));
    assert!(session.gate_snapshot().pairing_stale);
    let _ = server;
    let _ = client;
}

#[test]
fn usb_port_trait_defaults_to_detached() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    assert_eq!(ports.usb.control_port(), None);
    use viewer_app_core::pairing::UsbPort;
    assert_eq!(FixedUsb(6200).control_port(), Some(6200));
}
