//! 호스트 페어링 코어: QR 페이로드 파싱·엔드포인트 해석·자격증명 키 스키마·
//! 오류 분류기(TS apps/viewer-expo/src/pairing.ts). SecureStore·난수·시계·기기
//! 이름은 주입 포트([`ports`])다 — Rust 쪽에서 IO를 소유하지 않는다.
//!
//! 흐름은 [`credential`](토큰 저장·마이그레이션)과 [`flows`](pair 요청)에 있다.

pub mod credential;
pub mod flows;
pub mod ports;

pub use credential::{
    clear_stored_credential, clear_token, get_device_id, get_stored_credential,
    get_stored_pinned_host_key, get_stored_token, remember_pinned_host_key, PairingAttempt,
    PairingError, StoredCredentialView, TokenRollback,
};
pub use flows::{
    pair_with_host, pair_with_host_approval, pair_with_host_by_code, PairingApprovalResult,
    PairingFlowOptions, PAIRING_APPROVAL_TIMEOUT_MS,
};
pub use ports::{
    CancelToken, Clock, CorePorts, CredentialStore, HostKeyPins, MemoryCredentialStore, NoUsb,
    RecentHostEntry, RecentHostsPort, StoreError, SystemClock, UsbPort,
};

/// Host control-plane port when an endpoint names no explicit port.
/// TS defaults.ts — 의존성 없는 잎 모듈 규칙에 따라 여기서도 상수로 둔다.
pub const DEFAULT_CONTROL_PORT: u16 = 7777;

pub const DEVICE_ID_KEY: &str = "leftcar.deviceId";
/// 구버전 전역 토큰 키 — 모든 호스트가 하나의 토큰을 공유하던 v1. 폐기 대상.
pub const LEGACY_TOKEN_KEY: &str = "leftcar.token";
/// 최근 호스트 엔트리(JSON 배열) 저장 키 — 스키마 파서는
/// [`parse_recent_hosts_strict`], 저장 IO는 [`ports::RecentHostsPort`].
pub const RECENT_HOSTS_KEY: &str = "leftcar.recent_hosts";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QrPayload {
    pub id: String,
    pub secret: String,
    /// 호스트 Ed25519 공개키(b64url 32B) — 핸드셰이크 핀의 원천.
    pub host_key: String,
    pub host: String,
    pub port: u16,
    /// 호스트의 테일넷 별칭(100.64/10). QR 생성 시점에 테일넷 경로가 있으면
    /// 호스트가 싣는다. 구버전 호스트의 QR에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredCredential {
    pub token: String,
    pub target: HostEndpoint,
    /// Present only after the network handshake verified this identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key: Option<String>,
}

/// `^offer-[0-9a-f]{8}-...` v4 UUID 형태(대소문자 무시). regex 의존성 없이
/// 고정 형태를 직접 검증한다.
fn is_offer_id(id: &str) -> bool {
    let rest = match id.strip_prefix("offer-") {
        Some(rest) if rest.len() == 36 => rest,
        _ => return false,
    };
    let hex =
        |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c);
    let bytes = rest.as_bytes();
    let dashes = [8, 13, 18, 23];
    for &d in &dashes {
        if bytes[d] != b'-' {
            return false;
        }
    }
    for (i, &b) in bytes.iter().enumerate() {
        if dashes.contains(&i) {
            continue;
        }
        if !hex(b) {
            return false;
        }
    }
    // v4: 버전 니블 4, 변형 니블 8/9/a/b
    bytes[14] == b'4' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
}

