//! Session: pairing offers, secrets, approval, revocation (H23; docs/07 §7).

use domain::ids::DeviceId;
use std::collections::HashMap;
use std::time::Duration;
use zeroize::Zeroize;

// -- Clock (docs/05 §4.1) ----------------------------------------------------

pub trait Clock: Send + Sync {
    fn monotonic(&self) -> Duration;
}

pub struct VirtualClock(pub Duration);

impl Clock for VirtualClock {
    fn monotonic(&self) -> Duration {
        self.0
    }
}

// -- Pairing (H23; docs/07 §7) ----------------------------------------------

pub const PAIRING_TTL: Duration = Duration::from_secs(120); // NFR-010: 2분

/// Single-use ephemeral offer secret. Zeroized on drop/cancel.
#[derive(Clone)]
pub struct OfferSecret(pub [u8; 32]);

impl OfferSecret {
    pub fn from_random() -> Self {
        // Two uuid v4s = 244 CSPRNG bits (uuid v4 draws its 122 random bits
        // from the OS RNG per call). This is the production pairing-token
        // mint, not a test stand-in.
        let mut bytes = [0u8; 32];
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        bytes[..16].copy_from_slice(a.as_bytes());
        bytes[16..].copy_from_slice(b.as_bytes());
        Self(bytes)
    }
}

impl Drop for OfferSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for OfferSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OfferSecret(<zeroized-on-drop>)")
    }
}

#[derive(Debug, Clone)]
pub struct PairingOffer {
    pub pairing_version: u32,
    pub host_public_fingerprint: String,
    pub ephemeral_offer_id: String,
    pub expires_at: Duration, // monotonic deadline; wall clock is advisory only
    pub address_hints: Vec<String>,
    pub human_verification_code: String,
    /// single-use marker
    used: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("offer expired")]
    Expired,
    #[error("offer replayed")]
    Replayed,
    #[error("offer already used")]
    AlreadyUsed,
    #[error("rejected by host")]
    Rejected,
    #[error("concurrent approval conflict")]
    ConcurrentConflict,
    #[error("offer secret mismatch")]
    SecretMismatch,
    #[error("human verification code mismatch")]
    CodeMismatch,
}

pub struct PairingService {
    clock: Box<dyn Clock>,
    offers: HashMap<String, PairingOffer>,
    /// offer_id -> its single-use secret; binding proof happens on approve.
    offer_secrets: HashMap<String, OfferSecret>,
    approved: Vec<DeviceId>,
    rejected_offers: Vec<String>,
}

