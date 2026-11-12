//! secure-channel 스펙 이전(apps/viewer-expo/src/secure-channel.test.ts).
//! 상호 운용 고정 벡터는 TS·Rust 양쪽에 하드코딩되어 한 바이트라도 어긋나면
//! 뷰어와 호스트가 서로를 못 알아듣는다.

mod common;

use common::SeqRandom;
use secure_channel::{HostIdentity, OpenError};
use viewer_app_core::b64;
use viewer_app_core::secure::{
    client_finish, create_client_hello, hex_to_bytes, parse_sealed_line, parse_server_hello,
    sealed_line, to_hex, StreamSealer, MAX_PLAINTEXT,
};

// 상호 운용 고정 벡터: Rust `cargo test -p secure-channel -- print_vector
// --ignored --nocapture` 출력과 정확히 일치해야 한다.
const SPK: &str = "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
const SIG: &str = "b8d0ff5fadcb9f47824759df315d852be7ee5483c7f16dc9d3853604bb6134ffc545adf1fd57b517bc084589c0e62df4c8637dad9ee084b374bae31f6d365e0f";
const CLIENT_XK: &str = "358072d6365880d1aeea329adf9121383851ed21a28e3b75e965d0d2cd166254";
const SERVER_XK: &str = "79a631eede1bf9c98f12032cdeadd0e7a079398fc786b88cc846ec89af85a51a";
const K_C2S: &str = "dadcc9dd549eace6797cf9f0e9a6716ed8b9d789dcc5c4cbfd058c0634aaa69c";
const K_S2C: &str = "b5fd58db06e436bb0b3070e44b52d8a733e97e615b251763216871aab95ceb41";
const C2S_FRAME1: &str = "00000000000000018b799614f1525fccb77713ef3b7deb426d95ee162f9ea66bd70fbab35b6550577ee7fbbe2a6f786d2a8269ec94b872ed699c93dea0ca0d66";
const S2C_FRAME1: &str = "00000000000000011a4e46222f43ad16b566b2b078c137a28264d7818f8452227040f6f1c350ab87ee63f09d49ccb8f8c6755b3d3c190d04b997";

/// 벡터의 결정적 입력(Rust 테스트와 동일한 바이트 시퀀스).
fn fixed(start: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = start + index as u8;
    }
    bytes
}

fn arr32(hex: &str) -> [u8; 32] {
    hex_to_bytes(hex).expect("hex").try_into().expect("32B")
}

fn arr64(hex: &str) -> [u8; 64] {
    hex_to_bytes(hex).expect("hex").try_into().expect("64B")
}

struct VectorHandshake {
    server: secure_channel::ServerHello,
    nc: [u8; 32],
}

fn hello_from_vector() -> VectorHandshake {
    // 벡터의 x25519 비밀은 32..64(클라이언트), 64..96(서버) 바이트 시퀀스다.
    let nc = fixed(160);
    let server = secure_channel::ServerHello {
        ns: fixed(192),
        xk: arr32(SERVER_XK),
        spk: arr32(SPK),
        sig: arr64(SIG),
    };
    VectorHandshake { server, nc }
}

/// 벡터의 클라이언트 임시키 비밀(32..64)을 그대로 쓰는 clientFinish.
fn client_finish_with_vector_secret(
    vector: &VectorHandshake,
    pinned: Option<&[u8; 32]>,
) -> viewer_app_core::secure::HandshakeOutcome {
    let secret = x25519_dalek::StaticSecret::from(fixed(32));
    let client_hello = secure_channel::ClientHello {
        nc: vector.nc,
        xk: arr32(CLIENT_XK),
    };
    // 벡터 경로는 secure-channel의 raw client_finish를 쓴다 — 비밀을 벡터 값으로
    // 고정해야 하므로 랜덤 생성 절차를 거치지 않는다.
    let (keys, spk) = secure_channel::client_finish(&secret, &client_hello, &vector.server, pinned)
        .expect("finish");
    viewer_app_core::secure::HandshakeOutcome { keys, spk }
}

#[test]
fn rejects_malformed_sealed_envelopes_without_throwing() {
    for line in ["null", "[]", "true", "{\"e\":\"***\"}", "broken"] {
        assert_eq!(parse_sealed_line(line), None, "{line}");
    }
}

#[test]
fn rejects_oversized_frames_before_decryption() {
    let mut sealer = StreamSealer::new([0u8; 32]);
    let oversized = vec![0u8; MAX_PLAINTEXT + 25];
    assert_eq!(sealer.open(&oversized), Err(OpenError::TooLarge));
}

#[test]
fn rejects_a_correctly_signed_low_order_x25519_peer() {
    for low_order in [0u8, 1u8] {
        let mut xk_s = [0u8; 32];
        xk_s[0] = low_order;
        let nc = fixed(160);
        let ns = fixed(192);
        let xk_c = arr32(CLIENT_XK);
        let identity = HostIdentity::from_seed(fixed(0));
        let spk = identity.public_key();
        let message = secure_channel::signed_transcript(&nc, &ns, &xk_c, &xk_s, &spk);
        let sig = identity.sign(&message);
        let secret = x25519_dalek::StaticSecret::from(fixed(32));
        let client = secure_channel::ClientHello { nc, xk: xk_c };
        let server = secure_channel::ServerHello {
            ns,
            xk: xk_s,
            spk,
            sig,
        };
        // 서명은 정상이지만 작수 키는 공유비밀에 기여하지 못한다.
        assert!(matches!(
            secure_channel::client_finish(&secret, &client, &server, Some(&spk)),
            Err(secure_channel::HandshakeError::InvalidPeerKey)
        ));
    }
}

