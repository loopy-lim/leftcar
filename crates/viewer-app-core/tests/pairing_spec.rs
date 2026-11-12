//! pairing 스펙 이전(apps/viewer-expo/src/pairing.test.ts) — 소켓 상호작용은
//! control.connect() 안에 있으므로 인메모리 duplex 전송의 평문(루프백) 서버로
//! 검증하고, 저장은 메모리 CredentialStore로 검증한다.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{
    read_plain_request, test_ports, wait_until, GateHandle, ScriptedTransport, ServerEnd,
};
use viewer_app_core::pairing::flows::DEFAULT_POLL_MS;
use viewer_app_core::pairing::{
    can_submit_pairing_code, clear_stored_credential, clear_token, format_host_endpoint,
    get_device_id, get_stored_credential, get_stored_pinned_host_key, get_stored_token,
    identity_token_key, is_pairing_rejected_error, is_pairing_unsupported_error, is_trusted_host,
    pair_with_host, pair_with_host_approval, pair_with_host_by_code, parse_host_endpoint,
    parse_qr_payload, resolve_pairing_host, token_key, PairingApprovalResult, PairingAttempt,
    PairingError, PairingFlowOptions, QrPayload, RecentHostsPort, DEFAULT_CONTROL_PORT,
};

const OFFER_ID: &str = "offer-123e4567-e89b-42d3-a456-426614174000";
const OFFER_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 43
const HOST_KEY: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"; // 43
const TOKEN_64HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn payload() -> QrPayload {
    QrPayload {
        id: OFFER_ID.to_string(),
        secret: OFFER_SECRET.to_string(),
        host_key: HOST_KEY.to_string(),
        host: "127.0.0.1".to_string(),
        port: 7777,
        ts: None,
    }
}

fn flow_options() -> PairingFlowOptions {
    PairingFlowOptions {
        request_timeout_overrides: [("pair".to_string(), 200u64)].into_iter().collect(),
        ..PairingFlowOptions::new()
    }
}

/// 새 서버 끝단이 나타나기를 기다린다(페어링 흐름이 연결을 열 때까지).
fn wait_new_server(transport: &ScriptedTransport, seen: &AtomicUsize) -> ServerEnd {
    assert!(
        wait_until(
            || transport.connections_created() > seen.load(Ordering::SeqCst),
            Duration::from_secs(5)
        ),
        "a new connection appeared"
    );
    let index = seen.fetch_add(1, Ordering::SeqCst);
    transport.servers()[index].clone()
}

/// 페어링 서버 루프: pair 요청마다 스크립트 응답을 하나씩 보낸다.
fn script_pair_replies(
    transport: ScriptedTransport,
    replies: Vec<serde_json::Value>,
) -> Arc<AtomicUsize> {
    let requests_seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests_seen);
    std::thread::spawn(move || {
        for reply in replies {
            let initial = counter.load(Ordering::SeqCst);
            let server = loop {
                let servers = transport.servers();
                if servers.len() > initial {
                    break servers[initial].clone();
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let (command, _envelope) = read_plain_request(&server);
            assert_eq!(command, "pair");
            counter.fetch_add(1, Ordering::SeqCst);
            server.send(&reply.to_string());
        }
    });
    requests_seen
}

#[test]
fn qr_parsing_and_endpoint_helpers_match_the_ts_contract() {
    let valid = format!(
        "{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"192.168.1.5\",\"p\":7777}}"
    );
    let parsed = parse_qr_payload(&valid).expect("valid");
    assert_eq!(parsed.host, "192.168.1.5");
    assert_eq!(parsed.port, 7777);
    assert_eq!(parsed.ts, None);
    // QR에 코드가 싸여 있어도 신뢰하지 않는다.
    let with_code = format!(
        "{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"192.168.1.5\",\"p\":7777,\"c\":\"123456\"}}"
    );
    assert!(parse_qr_payload(&with_code).is_some());
    assert!(
        parse_qr_payload("{\"v\":1,\"id\":\"o\",\"s\":\"s\",\"h\":\"1.2.3.4\",\"p\":7777}")
            .is_none()
    );
    assert!(parse_qr_payload("not json").is_none());

    assert_eq!(
        parse_host_endpoint("192.168.0.134"),
        Some(viewer_app_core::pairing::HostEndpoint {
            host: "192.168.0.134".into(),
            port: DEFAULT_CONTROL_PORT,
        })
    );
    assert_eq!(parse_host_endpoint(":7777"), None);
    assert_eq!(
        format_host_endpoint("192.168.0.134", 7777),
        "192.168.0.134:7777"
    );
    assert_eq!(
        resolve_pairing_host(Some("192.168.0.134:7777"), "old:64098"),
        "192.168.0.134:7777"
    );
    assert_eq!(resolve_pairing_host(Some("  "), "current"), "current");

    assert!(is_trusted_host("100.100.20.30"));
    assert!(!is_trusted_host("224.0.0.1"));

    assert!(can_submit_pairing_code("123 456", true, false));
    assert!(!can_submit_pairing_code("12345", true, false));
}

#[test]
fn token_key_schemas_are_v2_endpoint_and_v3_identity() {
    assert_eq!(
        token_key("192.168.1.5", 7777),
        "leftcar.token.v2.192.168.1.5.7777"
    );
    assert_eq!(
        identity_token_key(HOST_KEY),
        format!("leftcar.token.v3.{HOST_KEY}")
    );
}

#[test]
fn pair_with_host_success_stores_the_token_and_releases_the_socket() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}})],
    );

    let token =
        pair_with_host(&ports, &payload(), "123456", &flow_options()).expect("pair succeeds");
    assert_eq!(token, TOKEN_64HEX);
    assert_eq!(replies.load(Ordering::SeqCst), 1);

    // 토큰은 발급 엔드포인트 아래에 저장되고 구버전 전역 키는 치워진다.
    let endpoint_key = token_key("127.0.0.1", 7777);
    assert_eq!(
        store.snapshot().get(&endpoint_key).map(String::as_str),
        Some(TOKEN_64HEX)
    );
    assert!(!store.snapshot().contains_key("leftcar.token"));
    // 루프백은 평문이라 hostKey 자격증명은 없다.
    assert_eq!(
        get_stored_token(
            &ports,
            &viewer_app_core::pairing::HostEndpoint {
                host: "127.0.0.1".into(),
                port: 7777,
            }
        )
        .expect("read")
        .as_deref(),
        Some(TOKEN_64HEX)
    );
    // 소켓이 샜는지 — 페어링 연결은 단일 목적이라 닫힌다.
    assert!(transport.last_server().destroyed());
}

