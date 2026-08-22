//! Contract tests (docs/04 §11, H02 acceptance).

use control_contract::host::{AddNumbersInput, AddNumbersOutput, PairingOfferView};
use control_contract::viewer::{ViewerAddNumbersInput, ViewerAddNumbersOutput};
use control_contract::{contract_hash, host_package, viewer_package};

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
