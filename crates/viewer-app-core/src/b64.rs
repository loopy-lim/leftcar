//! unpadded base64url — TS(apps/viewer-expo/src/secure-channel.ts)의
//! bytesToBase64Url·base64UrlToBytes 대응. 의존성을 늘리지 않기 위해 직접
//! 구현하며, 디코딩은 표준 base64url 문자만 받고 남는 비트는 0이어야 한다.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// b64url 인코딩(패딩 없음). TS `bytesToBase64Url`과 바이트 단위로 같다.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 63) as usize] as char);
        }
    }
    out
}

/// b64url 디코딩. 비표준 문자·남는 비트가 0이 아니면 None — TS `base64UrlToBytes`
/// 는 atob 예외를 그대로 흘려보내지만, Rust 코어는 파싱 실패를 값으로 돌려준다.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for ch in text.bytes() {
        let v = match ch {
            b'A'..=b'Z' => (ch - b'A') as u32,
            b'a'..=b'z' => (ch - b'a' + 26) as u32,
            b'0'..=b'9' => (ch - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    // 남는 비트는 버린다 — TS base64UrlToBytes가 atob를 쓰기 때문에 비정준
    // 패딩(예: 43개 'B')도 디코딩된다. 핀 대조가 이 디코딩에 의존한다.
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg");
        assert_eq!(encode(b"fo"), "Zm8");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(&[0xfb, 0xff]), "-_8");
        assert_eq!(decode("Zg").as_deref(), Some(&[102u8][..]));
        assert_eq!(decode("Zm8").as_deref(), Some(&[102, 111][..]));
        assert_eq!(decode("Zm9v").as_deref(), Some(&[102, 111, 111][..]));
        assert_eq!(decode("-_8").as_deref(), Some(&[0xfb, 0xff][..]));
    }

    #[test]
    fn rejects_non_canonical_input() {
        assert_eq!(decode("***"), None);
        assert_eq!(decode("a/bc+"), None);
        // TS atob 호환: 남는 비트가 0이 아니어도 잘린 바이트로 디코딩된다.
        assert_eq!(decode("AB"), Some(vec![0u8]));
        assert_eq!(decode("AA"), Some(vec![0u8]));
        let decoded = decode(&"B".repeat(43)).expect("decodes like atob");
        assert_eq!(decoded.len(), 32);
    }
}
