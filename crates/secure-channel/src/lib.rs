//! Leftcar 세션 암호화 (docs/07 — 평문 전송 계층 대체).
//!
//! 두 층을 제공한다.
//!
//! 1. 제어 평면 핸드셰이크: 호스트는 정체 Ed25519 키(QR로 핀)를 가지고,
//!    뷰어는 임시 X25519 키로 PFS 세션 키를 합의한다. 호스트 서명은
//!    `nc‖ns‖xk_c‖xk_s‖spk` 전체 전사(transcript)를 덮어 MITM·재생을 막는다.
//! 2. 방향별 AEAD 봉인: TCP(제어)는 엄격 단조 카운터, UDP(미디어)는 슬라이딩
//!    윈도우 재생 방지를 쓴다. 암호는 ChaCha20-Poly1305(RFC 8439) 하나 —
//!    Rust·Swift(CryptoKit)·TS(@noble) 모두 표준 구현으로 상호 운용된다.
//!
//! 와이어 레이아웃(봉인 프레임): `counter u64 BE ‖ poly1305 tag 16B ‖ ct`.
//! nonce는 `00 00 00 00 ‖ counter u64 BE`(12B) — 방향은 아예 다른 키로 분리되므로
//! 접두어가 필요 없다.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use x25519_dalek::{PublicKey as XPublicKey, StaticSecret as XStaticSecret};
use zeroize::Zeroize;

/// 핸드셰이크 서명이 덮는 전사 접두어. 프로토콜 버전을 바꾸면 함께 바꾼다.
pub const SIG_CONTEXT: &[u8] = b"leftcar-ctrl-v1";
/// HKDF info 라벨.
pub const HANDSHAKE_INFO: &[u8] = b"leftcar/control/v1";

pub const COUNTER_LEN: usize = 8;
pub const TAG_LEN: usize = 16;
/// 봉인 프레임의 최대 평문 크기(제어 프레임 상한과 동일).
pub const MAX_PLAINTEXT: usize = 16 * 1024 * 1024;
/// UDP 봉인 프레임의 최대 평문 크기(단편화된 미디어 데이터그램 상한).
pub const MAX_DATAGRAM: usize = 64 * 1024;
/// 허용되는 UDP 순서 뒤바뀜 폭. LAN에서 관측되는 재정렬보다 몇 배 크다.
pub const REPLAY_WINDOW: u64 = 8192;

// -- 호스트 정체 키 -----------------------------------------------------------

/// CSPRNG 바이트(호스트 측 논스·임시키·세션키 생성에 쓴다).
pub fn random_bytes(buf: &mut [u8]) {
    OsRng.fill_bytes(buf);
}

/// 호스트의 장기 Ed25519 정체 키. 공개키는 QR(`k` 필드)로 뷰어에 핀된다.
pub struct HostIdentity {
    signing: SigningKey,
}

impl HostIdentity {
    /// OS CSPRNG로 새 정체 키를 만든다. 최초 기동에서 한 번만 호출한다.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        Self::from_seed(seed)
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(&seed),
        }
    }

    pub fn seed(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }
}

