//! Media-path AEAD sealing for every viewer ↔ host datagram.
//!
//! The viewer generates a 32-byte session key (JS `randomBytes(32)`), hands
//! it to the native prepare calls and sends the same key to the host through
//! the already-encrypted control plane. Each direction derives its own key
//! from the session key (`secure_channel::media_keys`, HKDF-SHA256): sharing
//! the raw key would collide viewer→host frame #N with host→viewer frame #N
//! under the same (key, nonce) on every session. Possession of the key
//! replaces the legacy plaintext LCH1 challenge-token suffix authentication;
//! the sealed `LCH1 ‖ nonce` challenge keeps only its reachability/NAT-pinning
//! role.
//!
//! Wire layout (crates/secure-channel): `counter u64 BE ‖ ct
//! ‖ poly1305 tag 16B`, nonce `00{4} ‖ counter u64 BE`. Counters start at a random point
//! per instance, so a reconfigure that restarts the sealers under the reused
//! session key still cannot repeat a nonce.

use secure_channel::{DatagramSealer, OpenError};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zeroize::Zeroize;

/// Plaintext prefix of the sealed reachability challenge.
pub const CHALLENGE_PREFIX: &[u8] = b"LCH1";
/// Bound the authenticated challenge accepted by preflight and renderers.
pub const MAX_CHALLENGE_BYTES: usize = 192;

pub struct MediaSessionCrypto {
    /// 세션 키 사본 — [MediaSessionCrypto::session_key]가 재바인드 폴백에 쓴다.
    key: [u8; 32],
    /// Outgoing direction (viewer → host), sealed under the derived c2s key.
    tx: DatagramSealer,
    /// Incoming direction (host → viewer), opened under the derived s2c key.
    rx: DatagramSealer,
    /// Set once the sealed LCH1 challenge was observed, mirroring the old
    /// "session token known" gate for IDR/input/feedback sends.
    established: AtomicBool,
    /// A retried echo must not reset replay protection for already-opened media.
    challenges: Mutex<HashSet<Vec<u8>>>,
}

impl MediaSessionCrypto {
    /// 등록 시 쓴 세션 키 사본 — 재바인드 폴백이 같은 키로 재등록할 때 쓴다.
    pub fn session_key(&self) -> [u8; 32] {
        self.key
    }

    pub fn new(key: [u8; 32]) -> Self {
        let keys = secure_channel::media_keys(&key);
        Self {
            key,
            tx: DatagramSealer::new(keys.c2s),
            rx: DatagramSealer::new(keys.s2c),
            established: AtomicBool::new(false),
            challenges: Mutex::new(HashSet::new()),
        }
    }

    /// Seal one outgoing datagram. Fails only for oversized plaintext, which
    /// every call site already bounds by wire construction.
    pub fn seal(&self, plaintext: &[u8]) -> Option<Vec<u8>> {
        self.tx.seal(plaintext).ok()
    }

    /// Open one incoming datagram. `None` means forged, replayed, or
    /// truncated — callers drop the frame without any further parsing.
    pub fn open(&self, frame: &[u8]) -> Option<Vec<u8>> {
        let plaintext = self.rx.open(frame).ok()?;
        self.record_authenticated_challenge(&plaintext);
        Some(plaintext)
    }