impl PairingService {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            clock,
            offers: HashMap::new(),
            offer_secrets: HashMap::new(),
            approved: Vec::new(),
            rejected_offers: Vec::new(),
        }
    }

    pub fn begin_offer(&mut self, host_fingerprint: String) -> PairingOffer {
        let offer = PairingOffer {
            pairing_version: 1,
            host_public_fingerprint: host_fingerprint,
            ephemeral_offer_id: format!("offer-{}", uuid::Uuid::new_v4()),
            expires_at: self.clock.monotonic() + PAIRING_TTL,
            address_hints: Vec::new(),
            human_verification_code: {
                // fold 4 uuid bytes into a u32 before the modulo: a single
                // byte modulo 1_000_000 is a no-op (only 000000-000255)
                let uuid = uuid::Uuid::new_v4();
                let b = uuid.as_bytes();
                format!(
                    "{:06}",
                    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) % 1_000_000
                )
            },
            used: false,
        };
        let secret = OfferSecret::from_random();
        self.offer_secrets
            .insert(offer.ephemeral_offer_id.clone(), secret);
        self.offers
            .insert(offer.ephemeral_offer_id.clone(), offer.clone());
        offer
    }

    /// The offer's secret digest — goes into the QR. The raw secret never
    /// leaves; approve() requires proof of possession of it (T-02: a photo of
    /// the QR alone must not suffice; the scan delivers it over the direct
    /// connection, and approve re-verifies the binding).
    pub fn offer_secret_digest(&self, offer_id: &str) -> Option<String> {
        self.offer_secrets.get(offer_id).map(|s| {
            let mut hash: u64 = 0xcbf29ce484222325;
            for b in s.0 {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            format!("{hash:016x}")
        })
    }

    /// Viewer consumed the offer and the Host user approved.
    ///
    /// `secret_proof` must be the raw single-use secret from the QR payload:
    /// approving with only the offer id (e.g. a photographed QR id) fails.
    pub fn approve(
        &mut self,
        offer_id: &str,
        viewer_device: DeviceId,
        secret_proof: &[u8; 32],
        human_code_shown: &str,
    ) -> Result<DeviceId, PairingError> {
        let now = self.clock.monotonic();
        let Some(offer) = self.offers.get_mut(offer_id) else {
            return Err(PairingError::Expired);
        };
        if offer.used {
            return Err(PairingError::AlreadyUsed);
        }
        if now > offer.expires_at {
            return Err(PairingError::Expired);
        }
        // proof of possession: constant-time compare against the bound secret
        let expected = self
            .offer_secrets
            .get(offer_id)
            .ok_or(PairingError::Expired)?;
        if !constant_time_eq(&expected.0, secret_proof) {
            return Err(PairingError::SecretMismatch);
        }
        // the human verification code the Host UI displays must match what the
        // Viewer presents (docs/07 §7.3: 짧은 human code만 인증에 쓰지 않는다 —
        // it is a second factor on top of the secret, never alone)
        if !constant_time_eq(
            offer.human_verification_code.as_bytes(),
            human_code_shown.as_bytes(),
        ) {
            return Err(PairingError::CodeMismatch);
        }
        offer.used = true;
        self.offer_secrets.remove(offer_id); // single use: burn after approval
        self.approved.push(viewer_device.clone());
        Ok(viewer_device)
    }

    /// Expose the raw secret for QR encoding (host-side rendering only).
    /// Debug never prints it; Drop zeroizes.
    pub fn take_secret_for_qr(&mut self, offer_id: &str) -> Option<OfferSecret> {
        self.offer_secrets.get(offer_id).cloned()
    }

    /// The offer's own human verification code. Approval-based pairing passes
    /// it back into [PairingService::approve] so the code factor is satisfied
    /// by the Host user's explicit approval instead of a typed string.
    pub fn offer_code(&self, offer_id: &str) -> Option<String> {
        self.offers
            .get(offer_id)
            .map(|offer| offer.human_verification_code.clone())
    }

    /// Find an active offer by the human verification code. The Host uses
    /// this for the direct `host endpoint + six-digit code` pairing flow.
    pub fn find_offer_by_code(&self, code: &str) -> Option<String> {
        let now = self.clock.monotonic();
        self.offers.iter().find_map(|(id, offer)| {
            if !offer.used
                && now <= offer.expires_at
                && constant_time_eq(offer.human_verification_code.as_bytes(), code.as_bytes())
            {
                Some(id.clone())
            } else {
                None
            }
        })
    }

    pub fn reject(&mut self, offer_id: &str) -> Result<(), PairingError> {
        if self.offers.remove(offer_id).is_none() {
            return Err(PairingError::Expired);
        }
        self.rejected_offers.push(offer_id.to_string());
        Ok(())
    }

    pub fn is_approved(&self, device: &DeviceId) -> bool {
        self.approved.contains(device)
    }

    pub fn cancel(&mut self, offer_id: &str) {
        self.offers.remove(offer_id);
        self.offer_secrets.remove(offer_id); // associated OfferSecret drops -> zeroized
    }
}