/// 핀된 호스트 공개키로 서명을 검증한다. 실패 원인(키 불일치/서명 불량)은
/// 하나로 뭉친다 — 호출자는 "정체 확인 실패"만 알면 된다.
pub fn verify_host_signature(
    host_public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> bool {
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(host_public_key) else {
        return false;
    };
    key.verify(message, &Signature::from_bytes(signature)).is_ok()
}

/// 서명이 덮는 전사 바이트. TS 구현(apps/viewer-expo/src/secure-channel.ts)과
/// 바이트 단위로 일치해야 한다.
pub fn signed_transcript(
    nc: &[u8; 32],
    ns: &[u8; 32],
    xk_c: &[u8; 32],
    xk_s: &[u8; 32],
    spk: &[u8; 32],
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(SIG_CONTEXT.len() + 160);
    msg.extend_from_slice(SIG_CONTEXT);
    msg.extend_from_slice(nc);
    msg.extend_from_slice(ns);
    msg.extend_from_slice(xk_c);
    msg.extend_from_slice(xk_s);
    msg.extend_from_slice(spk);
    msg
}

// -- 핸드셰이크 ---------------------------------------------------------------

/// 뷰어 → 호스트 첫 줄. 평문으로 전송된다(공개 값만 담는다).
#[derive(Clone, Copy)]
pub struct ClientHello {
    /// 뷰어가 뽑는 32B 논스(HKDF 솔트 앞부분 + 재생 방지).
    pub nc: [u8; 32],
    /// 뷰어 임시 X25519 공개키.
    pub xk: [u8; 32],
}

/// 호스트 → 뷰어 두 번째 줄. 평문으로 전송된다(서명이 정체를 증명한다).
#[derive(Clone, Copy)]
pub struct ServerHello {
    pub ns: [u8; 32],
    /// 호스트 임시 X25519 공개키.
    pub xk: [u8; 32],
    /// 호스트 장기 공개키 — 뷰어가 핀된 QR 키와 비교한다.
    pub spk: [u8; 32],
    pub sig: [u8; 64],
}

/// 호스트 측 ServerHello 생성. `ns`/`xk_secret`은 호출자가 CSPRNG로 뽑아 넣는다.
pub fn server_hello(
    identity: &HostIdentity,
    client: &ClientHello,
    ns: [u8; 32],
    xk_secret: &XStaticSecret,
) -> ServerHello {
    let xk = XPublicKey::from(xk_secret).to_bytes();
    let spk = identity.public_key();
    let msg = signed_transcript(&client.nc, &ns, &client.xk, &xk, &spk);
    ServerHello {
        ns,
        xk,
        spk,
        sig: identity.sign(&msg),
    }
}

/// 호스트 측 세션키 도출.
pub fn server_keys(xk_secret: &XStaticSecret, client: &ClientHello, ns: &[u8; 32]) -> SessionKeys {
    let shared = x25519_shared(xk_secret, &client.xk);
    derive_keys(&shared, &client.nc, ns)
}

/// 호스트 측 응답 생성 + 키 도출을 한 번에. `ns`와 임시 X25519 비밀은 내부
/// CSPRNG에서 뽑는다 — 호출자는 ClientHello만 파싱하면 된다.
pub fn accept_client(identity: &HostIdentity, client: &ClientHello) -> (ServerHello, SessionKeys) {
    let mut ns = [0u8; 32];
    random_bytes(&mut ns);
    let mut seed = [0u8; 32];
    random_bytes(&mut seed);
    let secret = XStaticSecret::from(seed);
    seed.zeroize();
    let hello = server_hello(identity, client, ns, &secret);
    let keys = server_keys(&secret, client, &ns);
    (hello, keys)
}

/// 뷰어 측 ServerHello 검증 + 세션키 도출. `pinned_spk`가 Some이면 반드시
/// 일치해야 하고(QR 핀), None이면 TOFU로 반환값을 그대로 핀한다.
pub fn client_finish(
    client_secret: &XStaticSecret,
    client: &ClientHello,
    hello: &ServerHello,
    pinned_spk: Option<&[u8; 32]>,
) -> Result<(SessionKeys, [u8; 32]), HandshakeError> {
    if let Some(pinned) = pinned_spk {
        if pinned != &hello.spk {
            return Err(HandshakeError::HostKeyMismatch);
        }
    }
    let msg = signed_transcript(&client.nc, &hello.ns, &client.xk, &hello.xk, &hello.spk);
    if !verify_host_signature(&hello.spk, &msg, &hello.sig) {
        return Err(HandshakeError::BadSignature);
    }
    let shared = x25519_shared(client_secret, &hello.xk);
    let keys = derive_keys(&shared, &client.nc, &hello.ns);
    Ok((keys, hello.spk))
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum HandshakeError {
    #[error("host key does not match pinned key")]
    HostKeyMismatch,
    #[error("host signature invalid")]
    BadSignature,
}

/// 방향별 32B 키 쌍. c2s는 뷰어가 봉인/호스트가 열고, s2c는 그 반대다.
pub struct SessionKeys {
    pub c2s: [u8; 32],
    pub s2c: [u8; 32],
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 키 바이트는 절대 로그에 흘러가지 않는다.
        f.write_str("SessionKeys(<redacted>)")
    }
}

impl Drop for SessionKeys {
    fn drop(&mut self) {
        self.c2s.zeroize();
        self.s2c.zeroize();
    }
}

fn x25519_shared(secret: &XStaticSecret, peer: &[u8; 32]) -> [u8; 32] {
    secret
        .diffie_hellman(&XPublicKey::from(*peer))
        .to_bytes()
}

/// HKDF-SHA256: ikm = X25519 공유비밀, salt = nc‖ns, info = HANDSHAKE_INFO.
pub fn derive_keys(shared: &[u8; 32], nc: &[u8; 32], ns: &[u8; 32]) -> SessionKeys {
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(nc);
    salt.extend_from_slice(ns);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut okm = [0u8; 64];
    hk.expand(HANDSHAKE_INFO, &mut okm).expect("64B okm");
    let mut c2s = [0u8; 32];
    let mut s2c = [0u8; 32];
    c2s.copy_from_slice(&okm[..32]);
    s2c.copy_from_slice(&okm[32..]);
    okm.zeroize();
    SessionKeys { c2s, s2c }
}

// -- 봉인 ---------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("frame too short")]
    TooShort,
    #[error("frame exceeds size limit")]
    TooLarge,
    #[error("counter replayed or stale")]
    Replay,
    #[error("authentication failed")]
    Auth,
}

