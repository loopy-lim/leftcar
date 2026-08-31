use control_contract::host::{DisplayInfo, EncoderExperiment};
use control_contract::udp_stability::{AppliedUdpStability, UdpStabilityProfile};
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
        encoder_experiment: Mutex::new(EncoderExperiment::Auto),
    })
}

/// Backend that records the `ip` argument of every `start` call.
struct RecordingBackend {
    displays: Vec<DisplayInfo>,
    started_ips: Mutex<Vec<String>>,
    started_udp_stability: Mutex<Vec<AppliedUdpStability>>,
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
        _capture_backend: &str,
        _media_transport: &str,
        _content_mode: &str,
        _encoder_experiment: EncoderExperiment,
        udp_stability: &AppliedUdpStability,
    ) -> Result<u32, String> {
        self.started_ips.lock().unwrap().push(ip.to_owned());
        self.started_udp_stability
            .lock()
            .unwrap()
            .push(udp_stability.clone());
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
            encoder_experiment_diagnostics_available: false,
            encoder_experiment_requested: "auto".into(),
            encoder_experiment_applied: "rateControl".into(),
            encoder_experiment_fallback_reason: None,
            encoder_frame_drops: 0,
            encoder_frame_drop_fps: 0,
            valid_encode_output_fps: 90,
            encode_submit_call_p50_us: 0,
            encode_submit_call_p95_us: 0,
            encoder_callback_p50_us: 0,
            encoder_callback_p95_us: 0,
            packetization_in_flight: 0,
            base_frame_qp: None,
            base_frame_qp_changes: 0,
            capture_fps: 90,
            encode_submit_fps: 90,
            encode_output_fps: 90,
            rendered_fps: Some(90),
            capture_callbacks: 100,
            encode_output_callbacks: 100,
            encode_submit_failures: 0,
            encode_in_flight: 0,
            dropped: 0,
            network_dropped: 0,
            network_queue_dropped: 0,
            recovery_frames_dropped: 0,
            udp_send_failures: 0,
            udp_send_retries: 0,
            recovery_keyframes: 0,
            recovery_requests_suppressed: 0,
            capture_queue_dropped: 0,
            capture_to_encode_us: 0,
            max_capture_to_encode_us: 0,
            capture_queue_wait_us: 0,
            max_capture_queue_wait_us: 0,
            encode_output_us: 0,
            max_encode_output_us: 0,
            packetization_us: 0,
            max_packetization_us: 0,
            send_block_us: 0,
            max_send_block_us: 0,
            send_pace_us: 0,
            max_send_pace_us: 0,
            pending_frame: 0,
            pending_frame_bytes: 0,
            pending_frame_oldest_age_us: 0,
            capture_backend: "screenCaptureKit".into(),
            media_transport: "udp".into(),
            first_capture_ms: 20,
            first_encode_ms: 25,
            first_send_ms: 26,
            current_bitrate: 12_000_000,
            encoder_mode: "ave".into(),
            encoder_id: "com.apple.videotoolbox.videoencoder.ave.avc".into(),
            encoder_hardware_accelerated: Some(true),
            encoder_preset: "high-speed".into(),
            encoder_profile: "main".into(),
            encoder_applied_properties: vec!["HighSpeed".into(), "Quality".into()],
            encoder_unsupported_properties: vec!["SuggestedLookAheadFrameCount".into()],
            encoder_rejected_properties: vec!["Quality=-12900".into()],
            encoder_fallback_reason: None,
            quality_hint: None,
            quality_override: None,
            quality_adaptation_checks: 0,
            quality_adaptation_changes: 0,
            quality_adaptation_rejections: 0,
            quality_adaptation_last_status: "not_checked".into(),
            capture_interval_p95_us: 16_667,
            capture_to_encode_p95_us: 8_000,
            capture_queue_wait_p95_us: 1_000,
            encode_output_p95_us: 7_000,
            packetization_p95_us: 0,
            encode_output_interval_p95_us: 11_111,
            send_block_p95_us: 1_000,
            send_pace_p95_us: 0,
            last_au_bytes: 0,
            last_au_fragments: 0,
            last_au_parity: 0,
            last_au_datagrams: 0,
            last_au_expected_datagrams: 0,
            last_au_send_us: 0,
            last_au_is_keyframe: false,
            max_au_bytes: 0,
            max_au_fragments: 0,
            sent_datagrams: 0,
            sent_parity_datagrams: 0,
            error: None,
            receiver_frame_gaps: 0,
            receiver_input_drops: 0,
            receiver_incomplete_aus: 0,
            receiver_stale_frames: 0,
            receiver_stale_input_drops: None,
            receiver_output_burst_discards: 0,
            receiver_rtt_ms: None,
            receiver_wire_ms: None,
            receiver_feedback_age_ms: None,
            ..StatsInfo::default()
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
    assert!(resp.contains("\"platform\":\"test\""), "{resp}");
    assert!(resp.contains("\"captureBackends\""), "{resp}");
    assert!(resp.contains("\"id\":\"auto\""), "{resp}");
    assert!(resp.contains("\"id\":\"rateControl\""), "{resp}");
    assert!(resp.contains("\"id\":\"adaptiveQp\""), "{resp}");
    assert!(resp.contains("\"id\":\"encoderPool\""), "{resp}");
    assert!(resp.contains("\"udpStabilityCapabilities\""), "{resp}");
    assert!(resp.contains("\"fecParityOptions\":[2,4]"), "{resp}");
    assert!(!resp.contains("splitHorizontal"), "{resp}");
}

