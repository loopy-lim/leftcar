//! Input-only keyboard/mouse negotiation and bounded native duplex wire contract.
//!
//! Generated Draft7 schemas advertise portable scalar/header/dimension bounds.
//! Native validation remains authoritative for UTF8 byte length and Unicode
//! control rejection (JSON Schema length counts codepoints), cursor x/y relative
//! to this message's width/height (Draft7 has no sibling-value comparison), the
//! 8192-byte plaintext limit, and trusted session/epoch/origin context.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_KM_PLAINTEXT: usize = 8192;
pub const MAX_KM_SESSION_ID_BYTES: usize = 128;
pub const MAX_KM_DIMENSION: u32 = 32768;
pub const MAX_KM_DELTA: f64 = 65536.0;
pub const MAX_KM_KEY_CODE: u16 = 255;
pub const MAX_KM_BUTTON: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KmOrigin {
    Mac,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KmOwner {
    #[default]
    Local,
    MacToAndroid,
    AndroidToMac,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum KmPayload {
    Hello,
    State {
        owner: KmOwner,
    },
    Move {
        #[schemars(range(min = -65536, max = "MAX_KM_DELTA"))]
        dx: f64,
        #[schemars(range(min = -65536, max = "MAX_KM_DELTA"))]
        dy: f64,
    },
    Button {
        #[schemars(range(min = 0, max = "MAX_KM_BUTTON"))]
        button: u8,
        down: bool,
    },
    Wheel {
        #[schemars(range(min = -65536, max = "MAX_KM_DELTA"))]
        dx: f64,
        #[schemars(range(min = -65536, max = "MAX_KM_DELTA"))]
        dy: f64,
    },
    /// USB HID keyboard page usage ID (A=4, leftCtrl=224), never an OS keycode.
    Key {
        #[schemars(range(min = 0, max = "MAX_KM_KEY_CODE"))]
        code: u16,
        down: bool,
    },
    Cursor {
        /// Runtime additionally requires x<=this cursor message's width.
        #[schemars(range(min = 0, max = "MAX_KM_DIMENSION"))]
        x: f64,
        /// Runtime additionally requires y<=this cursor message's height.
        #[schemars(range(min = 0, max = "MAX_KM_DIMENSION"))]
        y: f64,
        #[schemars(range(min = 1, max = "MAX_KM_DIMENSION"))]
        width: u32,
        #[schemars(range(min = 1, max = "MAX_KM_DIMENSION"))]
        height: u32,
    },
    Edge {
        returning: bool,
    },
    ReleaseAll,
    Heartbeat,
    Stop,
}

// serde's internally tagged *unit* variants discard extra fields. The private
// struct-variant representation closes that hole for hello/releaseAll/etc.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum StrictPayload {
    Hello {},
    State {
        owner: KmOwner,
    },
    Move {
        dx: f64,
        dy: f64,
    },
    Button {
        button: u8,
        down: bool,
    },
    Wheel {
        dx: f64,
        dy: f64,
    },
    Key {
        code: u16,
        down: bool,
    },
    Cursor {
        x: f64,
        y: f64,
        width: u32,
        height: u32,
    },
    Edge {
        returning: bool,
    },
    ReleaseAll {},
    Heartbeat {},
    Stop {},
}

impl<'de> Deserialize<'de> for KmPayload {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match StrictPayload::deserialize(deserializer)? {
            StrictPayload::Hello {} => Self::Hello,
            StrictPayload::State { owner } => Self::State { owner },
            StrictPayload::Move { dx, dy } => Self::Move { dx, dy },
            StrictPayload::Button { button, down } => Self::Button { button, down },
            StrictPayload::Wheel { dx, dy } => Self::Wheel { dx, dy },
            StrictPayload::Key { code, down } => Self::Key { code, down },
            StrictPayload::Cursor {
                x,
                y,
                width,
                height,
            } => Self::Cursor {
                x,
                y,
                width,
                height,
            },
            StrictPayload::Edge { returning } => Self::Edge { returning },
            StrictPayload::ReleaseAll {} => Self::ReleaseAll,
            StrictPayload::Heartbeat {} => Self::Heartbeat,
            StrictPayload::Stop {} => Self::Stop,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmEnvelope {
    #[schemars(range(min = 1, max = 1))]
    pub version: u8,
    /// Runtime requires<=128 UTF8 bytes and no Unicode control characters.
    /// Schema maxLength is a necessary codepoint bound, not a UTF8 byte limit.
    #[schemars(length(min = 1, max = "MAX_KM_SESSION_ID_BYTES"))]
    pub session_id: String,
    #[schemars(range(min = 1))]
    pub epoch: u64,
    #[schemars(range(min = 1))]
    pub seq: u64,
    pub origin: KmOrigin,
    pub payload: KmPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KmArrangementSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmViewerConsent {
    pub mac_to_android: bool,
    pub android_to_mac: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmAndroidCapabilities {
    pub cursor_injection: bool,
    /// Verified observe-and-pass, never merely the consuming public motion subscription.
    pub mouse_observation: bool,
    pub keyboard_capture: bool,
    /// Separately verified consuming physical mouse capture; independent from keyboard/observer.
    pub mouse_capture: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmStartInput {
    pub arrangement_side: KmArrangementSide,
    #[schemars(range(min = 1, max = "MAX_KM_DIMENSION"))]
    pub tablet_width: u32,
    #[schemars(range(min = 1, max = "MAX_KM_DIMENSION"))]
    pub tablet_height: u32,
    pub viewer_consent: KmViewerConsent,
    pub android_capabilities: KmAndroidCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmReadiness {
    pub mac_capture: bool,
    pub mac_injection: bool,
    pub android: KmAndroidCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmStatus {
    pub owner: KmOwner,
    pub armed: bool,
    pub epoch: u64,
    pub capabilities: KmReadiness,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KmStartResult {
    pub session_id: String,
    pub epoch: u64,
    pub port: u16,
    /// Base64url-no-pad encoded random 32-byte seed, only in sealed authenticated control.
    pub session_key: String,
    pub status: KmStatus,
}

impl std::fmt::Debug for KmStartResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KmStartResult")
            .field("session_id", &self.session_id)
            .field("epoch", &self.epoch)
            .field("port", &self.port)
            .field("session_key", &"<redacted>")
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KmWireError {
    #[error("KM plaintext exceeds 8192 bytes")]
    TooLarge,
    #[error("invalid KM JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid KM envelope: {0}")]
    Invalid(&'static str),
    #[error("KM session does not match authenticated peer")]
    SessionMismatch,
    #[error("KM epoch does not match current host ownership")]
    EpochMismatch,
    #[error("KM origin does not match authenticated peer")]
    OriginMismatch,
}

impl KmEnvelope {
    pub fn validate(&self) -> Result<(), KmWireError> {
        if self.version != 1 {
            return Err(KmWireError::Invalid("version must be 1"));
        }
        if self.session_id.is_empty()
            || self.session_id.len() > MAX_KM_SESSION_ID_BYTES
            || self.session_id.chars().any(char::is_control)
        {
            return Err(KmWireError::Invalid(
                "sessionId must be 1..128 UTF8 bytes without controls",
            ));
        }
        if self.epoch == 0 || self.seq == 0 {
            return Err(KmWireError::Invalid("epoch and seq must be nonzero"));
        }
        match self.payload {
            KmPayload::Move { dx, dy } | KmPayload::Wheel { dx, dy }
                if !dx.is_finite()
                    || !dy.is_finite()
                    || dx.abs() > MAX_KM_DELTA
                    || dy.abs() > MAX_KM_DELTA =>
            {
                return Err(KmWireError::Invalid(
                    "motion must be finite and within 65536",
                ));
            }
            KmPayload::Button { button, .. } if button > MAX_KM_BUTTON => {
                return Err(KmWireError::Invalid("button must be 0..7"))
            }
            KmPayload::Key { code, .. } if code > MAX_KM_KEY_CODE => {
                return Err(KmWireError::Invalid(
                    "key code must be USB HID usage 0..255",
                ))
            }
            KmPayload::Cursor {
                x,
                y,
                width,
                height,
            } => {
                validate_dimensions(width, height)?;
                if !x.is_finite()
                    || !y.is_finite()
                    || x < 0.0
                    || y < 0.0
                    || x > width as f64
                    || y > height as f64
                {
                    return Err(KmWireError::Invalid(
                        "cursor must be finite and inside its dimensions",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Trusted session context is provided by the authenticated native transport,
    /// never copied from the peer's envelope. State transitions advance epoch.
    pub fn validate_for(
        &self,
        session_id: &str,
        epoch: u64,
        origin: KmOrigin,
    ) -> Result<(), KmWireError> {
        self.validate()?;
        if self.session_id != session_id {
            return Err(KmWireError::SessionMismatch);
        }
        if self.epoch != epoch {
            return Err(KmWireError::EpochMismatch);
        }
        if self.origin != origin {
            return Err(KmWireError::OriginMismatch);
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, KmWireError> {
        if bytes.len() > MAX_KM_PLAINTEXT {
            return Err(KmWireError::TooLarge);
        }
        let message: Self = serde_json::from_slice(bytes)?;
        message.validate()?;
        Ok(message)
    }

    pub fn encode(&self) -> Result<Vec<u8>, KmWireError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_KM_PLAINTEXT {
            return Err(KmWireError::TooLarge);
        }
        Ok(bytes)
    }
}

impl KmStartInput {
    /// Geometry/schema validation; actual authority and readiness are verified
    /// independently by host/Android OS preflight before any capture or socket.
    pub fn validate(&self) -> Result<(), KmWireError> {
        validate_dimensions(self.tablet_width, self.tablet_height)
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), KmWireError> {
    if !(1..=MAX_KM_DIMENSION).contains(&width) || !(1..=MAX_KM_DIMENSION).contains(&height) {
        return Err(KmWireError::Invalid("dimensions must be 1..32768"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const HELLO: &[u8] = br#"{"version":1,"sessionId":"km-vector-v1","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}"#;
    fn hello() -> KmEnvelope {
        KmEnvelope::decode(HELLO).unwrap()
    }

    #[test]
    fn bounded_decoder_rejects_invalid_header_and_oversized_plaintext() {
        for bytes in [
            br#"{"version":2,"sessionId":"s","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}"#.as_slice(),
            br#"{"version":1,"sessionId":"","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}"#,
            br#"{"version":1,"sessionId":"s","epoch":0,"seq":1,"origin":"android","payload":{"kind":"hello"}}"#,
            br#"{"version":1,"sessionId":"s","epoch":1,"seq":0,"origin":"android","payload":{"kind":"hello"}}"#,
        ] { assert!(KmEnvelope::decode(bytes).is_err(), "{}", String::from_utf8_lossy(bytes)); }
        let mut oversized = HELLO.to_vec();
        oversized.resize(8193, b' ');
        assert!(matches!(
            KmEnvelope::decode(&oversized),
            Err(KmWireError::TooLarge)
        ));
    }

    #[test]
    fn trusted_session_epoch_and_origin_are_mandatory() {
        let message = hello();
        assert!(matches!(
            message.validate_for("other", 1, KmOrigin::Android),
            Err(KmWireError::SessionMismatch)
        ));
        assert!(matches!(
            message.validate_for("km-vector-v1", 2, KmOrigin::Android),
            Err(KmWireError::EpochMismatch)
        ));
        assert!(matches!(
            message.validate_for("km-vector-v1", 1, KmOrigin::Mac),
            Err(KmWireError::OriginMismatch)
        ));
        assert!(message
            .validate_for("km-vector-v1", 1, KmOrigin::Android)
            .is_ok());
    }

    #[test]
    fn sender_and_receiver_reject_out_of_range_input() {
        let invalid = [
            KmPayload::Move {
                dx: f64::NAN,
                dy: 0.0,
            },
            KmPayload::Move {
                dx: 65537.0,
                dy: 0.0,
            },
            KmPayload::Wheel {
                dx: 0.0,
                dy: f64::INFINITY,
            },
            KmPayload::Button {
                button: 8,
                down: true,
            },
            KmPayload::Key {
                code: 256,
                down: true,
            },
            KmPayload::Cursor {
                x: -1.0,
                y: 0.0,
                width: 100,
                height: 100,
            },
            KmPayload::Cursor {
                x: 101.0,
                y: 0.0,
                width: 100,
                height: 100,
            },
            KmPayload::Cursor {
                x: 0.0,
                y: 0.0,
                width: 0,
                height: 100,
            },
        ];
        for payload in invalid {
            let message = KmEnvelope { payload, ..hello() };
            assert!(message.encode().is_err(), "{message:?}");
        }
        let mut message = hello();
        message.session_id = "é".repeat(65);
        assert!(
            message.validate().is_err(),
            "session byte bound, not character count"
        );
    }

    #[test]
    fn parser_denies_unknown_fields_including_unit_payloads() {
        for payload in [
            r#"{"kind":"hello","extra":true}"#,
            r#"{"kind":"move","dx":1,"dy":2,"extra":0}"#,
            r#"{"kind":"state","owner":"local","extra":0}"#,
        ] {
            let json = format!(
                r#"{{"version":1,"sessionId":"s","epoch":1,"seq":1,"origin":"android","payload":{payload}}}"#
            );
            assert!(KmEnvelope::decode(json.as_bytes()).is_err());
        }
        let json = String::from_utf8(HELLO.to_vec())
            .unwrap()
            .replace("\"version\":1", "\"extra\":0,\"version\":1");
        assert!(KmEnvelope::decode(json.as_bytes()).is_err());
    }

    #[test]
    fn exact_native_wire_roundtrips_all_payloads() {
        assert_eq!(hello().encode().unwrap(), HELLO);
        let valid = [
            KmPayload::State {
                owner: KmOwner::MacToAndroid,
            },
            KmPayload::Move {
                dx: -10.0,
                dy: 20.0,
            },
            KmPayload::Button {
                button: 1,
                down: true,
            },
            KmPayload::Wheel { dx: 0.0, dy: -1.0 },
            KmPayload::Key {
                code: 4,
                down: false,
            },
            KmPayload::Cursor {
                x: 10.0,
                y: 20.0,
                width: 2560,
                height: 1600,
            },
            KmPayload::Edge { returning: true },
            KmPayload::ReleaseAll,
            KmPayload::Heartbeat,
            KmPayload::Stop,
        ];
        for payload in valid {
            let message = KmEnvelope { payload, ..hello() };
            assert_eq!(
                KmEnvelope::decode(&message.encode().unwrap()).unwrap(),
                message
            );
        }
    }

    // Evaluate only Draft7 scalar bound keywords used by these fixtures. This
    // focused parity check consumes real emitted schema fragments; it is not a
    // replacement for the native decoder or a general JSON Schema validator.
    fn scalar_bounds_accept(schema: &serde_json::Value, candidate: &serde_json::Value) -> bool {
        if let Some(number) = candidate.as_f64() {
            if schema["type"] == "integer" && number.fract() != 0.0 {
                return false;
            }
            schema["minimum"].as_f64().is_none_or(|min| number >= min)
                && schema["maximum"].as_f64().is_none_or(|max| number <= max)
        } else if let Some(string) = candidate.as_str() {
            let length = string.chars().count() as u64;
            schema["minLength"].as_u64().is_none_or(|min| length >= min)
                && schema["maxLength"].as_u64().is_none_or(|max| length <= max)
        } else {
            panic!("fixture must be a number or string");
        }
    }

    fn payload_property<'a>(
        schema: &'a serde_json::Value,
        kind: &str,
        field: &str,
    ) -> &'a serde_json::Value {
        &schema["definitions"]["KmPayload"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variant| variant["properties"]["kind"]["enum"][0] == kind)
            .unwrap()["properties"][field]
    }

    #[test]
    fn schema_scalar_bounds_match_envelope_header_decoder() {
        use serde_json::json;
        let schema = serde_json::to_value(schemars::schema_for!(KmEnvelope)).unwrap();
        for (field, candidate, accepted) in [
            ("version", json!(0), false),
            ("version", json!(1), true),
            ("version", json!(2), false),
            ("epoch", json!(0), false),
            ("epoch", json!(1), true),
            ("seq", json!(0), false),
            ("seq", json!(1), true),
            ("sessionId", json!(""), false),
            ("sessionId", json!("s"), true),
            ("sessionId", json!("x".repeat(128)), true),
            ("sessionId", json!("x".repeat(129)), false),
        ] {
            let mut message: serde_json::Value = serde_json::from_slice(HELLO).unwrap();
            message[field] = candidate.clone();
            assert_eq!(
                KmEnvelope::decode(&serde_json::to_vec(&message).unwrap()).is_ok(),
                accepted,
                "decoder {field}={candidate}"
            );
            assert_eq!(
                scalar_bounds_accept(&schema["properties"][field], &candidate),
                accepted,
                "schema {field}={candidate}"
            );
        }
    }

    #[test]
    fn schema_scalar_bounds_match_payload_decoder() {
        use serde_json::json;
        let schema = serde_json::to_value(schemars::schema_for!(KmEnvelope)).unwrap();
        for (kind, field, candidates) in [
            (
                "move",
                "dx",
                vec![
                    (-65537, false),
                    (-65536, true),
                    (65536, true),
                    (65537, false),
                ],
            ),
            (
                "move",
                "dy",
                vec![
                    (-65537, false),
                    (-65536, true),
                    (65536, true),
                    (65537, false),
                ],
            ),
            (
                "wheel",
                "dx",
                vec![
                    (-65537, false),
                    (-65536, true),
                    (65536, true),
                    (65537, false),
                ],
            ),
            (
                "wheel",
                "dy",
                vec![
                    (-65537, false),
                    (-65536, true),
                    (65536, true),
                    (65537, false),
                ],
            ),
            ("button", "button", vec![(0, true), (7, true), (8, false)]),
            (
                "key",
                "code",
                vec![(0, true), (4, true), (224, true), (255, true), (256, false)],
            ),
            (
                "cursor",
                "x",
                vec![(-1, false), (0, true), (32768, true), (32769, false)],
            ),
            (
                "cursor",
                "y",
                vec![(-1, false), (0, true), (32768, true), (32769, false)],
            ),
            (
                "cursor",
                "width",
                vec![(0, false), (1, true), (32768, true), (32769, false)],
            ),
            (
                "cursor",
                "height",
                vec![(0, false), (1, true), (32768, true), (32769, false)],
            ),
        ] {
            let payload = match kind {
                "move" | "wheel" => json!({"kind":kind,"dx":0,"dy":0}),
                "button" => json!({"kind":kind,"button":0,"down":true}),
                "key" => json!({"kind":kind,"code":4,"down":true}),
                "cursor" => json!({"kind":kind,"x":0,"y":0,"width":32768,"height":32768}),
                _ => unreachable!(),
            };
            for (candidate, accepted) in candidates {
                let mut message: serde_json::Value = serde_json::from_slice(HELLO).unwrap();
                message["payload"] = payload.clone();
                message["payload"][field] = json!(candidate);
                assert_eq!(
                    KmEnvelope::decode(&serde_json::to_vec(&message).unwrap()).is_ok(),
                    accepted,
                    "decoder {kind}.{field}={candidate}"
                );
                assert_eq!(
                    scalar_bounds_accept(payload_property(&schema, kind, field), &json!(candidate)),
                    accepted,
                    "schema {kind}.{field}={candidate}"
                );
            }
        }
    }

    #[test]
    fn schema_dimensions_match_start_validator() {
        use serde_json::json;
        let schema = serde_json::to_value(schemars::schema_for!(KmStartInput)).unwrap();
        let valid = KmStartInput {
            arrangement_side: KmArrangementSide::Right,
            tablet_width: 2560,
            tablet_height: 1600,
            viewer_consent: KmViewerConsent::default(),
            android_capabilities: KmAndroidCapabilities::default(),
        };
        for field in ["tabletWidth", "tabletHeight"] {
            for (candidate, accepted) in [(0, false), (1, true), (32768, true), (32769, false)] {
                let mut message = serde_json::to_value(&valid).unwrap();
                message[field] = json!(candidate);
                let input: KmStartInput = serde_json::from_value(message).unwrap();
                assert_eq!(
                    input.validate().is_ok(),
                    accepted,
                    "validator {field}={candidate}"
                );
                assert_eq!(
                    scalar_bounds_accept(&schema["properties"][field], &json!(candidate)),
                    accepted,
                    "schema {field}={candidate}"
                );
            }
        }
    }

    #[test]
    fn runtime_byte_control_and_relative_cursor_invariants_remain_authoritative() {
        let schema = serde_json::to_value(schemars::schema_for!(KmEnvelope)).unwrap();
        let mut message = hello();
        message.session_id = "é".repeat(65);
        assert!(scalar_bounds_accept(
            &schema["properties"]["sessionId"],
            &serde_json::json!(message.session_id)
        ));
        assert!(
            message.validate().is_err(),
            "65 codepoints exceed128 UTF8 bytes"
        );
        message = hello();
        message.session_id = "s\u{0085}".into();
        assert!(
            message.validate().is_err(),
            "runtime rejects Unicode control characters"
        );
        message = hello();
        message.payload = KmPayload::Cursor {
            x: 101.0,
            y: 0.0,
            width: 100,
            height: 100,
        };
        assert!(scalar_bounds_accept(
            payload_property(&schema, "cursor", "x"),
            &serde_json::json!(101)
        ));
        assert!(
            message.validate().is_err(),
            "cursor must also be<=its actual width"
        );
    }

    #[test]
    fn mouse_capture_capability_is_explicit_and_independent() {
        let json = r#"{"arrangementSide":"right","tabletWidth":2560,"tabletHeight":1600,"viewerConsent":{"macToAndroid":true,"androidToMac":true},"androidCapabilities":{"cursorInjection":true,"mouseObservation":true,"keyboardCapture":true,"mouseCapture":false}}"#;
        let input: KmStartInput = serde_json::from_str(json)
            .expect("explicit mouseCapture field is required in shared schema");
        let wire = serde_json::to_value(input).unwrap();
        assert_eq!(wire["androidCapabilities"]["mouseCapture"], false);
        assert_eq!(wire["androidCapabilities"]["keyboardCapture"], true);
        assert_eq!(wire["androidCapabilities"]["mouseObservation"], true);
    }

    #[test]
    fn start_contract_is_camel_case_and_rejects_invalid_geometry() {
        let json = r#"{"arrangementSide":"right","tabletWidth":2560,"tabletHeight":1600,"viewerConsent":{"macToAndroid":true,"androidToMac":true},"androidCapabilities":{"cursorInjection":true,"mouseObservation":true,"keyboardCapture":true,"mouseCapture":true}}"#;
        let mut input: KmStartInput = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&input).unwrap(), json);
        assert!(input.validate().is_ok());
        input.tablet_width = 0;
        assert!(input.validate().is_err());
        input.tablet_width = 32769;
        assert!(input.validate().is_err());
    }
}