fn nonce_for(counter: u64) -> Nonce {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::from(nonce)
}

fn cipher(key: &[u8; 32]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&Key::from(*key))
}

/// 프레임 상단의 카운터를 읽는다(봉인기 밖에서도 필요할 때).
pub fn frame_counter(frame: &[u8]) -> Option<u64> {
    if frame.len() < COUNTER_LEN + TAG_LEN {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&frame[..COUNTER_LEN]);
    Some(u64::from_be_bytes(bytes))
}

/// TCP(제어 평면)용 봉인기 — 엄격 단조 카운터. 방향마다 하나씩 만든다.
pub struct StreamSealer {
    key: [u8; 32],
    next: u64,
    last_seen: u64,
}

impl StreamSealer {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            next: 1,
            last_seen: 0,
        }
    }

    /// `counter ‖ tag ‖ ct`. 최대 [MAX_PLAINTEXT]바이트까지.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, OpenError> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(OpenError::TooLarge);
        }
        let counter = self.next;
        self.next = self.next.wrapping_add(1);
        let ct = cipher(&self.key)
            .encrypt(&nonce_for(counter), Payload::from(plaintext))
            .expect("chacha seal cannot fail for in-memory buffers");
        let mut out = Vec::with_capacity(COUNTER_LEN + ct.len());
        out.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// 카운터는 이전에 본 값보다 커야 한다(TCP는 순서 보장 — 손실·재생 모두
    /// 비정상이다). 되감긴 프레임은 전부 [OpenError::Replay].
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, OpenError> {
        if frame.len() < COUNTER_LEN + TAG_LEN {
            return Err(OpenError::TooShort);
        }
        if frame.len() - COUNTER_LEN > MAX_PLAINTEXT + TAG_LEN {
            return Err(OpenError::TooLarge);
        }
        let Some(counter) = frame_counter(frame) else {
            return Err(OpenError::TooShort);
        };
        if counter == 0 || counter <= self.last_seen {
            return Err(OpenError::Replay);
        }
        let pt = cipher(&self.key)
            .decrypt(&nonce_for(counter), Payload::from(&frame[COUNTER_LEN..]))
            .map_err(|_| OpenError::Auth)?;
        self.last_seen = counter;
        Ok(pt)
    }
}

