//! connect token injection·unauthorized·formatErrorMessage·host capture
//! capabilities 스펙 이전(apps/viewer-expo/src/control.test.ts).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{read_plain_request, wait_until, RecordingProvider, ScriptedTransport};
use viewer_app_core::control::records::{
    preferred_capture_backend, CaptureBackendInfo, CatalogView,
};
use viewer_app_core::control::{
    format_error_message, is_unauthorized_error, ConnectOptions, ControlErrorKind, ErrorMessages,
};
use viewer_app_core::pairing::is_valid_issued_token;

#[test]
fn adds_the_provider_token_to_every_request_envelope() {
    let transport = ScriptedTransport::new();
    let options =
        ConnectOptions::new().token_provider(RecordingProvider::with_token(Some("tok-123")));
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let (tx, rx) = std::sync::mpsc::channel();
    let client_for_thread = client.clone();
    std::thread::spawn(move || {
        let _ = tx.send(
            client_for_thread
                .request_simple("addNumbers", Some(&serde_json::json!({"a": 1, "b": 2}))),
        );
    });
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let (_command, envelope) = read_plain_request(&server);
    assert_eq!(
        envelope,
        serde_json::json!({
            "command": "addNumbers",
            "args": {"a": 1, "b": 2},
            "token": "tok-123",
        })
    );
    server.send(&serde_json::json!({"ok": true, "result": {"n": 3}}).to_string());
    let value = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::json!({"n": 3}));
    client.close();
}

#[test]
fn omits_the_token_field_when_the_provider_returns_null() {
    let transport = ScriptedTransport::new();
    let options = ConnectOptions::new().token_provider(RecordingProvider::with_token(None));
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let (tx, rx) = std::sync::mpsc::channel();
    let client_for_thread = client.clone();
    std::thread::spawn(move || {
        let _ = tx.send(client_for_thread.request_simple("getCatalog", None));
    });
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let (_command, envelope) = read_plain_request(&server);
    assert_eq!(
        envelope,
        serde_json::json!({"command": "getCatalog", "args": {}})
    );
    server.send(&serde_json::json!({"ok": true, "result": {"displays": []}}).to_string());
    let value = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::json!({"displays": []}));
    client.close();
}

#[test]
fn sends_no_token_when_no_provider_is_given() {
    let transport = ScriptedTransport::new();
    let client = common::connect_plain(&transport, "127.0.0.1", ConnectOptions::new());
    let server = transport.last_server();

    let (tx, rx) = std::sync::mpsc::channel();
    let client_for_thread = client.clone();
    std::thread::spawn(move || {
        let _ = tx.send(client_for_thread.request_simple(
            "pair",
            Some(&serde_json::json!({"offerId": "o", "code": "123456"})),
        ));
    });
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    let (_command, envelope) = read_plain_request(&server);
    assert_eq!(
        envelope.get("command").and_then(|v| v.as_str()),
        Some("pair")
    );
    assert!(envelope.get("token").is_none());
    let issued_token = "a".repeat(64);
    server.send(&serde_json::json!({"ok": true, "result": {"token": issued_token}}).to_string());
    let value = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect("ok");
    assert_eq!(value, serde_json::json!({"token": issued_token}));
    assert!(is_valid_issued_token(&issued_token));
    client.close();
}

#[test]
fn provider_failure_rejects_the_request_without_writing_anything() {
    let transport = ScriptedTransport::new();
    let options = ConnectOptions::new()
        .token_provider(RecordingProvider::failing("secure store unavailable"));
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let error = client
        .request_simple("getCatalog", None)
        .expect_err("provider failure");
    assert_eq!(error.message, "secure store unavailable");
    assert_eq!(server.written_len(), 0);
    client.close();
}

#[test]
fn caches_the_token_so_later_requests_skip_the_provider() {
    let transport = ScriptedTransport::new();
    let provider = RecordingProvider::with_token(Some("tok-cached"));
    let options = ConnectOptions::new().token_provider(Arc::clone(&provider) as Arc<_>);
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let probe = common::issue(&client, "getStatus");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    server.send(&serde_json::json!({"ok": true, "result": {"sessions": []}}).to_string());
    probe
        .recv_timeout(Duration::from_secs(5))
        .expect("probe")
        .expect("ok");

    // 두 번째 요청은 provider를 다시 부르지 않는다(캐시).
    let start = common::issue(&client, "startStream");
    assert!(wait_until(
        || server.written_len() == 2,
        Duration::from_secs(5)
    ));
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    server.send(&serde_json::json!({"ok": true, "result": {"session": 1}}).to_string());
    start
        .recv_timeout(Duration::from_secs(5))
        .expect("start")
        .expect("ok");
    client.close();
}