#[tokio::test]
async fn udp_stability_is_negotiated_echoed_and_passed_to_backend() {
    let p = pairing();
    let recorder = Arc::new(RecordingBackend {
        displays: vec![DisplayInfo {
            index: 0,
            name: "Main".into(),
            width: 3840,
            height: 2160,
        }],
        started_ips: Mutex::new(Vec::new()),
        started_udp_stability: Mutex::new(Vec::new()),
    });
    let server = Arc::new(ControlServer::new(recorder.clone(), p.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        server.run(listener).await;
    });
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-udp-stability").await;

    let response = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5000,"width":3840,"height":2160,"fps":60,"udpStability":{"profile":"stable","viewer":{"version":1,"maxFecParityShards":4,"splitFeedbackBytes":120}}}"#,
        &token,
    )
    .await;
    assert!(response.contains("\"ok\":true"), "{response}");
    assert!(response.contains("\"applied\":\"stable\""), "{response}");
    assert!(response.contains("\"burstDatagrams\":2"), "{response}");
    assert!(response.contains("\"fecParityShards\":4"), "{response}");

    let applied = recorder.started_udp_stability.lock().unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].applied, UdpStabilityProfile::Stable);
    assert_eq!(applied[0].burst_datagrams, 2);
    assert_eq!(applied[0].fec_parity_shards, 4);
    drop(applied);

    let status = send_request(&mut sock, "getStatus", "{}", &token).await;
    assert!(status.contains("\"udpStability\""), "{status}");
    assert!(status.contains("\"fecParityShards\":4"), "{status}");
}

#[tokio::test]
async fn encoder_experiment_is_carried_to_session_and_reserved_profiles_are_rejected() {
    let (addr, p) = spawn_test_server().await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    let token = pair_over_socket(&mut sock, &p, "viewer-encoder-experiment").await;

    let start = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5002,"width":3840,"height":2160,"fps":60,"encoderExperiment":"adaptiveQp"}"#,
        &token,
    )
    .await;
    assert!(start.contains("\"ok\":true"), "{start}");

    let status = send_request(&mut sock, "getStatus", "{}", &token).await;
    assert!(
        status.contains("\"encoderExperimentRequested\":\"adaptiveQp\""),
        "{status}"
    );
    assert!(
        status.contains("\"encoderExperimentApplied\":\"adaptiveQp\""),
        "{status}"
    );
    assert!(
        status.contains("\"encoderExperimentDiagnosticsAvailable\":true"),
        "{status}"
    );

    let session: serde_json::Value = serde_json::from_str(&start).unwrap();
    let session_id = session["result"]["session"].as_u64().unwrap();
    let _ = send_request(
        &mut sock,
        "stopStream",
        &format!(r#"{{"session":{session_id}}}"#),
        &token,
    )
    .await;

    let reserved = send_request(
        &mut sock,
        "startStream",
        r#"{"sourceIndex":0,"viewerPort":5002,"width":3840,"height":2160,"fps":60,"encoderExperiment":"splitHorizontal"}"#,
        &token,
    )
    .await;
    assert!(reserved.contains("\"ok\":false"), "{reserved}");
    assert!(
        reserved.contains("unsupported encoder experiment"),
        "{reserved}"
    );
}

#[tokio::test]
async fn test_full_stream_lifecycle() {
    let p = pairing();
    let server = Arc::new(ControlServer::new(
        Arc::new(RecordingBackend {
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
            started_ips: Mutex::new(Vec::new()),
            started_udp_stability: Mutex::new(Vec::new()),
        }),
        p.clone(),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        server.run(listener).await;
    });
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
    assert!(
        status_resp.contains("\"encoderMode\":\"ave\""),
        "{status_resp}"
    );
    assert!(
        status_resp.contains("\"encoderID\":\"com.apple.videotoolbox.videoencoder.ave.avc\""),
        "{status_resp}"
    );
    assert!(
        status_resp.contains("\"encoderHardwareAccelerated\":true"),
        "{status_resp}"
    );
    assert!(
        status_resp.contains("\"encoderAppliedProperties\":[\"HighSpeed\",\"Quality\"]"),
        "{status_resp}"
    );
    assert!(
        status_resp.contains("\"encoderExperimentDiagnosticsAvailable\":false"),
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
        r#"{"sourceIndex":1,"viewerPort":5001,"width":2560,"height":1440,"fps":90}"#,
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

    // connection must be closed: Unix commonly reports EOF while Windows may
    // report WSAECONNABORTED/WSAECONNRESET for the same peer-initiated close.
    let _ = sock
        .write_all(b"{\"command\":\"getCatalog\",\"args\":{}}\n")
        .await;
    let mut buf = [0u8; 64];
    let n = sock.read(&mut buf).await.unwrap_or(0);
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
async fn startstream_rejects_unrelated_viewer_ip_and_uses_peer() {
    let p = pairing();
    let recorder = Arc::new(RecordingBackend {
        displays: vec![DisplayInfo {
            index: 0,
            name: "Main".into(),
            width: 1920,
            height: 1080,
        }],
        started_ips: Mutex::new(Vec::new()),
        started_udp_stability: Mutex::new(Vec::new()),
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