/// UDP(미디어)용 봉인기 — 발신은 원자 카운터, 수신은 슬라이딩 윈도우 재생
/// 방지. 같은 방향의 발신·수신 스레드가 여러 개여도 안전하다.
pub struct DatagramSealer {
    key: [u8; 32],
    next: AtomicU64,
    rx: Mutex<ReplayWindow>,
}

impl DatagramSealer {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            next: AtomicU64::new(1),
            rx: Mutex::new(ReplayWindow::new()),
        }
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, OpenError> {
        if plaintext.len() > MAX_DATAGRAM {
            return Err(OpenError::TooLarge);
        }
        let counter = self.next.fetch_add(1, Ordering::Relaxed);
        let ct = cipher(&self.key)
            .encrypt(&nonce_for(counter), Payload::from(plaintext))
            .expect("chacha seal cannot fail for in-memory buffers");
        let mut out = Vec::with_capacity(COUNTER_LEN + ct.len());
        out.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn open(&self, frame: &[u8]) -> Result<Vec<u8>, OpenError> {
        if frame.len() < COUNTER_LEN + TAG_LEN {
            return Err(OpenError::TooShort);
        }
        if frame.len() - COUNTER_LEN > MAX_DATAGRAM + TAG_LEN {
            return Err(OpenError::TooLarge);
        }
        let Some(counter) = frame_counter(frame) else {
            return Err(OpenError::TooShort);
        };
        if counter == 0 {
            return Err(OpenError::Replay);
        }
        {
            let rx = self.rx.lock().unwrap();
            if !rx.check(counter) {
                return Err(OpenError::Replay);
            }
        }
        let pt = cipher(&self.key)
            .decrypt(&nonce_for(counter), Payload::from(&frame[COUNTER_LEN..]))
            .map_err(|_| OpenError::Auth)?;
        self.rx.lock().unwrap().accept(counter);
        Ok(pt)
    }
}

/// RFC 6479 비트맵 윈도우: 최고 카운터와 그 이전 [REPLAY_WINDOW]개의 등장
/// 여부를 1비트씩 기억한다.
struct ReplayWindow {
    highest: u64,
    bits: Vec<u64>,
}

impl ReplayWindow {
    fn new() -> Self {
        let words = (REPLAY_WINDOW / 64) as usize;
        Self {
            highest: 0,
            bits: vec![0u64; words],
        }
    }

    fn check(&self, counter: u64) -> bool {
        if counter > self.highest {
            return true;
        }
        let age = self.highest - counter;
        // age 0(최고 카운터 자체)와 윈도우 밖은 모두 "이미 봤거나 버린다".
        if age == 0 || age >= REPLAY_WINDOW {
            return false;
        }
        let word = (age / 64) as usize;
        let bit = (age % 64) as u32;
        self.bits[word] & (1u64 << bit) == 0
    }

    fn accept(&mut self, counter: u64) {
        if counter > self.highest {
            let delta = counter - self.highest;
            if delta >= REPLAY_WINDOW {
                self.bits.iter_mut().for_each(|w| *w = 0);
            } else {
                self.shift_bits(delta as u32);
                // 이전 최고 카운터는 비트로 기록되지 않았으므로 새 나이에
                // 명시적으로 심는다.
                let word = (delta / 64) as usize;
                let bit = (delta % 64) as u32;
                self.bits[word] |= 1u64 << bit;
            }
            self.highest = counter;
            return;
        }
        let age = self.highest - counter;
        if age >= 1 && age < REPLAY_WINDOW {
            let word = (age / 64) as usize;
            let bit = (age % 64) as u32;
            self.bits[word] |= 1u64 << bit;
        }
    }