#[test]
fn derives_the_exact_session_keys_of_the_cross_language_vector() {
    let vector = hello_from_vector();
    let outcome = client_finish_with_vector_secret(&vector, None);
    assert_eq!(to_hex(&outcome.keys.c2s), K_C2S);
    assert_eq!(to_hex(&outcome.keys.s2c), K_S2C);
}

#[test]
fn seals_frame1_byte_for_byte_like_the_cross_language_vector() {
    let vector = hello_from_vector();
    let outcome = client_finish_with_vector_secret(&vector, None);
    let mut sealer = StreamSealer::new(outcome.keys.c2s);
    let frame = sealer
        .seal(br#"{"command":"pair","args":{},"token":"a"}"#)
        .expect("seal");
    assert_eq!(to_hex(&frame), C2S_FRAME1);

    let mut echo = StreamSealer::new(outcome.keys.s2c);
    let frame2 = echo
        .seal(br#"{"ok":true,"result":{"token":"b"}}"#)
        .expect("seal");
    assert_eq!(to_hex(&frame2), S2C_FRAME1);
}

#[test]
fn accepts_a_pinned_matching_key_and_rejects_a_mismatching_pin() {
    let vector = hello_from_vector();
    let pinned = arr32(SPK);
    let ok = client_finish_with_vector_secret(&vector, Some(&pinned));
    assert_eq!(to_hex(&ok.spk), SPK);

    let secret = x25519_dalek::StaticSecret::from(fixed(32));
    let client = secure_channel::ClientHello {
        nc: vector.nc,
        xk: arr32(CLIENT_XK),
    };
    let mismatch =
        secure_channel::client_finish(&secret, &client, &vector.server, Some(&[9u8; 32]))
            .expect_err("mismatching pin");
    assert_eq!(mismatch, secure_channel::HandshakeError::HostKeyMismatch);
    assert!(mismatch.to_string().contains("pinned key"));
}

#[test]
fn rejects_a_tampered_signature() {
    let vector = hello_from_vector();
    let mut tampered = vector.server;
    tampered.sig[0] ^= 1;
    let secret = x25519_dalek::StaticSecret::from(fixed(32));
    let client = secure_channel::ClientHello {
        nc: vector.nc,
        xk: arr32(CLIENT_XK),
    };
    let error =
        secure_channel::client_finish(&secret, &client, &tampered, None).expect_err("tampered");
    assert_eq!(error, secure_channel::HandshakeError::BadSignature);
}

#[test]
fn round_trips_create_client_hello_through_parse_server_hello_with_a_scripted_server() {
    // 스크립트 서버: 같은 라이브러리로 ServerHello를 만들어 준다(자기 일치).
    let random = SeqRandom(7);
    let ephemeral = create_client_hello(&random);
    let line = serde_json::json!({
        "v": 1,
        "hello": "s",
        "ns": b64::encode(&[3u8; 32]),
        "xk": b64::encode(&[4u8; 32]),
        "spk": b64::encode(&[5u8; 32]),
        "sig": b64::encode(&[6u8; 64]),
    })
    .to_string();
    let parsed = parse_server_hello(&line).expect("parses");
    assert_eq!(parsed.ns, [3u8; 32]);
    // 서명은 가짜이므로 실패해야 한다 — 구조 파싱만 확인.
    let error = match client_finish(&ephemeral, &parsed, None) {
        Err(error) => error,
        Ok(_) => panic!("fake signature must fail"),
    };
    assert_eq!(error, secure_channel::HandshakeError::BadSignature);
}

#[test]
fn stream_sealer_round_trips_and_enforces_strict_counters() {
    let mut tx = StreamSealer::new([1u8; 32]);
    let mut rx = StreamSealer::new([1u8; 32]);
    let f1 = tx.seal(b"one").expect("seal");
    let f2 = tx.seal(b"two").expect("seal");
    assert_eq!(rx.open(&f1).as_deref(), Ok("one".as_bytes()));
    assert_eq!(rx.open(&f2).as_deref(), Ok("two".as_bytes()));
    // 재생은 거부된다.
    assert_eq!(rx.open(&f1), Err(OpenError::Replay));
    // 다른 키는 인증 실패.
    let mut other = StreamSealer::new([2u8; 32]);
    assert_eq!(other.open(&f1), Err(OpenError::Auth));
    // 잘림.
    assert_eq!(rx.open(&f1[..8]), Err(OpenError::TooShort));
    // 카운터는 되감기지 않는다.
    let f3 = tx.seal(b"three").expect("seal");
    let f4 = tx.seal(b"four").expect("seal");
    assert_eq!(rx.open(&f3).as_deref(), Ok("three".as_bytes()));
    assert_eq!(rx.open(&f4).as_deref(), Ok("four".as_bytes()));
    assert_eq!(rx.open(&f3), Err(OpenError::Replay));
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
fn derive_keys_salt_order_is_nc_then_ns() {
    // informational — 실제 솔트 순서는 벡터 테스트가 고정한다.
    let keys = secure_channel::derive_keys(&[1u8; 32], &[2u8; 32], &[3u8; 32]);
    assert_eq!(keys.c2s.len(), 32);
    assert_eq!(keys.s2c.len(), 32);
    // nc‖ns와 ns‖nc는 다른 키를 내야 한다(솔트 순서가 실제로 유효함).
    let swapped = secure_channel::derive_keys(&[1u8; 32], &[3u8; 32], &[2u8; 32]);
    assert_ne!(keys.c2s, swapped.c2s);
}