#[test]
fn pair_with_host_requires_the_separately_displayed_code() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let error = pair_with_host(&ports, &payload(), "", &flow_options()).expect_err("empty code");
    assert_eq!(error, PairingError::CodeInvalid);
    assert!(error.to_string().contains("leftcar:errPairingCodeInvalid"));
    assert_eq!(transport.connections_created(), 0);
}

#[test]
fn pair_with_host_rejects_a_malformed_issued_token_instead_of_storing_it() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": true, "result": {"token": "not-a-valid-token"}})],
    );
    let error =
        pair_with_host(&ports, &payload(), "123456", &flow_options()).expect_err("malformed token");
    assert_eq!(error, PairingError::ResponseInvalid);
    assert!(!store.snapshot().contains_key(&token_key("127.0.0.1", 7777)));
    assert!(transport.last_server().destroyed());
}

#[test]
fn a_failed_pairing_keeps_a_previously_stored_token() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let endpoint_key = token_key("127.0.0.1", 7777);
    store.put(&endpoint_key, TOKEN_64HEX);
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": false, "error": "pairing failed"})],
    );
    let error =
        pair_with_host(&ports, &payload(), "000000", &flow_options()).expect_err("pairing failed");
    assert!(is_pairing_unsupported_error(&error.to_string()));
    // 토큰은 발급한 호스트의 것이다 — 다른 시도가 그것을 지우지 않는다.
    assert_eq!(
        store.snapshot().get(&endpoint_key).map(String::as_str),
        Some(TOKEN_64HEX)
    );
}

#[test]
fn pairs_directly_with_a_host_endpoint_and_normalizes_the_code() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let server_holder = Arc::new(std::sync::Mutex::new(None::<ServerEnd>));
    let holder = Arc::clone(&server_holder);
    let transport_for_thread = transport.clone();
    std::thread::spawn(move || {
        let server = loop {
            let servers = transport_for_thread.servers();
            if !servers.is_empty() {
                break servers[0].clone();
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let (command, envelope) = read_plain_request(&server);
        assert_eq!(command, "pair");
        let args = envelope
            .get("args")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        assert_eq!(args.get("code").and_then(|v| v.as_str()), Some("123456"));
        assert!(envelope.get("token").is_none());
        server.send(&serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}}).to_string());
        *holder.lock().unwrap() = Some(server);
    });

    let token = pair_with_host_by_code(&ports, "localhost", 8888, "123 456", &flow_options())
        .expect("pair succeeds");
    assert_eq!(token, TOKEN_64HEX);
    assert_eq!(
        store
            .snapshot()
            .get(&token_key("localhost", 8888))
            .map(String::as_str),
        Some(TOKEN_64HEX)
    );
    assert!(transport.last_server().destroyed());
    let _ = server_holder;
}