    /// 비트 배열(나이 순)을 `shift`비트 만큼 높은 나이 쪽으로 민다.
    fn shift_bits(&mut self, shift: u32) {
        let words = self.bits.len();
        let word_shift = (shift / 64) as usize;
        let bit_shift = shift % 64;
        if word_shift >= words {
            self.bits.iter_mut().for_each(|w| *w = 0);
            return;
        }
        let mut next = vec![0u64; words];
        for i in word_shift..words {
            let src = i - word_shift;
            next[i] = self.bits[src] << bit_shift;
            if bit_shift > 0 && src >= 1 {
                next[i] |= self.bits[src - 1] >> (64 - bit_shift);
            }
        }
        self.bits = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(bytes: u8) -> [u8; 32] {
        (bytes..bytes + 32).collect::<Vec<u8>>().try_into().unwrap()
    }

    /// TS(apps/viewer-expo/src/secure-channel.test.ts)와 공유하는 고정 벡터.
    /// `cargo test -p secure-channel --release -- print_vector --ignored --nocapture`
    /// 로 출력한 값을 양쪽에 하드코딩해 상호 운용을 잠근다.
    #[test]
    #[ignore = "vector printer: run with --ignored --nocapture"]
    fn print_vector() {
        let identity = HostIdentity::from_seed(fixed(0));
        let c_secret = XStaticSecret::from(fixed(32));
        let s_secret = XStaticSecret::from(fixed(64));
        let nc: [u8; 32] = fixed(160);
        let ns: [u8; 32] = fixed(192);
        let client = ClientHello {
            nc,
            xk: XPublicKey::from(&c_secret).to_bytes(),
        };
        let hello = server_hello(&identity, &client, ns, &s_secret);
        let (keys, spk) = client_finish(&c_secret, &client, &hello, Some(&identity.public_key()))
            .unwrap();
        println!("spk        = {}", hex::encode(spk));
        println!("sig        = {}", hex::encode(hello.sig));
        println!("client_xk  = {}", hex::encode(client.xk));
        println!("server_xk  = {}", hex::encode(hello.xk));
        println!("k_c2s      = {}", hex::encode(keys.c2s));
        println!("k_s2c      = {}", hex::encode(keys.s2c));
        let mut c2s_sealer = StreamSealer::new(keys.c2s.clone());
        let frame = c2s_sealer
            .seal(br#"{"command":"pair","args":{},"token":"a"}"#)
            .unwrap();
        println!("c2s_frame1 = {}", hex::encode(&frame));
        let mut s2c_sealer = StreamSealer::new(keys.s2c.clone());
        let frame2 = s2c_sealer.seal(br#"{"ok":true,"result":{"token":"b"}}"#).unwrap();
        println!("s2c_frame1 = {}", hex::encode(&frame2));
    }

    #[test]
    fn handshake_roundtrip_and_rejections() {
        let identity = HostIdentity::from_seed(fixed(0));
        let c_secret = XStaticSecret::from(fixed(32));
        let s_secret = XStaticSecret::from(fixed(64));
        let nc = fixed(160);
        let ns = fixed(192);
        let client = ClientHello {
            nc,
            xk: XPublicKey::from(&c_secret).to_bytes(),
        };
        let hello = server_hello(&identity, &client, ns, &s_secret);
        let skeys = server_keys(&s_secret, &client, &ns);
        let (ckeys, spk) = client_finish(&c_secret, &client, &hello, None).unwrap();
        assert_eq!(spk, identity.public_key());
        assert_eq!(ckeys.c2s, skeys.c2s);
        assert_eq!(ckeys.s2c, skeys.s2c);

        // 핀 불일치는 거부된다.
        assert_eq!(
            client_finish(&c_secret, &client, &hello, Some(&[7u8; 32])).unwrap_err(),
            HandshakeError::HostKeyMismatch
        );
        // 서명 변조는 거부된다.
        let mut tampered = hello;
        tampered.sig[0] ^= 1;
        assert_eq!(
            client_finish(&c_secret, &client, &tampered, None).unwrap_err(),
            HandshakeError::BadSignature
        );
        // 전사 변조(중간자가 xk_s를 바꿈)는 서명 검증이 막는다.
        let mut hijacked = hello;
        hijacked.xk = XPublicKey::from(&XStaticSecret::from(fixed(99))).to_bytes();
        assert_eq!(
            client_finish(&c_secret, &client, &hijacked, None).unwrap_err(),
            HandshakeError::BadSignature
        );
    }

    #[test]
    fn stream_sealer_strict_counter() {
        let mut tx = StreamSealer::new([1u8; 32]);
        let mut rx = StreamSealer::new([1u8; 32]);
        let f1 = tx.seal(b"one").unwrap();
        let f2 = tx.seal(b"two").unwrap();
        assert_eq!(rx.open(&f1).unwrap(), b"one");
        assert_eq!(rx.open(&f2).unwrap(), b"two");
        // 재생은 거부
        assert_eq!(rx.open(&f1), Err(OpenError::Replay));
        // 다른 키는 인증 실패
        let mut other = StreamSealer::new([2u8; 32]);
        assert_eq!(other.open(&f1), Err(OpenError::Auth));
        // 잘림
        assert_eq!(rx.open(&f1[..8]), Err(OpenError::TooShort));
        // 카운터 되감기
        let f3 = tx.seal(b"three").unwrap();
        assert_eq!(rx.open(&f3).unwrap(), b"three");
        let f4 = tx.seal(b"four").unwrap();
        assert_eq!(rx.open(&f4).unwrap(), b"four");
    }

    #[test]
    fn datagram_sealer_allows_reordering_rejects_replay() {
        let tx = DatagramSealer::new([3u8; 32]);
        let rx = DatagramSealer::new([3u8; 32]);
        let f1 = tx.seal(b"a").unwrap();
        let f2 = tx.seal(b"b").unwrap();
        let f3 = tx.seal(b"c").unwrap();
        // 뒤바뀐 도착도 복호된다.
        assert_eq!(rx.open(&f3).unwrap(), b"c");
        assert_eq!(rx.open(&f1).unwrap(), b"a");
        assert_eq!(rx.open(&f2).unwrap(), b"b");
        // 중복(재생)은 거부된다.
        assert_eq!(rx.open(&f1), Err(OpenError::Replay));
    }

    #[test]
    fn datagram_window_slides() {
        let tx = DatagramSealer::new([4u8; 32]);
        let rx = DatagramSealer::new([4u8; 32]);
        let old = tx.seal(b"old").unwrap();
        // 윈도우보다 먼 미래 프레임을 밀어 넣는다.
        for _ in 0..(REPLAY_WINDOW + 64) {
            let f = tx.seal(b"x").unwrap();
            rx.open(&f).unwrap();
        }
        assert_eq!(rx.open(&old), Err(OpenError::Replay));
    }

    #[test]
    fn datagram_window_shift_is_exact() {
        // shift 1..96 구간에서 윈도우 이동이 비트를 잃어버리지 않는지 검증.
        for shift in 1u64..96 {
            let mut w = ReplayWindow::new();
            w.accept(1);
            assert!(!w.check(1), "accepted counter is not a replay, shift={shift}");
            w.accept(1 + shift);
            assert!(!w.check(1 + shift), "shift={shift}");
            if shift > 1 {
                // 사이의 미접수 카운터는 여전히 수용 가능해야 한다.
                assert!(
                    w.check(1 + (shift + 1) / 2),
                    "unseen counter inside window must be acceptable, shift={shift}"
                );
            }
            // 옛 접수 기록은 이동 후에도 유지된다.
            assert!(!w.check(1), "accepted bit must persist, shift={shift}");
        }
    }

    #[test]
    fn datagram_seal_counter_is_unique_across_threads() {
        use std::sync::Arc;
        let tx = Arc::new(DatagramSealer::new([5u8; 32]));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    (0..25).map(|_| tx.seal(b"p").unwrap()).collect::<Vec<_>>()
                })
            })
            .collect();
        let mut counters = Vec::new();
        for handle in handles {
            for frame in handle.join().unwrap() {
                counters.push(frame_counter(&frame).unwrap());
            }
        }
        counters.sort_unstable();
        counters.dedup();
        assert_eq!(counters.len(), 100);
    }
}
