//! 뷰어 측 세션 암호 절차 — crates/secure-channel의 호스트 절차 위에 클라이언트
//! 절차(ClientHello 생성·ServerHello 파싱·client_finish 래핑)와 전송 줄
//! 프레이밍(`{"e":"<b64url(counter‖ct‖tag)>"}"`)을 얹는다.
//!
//! TS 원본: apps/viewer-expo/src/secure-channel.ts. 상호 운용은 고정 벡터로
//! 잠긴다(키·프레임 값은 cargo test -p secure-channel print_vector 출력과
//! 정확히 일치해야 한다).

use secure_channel::{ClientHello, ServerHello, SessionKeys};
use x25519_dalek::{PublicKey as XPublicKey, StaticSecret as XStaticSecret};
use zeroize::Zeroize;

use crate::b64;

pub use secure_channel::{
    client_finish as raw_client_finish, derive_keys, StreamSealer, COUNTER_LEN, HANDSHAKE_INFO,
    MAX_PLAINTEXT, SIG_CONTEXT, TAG_LEN,
};

/// 결정적 랜덤 주입 지점. 운영 경로는 [`OsRandom`](OS CSPRNG), 테스트·임베딩은
/// 자체 구현을 넣는다(TS setRandomSource 대응).
pub trait RandomSource: Send + Sync {
    fn fill(&self, buf: &mut [u8]);
}

/// OS CSPRNG(호스트 측과 동일한 원천).
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill(&self, buf: &mut [u8]) {
        secure_channel::random_bytes(buf);
    }
}

/// ClientHello와 그 비밀. 비밀은 drop에서 0화된다(x25519-dalek StaticSecret).
pub struct ClientEphemeral {
    secret: XStaticSecret,
    /// 평문으로 전송되는 공개 값.
    pub hello: ClientHello,
}

/// 임시 X25519 쌍과 논스를 뽑는다(TS createClientHello).
pub fn create_client_hello(random: &dyn RandomSource) -> ClientEphemeral {
    let mut secret_bytes = [0u8; 32];
    random.fill(&mut secret_bytes);
    let secret = XStaticSecret::from(secret_bytes);
    secret_bytes.zeroize();
    let mut nc = [0u8; 32];
    random.fill(&mut nc);
    let hello = ClientHello {
        nc,
        xk: XPublicKey::from(&secret).to_bytes(),
    };
    ClientEphemeral { secret, hello }
}

/// ClientHello JSON 줄. 평문으로 전송되며 TS `encodeClientHello`와 같은 필드
/// 순서(`v, hello, nc, xk`)와 바이트를 유지한다.
pub fn encode_client_hello(hello: &ClientHello) -> String {
    format!(
        "{{\"v\":1,\"hello\":\"c\",\"nc\":\"{}\",\"xk\":\"{}\"}}",
        b64::encode(&hello.nc),
        b64::encode(&hello.xk)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ServerHelloParseError {
    #[error("not a ServerHello")]
    NotServerHello,
    #[error("malformed ServerHello")]
    Malformed,
}

/// ServerHello 줄 파싱(TS parseServerHello). `hello != "s"`면
/// [`ServerHelloParseError::NotServerHello`], 길이·디코드 실패면
/// [`ServerHelloParseError::Malformed`]다.
pub fn parse_server_hello(line: &str) -> Result<ServerHello, ServerHelloParseError> {
    let parsed: serde_json::Value =
        serde_json::from_str(line).map_err(|_| ServerHelloParseError::Malformed)?;
    if parsed.get("hello").and_then(|v| v.as_str()) != Some("s") {
        return Err(ServerHelloParseError::NotServerHello);
    }
    let field = |name: &str, len: usize| -> Result<Vec<u8>, ServerHelloParseError> {
        let text = parsed
            .get(name)
            .and_then(|v| v.as_str())
            .ok_or(ServerHelloParseError::Malformed)?;
        let bytes = b64::decode(text).ok_or(ServerHelloParseError::Malformed)?;
        if bytes.len() != len {
            return Err(ServerHelloParseError::Malformed);
        }
        Ok(bytes)
    };
    let into32 = |bytes: Vec<u8>| -> [u8; 32] { bytes.try_into().expect("32B checked above") };
    let ns = into32(field("ns", 32)?);
    let xk = into32(field("xk", 32)?);
    let spk = into32(field("spk", 32)?);
    let sig: [u8; 64] = field("sig", 64)?.try_into().expect("64B checked above");
    Ok(ServerHello { ns, xk, spk, sig })
}

/// client_finish 결과: 검증된 호스트 공개키(TOFU 핀의 원천)와 방향별 키.
pub struct HandshakeOutcome {
    pub keys: SessionKeys,
    pub spk: [u8; 32],
}

impl std::fmt::Debug for HandshakeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 키 바이트는 절대 로그에 흘러가지 않는다.
        f.debug_struct("HandshakeOutcome")
            .field("spk", &to_hex(&self.spk))
            .finish()
    }
}

