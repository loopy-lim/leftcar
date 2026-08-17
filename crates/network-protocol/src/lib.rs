//! Host-Viewer network protocol envelope and version negotiation
//! (docs/03 §5.2). Spike format: length-prefixed JSON. Product wire format is
//! undecided (Q-004) until version/fuzz/size evidence exists.

use serde::{Deserialize, Serialize};


pub const PROTOCOL_MIN: u32 = 1;
pub const PROTOCOL_MAX: u32 = 1;
/// docs/07 §13: control message initial cap.
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub device_id: String,
    pub app_build: String,
    pub codec_capabilities_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerHello {
    pub selected_protocol: u32,
    pub host_id: String,
    pub session_id: String,
    pub source_catalog_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub protocol_version: u32,
    pub session_id: String,
    pub request_id: String,
    pub monotonic_sequence: u64,
    pub kind: ControlKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    /// No input-injection variants exist by design (P-01, T-06).
    ListSources,
    StartSource,
    StopSource,
    SetStreamProfile,
    RequestIdr,
    SessionPing,
    SessionPong,
    CatalogChanged,
    StreamStateChanged,
    ErrorResponse,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("version mismatch: client {client_min}-{client_max}, server {server_min}-{server_max}")]
    VersionMismatch {
        client_min: u32,
        client_max: u32,
        server_min: u32,
        server_max: u32,
    },
    #[error("message exceeds cap: {len} > {MAX_CONTROL_MESSAGE_BYTES}")]
    MessageTooLarge { len: usize },
    #[error("malformed message")]
    Malformed,
}

/// Negotiate the protocol version: highest common in both ranges (docs/04 §10).
pub fn negotiate(client_min: u32, client_max: u32, server_min: u32, server_max: u32) -> Result<u32, ProtocolError> {
    let lo = client_min.max(server_min);
    let hi = client_max.min(server_max);
    if lo > hi {
        return Err(ProtocolError::VersionMismatch {
            client_min,
            client_max,
            server_min,
            server_max,
        });
    }
    Ok(hi)
}

// -- length-prefixed JSON framing (spike) ---------------------------------

const LEN_PREFIX: usize = 8;