/// `^[A-Za-z0-9_-]{43}$` — 오퍼 시크릿과 호스트 공개키(b64url 32B = 43문자)
/// 공용 형태.
fn is_b64url_43(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// 호스트 Ed25519 공개키(b64url 32B = 43문자) 형태 검사.
pub fn is_host_key_shape(value: &str) -> bool {
    is_b64url_43(value)
}

/// 발급 토큰은 64자리 16진수다.
pub fn is_valid_issued_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Remote control uses authenticated encryption and paired Host identity.
/// Supported routes are loopback, private LAN, link-local, and Tailscale;
/// public Internet endpoints need a separate NAT/reachability design.
pub fn is_trusted_host(host: &str) -> bool {
    let mut normalized = host.trim().to_lowercase();
    // replace(/\.$/, "") — 끝의 점 하나만 뗀다.
    if normalized.ends_with('.') {
        normalized.pop();
    }
    if normalized.is_empty() || normalized.len() > 253 {
        return false;
    }
    if normalized == "localhost" || normalized.ends_with(".local") {
        return true;
    }
    if normalized.ends_with(".ts.net") {
        return true;
    }
    let octets: Vec<&str> = normalized.split('.').collect();
    if octets.len() != 4 {
        return false;
    }
    let mut values = [0u32; 4];
    for (index, part) in octets.iter().enumerate() {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let value: u32 = part.parse().unwrap_or(u32::MAX);
        if value > 255 {
            return false;
        }
        values[index] = value;
    }
    let a = values[0];
    // Valid unicast IPv4 (excludes 0.0.0.0/8, multicast 224.0.0.0/4, reserved
    // 240.0.0.0/4, broadcast). Covers private LAN, loopback, link-local,
    // Carrier-Grade NAT / Tailscale (100.64.0.0/10), and public WAN endpoints.
    a > 0 && a < 224
}

/// Parse an explicitly selected host endpoint without inventing a localhost
/// fallback.
pub fn parse_host_endpoint(endpoint: &str) -> Option<HostEndpoint> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Some(separator) = trimmed.rfind(':') else {
        return is_trusted_host(trimmed).then(|| HostEndpoint {
            host: trimmed.to_string(),
            port: DEFAULT_CONTROL_PORT,
        });
    };
    let host = trimmed[..separator].trim();
    let raw_port = trimmed[separator + 1..].trim();
    if !raw_port.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let port: u32 = raw_port.parse().ok()?;
    if !is_trusted_host(host) || !(1..=65535).contains(&port) {
        return None;
    }
    Some(HostEndpoint {
        host: host.to_string(),
        port: port as u16,
    })
}