#[test]
fn stores_one_token_per_host_endpoint() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![
            serde_json::json!({"ok": true, "result": {"token": "a".repeat(64)}}),
            serde_json::json!({"ok": true, "result": {"token": "b".repeat(64)}}),
        ],
    );
    pair_with_host(&ports, &payload(), "123456", &flow_options()).expect("pair A");
    pair_with_host_by_code(&ports, "localhost", 8888, "123456", &flow_options()).expect("pair B");

    assert_eq!(
        store
            .snapshot()
            .get(&token_key("127.0.0.1", 7777))
            .map(String::as_str),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(
        store
            .snapshot()
            .get(&token_key("localhost", 8888))
            .map(String::as_str),
        Some("b".repeat(64).as_str())
    );
    // 스코프 검사: 다른 호스트는 null이다.
    assert_eq!(
        get_stored_token(
            &ports,
            &viewer_app_core::pairing::HostEndpoint {
                host: "localhost".into(),
                port: 7777
            }
        )
        .expect("read"),
        None
    );
}

#[test]
fn get_device_id_is_stable_per_install() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let first = get_device_id(&ports).expect("device id");
    assert!(!first.is_empty());
    assert_eq!(
        store.snapshot().get("leftcar.deviceId").map(String::as_str),
        Some(first.as_str())
    );
    let second = get_device_id(&ports).expect("device id");
    assert_eq!(first, second);
}

#[test]
fn clear_token_removes_only_the_given_hosts_key() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    store.put(&token_key("127.0.0.1", 7777), TOKEN_64HEX);
    store.put(&token_key("localhost", 8888), &"b".repeat(64));
    clear_token(
        &ports,
        &viewer_app_core::pairing::HostEndpoint {
            host: "127.0.0.1".into(),
            port: 7777,
        },
    )
    .expect("clear");
    assert!(!store.snapshot().contains_key(&token_key("127.0.0.1", 7777)));
    assert_eq!(
        store
            .snapshot()
            .get(&token_key("localhost", 8888))
            .map(String::as_str),
        Some("b".repeat(64).as_str())
    );
}

#[test]
fn credential_lookup_prefers_the_verified_identity_and_refreshes_the_endpoint_alias() {
    let transport = ScriptedTransport::new();
    let (ports, store, recent) = test_ports(Arc::new(transport.clone()));
    let endpoint = viewer_app_core::pairing::HostEndpoint {
        host: "192.168.1.99".into(),
        port: 7777,
    };
    store.put(&token_key(&endpoint.host, endpoint.port), TOKEN_64HEX);
    store.put(&identity_token_key(HOST_KEY), &"b".repeat(64));

    let credential =
        get_stored_credential(&ports, &endpoint, Some(HOST_KEY), None, true).expect("credential");
    let credential = credential.expect("identity token found");
    assert_eq!(credential.token, "b".repeat(64));
    assert_eq!(credential.host_key.as_deref(), Some(HOST_KEY));
    // 엔드포인트 별칭도 정체 토큰을 따라간다.
    assert_eq!(
        store
            .snapshot()
            .get(&token_key(&endpoint.host, endpoint.port))
            .map(String::as_str),
        Some("b".repeat(64).as_str())
    );
    // TOFU 핀도 같은 거래에서 반영된다.
    let entries = recent.entries().expect("recent entries");
    assert!(entries
        .iter()
        .any(|entry| entry.host_key.as_deref() == Some(HOST_KEY)));
    // 이 엔드포인트는 이후 저장된 핀으로 해석된다.
    assert_eq!(
        get_stored_pinned_host_key(&ports, "192.168.1.99", 7777)
            .expect("pin")
            .as_deref(),
        Some(HOST_KEY)
    );
}

#[test]
fn a_stale_credential_incarnation_cannot_clear_a_newly_paired_token() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let endpoint = viewer_app_core::pairing::HostEndpoint {
        host: "127.0.0.1".into(),
        port: 7777,
    };
    store.put(&identity_token_key(HOST_KEY), TOKEN_64HEX);
    store.put(&token_key(&endpoint.host, endpoint.port), TOKEN_64HEX);
    let stale = get_stored_credential(&ports, &endpoint, Some(HOST_KEY), None, true)
        .expect("credential")
        .expect("stale");

    let fresh = "c".repeat(64);
    store.put(&identity_token_key(HOST_KEY), &fresh);
    store.put(&token_key(&endpoint.host, endpoint.port), &fresh);
    clear_stored_credential(&ports, &stale).expect("clear stale");

    assert_eq!(
        store
            .snapshot()
            .get(&identity_token_key(HOST_KEY))
            .map(String::as_str),
        Some(fresh.as_str())
    );
    assert_eq!(
        store
            .snapshot()
            .get(&token_key(&endpoint.host, endpoint.port))
            .map(String::as_str),
        Some(fresh.as_str())
    );
}