    /// In-place variant of [MediaSessionCrypto::open] for the RX hot loop:
    /// decrypts the sealed frame inside the caller's receive buffer and
    /// returns the plaintext slice, avoiding one heap allocation and copy per
    /// datagram. `None` means forged, replayed, or truncated — the buffer
    /// contents must then be discarded.
    pub fn open_into<'a>(&self, frame: &'a mut [u8]) -> Option<&'a mut [u8]> {
        let plaintext = self.rx.open_into(frame).ok()?;
        self.record_authenticated_challenge(plaintext);
        Some(plaintext)
    }

    /// Active single/split renderers open every datagram before classifying
    /// it. Remember challenges here too, so a later suspended/prepared retry
    /// cannot mistake them for a new Host incarnation and reset the window.
    /// Ordinary media needs no extra decryption, allocation, or lock.
    fn record_authenticated_challenge(&self, plaintext: &[u8]) {
        if plaintext.starts_with(CHALLENGE_PREFIX) && plaintext.len() <= MAX_CHALLENGE_BYTES {
            self.challenges.lock().unwrap().insert(plaintext.to_vec());
            self.establish();
        }
    }

    /// Authenticate a host `LCH1` challenge and enable session control sends.
    /// Ordinary media stays sealed for the renderer without consuming its
    /// receive counter in the prepared UDP/TCP/USB listener.
    ///
    /// Host `backend.start` retries mint a fresh TX sealer (random counter
    /// start) under the same media key while this viewer keeps one crypto
    /// instance — the persisted RX watermark can then sit above every
    /// retried challenge, and the windowed open rejects them as replays. A
    /// new challenge nonce authenticated under the s2c key allows that
    /// restart. Remember previous nonces so duplicate or delayed challenges
    /// can be echoed without ever resetting media replay protection again.
    pub fn open_challenge(&self, frame: &[u8]) -> Option<Vec<u8>> {
        let plaintext = self.rx.authenticate(frame).ok()?;
        if !plaintext.starts_with(CHALLENGE_PREFIX) || plaintext.len() > MAX_CHALLENGE_BYTES {
            return None;
        }
        let mut challenges = self.challenges.lock().unwrap();
        if !challenges.contains(&plaintext) {
            match self.rx.open(frame) {
                Ok(_) => {}
                Err(OpenError::Replay) => {
                    self.rx.reset_receive_window();
                    self.rx.open(frame).ok()?;
                }
                Err(_) => return None,
            }
            challenges.insert(plaintext.clone());
        }
        self.establish();
        Some(plaintext)
    }

    /// Record that the challenge handshake completed. Later sends are gated
    /// on this exactly like the retired token-empty checks.
    pub fn establish(&self) {
        self.established.store(true, Ordering::SeqCst);
    }

    pub fn is_established(&self) -> bool {
        self.established.load(Ordering::SeqCst)
    }
}

