//! Media-path AEAD sealing for every viewer ↔ host datagram.
//!
//! The viewer generates a 32-byte session key (JS `randomBytes(32)`), hands
//! it to the native prepare calls and sends the same key to the host through
//! the already-encrypted control plane. Both directions use the very same
//! key with no derivation: the two send counters are independent sequences
//! and the Poly1305 tag binds the ciphertext content, so a cross-direction
//! nonce collision is impossible. Possession of the key replaces the legacy
//! plaintext LCH1 challenge-token suffix authentication; the sealed
//! `LCH1 ‖ nonce` challenge keeps only its reachability/NAT-pinning role.
//!
//! Wire layout (crates/secure-channel): `counter u64 BE ‖ poly1305 tag 16B
//! ‖ ct`, nonce `00{4} ‖ counter u64 BE`.

use secure_channel::DatagramSealer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Plaintext prefix of the sealed reachability challenge.
pub const CHALLENGE_PREFIX: &[u8] = b"LCH1";

pub struct MediaSessionCrypto {
    /// 세션 키 사본 — [MediaSessionCrypto::session_key]가 재바인드 폴백에 쓴다.
    key: [u8; 32],
    /// Outgoing direction (viewer → host).
    tx: DatagramSealer,
    /// Incoming direction (host → viewer).
    rx: DatagramSealer,
    /// Set once the sealed LCH1 challenge was observed, mirroring the old
    /// "session token known" gate for IDR/input/feedback sends.
    established: AtomicBool,
}

impl MediaSessionCrypto {
    /// 등록 시 쓴 세션 키 사본 — 재바인드 폴백이 같은 키로 재등록할 때 쓴다.
    pub fn session_key(&self) -> [u8; 32] {
        self.key
    }

    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            tx: DatagramSealer::new(key),
            rx: DatagramSealer::new(key),
            established: AtomicBool::new(false),
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
        self.rx.open(frame).ok()
    }

    /// Recognize a sealed host `LCH1` reachability challenge. Returns the
    /// challenge plaintext when `frame` opens and starts with `LCH1`.
    pub fn open_challenge(&self, frame: &[u8]) -> Option<Vec<u8>> {
        let plaintext = self.open(frame)?;
        plaintext.starts_with(CHALLENGE_PREFIX).then_some(plaintext)
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

/// Shared ownership for one logical session: the prepared listeners, the
/// renderer workers, and the input worker all seal/open through one instance.
pub type SharedMediaCrypto = Arc<MediaSessionCrypto>;

#[cfg(test)]
mod tests {
    use super::*;

    fn key(bytes: u8) -> [u8; 32] {
        (bytes..bytes + 32).collect::<Vec<u8>>().try_into().unwrap()
    }

    #[test]
    fn sealed_challenge_round_trips_between_directions() {
        // Mirror of the real handshake: the host seals "LCH1+nonce" with the
        // shared key; the viewer opens it and echoes the same plaintext.
        let host_tx = DatagramSealer::new(key(1));
        let viewer = MediaSessionCrypto::new(key(1));
        let plaintext = [b"LCH1".as_slice(), b"challenge-nonce"].concat();
        let wire = host_tx.seal(&plaintext).unwrap();
        assert!(!wire.starts_with(CHALLENGE_PREFIX));
        let opened = viewer.open_challenge(&wire).expect("challenge opens");
        assert_eq!(opened, plaintext);
        let echo = viewer.seal(&opened).expect("echo seals");
        let mut host_rx = DatagramSealer::new(key(1));
        assert_eq!(host_rx.open(&echo).unwrap(), plaintext);
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
    }

    #[test]
    fn established_gate_tracks_the_challenge_handshake() {
        let viewer = MediaSessionCrypto::new(key(4));
        assert!(!viewer.is_established());
        viewer.establish();
        assert!(viewer.is_established());
    }

    #[test]
    fn counters_survive_across_directions_without_collision() {
        // Both directions share the key; each direction's counter sequence is
        // independent, so both sides may use counter 1 concurrently.
        let a = MediaSessionCrypto::new(key(5));
        let b = MediaSessionCrypto::new(key(5));
        let frame_a = a.seal(b"from-a").unwrap();
        let frame_b = b.seal(b"from-b").unwrap();
        assert_eq!(a.open(&frame_b).unwrap(), b"from-b");
        assert_eq!(b.open(&frame_a).unwrap(), b"from-a");
    }

    #[test]
    fn replay_is_rejected_by_the_incoming_direction() {
        let viewer = MediaSessionCrypto::new(key(6));
        let tx = DatagramSealer::new(key(6));
        let sealed = tx.seal(b"payload").unwrap();
        assert_eq!(viewer.open(&sealed), Some(b"payload".to_vec()));
        // Replaying the same frame is rejected exactly like the raw sealer's
        // sliding window rejects it.
        assert_eq!(viewer.open(&sealed), None);
        assert_eq!(
            tx.seal(b"payload").ok().and_then(|frame| viewer.open(&frame)),
            Some(b"payload".to_vec()),
            "a fresh counter is not a replay"
        );
    }
}