#[test]
fn a_cancelled_pairing_lifetime_rolls_back_persisted_credentials() {
    let transport = ScriptedTransport::new();
    let (ports, store, recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}})],
    );
    let attempt = PairingAttempt::new();
    let options = PairingFlowOptions {
        attempt: Some(Arc::clone(&attempt)),
        ..flow_options()
    };
    pair_with_host_by_code(&ports, "localhost", 8888, "123456", &options).expect("pair");
    let endpoint_key = token_key("localhost", 8888);
    assert!(store.snapshot().contains_key(&endpoint_key));

    attempt.cancel().expect("cancel");
    assert!(!store.snapshot().contains_key(&endpoint_key));
    assert!(recent.raw().is_none());
}

// -- 승인 기반 QR 페어링 -------------------------------------------------------

#[test]
fn approval_polls_pending_until_the_host_completes_approval() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let replies = script_pair_replies(
        transport.clone(),
        vec![
            serde_json::json!({"ok": true, "result": {"status": "pending"}}),
            serde_json::json!({"ok": true, "result": {"status": "pending"}}),
            serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}}),
        ],
    );
    let pending_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&pending_count);
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        on_pending: Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })),
        ..flow_options()
    };
    let result = pair_with_host_approval(&ports, &payload(), &options).expect("approved");
    assert_eq!(
        result,
        PairingApprovalResult::Approved {
            token: TOKEN_64HEX.to_string()
        }
    );
    assert_eq!(replies.load(Ordering::SeqCst), 3);
    assert_eq!(pending_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        store
            .snapshot()
            .get(&token_key("127.0.0.1", 7777))
            .map(String::as_str),
        Some(TOKEN_64HEX)
    );
}

#[test]
fn approval_returns_rejected_when_the_host_reports_an_explicit_denial() {
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": false, "error": "pairing rejected"})],
    );
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        ..flow_options()
    };
    let result = pair_with_host_approval(&ports, &payload(), &options).expect("rejected");
    assert_eq!(result, PairingApprovalResult::Rejected);
    assert!(!store.snapshot().contains_key("leftcar.token"));
}

#[test]
fn approval_keeps_polling_through_an_individual_request_timeout() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    // 두 번째 폴링은 응답 없이 시간을 끈다 — 개별 요청 타임아웃은 흐름을 죽이지
    // 않고 다음 폴링이 상태를 회수한다.
    let holder: Arc<std::sync::Mutex<Vec<ServerEnd>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let holder_for_thread = Arc::clone(&holder);
    let transport_for_thread = transport.clone();
    std::thread::spawn(move || {
        for round in 0..3usize {
            let server = loop {
                let servers = transport_for_thread.servers();
                if servers.len() > round {
                    break servers[round].clone();
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let (command, _envelope) = read_plain_request(&server);
            assert_eq!(command, "pair");
            holder_for_thread.lock().unwrap().push(server.clone());
            if round == 1 {
                // 응답 없이 요청 데드라인(200ms)을 넘긴다 — 타임아웃으로 채널이
                // 파괴되고 이 라운드는 응답하지 않는다.
                std::thread::sleep(Duration::from_millis(400));
            } else {
                let reply = if round < 2 {
                    serde_json::json!({"ok": true, "result": {"status": "pending"}})
                } else {
                    serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}})
                };
                server.send(&reply.to_string());
            }
        }
    });
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        ..flow_options()
    };
    let result = pair_with_host_approval(&ports, &payload(), &options).expect("approved");
    assert_eq!(
        result,
        PairingApprovalResult::Approved {
            token: TOKEN_64HEX.to_string()
        }
    );
}

#[test]
fn approval_times_out_when_approval_never_arrives() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": true, "result": {"status": "pending"}})],
    );
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        timeout_ms: Some(60),
        ..flow_options()
    };
    let error = pair_with_host_approval(&ports, &payload(), &options).expect_err("timeout");
    assert_eq!(error, PairingError::ApprovalTimeout);
    assert!(error
        .to_string()
        .contains("leftcar:errPairingApprovalTimeout"));
}