/// ServerHello 검증 + 세션키 도출(TS clientFinish). `pinned_spk`가 있으면
/// 반드시 일치해야 하고(QR 핀), 없으면 TOFU로 검증된 키를 그대로 돌려준다.
/// 오류 문자열은 TS와 동일하다 — "pinned key"/"signature" 접두 매칭과
/// formatErrorMessage 분류가 그대로 동작한다.
pub fn client_finish(
    ephemeral: &ClientEphemeral,
    hello: &ServerHello,
    pinned_spk: Option<&[u8; 32]>,
) -> Result<HandshakeOutcome, secure_channel::HandshakeError> {
    let (keys, spk) =
        secure_channel::client_finish(&ephemeral.secret, &ephemeral.hello, hello, pinned_spk)?;
    Ok(HandshakeOutcome { keys, spk })
}

/// 봉인 프레임을 전송 줄(`{"e":"<b64url>"}`)로 감싼다(TS sealedLine).
pub fn sealed_line(frame: &[u8]) -> String {
    format!("{{\"e\":\"{}\"}}", b64::encode(frame))
}

/// 전송 줄에서 봉인 프레임을 꺼낸다. 봉인 줄이 아니면 None
/// (TS parseSealedLine — null 대응).
pub fn parse_sealed_line(line: &str) -> Option<Vec<u8>> {
    let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
    let encoded = parsed.get("e")?.as_str()?;
    b64::decode(encoded)
}

/// 16진수 문자열(TS toHex — 벡터 비교용).
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 16진수 파싱. 홀수 길이·비16진수 문자면 None(TS hexToBytes — 그쪽은 예외를
/// 던지지만 Rust 코어는 값으로 실패를 돌려준다).
pub fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16)?;
            let lo = (pair[1] as char).to_digit(16)?;
            Some(((hi << 4) | lo) as u8)
        })
        .collect::<Option<Vec<u8>>>()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deterministic(seed: u8) -> impl RandomSource + 'static {
        struct S(u8);
        impl RandomSource for S {
            fn fill(&self, buf: &mut [u8]) {
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = self.0.wrapping_add(i as u8);
                }
            }
        }
        S(seed)
    }

    #[test]
    fn client_hello_line_matches_ts_field_order() {
        let random = deterministic(7);
        let ephemeral = create_client_hello(&random);
        let line = encode_client_hello(&ephemeral.hello);
        let nc = b64::encode(&ephemeral.hello.nc);
        let xk = b64::encode(&ephemeral.hello.xk);
        assert_eq!(
            line,
            format!("{{\"v\":1,\"hello\":\"c\",\"nc\":\"{nc}\",\"xk\":\"{xk}\"}}")
        );
    }

    #[test]
    fn sealed_line_framing_round_trips() {
        let frame = [1u8, 2, 3, 4, 5];
        let parsed = parse_sealed_line(&sealed_line(&frame)).expect("frame");
        assert_eq!(parsed, vec![1, 2, 3, 4, 5]);
        assert_eq!(parse_sealed_line("{\"command\":\"x\"}"), None);
        assert_eq!(parse_sealed_line("not json"), None);
    }

    #[test]
    fn parses_and_rejects_server_hello_lines() {
        let line = format!(
            "{{\"v\":1,\"hello\":\"s\",\"ns\":\"{}\",\"xk\":\"{}\",\"spk\":\"{}\",\"sig\":\"{}\"}}",
            b64::encode(&[3u8; 32]),
            b64::encode(&[4u8; 32]),
            b64::encode(&[5u8; 32]),
            b64::encode(&[6u8; 64]),
        );
        let parsed = parse_server_hello(&line).expect("parses");
        assert_eq!(parsed.ns, [3u8; 32]);
        assert_eq!(parsed.sig, [6u8; 64]);

        assert!(matches!(
            parse_server_hello("{\"v\":1,\"hello\":\"c\"}"),
            Err(ServerHelloParseError::NotServerHello)
        ));
        assert!(matches!(
            parse_server_hello(
                "{\"hello\":\"s\",\"ns\":\"\",\"xk\":\"\",\"spk\":\"\",\"sig\":\"\"}"
            ),
            Err(ServerHelloParseError::Malformed)
        ));
        assert!(matches!(
            parse_server_hello("not json"),
            Err(ServerHelloParseError::Malformed)
        ));
    }

    #[test]
    fn hex_helpers_round_trip() {
        assert_eq!(to_hex(&[0x0f, 0xa0]), "0fa0");
        assert_eq!(hex_to_bytes("0fa0").as_deref(), Some(&[0x0f, 0xa0][..]));
        assert_eq!(hex_to_bytes("0fa"), None);
        assert_eq!(hex_to_bytes("zz"), None);
    }
}