impl Drop for MediaSessionCrypto {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Shared ownership for one logical session: the prepared listeners, the
/// renderer workers, and the input worker all seal/open through one instance.
pub type SharedMediaCrypto = Arc<MediaSessionCrypto>;

/// Deterministic 32-byte key (bytes..bytes+32) shared by protocol test
/// modules so seal/open vectors stay comparable across crates' tests.
#[cfg(test)]
pub(crate) fn test_media_key(bytes: u8) -> [u8; 32] {
    (bytes..bytes + 32).collect::<Vec<u8>>().try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::test_media_key as key;
    use super::*;

    /// 미디어 방향별 도출 키 — secure-channel의 고정 벡터와 같은 값이어야
    /// 한다(키 분리가 구현 간에 일치하는지 잠금).
    #[test]
    fn derived_direction_keys_match_the_canonical_vector() {
        let keys = secure_channel::media_keys(&key(0));
        let expected_c2s: [u8; 32] = [
            0x56, 0x08, 0xc4, 0xec, 0x91, 0xf0, 0x1a, 0x93, 0xaf, 0xdd, 0x87, 0x6d, 0xa3, 0x41,
            0x9c, 0xaf, 0xd5, 0xfc, 0x68, 0x62, 0xfa, 0xaa, 0x6c, 0x15, 0xa0, 0x08, 0xb1, 0x6d,
            0xab, 0x6a, 0xc7, 0x2f,
        ];
        let expected_s2c: [u8; 32] = [
            0x2c, 0x10, 0x0b, 0x32, 0xa5, 0x07, 0xab, 0x3a, 0xf0, 0x7e, 0xc3, 0xd3, 0x9a, 0x61,
            0xdf, 0x70, 0x72, 0xd1, 0xd9, 0x7e, 0x22, 0xd0, 0xe4, 0x3d, 0xa1, 0x9d, 0x44, 0xfa,
            0xbe, 0xda, 0xf1, 0x82,
        ];
        assert_eq!(keys.c2s, expected_c2s);
        assert_eq!(keys.s2c, expected_s2c);
    }

    #[test]
    fn sealed_challenge_round_trips_between_directions() {
        // Mirror of the real handshake: the host seals "LCH1+nonce" with the
        // derived s2c key; the viewer opens it and echoes the same plaintext
        // under the derived c2s key.
        let keys = secure_channel::media_keys(&key(1));
        let host_tx = DatagramSealer::new(keys.s2c);
        let viewer = MediaSessionCrypto::new(key(1));
        let plaintext = [b"LCH1".as_slice(), b"challenge-nonce"].concat();
        let wire = host_tx.seal(&plaintext).unwrap();
        assert!(!wire.starts_with(CHALLENGE_PREFIX));
        let opened = viewer.open_challenge(&wire).expect("challenge opens");
        assert_eq!(opened, plaintext);
        assert!(
            viewer.is_established(),
            "authenticated preflight enables control sends"
        );
        let echo = viewer.seal(&opened).expect("echo seals");
        let host_rx = DatagramSealer::new(keys.c2s);
        assert_eq!(host_rx.open(&echo).unwrap(), plaintext);
    }

    /// 백엔드 재시작 재시도: 호스트는 같은 미디어 키로 TX 봉인기를 새로 만들어
    /// 무작위 시작 카운터를 다시 뽑고, 뷰어의 수신 워터마크는 첫 시도 구간에
    /// 올라가 있다. 유효하게 봉인된 LCH1 챌린지가 재시작의 증거가 되어 창을
    /// 초기화하고, LCH1이 아닌 프레임은 절대 창을 초기화하지 않음을 잠근다.
    #[test]
    fn restarted_host_challenge_below_watermark_resets_the_receive_window() {
        let viewer = MediaSessionCrypto::new(key(9));
        let keys = secure_channel::media_keys(&key(9));
        // 두 개의 독립 TX 인스턴스(첫 시도 / 재시작)를 만들고, 카운터가 큰 쪽을
        // 첫 시도로 역할을 고정해 "재시작이 워터마크 아래에서 시작"하게 만든다.
        let (first, restarted) = (DatagramSealer::new(keys.s2c), DatagramSealer::new(keys.s2c));
        let first_plaintext = [CHALLENGE_PREFIX, b"first-nonce".as_slice()].concat();
        let retry_plaintext = [CHALLENGE_PREFIX, b"retry-nonce".as_slice()].concat();
        let frames = |sealer: &DatagramSealer, challenge: &[u8]| {
            (
                sealer.seal(challenge).unwrap(),
                sealer.seal(b"media").unwrap(),
            )
        };
        let (first_challenge, first_media) = frames(&first, &first_plaintext);
        let (retry_challenge, retry_media) = frames(&restarted, &retry_plaintext);
        let first_is_high = secure_channel::frame_counter(&first_media).unwrap()
            > secure_channel::frame_counter(&retry_media).unwrap();
        let (high_challenge, high_plaintext, low_challenge, low_plaintext, low_media) =
            if first_is_high {
                (
                    first_challenge,
                    first_plaintext,
                    retry_challenge,
                    retry_plaintext,
                    retry_media,
                )
            } else {
                (
                    retry_challenge,
                    retry_plaintext,
                    first_challenge,
                    first_plaintext,
                    first_media,
                )
            };

        // 첫 시도의 챌린지가 워터마크를 올린다.
        assert_eq!(
            viewer.open_challenge(&high_challenge).as_deref(),
            Some(high_plaintext.as_slice())
        );
        // 재시작 인스턴스의 미디어는 워터마크 아래라 창 경로에서 거부되고,
        // LCH1이 아니므로 창을 초기화하지도 않는다.
        assert_eq!(viewer.open(&low_media), None);
        assert_eq!(viewer.open_challenge(&low_media), None);
        assert_eq!(
            viewer.open(&low_media),
            None,
            "a non-challenge never resets the window"
        );
        // 재시작한 호스트의 유효한 LCH1은 재시작 증거 — 창을 초기화하고 받는다.
        assert_eq!(
            viewer.open_challenge(&low_challenge).as_deref(),
            Some(low_plaintext.as_slice())
        );
        // 초기화된 창 위로 재시작 인스턴스의 미디어가 흐르고, 재생 방지는 그대로다.
        assert_eq!(viewer.open(&low_media).as_deref(), Some(&b"media"[..]));
        assert_eq!(viewer.open(&low_media), None);
    }

    #[test]
    fn foreign_key_and_garbage_never_open() {
        let viewer = MediaSessionCrypto::new(key(2));
        let impostor = DatagramSealer::new(key(3));
        let sealed = impostor.seal(b"LCH1forged").unwrap();
        assert_eq!(viewer.open(&sealed), None);
        assert_eq!(viewer.open(b"not-sealed"), None);
        assert_eq!(viewer.open(&[]), None);
        assert_eq!(viewer.open_challenge(&sealed), None);
        assert!(!viewer.is_established());
    }

    /// 원본 세션 키를 그대로 쓰는 구세대 봉인기는 더 이상 열리지 않는다 —
    /// 방향 키 도출이 강제됐음을 고정한다.
    #[test]
    fn raw_session_key_sealers_no_longer_interoperate() {
        let viewer = MediaSessionCrypto::new(key(7));
        let legacy = DatagramSealer::new(key(7));
        let sealed = legacy.seal(b"LCH1legacy").unwrap();
        assert_eq!(viewer.open(&sealed), None);
    }

    #[test]
    fn established_gate_tracks_the_challenge_handshake() {
        let viewer = MediaSessionCrypto::new(key(4));
        let host_tx = DatagramSealer::new(secure_channel::media_keys(&key(4)).s2c);
        let challenge = host_tx.seal(b"LCH1authenticated-nonce").unwrap();
        let mut forged = challenge.clone();
        *forged.last_mut().unwrap() ^= 1;
        let oversized = host_tx
            .seal(&[CHALLENGE_PREFIX, &[b'x'; MAX_CHALLENGE_BYTES][..]].concat())
            .unwrap();
        assert!(!viewer.is_established());
        for invalid in [
            &forged[..],
            &challenge[..12],
            b"LCH1plaintext-forgery",
            &[],
            &oversized,
        ] {
            assert_eq!(viewer.open_challenge(invalid), None);
            assert!(
                !viewer.is_established(),
                "only an authenticated challenge establishes"
            );
        }
        assert_eq!(
            viewer.open_challenge(&challenge).as_deref(),
            Some(&b"LCH1authenticated-nonce"[..])
        );
        assert!(viewer.is_established());
    }

    #[test]
    fn challenge_classification_preserves_media_for_the_renderer() {
        let viewer = MediaSessionCrypto::new(key(10));
        let host_tx = DatagramSealer::new(secure_channel::media_keys(&key(10)).s2c);
        let media = host_tx.seal(b"CFGmedia-configuration").unwrap();
        assert_eq!(viewer.open_challenge(&media), None);
        assert!(!viewer.is_established());
        assert_eq!(
            viewer.open(&media).as_deref(),
            Some(&b"CFGmedia-configuration"[..])
        );
        assert_eq!(viewer.open_challenge(&media), None);
        assert_eq!(
            viewer.open(&media),
            None,
            "classification cannot revive received media"
        );
    }

    #[test]
    fn repeated_challenges_do_not_revive_received_media() {
        let viewer = MediaSessionCrypto::new(key(11));
        let host_tx = DatagramSealer::new(secure_channel::media_keys(&key(11)).s2c);
        let first = host_tx.seal(b"LCH1first-nonce").unwrap();
        assert!(viewer.open_challenge(&first).is_some());
        let media = host_tx.seal(b"media").unwrap();
        assert_eq!(viewer.open(&media).as_deref(), Some(&b"media"[..]));
        assert!(
            viewer.open_challenge(&first).is_some(),
            "a lost echo may be retried"
        );
        assert_eq!(
            viewer.open(&media),
            None,
            "duplicate challenge cannot reset replay state"
        );
        let resealed_first = host_tx.seal(b"LCH1first-nonce").unwrap();
        assert!(viewer.open_challenge(&resealed_first).is_some());
        assert_eq!(
            viewer.open(&media),
            None,
            "a freshly sealed retry cannot reset replay state"
        );

        let second = host_tx.seal(b"LCH1second-nonce").unwrap();
        assert!(viewer.open_challenge(&second).is_some());
        let next_media = host_tx.seal(b"next-media").unwrap();
        assert_eq!(
            viewer.open(&next_media).as_deref(),
            Some(&b"next-media"[..])
        );
        assert!(viewer.open_challenge(&first).is_some());
        assert_eq!(
            viewer.open(&next_media),
            None,
            "an older challenge cannot reset replay state"
        );
    }

    #[test]
    fn active_single_renderer_records_challenge_before_suspend_retry() {
        assert_active_challenge_survives_suspend(true);
    }

    #[test]
    fn active_split_renderer_records_challenge_before_suspend_retry() {
        assert_active_challenge_survives_suspend(false);
    }

    fn assert_active_challenge_survives_suspend(in_place: bool) {
        // Single uses open_into; split uses open. Both then share their same
        // crypto with a suspended worker that can echo a challenge retry.
        let viewer = Arc::new(MediaSessionCrypto::new(key(13)));
        let host_tx = DatagramSealer::new(secure_channel::media_keys(&key(13)).s2c);
        let challenge = host_tx.seal(b"LCH1active-session-nonce").unwrap();
        if in_place {
            let mut packet = challenge.clone();
            assert_eq!(
                viewer.open_into(&mut packet).unwrap(),
                b"LCH1active-session-nonce"
            );
        } else {
            assert_eq!(
                viewer.open(&challenge).unwrap(),
                b"LCH1active-session-nonce"
            );
        }
        assert!(
            viewer.is_established(),
            "the common open path records the challenge"
        );
        let media = host_tx.seal(b"active-media").unwrap();
        assert_eq!(viewer.open(&media).as_deref(), Some(&b"active-media"[..]));
        let suspended = Arc::clone(&viewer);
        assert!(suspended.open_challenge(&challenge).is_some());
        assert_eq!(
                viewer.open(&media),
                None,
                "an active challenge retried while suspended must preserve replay protection (in_place={in_place})"
            );
    }

    /// 두 논리 세션(좌·우 타일 등)이 같은 키의 독립 인스턴스를 써도 각자의
    /// 방향 키와 무작위 시작 카운터 덕에 (key, nonce) 가 겹치지 않는다.
    #[test]
    fn independent_instances_never_share_a_counter_range() {
        let a = MediaSessionCrypto::new(key(5));
        let b = MediaSessionCrypto::new(key(5));
        let frame_a = a.seal(b"from-a").unwrap();
        let frame_b = b.seal(b"from-b").unwrap();
        assert_ne!(
            secure_channel::frame_counter(&frame_a).unwrap(),
            secure_channel::frame_counter(&frame_b).unwrap()
        );
    }

    #[test]
    fn replay_is_rejected_by_the_incoming_direction() {
        let viewer = MediaSessionCrypto::new(key(6));
        let keys = secure_channel::media_keys(&key(6));
        // 수신 방향(host → viewer)은 s2c 키로 봉인된 프레임이다.
        let host_tx = DatagramSealer::new(keys.s2c);
        let sealed = host_tx.seal(b"payload").unwrap();
        assert_eq!(viewer.open(&sealed), Some(b"payload".to_vec()));
        // Replaying the same frame is rejected exactly like the raw sealer's
        // sliding window rejects it.
        assert_eq!(viewer.open(&sealed), None);
        assert_eq!(
            host_tx
                .seal(b"payload")
                .ok()
                .and_then(|frame| viewer.open(&frame)),
            Some(b"payload".to_vec()),
            "a fresh counter is not a replay"
        );
    }
}