#[test]
fn approval_surfaces_old_host_pairing_failures_for_the_pin_fallback() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let _replies = script_pair_replies(
        transport.clone(),
        vec![serde_json::json!({"ok": false, "error": "pairing failed"})],
    );
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        ..flow_options()
    };
    let error = pair_with_host_approval(&ports, &payload(), &options).expect_err("unsupported");
    assert!(is_pairing_unsupported_error(&error.to_string()));
    assert!(!is_pairing_unsupported_error("pairing rejected"));
    assert!(is_pairing_rejected_error("pairing rejected"));
}

#[test]
fn cancellation_during_the_approval_handshake_closes_the_late_client_without_pair() {
    let transport = ScriptedTransport::new();
    let gate = GateHandle::new();
    transport.push_outcome(common::ConnectOutcome::Hold(Arc::clone(&gate)));
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let attempt = PairingAttempt::new();
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        attempt: Some(Arc::clone(&attempt)),
        ..flow_options()
    };
    let ports_for_thread = ports.clone();
    let payload_for_thread = payload();
    let pending = std::thread::spawn(move || {
        pair_with_host_approval(&ports_for_thread, &payload_for_thread, &options)
    });
    // 흐름이 connect에 들어섰다(게이트 대기)는 신호를 기다린다.
    assert!(wait_until(|| gate.waiters() >= 1, Duration::from_secs(5)));
    attempt.cancel().expect("cancel");
    gate.release();
    let error = pending.join().expect("flow thread").expect_err("cancelled");
    assert_eq!(error, PairingError::Cancelled);
    // pair 요청은 발화하지 않았고 늦게 도착한 클라이언트는 닫혔다.
    assert!(wait_until(
        || {
            transport
                .servers()
                .first()
                .map(|s| s.destroyed())
                .unwrap_or(false)
        },
        Duration::from_secs(5)
    ));
    let written = transport
        .servers()
        .first()
        .map(|s| s.written())
        .unwrap_or_default();
    assert!(
        written.iter().all(|line| !line.contains("\"pair\"")),
        "no pair request: {written:?}"
    );
    assert!(!store.snapshot().contains_key(&token_key("127.0.0.1", 7777)));
}

#[test]
fn approval_does_not_store_the_token_when_the_caller_cancels_during_the_wait() {
    // F18: 요청이 끝난 직후(store 전)에도 취소를 검사한다.
    let transport = ScriptedTransport::new();
    let (ports, store, _recent) = test_ports(Arc::new(transport.clone()));
    let cancel = viewer_app_core::pairing::CancelToken::new();
    let pending_cancel = Arc::clone(&cancel);
    let transport_for_thread = transport.clone();
    std::thread::spawn(move || {
        let server = loop {
            let servers = transport_for_thread.servers();
            if !servers.is_empty() {
                break servers[0].clone();
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let (command, _envelope) = read_plain_request(&server);
        assert_eq!(command, "pair");
        // 취소가 도달할 때까지 응답을 미룬다.
        let _ = pending_cancel.wait(1000);
        server.send(&serde_json::json!({"ok": true, "result": {"token": TOKEN_64HEX}}).to_string());
    });
    let options = PairingFlowOptions {
        poll_ms: Some(1),
        cancel: Some(Arc::clone(&cancel)),
        ..flow_options()
    };
    let pending_ports = ports.clone();
    let pending_payload = payload();
    let pending = std::thread::spawn(move || {
        pair_with_host_approval(&pending_ports, &pending_payload, &options)
    });
    cancel.cancel();
    let error = pending.join().expect("flow thread").expect_err("cancelled");
    assert_eq!(error, PairingError::Cancelled);
    assert!(!store.snapshot().contains_key(&token_key("127.0.0.1", 7777)));
}

#[test]
fn approval_rejects_immediately_when_the_signal_is_already_aborted() {
    let transport = ScriptedTransport::new();
    let (ports, _store, _recent) = test_ports(Arc::new(transport.clone()));
    let cancel = viewer_app_core::pairing::CancelToken::new();
    cancel.cancel();
    let options = PairingFlowOptions {
        cancel: Some(cancel),
        ..flow_options()
    };
    let error = pair_with_host_approval(&ports, &payload(), &options).expect_err("cancelled");
    assert_eq!(error, PairingError::Cancelled);
    assert_eq!(transport.connections_created(), 0);
}

#[test]
fn default_poll_matches_the_ts_constant() {
    assert_eq!(DEFAULT_POLL_MS, 2_500);
    assert_eq!(
        viewer_app_core::pairing::PAIRING_APPROVAL_TIMEOUT_MS,
        150_000
    );
}