pub fn format_host_endpoint(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Prefer the endpoint explicitly selected for this pairing attempt.
pub fn resolve_pairing_host(route_endpoint: Option<&str>, current_host: &str) -> String {
    route_endpoint
        .and_then(|r| {
            let trimmed = r.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| current_host.to_string())
}

/// Keep this predicate shared with the UI so the enabled state cannot drift
/// from the direct Host endpoint + six-digit pairing contract.
pub fn can_submit_pairing_code(code: &str, has_host_target: bool, busy: bool) -> bool {
    let normalized = normalize_pairing_code(code);
    has_host_target
        && !busy
        && normalized.len() == 6
        && normalized.bytes().all(|c| c.is_ascii_digit())
}

/// 붙여넣은 코드의 공백을 제거한다(TS code.trim().replace(/\s+/g, "")).
pub(crate) fn normalize_pairing_code(code: &str) -> String {
    code.trim().chars().filter(|c| !c.is_whitespace()).collect()
}

/// `{"v":2,"id":..,"s":..,"k":..,"h":..,"p":..[,"ts":..]}` → QrPayload; 어떤
/// 불일치든 None. v2부터 `k`(호스트 공개키)가 필수다 — 핀 없는 연결은 중간자를
/// 감지할 수 없으므로 QR이 핀의 원천이다. `ts`는 선택적 테일넷 별칭이며, 있으면
/// 신뢰 가능한 호스트 문자열이어야 한다. 여분 필드(`c` 등)는 무시한다.
pub fn parse_qr_payload(text: &str) -> Option<QrPayload> {
    if text.is_empty() {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let string_field = |name: &str| -> Option<&str> { raw.get(name).and_then(|v| v.as_str()) };
    let id = string_field("id")?;
    let secret = string_field("s")?;
    let host_key = string_field("k")?;
    let host = string_field("h")?;
    let port_value = raw.get("p")?;
    // TS typeof raw.p === "number" && Number.isInteger(raw.p)
    let port = port_value.as_i64()?;
    if port_value.as_f64() != Some(port as f64) {
        return None;
    }
    if raw.get("v").and_then(|v| v.as_i64()) != Some(2)
        || !is_offer_id(id)
        || !is_b64url_43(secret)
        || !is_b64url_43(host_key)
        || !is_trusted_host(host)
        || port <= 0
        || port >= 65536
    {
        return None;
    }
    let ts = match raw.get("ts") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => {
            let ts = value.as_str()?;
            if !is_trusted_host(ts) {
                return None;
            }
            Some(ts.to_string())
        }
    };
    Some(QrPayload {
        id: id.to_string(),
        secret: secret.to_string(),
        host_key: host_key.to_string(),
        host: host.to_string(),
        port: port as u16,
        ts,
    })
}

/// 토큰은 발급한 호스트 엔드포인트별로 저장된다 — 한 호스트의 401·토큰 삭제가
/// 다른 페어링된 호스트를 잠그지 않는다. SecureStore 키는 영숫자와 `.`, `-`,
/// `_`만 허용하므로 그 외 문자(IPv6 등)는 치환한다. 포트는 항상 정수라 끝에
/// 붙이면 모호하지 않다.
pub fn token_key(host: &str, port: u16) -> String {
    let sanitized_host: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("leftcar.token.v2.{sanitized_host}.{port}")
}

/// 검증된 호스트 정체 아래의 고정 토큰 키(v3).
pub fn identity_token_key(host_key: &str) -> String {
    format!("leftcar.token.v3.{host_key}")
}

/// 구버전 호스트는 코드 없는 pair을 "pairing failed"로 거절한다 — PIN 폴백 신호.
pub fn is_pairing_unsupported_error(message: &str) -> bool {
    // /pairing failed/i
    message.to_lowercase().contains("pairing failed")
}

pub fn is_pairing_rejected_error(message: &str) -> bool {
    // /pairing rejected/i
    message.to_lowercase().contains("pairing rejected")
}

/// TS recent-hosts.ts의 strict 파서 — 정체 읽기가 망가진 저장 핀을 첫 사용 신뢰로
/// 바꾸지 못게 한다. 망가진 데이터는 오류로 실패 닫힘(fail closed)한다.
pub fn parse_recent_hosts_strict(raw: Option<&str>) -> Result<Vec<RecentHostEntry>, StoreError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let invalid = || StoreError::new("Invalid saved host identities");
    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|_| invalid())?;
    let Some(items) = parsed.as_array() else {
        return Err(invalid());
    };
    const MAX_RECENT_HOSTS: usize = 5;
    const MAX_HOST_ALIASES: usize = 4;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let host = item
            .get("host")
            .and_then(|v| v.as_str())
            .filter(|h| !h.trim().is_empty())
            .ok_or_else(invalid)?;
        let port_value = item.get("port").ok_or_else(invalid)?;
        let port = port_value.as_i64().ok_or_else(invalid)?;
        if port_value.as_f64() != Some(port as f64) || !(1..=65535).contains(&port) {
            return Err(invalid());
        }
        let last_connected = item.get("lastConnected").ok_or_else(invalid)?;
        let last = last_connected.as_f64().ok_or_else(invalid)?;
        if !last.is_finite() {
            return Err(invalid());
        }
        let host_key = match item.get("hostKey") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => {
                let key = value.as_str().ok_or_else(invalid)?;
                if !is_b64url_43(key) {
                    return Err(StoreError::new("Invalid saved host identity key"));
                }
                Some(key.to_string())
            }
        };
        let aliases = match item.get("aliases") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(value) => {
                let list = value.as_array().ok_or_else(invalid)?;
                if list.len() > MAX_HOST_ALIASES {
                    return Err(invalid());
                }
                let mut aliases = Vec::with_capacity(list.len());
                for entry in list {
                    let alias = entry.as_str().ok_or_else(invalid)?;
                    let trimmed = alias.trim();
                    // 포트는 별도 필드라 호스트 문자열에 콜론이 오면 저장 오류다.
                    if trimmed.is_empty()
                        || trimmed.len() > 253
                        || alias.contains(' ')
                        || alias.contains(':')
                    {
                        return Err(invalid());
                    }
                    aliases.push(alias.to_string());
                }
                aliases
            }
        };
        out.push(RecentHostEntry {
            host: host.to_string(),
            port: port as u16,
            name: item
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            host_key,
            aliases,
        });
    }
    out.truncate(MAX_RECENT_HOSTS);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER_ID: &str = "offer-123e4567-e89b-42d3-a456-426614174000";
    const OFFER_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 43
    const HOST_KEY: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"; // 43

    fn valid_qr() -> String {
        format!(
            "{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"192.168.1.5\",\"p\":7777}}"
        )
    }

    #[test]
    fn offer_id_shape_matches_ts_pattern() {
        assert!(is_offer_id(OFFER_ID));
        assert!(!is_offer_id("offer-123e4567-e89b-12d3-a456-426614174000")); // v1 UUID
        assert!(!is_offer_id("offer-123e4567-e89b-42d3-c456-426614174000")); // 변형 니블
        assert!(!is_offer_id("offer-short"));
    }

    #[test]
    fn parse_qr_payload_accepts_valid_payload_and_tailnet_alias() {
        let parsed = parse_qr_payload(&valid_qr()).expect("valid");
        assert_eq!(parsed.id, OFFER_ID);
        assert_eq!(parsed.host, "192.168.1.5");
        assert_eq!(parsed.port, 7777);
        assert_eq!(parsed.host_key, HOST_KEY);
        assert_eq!(parsed.ts, None);

        let with_alias = format!(
            "{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"192.168.1.5\",\"p\":7777,\"ts\":\"100.80.133.120\"}}"
        );
        assert_eq!(
            parse_qr_payload(&with_alias).expect("alias").ts,
            Some("100.80.133.120".to_string())
        );
        let ts_net_alias = with_alias.replace("100.80.133.120", "mac.example.ts.net");
        assert_eq!(
            parse_qr_payload(&ts_net_alias).expect("ts.net").ts,
            Some("mac.example.ts.net".to_string())
        );
        let bad_alias = with_alias.replace("100.80.133.120", "not a host");
        assert_eq!(parse_qr_payload(&bad_alias), None);
    }

    #[test]
    fn never_trusts_a_code_embedded_in_a_qr_payload() {
        let qr_with_code = format!(
            "{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"192.168.1.5\",\"p\":7777,\"c\":\"123456\"}}"
        );
        let parsed = parse_qr_payload(&qr_with_code).expect("extra fields ignored");
        assert_eq!(parsed.host, "192.168.1.5");
        assert_eq!(parsed.ts, None);
    }

    #[test]
    fn parse_qr_payload_rejects_wrong_version_and_missing_fields() {
        let bad = [
            "{\"v\":1,\"id\":\"o\",\"s\":\"s\",\"h\":\"1.2.3.4\",\"p\":7777}",          // v1
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"h\":\"1.2.3.4\",\"p\":7777}}"), // missing k
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"h\":\"1.2.3.4\",\"p\":7777}}"), // missing s
            "{\"v\":2,\"id\":\"o\",\"s\":\"s\",\"k\":\"k\",\"p\":7777}",               // missing h
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"1.2.3.4\"}}"), // missing p
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"1.2.3.4\",\"p\":\"7777\"}}"), // port not number
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"1.2.3.4\",\"p\":7777.5}}"), // non-integer
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"1.2.3.4\",\"p\":0}}"),       // low
            &format!("{{\"v\":2,\"id\":\"{OFFER_ID}\",\"s\":\"{OFFER_SECRET}\",\"k\":\"{HOST_KEY}\",\"h\":\"1.2.3.4\",\"p\":70000}}"),   // high
            "not json",
            "",
        ];
        for text in bad {
            assert_eq!(parse_qr_payload(text), None, "rejected: {text}");
        }
    }

    #[test]
    fn host_endpoint_parsing_prefers_explicit_selection() {
        assert_eq!(
            resolve_pairing_host(Some("192.168.0.134:7777"), "192.168.0.134:64098"),
            "192.168.0.134:7777"
        );
        assert_eq!(
            parse_host_endpoint(&format_host_endpoint("192.168.0.134", 7777)),
            Some(HostEndpoint {
                host: "192.168.0.134".into(),
                port: 7777
            })
        );
        assert_eq!(
            parse_host_endpoint("192.168.0.134"),
            Some(HostEndpoint {
                host: "192.168.0.134".into(),
                port: 7777
            })
        );
        assert_eq!(
            parse_host_endpoint("localhost:7777"),
            Some(HostEndpoint {
                host: "localhost".into(),
                port: 7777
            })
        );
        // never invents localhost
        for bad in [
            "",
            "   ",
            ":7777",
            "192.168.0.134:not-a-port",
            "192.168.0.134:0",
        ] {
            assert_eq!(parse_host_endpoint(bad), None, "{bad}");
        }
    }

    #[test]
    fn trusted_host_accepts_private_tailscale_and_public_unicast() {
        assert!(is_trusted_host("10.0.0.5"));
        assert!(is_trusted_host("100.100.20.30"));
        assert!(is_trusted_host("my-mac.example.ts.net"));
        assert!(is_trusted_host("8.8.8.8"));
        assert!(is_trusted_host("1.217.35.59"));
        assert!(is_trusted_host("localhost"));
        assert!(is_trusted_host("my-mac.local"));
        assert!(!is_trusted_host("example.com"));
        assert!(!is_trusted_host("0.0.0.0"));
        assert!(!is_trusted_host("224.0.0.1"));
        assert!(!is_trusted_host("255.255.255.255"));
        assert!(!is_trusted_host(""));
        assert!(!is_trusted_host("a".repeat(254).as_str()));
    }

    #[test]
    fn token_keys_match_the_ts_schemas() {
        assert_eq!(
            token_key("192.168.1.5", 7777),
            "leftcar.token.v2.192.168.1.5.7777"
        );
        // SecureStore 허용 문자 밖은 치환한다(IPv6 등).
        assert_eq!(token_key("::1", 6200), "leftcar.token.v2.__1.6200");
        assert_eq!(
            identity_token_key(HOST_KEY),
            format!("leftcar.token.v3.{HOST_KEY}")
        );
        assert!(is_valid_issued_token(&"a".repeat(64)));
        assert!(!is_valid_issued_token("not-a-valid-token"));
        assert!(!is_valid_issued_token(&"A".repeat(64)));
    }

    #[test]
    fn pairing_code_readiness_and_classifiers() {
        assert!(!can_submit_pairing_code("12345", true, false));
        assert!(!can_submit_pairing_code("123456", false, false));
        assert!(!can_submit_pairing_code("123456", true, true));
        assert!(can_submit_pairing_code("123456", true, false));
        // 붙여넣은 공백은 정규화한다.
        assert!(can_submit_pairing_code("123 456", true, false));

        assert!(is_pairing_unsupported_error("pairing failed"));
        assert!(!is_pairing_unsupported_error("pairing rejected"));
        assert!(is_pairing_rejected_error("Pairing Rejected"));
        assert!(!is_pairing_rejected_error("pairing failed"));
    }

    #[test]
    fn strict_recent_hosts_fail_closed_on_corrupt_identities() {
        let valid = r#"[{"host":"10.0.0.5","port":7777,"hostKey":"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB","lastConnected":1}]"#;
        let entries = parse_recent_hosts_strict(Some(valid)).expect("valid");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host_key.as_deref(), Some(HOST_KEY));

        for raw in [
            "broken",
            "{}",
            r#"[{"host":"10.0.0.5","port":7777,"hostKey":3,"lastConnected":1}]"#,
        ] {
            assert!(parse_recent_hosts_strict(Some(raw)).is_err(), "{raw}");
        }
        assert_eq!(parse_recent_hosts_strict(None).expect("empty"), Vec::new());
    }
}
