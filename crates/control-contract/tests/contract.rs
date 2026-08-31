//! Contract tests (docs/04 §11, H02 acceptance).

use control_contract::host::{
    phase_a_encoder_experiments, AddNumbersInput, AddNumbersOutput, CatalogView, PairingOfferView,
    StartStreamInput, StartStreamOutput,
};

#[test]
fn split_vertical_capability_roundtrips() {
    let info = control_contract::host::EncoderExperimentInfo {
        id: control_contract::host::EncoderExperiment::SplitVertical,
        label: "4K dual encoder".into(),
        hint: "Wi-Fi UDP only".into(),
        requires_reconnect: true,
    };
    assert!(serde_json::to_string(&info)
        .unwrap()
        .contains("splitVertical"));
}
use control_contract::udp_stability::{
    host_udp_stability_capabilities, resolve_udp_stability, AppliedUdpStability,
    UdpStabilityCapabilities, UdpStabilityProfile, UdpStabilityRequest, ViewerUdpCapabilities,
};
use control_contract::viewer::{ViewerAddNumbersInput, ViewerAddNumbersOutput};
use control_contract::{contract_hash, host_package, viewer_package};

fn udp_capabilities() -> UdpStabilityCapabilities {
    UdpStabilityCapabilities {
        version: 1,
        profiles: vec![
            UdpStabilityProfile::Auto,
            UdpStabilityProfile::Responsive,
            UdpStabilityProfile::Balanced,
            UdpStabilityProfile::Stable,
        ],
        burst_datagram_options: vec![2, 4, 8],
        fec_parity_options: vec![2, 4],
        adaptive_pacing: true,
        requires_reconnect: true,
    }
}

fn viewer_udp_capabilities(max_fec_parity_shards: u8) -> ViewerUdpCapabilities {
    ViewerUdpCapabilities {
        version: 1,
        max_fec_parity_shards,
        split_feedback_bytes: 120,
    }
}

#[test]
fn udp_stability_legacy_omission_preserves_current_wire_policy() {
    assert_eq!(
        resolve_udp_stability(None, &udp_capabilities()).unwrap(),
        AppliedUdpStability {
            requested: UdpStabilityProfile::Responsive,
            applied: UdpStabilityProfile::Responsive,
            burst_datagrams: 8,
            fec_parity_shards: 2,
            adaptive_pacing: false,
            fallback_reason: None,
        }
    );
}

#[test]
fn udp_stability_auto_enables_bounded_adaptation() {
    let request = UdpStabilityRequest {
        profile: UdpStabilityProfile::Auto,
        burst_datagrams: None,
        fec_parity_shards: None,
        adaptive_pacing: None,
        viewer: Some(viewer_udp_capabilities(4)),
    };
    assert_eq!(
        resolve_udp_stability(Some(&request), &udp_capabilities()).unwrap(),
        AppliedUdpStability {
            requested: UdpStabilityProfile::Auto,
            applied: UdpStabilityProfile::Auto,
            burst_datagrams: 4,
            fec_parity_shards: 2,
            adaptive_pacing: true,
            fallback_reason: None,
        }
    );
}

#[test]
fn udp_stability_stable_requires_four_parity_receiver_support() {
    let supported = UdpStabilityRequest {
        profile: UdpStabilityProfile::Stable,
        burst_datagrams: None,
        fec_parity_shards: None,
        adaptive_pacing: None,
        viewer: Some(viewer_udp_capabilities(4)),
    };
    let applied = resolve_udp_stability(Some(&supported), &udp_capabilities()).unwrap();
    assert_eq!(applied.burst_datagrams, 2);
    assert_eq!(applied.fec_parity_shards, 4);
    assert!(!applied.adaptive_pacing);

    let unsupported = UdpStabilityRequest {
        viewer: Some(viewer_udp_capabilities(2)),
        ..supported
    };
    assert_eq!(
        resolve_udp_stability(Some(&unsupported), &udp_capabilities()).unwrap_err(),
        "안정성 우선 모드는 4개 FEC parity를 지원하는 Viewer가 필요합니다"
    );
}

#[test]
fn udp_stability_custom_rejects_unadvertised_values() {
    let request = UdpStabilityRequest {
        profile: UdpStabilityProfile::Custom,
        burst_datagrams: Some(6),
        fec_parity_shards: Some(4),
        adaptive_pacing: Some(false),
        viewer: Some(viewer_udp_capabilities(4)),
    };
    assert_eq!(
        resolve_udp_stability(Some(&request), &udp_capabilities()).unwrap_err(),
        "Host가 UDP burst 6 설정을 지원하지 않습니다"
    );
}

#[test]
fn udp_stability_advertises_and_accepts_clean_lan_burst_sixteen() {
    let host = host_udp_stability_capabilities();
    assert_eq!(host.burst_datagram_options, vec![2, 4, 8, 16]);
    let request = UdpStabilityRequest {
        profile: UdpStabilityProfile::Custom,
        burst_datagrams: Some(16),
        fec_parity_shards: Some(2),
        adaptive_pacing: Some(false),
        viewer: Some(viewer_udp_capabilities(4)),
    };
    let applied = resolve_udp_stability(Some(&request), &host).unwrap();
    assert_eq!(applied.burst_datagrams, 16);
    assert_eq!(applied.fec_parity_shards, 2);
    assert!(!applied.adaptive_pacing);
}

/// The H02 proof: host adapter path computes 20 + 22 = 42 through the real
/// Rustra invocation pipeline.
#[test]
fn host_add_numbers_20_22_is_42() {
    let package = host_package();
    let out: AddNumbersOutput = package
        .invoke("addNumbers", AddNumbersInput { a: 20, b: 22 })
        .expect("invoke succeeds");
    assert_eq!(out.value, 42);
}