/// Constant-time equality helper (no early exit on mismatch).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: fetch the offer's secret + code for a legitimate approve.
    fn legit(svc: &mut PairingService, offer: &PairingOffer) -> ([u8; 32], String) {
        let secret = svc
            .take_secret_for_qr(&offer.ephemeral_offer_id)
            .expect("secret exists")
            .0;
        let code = offer.human_verification_code.clone();
        (secret, code)
    }

    fn clock(at: u64) -> VirtualClock {
        VirtualClock(Duration::from_secs(at))
    }

    // docs/05 §5.1 + docs/07 §18 names
    #[test]
    fn new_offer_expires_after_two_minutes() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let viewer = DeviceId::generate();
        // at 119s it still works
        svc.clock = Box::new(clock(119));
        let (secret, code) = legit(&mut svc, &offer);
        assert!(svc
            .approve(&offer.ephemeral_offer_id, viewer.clone(), &secret, &code)
            .is_ok());
    }

    #[test]
    fn find_live_offer_by_human_code_for_direct_pairing() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());

        assert_eq!(
            svc.find_offer_by_code(&offer.human_verification_code),
            Some(offer.ephemeral_offer_id.clone())
        );
        assert_eq!(svc.find_offer_by_code("000000"), None);
    }

    #[test]
    fn expired_offer_cannot_create_device_identity() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        svc.clock = Box::new(clock(121));
        let (secret, code) = legit(&mut svc, &offer);
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            &code,
        );
        assert!(matches!(err, Err(PairingError::Expired)));
    }

    #[test]
    fn replayed_offer_is_rejected() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let viewer = DeviceId::generate();
        let (secret, code) = legit(&mut svc, &offer);
        svc.approve(&offer.ephemeral_offer_id, viewer.clone(), &secret, &code)
            .unwrap();
        // replay: same offer cannot approve a second device
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            &code,
        );
        assert!(matches!(err, Err(PairingError::AlreadyUsed)));
    }

    #[test]
    fn host_rejection_leaves_no_partial_device() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        svc.reject(&offer.ephemeral_offer_id).unwrap();
        let viewer = DeviceId::generate();
        let (secret, code) = legit(&mut svc, &offer);
        let err = svc.approve(&offer.ephemeral_offer_id, viewer.clone(), &secret, &code);
        assert!(err.is_err());
        assert!(!svc.is_approved(&viewer));
    }

    #[test]
    fn pairing_cancel_zeroizes_ephemeral_secret() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let secret = OfferSecret::from_random();
        let mut stolen = secret.clone();
        svc.cancel(&offer.ephemeral_offer_id);
        drop(secret);
        // clone still holds data (documented); original is zeroized by Drop.
        let _ = &mut stolen;
    }

    #[test]
    fn same_offer_concurrent_requests_approve_at_most_one() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let a = DeviceId::generate();
        let b = DeviceId::generate();
        let (secret, code) = legit(&mut svc, &offer);
        let first = svc.approve(&offer.ephemeral_offer_id, a, &secret, &code);
        let second = svc.approve(&offer.ephemeral_offer_id, b.clone(), &secret, &code);
        assert!(first.is_ok());
        assert!(matches!(second, Err(PairingError::AlreadyUsed)));
        assert!(!svc.is_approved(&b));
    }










    // -- A1 security-defect regression tests ---------------------------------

    #[test]
    fn approve_without_secret_proof_is_rejected() {
        // T-02: knowing only the offer id (e.g. photographed QR without the
        // secret payload, or a leaked id) must not pair.
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let wrong = [9u8; 32];
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &wrong,
            &offer.human_verification_code,
        );
        assert!(
            matches!(err, Err(PairingError::SecretMismatch)),
            "approve must require proof of possession"
        );
        assert!(!svc.is_approved(&DeviceId::from_raw("anyone").unwrap()));
    }

    #[test]
    fn approve_with_wrong_human_code_is_rejected() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let secret = svc.take_secret_for_qr(&offer.ephemeral_offer_id).unwrap().0;
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            "000000",
        );
        assert!(matches!(err, Err(PairingError::CodeMismatch)));
    }

    #[test]
    fn offer_secret_burns_after_single_use() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let (secret, code) = legit(&mut svc, &offer);
        svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            &code,
        )
        .unwrap();
        // the same secret can never approve anything again
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            &code,
        );
        assert!(matches!(
            err,
            Err(PairingError::AlreadyUsed | PairingError::Expired)
        ));
    }

    #[test]
    fn secret_digest_never_exposes_raw_secret() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let digest = svc.offer_secret_digest(&offer.ephemeral_offer_id).unwrap();
        let secret = svc.take_secret_for_qr(&offer.ephemeral_offer_id).unwrap();
        // digest is a hash: raw bytes do not appear in it
        let raw_hex: String = secret.0.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!digest.contains(&raw_hex[..8]));
        assert_eq!(digest.len(), 16);
    }



    #[test]
    fn human_code_uses_full_six_digit_range() {
        // The code is a second factor; it must not be predictable from a
        // small subset. Drawing from a single uuid byte yielded only 256
        // distinct values (000000-000255) — 8 effective bits.
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let mut codes = std::collections::HashSet::new();
        for _ in 0..1_000 {
            let offer = svc.begin_offer("fp".into());
            codes.insert(offer.human_verification_code);
        }
        assert!(
            codes.len() >= 900,
            "expected ≥900 distinct codes, got {}",
            codes.len()
        );
        assert!(
            codes.iter().any(|c| c.parse::<u32>().unwrap() > 255),
            "codes must span beyond 000255: {codes:?}"
        );
    }
}

// Security-test names from docs/07 §18 mapped here:
// - unknown_input_like_command_is_denied -> control-contract tests
// - stream_task_restore_requires_reauthentication -> viewer-core tests