#[test]
fn unauthorized_error_handling_kind_and_beats_the_close_event() {
    let transport = ScriptedTransport::new();
    let options =
        ConnectOptions::new().token_provider(RecordingProvider::with_token(Some("bad-token")));
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let promise = common::issue(&client, "getCatalog");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    // Host behavior: reply unauthorized, then close the connection.
    server.send(&serde_json::json!({"ok": false, "error": "unauthorized"}).to_string());
    server.close();
    let error = promise
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect_err("unauthorized");
    assert!(is_unauthorized_error(&error));
    assert_eq!(error.kind, ControlErrorKind::Unauthorized);
    assert_eq!(error.message, "unauthorized");
    // unauthorized 응답은 토큰 캐시를 흘린다 — 다음 요청이 provider를 다시 부른다.
    // (소켓은 이미 닫혀 요청이 실패하므로 여기선 kind만 단정한다.)
    let _ = client;
}

#[test]
fn other_remote_errors_keep_the_remote_kind() {
    let transport = ScriptedTransport::new();
    let options = ConnectOptions::new().token_provider(RecordingProvider::with_token(Some("tok")));
    let client = common::connect_plain(&transport, "127.0.0.1", options);
    let server = transport.last_server();

    let promise = common::issue(&client, "nope");
    assert!(wait_until(
        || server.written_len() == 1,
        Duration::from_secs(5)
    ));
    server.send(&serde_json::json!({"ok": false, "error": "unknown command"}).to_string());
    let error = promise
        .recv_timeout(Duration::from_secs(5))
        .expect("settles")
        .expect_err("remote");
    assert!(!is_unauthorized_error(&error));
    assert_eq!(error.kind, ControlErrorKind::Remote);
    assert_eq!(error.message, "unknown command");
    client.close();
}

#[test]
fn formats_error_messages_like_the_ts_classifier() {
    let t = ErrorMessages::KOREAN;
    // 매핑되지 않은 영어 원문은 안내문 하나로 통일된다(원문은 콘솔 기록).
    assert_eq!(
        format_error_message("custom error", &t),
        "문제가 발생했습니다. 잠시 후 다시 시도해 주세요."
    );
    assert_eq!(
        format_error_message("ECONNREFUSED", &t),
        "컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요."
    );
    assert_eq!(
        format_error_message("msg error", &t),
        "문제가 발생했습니다. 잠시 후 다시 시도해 주세요."
    );
    assert_eq!(
        format_error_message("", &t),
        "문제가 발생했습니다. 잠시 후 다시 시도해 주세요."
    );
    // 큐레이션된 한국어 문구는 그대로.
    assert_eq!(
        format_error_message("신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다", &t),
        "신뢰하는 같은 Wi-Fi의 컴퓨터만 연결할 수 있습니다"
    );
    // 호스트 화면 공유 권한 실패는 설정 안내로.
    assert_eq!(
        format_error_message(
            "startStream failed: screen-recording permission is not granted to Leftcar Host",
            &t
        ),
        "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요."
    );
    assert_eq!(
        format_error_message("Screen Recording permission required", &t),
        "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 Leftcar를 허용해 주세요."
    );
    // 명시적 페어링 거절은 거절 안내로.
    assert_eq!(
        format_error_message("pairing rejected", &t),
        "컴퓨터에서 연결 요청을 거절했습니다. 다시 시도하려면 QR을 다시 스캔해 주세요."
    );
    // non-Error 소켓 오류도 'undefined'를 만들지 않는다.
    assert!(!format_error_message("ECONNREFUSED", &t).contains("undefined"));
}

#[test]
fn selects_host_capture_backends_like_the_ts_helper() {
    let wgc = CatalogView {
        capture_backends: vec![CaptureBackendInfo {
            id: "windowsGraphicsCapture".into(),
            label: "Windows Graphics Capture".into(),
            hint: "hardware H.264".into(),
        }],
        ..Default::default()
    };
    assert_eq!(
        preferred_capture_backend(Some(&wgc), Some("screenCaptureKit")),
        "windowsGraphicsCapture"
    );

    let catalog = CatalogView {
        capture_backends: vec![
            CaptureBackendInfo {
                id: "screenCaptureKit".into(),
                label: "SCK".into(),
                hint: "default".into(),
            },
            CaptureBackendInfo {
                id: "cgDisplayStream".into(),
                label: "CGDS".into(),
                hint: "compat".into(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        preferred_capture_backend(Some(&catalog), Some("cgDisplayStream")),
        "cgDisplayStream"
    );
    assert_eq!(preferred_capture_backend(None, None), "screenCaptureKit");

    let automatic = CatalogView {
        capture_backends: vec![CaptureBackendInfo {
            id: "cgDisplayStream".into(),
            label: "Automatic".into(),
            hint: "pickerless".into(),
        }],
        ..Default::default()
    };
    assert_eq!(
        preferred_capture_backend(Some(&automatic), Some("")),
        "cgDisplayStream"
    );
}

#[test]
fn invalid_issued_tokens_are_shape_checked_like_the_host_contract() {
    assert!(is_valid_issued_token(&"a".repeat(64)));
    assert!(!is_valid_issued_token("not-a-valid-token"));
}