/// The H09 proof: viewer package path also computes 42 through Rustra.
#[test]
fn viewer_add_numbers_20_22_is_42() {
    let package = viewer_package();
    let out: ViewerAddNumbersOutput = package
        .invoke("viewerAddNumbers", ViewerAddNumbersInput { a: 20, b: 22 })
        .expect("invoke succeeds");
    assert_eq!(out.value, 42);
}

#[test]
fn unknown_command_is_rejected() {
    let package = host_package();
    let result: rustra::Result<AddNumbersOutput> =
        package.invoke("sendKeyboard", AddNumbersInput { a: 1, b: 2 });
    assert!(
        result.is_err(),
        "unknown/input-like commands must be denied (T-06)"
    );
}

#[test]
fn viewer_contract_does_not_expose_high_rate_input_commands() {
    // Input is a token-bound native datagram plane, not a Rustra command.
    let generated = viewer_package().generate_typescript().expect("generates");
    let surface = format!("{}{}", generated.commands_ts, generated.types_ts);
    for banned in [
        "sendKeyboard",
        "sendMouse",
        "injectInput",
        "clipboard",
        "sendkeyboard",
        "sendmouse",
        "injectinput",
    ] {
        assert!(
            !surface.contains(banned),
            "viewer contract leaked input-like symbol {banned}"
        );
    }
}

#[test]
fn video_payload_type_is_absent_from_generated_typescript() {
    let host = host_package().generate_typescript().expect("generates");
    let viewer = viewer_package().generate_typescript().expect("generates");
    for generated in [host, viewer] {
        let surface = format!("{}{}", generated.types_ts, generated.commands_ts);
        for banned in [
            "EncodedFrame",
            "NalUnit",
            "VideoPacket",
            "payload: number[]",
        ] {
            assert!(
                !surface.contains(banned),
                "generated TS leaked video type {banned}"
            );
        }
    }
}

#[test]
fn pairing_offer_view_never_contains_private_key() {
    // Field-level check: the view type has only public/ephemeral fields.
    let view = PairingOfferView {
        pairing_version: 1,
        host_public_fingerprint: "ab12cd34".into(),
        ephemeral_offer_id: "offer-1".into(),
        expiry_unix: 1_000,
        address_hints: vec!["leftcar://host".into()],
        human_verification_code: "123-456".into(),
    };
    let json = serde_json::to_string(&view).unwrap().to_lowercase();
    for banned in ["privatekey", "private_key", "secret", "token"] {
        assert!(
            !json.contains(banned),
            "pairing view leaked {banned}: {json}"
        );
    }
}

#[test]
fn generated_contract_hash_is_stable() {
    let a = contract_hash();
    let b = contract_hash();
    assert_eq!(a, b, "contract hash must be deterministic");
    assert_eq!(a.len(), 16);
}

#[test]
fn schema_lists_only_declared_commands() {
    let host = host_package().generate_typescript().expect("generates");
    let schema: serde_json::Value = serde_json::from_str(&host.schema_json).expect("valid json");
    let text = schema.to_string();
    assert!(text.contains("addNumbers"));
    // and nothing input-like
    assert!(!text.to_lowercase().contains("keyboard"));
}

#[test]
fn catalog_advertises_phase_a_encoder_experiments() {
    let catalog = CatalogView {
        platform: "macos".into(),
        capture_backends: Vec::new(),
        media_host: None,
        displays: Vec::new(),
        encoder_experiments: phase_a_encoder_experiments(),
        udp_stability_capabilities: Some(udp_capabilities()),
    };
    let advertised: Vec<_> = catalog
        .encoder_experiments
        .iter()
        .map(|info| (info.id.as_str(), info.requires_reconnect))
        .collect();
    assert_eq!(
        advertised,
        vec![
            ("auto", true),
            ("rateControl", true),
            ("adaptiveQp", true),
            ("encoderPool", true),
        ]
    );
}

#[test]
fn udp_stability_contract_keeps_old_peers_compatible() {
    let catalog: CatalogView = serde_json::from_str(
        r#"{"platform":"macos","captureBackends":[],"displays":[],"encoderExperiments":[]}"#,
    )
    .unwrap();
    assert!(catalog.udp_stability_capabilities.is_none());

    let input: StartStreamInput = serde_json::from_str(
        r#"{"sourceIndex":0,"viewerPort":5001,"width":3840,"height":2160,"fps":60}"#,
    )
    .unwrap();
    assert!(input.udp_stability.is_none());

    let output: StartStreamOutput = serde_json::from_str(r#"{"session":7}"#).unwrap();
    assert!(output.udp_stability.is_none());
}

#[test]
fn udp_stability_contract_roundtrips_requested_and_applied_settings() {
    let input: StartStreamInput = serde_json::from_str(
        r#"{
          "sourceIndex":0,
          "viewerPort":5001,
          "width":3840,
          "height":2160,
          "fps":60,
          "udpStability":{
            "profile":"stable",
            "viewer":{"version":1,"maxFecParityShards":4,"splitFeedbackBytes":120}
          }
        }"#,
    )
    .unwrap();
    assert_eq!(
        input.udp_stability.as_ref().unwrap().profile,
        UdpStabilityProfile::Stable
    );

    let output = StartStreamOutput {
        session: 7,
        udp_stability: Some(AppliedUdpStability {
            requested: UdpStabilityProfile::Stable,
            applied: UdpStabilityProfile::Stable,
            burst_datagrams: 2,
            fec_parity_shards: 4,
            adaptive_pacing: false,
            fallback_reason: None,
        }),
    };
    let encoded = serde_json::to_string(&output).unwrap();
    assert!(encoded.contains("\"udpStability\""));
    let decoded: StartStreamOutput = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.udp_stability.unwrap().fec_parity_shards, 4);
}