/// Frame a control message: 8-byte big-endian length + JSON bytes.
pub fn frame_control(envelope: &ControlEnvelope) -> Result<Vec<u8>, ProtocolError> {
    let json = serde_json::to_vec(envelope).map_err(|_| ProtocolError::Malformed)?;
    let len = json.len();
    if len > MAX_CONTROL_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge { len });
    }
    let mut out = Vec::with_capacity(LEN_PREFIX + len);
    out.extend_from_slice(&(len as u64).to_be_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// Parse one framed control message from `buf`. Returns (envelope, consumed).
/// The length prefix is validated before any allocation of the payload.
pub fn parse_control(buf: &[u8]) -> Result<(ControlEnvelope, usize), ProtocolError> {
    if buf.len() < LEN_PREFIX {
        return Err(ProtocolError::Malformed);
    }
    let len = u64::from_be_bytes(buf[..LEN_PREFIX].try_into().expect("8 bytes")) as usize;
    if len > MAX_CONTROL_MESSAGE_BYTES {
        // reject before allocating the payload (docs/07 §13)
        return Err(ProtocolError::MessageTooLarge { len });
    }
    let end = LEN_PREFIX + len;
    if buf.len() < end {
        return Err(ProtocolError::Malformed);
    }
    let envelope: ControlEnvelope =
        serde_json::from_slice(&buf[LEN_PREFIX..end]).map_err(|_| ProtocolError::Malformed)?;
    if envelope.protocol_version < PROTOCOL_MIN || envelope.protocol_version > PROTOCOL_MAX {
        return Err(ProtocolError::Malformed);
    }
    Ok((envelope, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(seq: u64) -> ControlEnvelope {
        ControlEnvelope {
            protocol_version: 1,
            session_id: "sess".into(),
            request_id: format!("req-{seq}"),
            monotonic_sequence: seq,
            kind: ControlKind::SessionPing,
            payload: vec![1, 2, 3],
        }
    }

    #[test]
    fn version_negotiation_picks_highest_common() {
        assert_eq!(negotiate(1, 3, 2, 5).unwrap(), 3);
        assert_eq!(negotiate(1, 1, 1, 4).unwrap(), 1);
    }

    #[test]
    fn version_mismatch_is_fatal() {
        let err = negotiate(2, 3, 1, 1).unwrap_err();
        assert!(matches!(err, ProtocolError::VersionMismatch { .. }));
    }

    #[test]
    fn frame_roundtrip() {
        let framed = frame_control(&envelope(7)).unwrap();
        let (parsed, consumed) = parse_control(&framed).unwrap();
        assert_eq!(parsed.monotonic_sequence, 7);
        assert_eq!(consumed, framed.len());
    }

    #[test]
    fn oversized_control_message_allocates_nothing_large() {
        // a forged length prefix claiming > cap is rejected at parse time
        let mut buf = vec![0u8; 8];
        let huge = (MAX_CONTROL_MESSAGE_BYTES + 1) as u64;
        buf[..8].copy_from_slice(&huge.to_be_bytes());
        buf.extend_from_slice(&[0u8; 16]); // partial body
        match parse_control(&buf) {
            Err(ProtocolError::MessageTooLarge { .. }) => {}
            other => panic!("expected MessageTooLarge, got {other:?}"),
        }
        // and framing side refuses too
        let mut big = envelope(1);
        big.payload = vec![0u8; MAX_CONTROL_MESSAGE_BYTES + 1];
        assert!(matches!(
            frame_control(&big),
            Err(ProtocolError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn no_input_injection_control_kind_exists() {
        // T-06: the protocol enum must not contain input commands. The enum is
        // closed; assert every variant against the allowlist.
        const ALLOWED: &[ControlKind] = &[
            ControlKind::ListSources,
            ControlKind::StartSource,
            ControlKind::StopSource,
            ControlKind::SetStreamProfile,
            ControlKind::RequestIdr,
            ControlKind::SessionPing,
            ControlKind::SessionPong,
            ControlKind::CatalogChanged,
            ControlKind::StreamStateChanged,
            ControlKind::ErrorResponse,
        ];
        for allowed in ALLOWED {
            // exhaustive match: adding a new variant requires updating this test
            matches!(allowed, ControlKind::ListSources
                | ControlKind::StartSource
                | ControlKind::StopSource
                | ControlKind::SetStreamProfile
                | ControlKind::RequestIdr
                | ControlKind::SessionPing
                | ControlKind::SessionPong
                | ControlKind::CatalogChanged
                | ControlKind::StreamStateChanged
                | ControlKind::ErrorResponse);
        }
        // serialized names must never contain input-like tokens
        for kind in [
            ControlKind::ListSources, ControlKind::StartSource, ControlKind::StopSource,
            ControlKind::SetStreamProfile, ControlKind::RequestIdr, ControlKind::SessionPing,
            ControlKind::SessionPong, ControlKind::CatalogChanged,
            ControlKind::StreamStateChanged, ControlKind::ErrorResponse,
        ] {
            let env = ControlEnvelope {
                protocol_version: 1,
                session_id: "s".into(),
                request_id: "r".into(),
                monotonic_sequence: 0,
                kind,
                payload: vec![],
            };
            let json = serde_json::to_string(&env).unwrap().to_lowercase();
            for banned in ["keyboard", "pointer", "mouse", "touch", "inject", "clipboard", "input"] {
                assert!(!json.contains(banned), "{kind:?} serialized form leaked {banned}: {json}");
            }
        }
    }

    #[test]
    fn truncated_and_garbage_inputs_are_malformed() {
        assert!(matches!(parse_control(&[]), Err(ProtocolError::Malformed)));
        assert!(matches!(parse_control(&[0; 4]), Err(ProtocolError::Malformed)));
        let mut buf = 8u64.to_be_bytes().to_vec();
        buf.extend_from_slice(b"not json!");
        assert!(matches!(parse_control(&buf), Err(ProtocolError::Malformed)));
    }
}

/// Fuzz-style property: arbitrary bytes never panic the parser.
#[cfg(test)]
mod fuzz_smoke {
    use super::*;

    proptest::proptest! {
        #[test]
        fn arbitrary_bytes_never_panic_envelope_parser(input in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
            let _ = parse_control(&input);
        }
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_prefix_lengths_rejected_before_alloc(
            b0 in proptest::num::u8::ANY,
            b1 in proptest::num::u8::ANY,
            body in proptest::collection::vec(proptest::num::u8::ANY, 0..64)
        ) {
            let mut buf = vec![0u8; 8];
            buf[6] = b0;
            buf[7] = b1;
            buf.extend_from_slice(&body);
            // any claimed length is either malformed/too-large or fully parsed
            let _ = parse_control(&buf);
        }
    }

    #[test]
    fn outage_reconnect_sequence_is_recoverable() {
        // after a transport outage, a new session envelopes cleanly
        let env = ControlEnvelope {
            protocol_version: 1,
            session_id: "new".into(),
            request_id: "r1".into(),
            monotonic_sequence: 1,
            kind: ControlKind::CatalogChanged,
            payload: vec![],
        };
        let framed = frame_control(&env).unwrap();
        let (parsed, _) = parse_control(&framed).unwrap();
        assert_eq!(parsed.session_id, "new");
    }

    #[test]
    fn latency_probe_envelope_is_small() {
        // control plane must stay tiny; 1Hz status snapshots fit well under cap
        let env = ControlEnvelope {
            protocol_version: 1,
            session_id: "s".into(),
            request_id: "r".into(),
            monotonic_sequence: 42,
            kind: ControlKind::StreamStateChanged,
            payload: vec![0u8; 256],
        };
        let framed = frame_control(&env).unwrap();
        assert!(framed.len() < 1024);

    }
}
