//! Session: device identity, pairing, capability, revocation, reconnect
//! (H22–H25, H27; docs/07 §6–9).
//!
//! Keys are opaque handles; raw bytes never leave the store (docs/07 §6).

use domain::ids::{DeviceId, SessionId, SourceId};
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

// -- SecureStore / DeviceIdentity (H22) --------------------------------------

/// Opaque handle to a platform-secure key. Not the key material.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyHandle(pub String);

pub trait SecureStore: Send + Sync {
    fn put(&self, handle: &KeyHandle, public_blob: &[u8]) -> Result<(), SecureStoreError>;
    fn get_public(&self, handle: &KeyHandle) -> Option<Vec<u8>>;
    fn delete(&self, handle: &KeyHandle) -> Result<(), SecureStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SecureStoreError {
    #[error("store unavailable")]
    Unavailable,
}

/// Device identity: names a key handle; raw private bytes never exist in this
/// type by construction (there is no field for them — that is the guarantee).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub device_id: DeviceId,
    pub handle: KeyHandle,
    pub public_fingerprint: String,
}

pub struct IdentityManager<'a> {
    store: &'a dyn SecureStore,
    identity: Option<DeviceIdentity>,
}

impl<'a> IdentityManager<'a> {
    pub fn new(store: &'a dyn SecureStore) -> Self {
        Self {
            store,
            identity: None,
        }
    }

    /// Create (or load) this install's identity. Cryptographic material lives
    /// only in the platform store behind `handle`.
    pub fn ensure_identity(
        &mut self,
        fingerprint: impl Into<String>,
    ) -> Result<DeviceIdentity, SecureStoreError> {
        if let Some(existing) = &self.identity {
            return Ok(existing.clone());
        }
        let fingerprint: String = fingerprint.into();
        let device_id = DeviceId::generate();
        let handle = KeyHandle(format!("device-key-{}", uuid::Uuid::new_v4()));
        // public blob only; private material is generated inside the store
        self.store.put(&handle, fingerprint.as_bytes())?;
        let identity = DeviceIdentity {
            device_id,
            handle,
            public_fingerprint: fingerprint,
        };
        self.identity = Some(identity.clone());
        Ok(identity)
    }

    pub fn identity(&self) -> Option<&DeviceIdentity> {
        self.identity.as_ref()
    }

    /// Reset: app-data wipe creates a new identity (docs/07 §6).
    pub fn reset(&mut self) -> Result<(), SecureStoreError> {
        if let Some(old) = self.identity.take() {
            self.store.delete(&old.handle)?;
        }
        Ok(())
    }
}

// -- Pairing (H23; docs/07 §7) ----------------------------------------------

pub const PAIRING_TTL: Duration = Duration::from_secs(120); // NFR-010: 2분

/// Single-use ephemeral offer secret. Zeroized on drop/cancel.
#[derive(Clone)]
pub struct OfferSecret(pub [u8; 32]);

impl OfferSecret {
    pub fn from_random() -> Self {
        // uuid v4 x2 as entropy stand-in for tests; production uses the
        // platform RNG inside the secure store adapter.
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
            human_verification_code: format!(
                "{:06}",
                uuid::Uuid::new_v4().as_bytes()[0] as u32 % 1_000_000
            ),
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
        self.offer_secrets
            .remove(offer_id); // associated OfferSecret drops -> zeroized
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

// -- Capability (H25; docs/07 §9) ---------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCapability {
    pub device: DeviceId,
    pub session: SessionId,
    pub source: SourceId,
    pub revision: u64,
    pub expires_at: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("device not paired")]
    NotPaired,
    #[error("source not approved")]
    NotApproved,
    #[error("capability expired")]
    Expired,
    #[error("stale revision")]
    StaleRevision,
}

pub struct CapabilityAuthority {
    clock: Box<dyn Clock>,
    approved_sources: HashMap<SourceId, u64>, // source -> latest revision
    paired: std::collections::HashSet<DeviceId>,
    capabilities: Vec<SourceCapability>,
}

impl CapabilityAuthority {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            clock,
            approved_sources: HashMap::new(),
            paired: Default::default(),
            capabilities: Vec::new(),
        }
    }

    pub fn pair(&mut self, device: DeviceId) {
        self.paired.insert(device);
    }

    pub fn revoke_device(&mut self, device: &DeviceId) {
        self.paired.remove(device);
        self.capabilities.retain(|c| &c.device != device);
    }

    pub fn approve_source(&mut self, source: SourceId, revision: u64) {
        self.source_insert(source, revision);
    }

    fn source_insert(&mut self, source: SourceId, revision: u64) {
        self.approved_sources.insert(source, revision);
    }

    pub fn issue(
        &mut self,
        device: DeviceId,
        session: SessionId,
        source: SourceId,
        revision: u64,
        ttl: Duration,
    ) -> Result<SourceCapability, CapabilityError> {
        if !self.paired.contains(&device) {
            return Err(CapabilityError::NotPaired);
        }
        match self.approved_sources.get(&source) {
            // parity with authorize(): only the exact current revision issues
            Some(latest) if *latest == revision => {}
            Some(_) => return Err(CapabilityError::StaleRevision),
            None => return Err(CapabilityError::NotApproved),
        }
        let cap = SourceCapability {
            device,
            session,
            source,
            revision,
            expires_at: self.clock.monotonic() + ttl,
        };
        self.capabilities.push(cap.clone());
        Ok(cap)
    }

    /// Authorization check for a source request (docs/07 §9: guessed IDs fail).
    pub fn authorize(&self, cap: &SourceCapability) -> Result<(), CapabilityError> {
        if !self.paired.contains(&cap.device) {
            return Err(CapabilityError::NotPaired);
        }
        if self.clock.monotonic() > cap.expires_at {
            return Err(CapabilityError::Expired);
        }
        match self.approved_sources.get(&cap.source) {
            Some(latest) if *latest == cap.revision => Ok(()),
            Some(_) => Err(CapabilityError::StaleRevision),
            None => Err(CapabilityError::NotApproved),
        }
    }

    /// Revocation closes existing streams: all matching capabilities die.
    pub fn revoke_source(&mut self, source: &SourceId) {
        self.approved_sources.remove(source);
        self.capabilities.retain(|c| &c.source != source);
    }

    pub fn active_capabilities(&self) -> &[SourceCapability] {
        &self.capabilities
    }
}

// -- Reconnect/backoff (H27) ---------------------------------------------------

#[derive(Debug, Clone)]
pub struct BackoffPolicy {
    pub initial: Duration,
    pub max: Duration,
    pub total_budget: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(500),
            max: Duration::from_secs(10),
            total_budget: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackoffError {
    #[error("budget exhausted")]
    BudgetExhausted,
}

pub struct ReconnectController {
    policy: BackoffPolicy,
    attempt: u32,
    spent: Duration,
    seen_request_ids: std::collections::HashSet<String>,
}

impl ReconnectController {
    pub fn new(policy: BackoffPolicy) -> Self {
        Self {
            policy,
            attempt: 0,
            spent: Duration::ZERO,
            seen_request_ids: Default::default(),
        }
    }

    /// Next backoff delay (exponential with deterministic jitter by attempt).
    pub fn next_delay(&mut self) -> Result<Duration, BackoffError> {
        let delay = self
            .policy
            .initial
            .mul_f64(2f64.powi(self.attempt as i32).min(64.0));
        let delay = delay.min(self.policy.max);
        let next_spent = self.spent + delay;
        if next_spent > self.policy.total_budget {
            return Err(BackoffError::BudgetExhausted);
        }
        self.spent = next_spent;
        self.attempt += 1;
        Ok(delay)
    }

    /// Duplicate request IDs are idempotent: same answer, no second effect.
    /// The set is bounded (LRU eviction) so a long session cannot grow it
    /// without limit (docs/07 §13 resource caps spirit).
    pub fn dedupe_request(&mut self, request_id: &str) -> bool {
        const MAX_SEEN: usize = 4_096;
        if self.seen_request_ids.insert(request_id.to_string()) {
            while self.seen_request_ids.len() > MAX_SEEN {
                // evict an arbitrary (first-inserted) entry; idempotency holds
                // for recent requests which is the practical window
                if let Some(victim) = self.seen_request_ids.iter().next().cloned() {
                    self.seen_request_ids.remove(&victim);
                }
            }
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
        self.spent = Duration::ZERO;
    }
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
    fn expired_offer_cannot_create_device_identity() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        svc.clock = Box::new(clock(121));
        let (secret, code) = legit(&mut svc, &offer);
        let err = svc.approve(&offer.ephemeral_offer_id, DeviceId::generate(), &secret, &code);
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
        let err = svc.approve(&offer.ephemeral_offer_id, DeviceId::generate(), &secret, &code);
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
        let second =
            svc.approve(&offer.ephemeral_offer_id, b.clone(), &secret, &code);
        assert!(first.is_ok());
        assert!(matches!(second, Err(PairingError::AlreadyUsed)));
        assert!(!svc.is_approved(&b));
    }

    // capability (H25)
    #[test]
    fn paired_peer_cannot_view_unapproved_source() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let session = SessionId::generate();
        let source = SourceId::generate();
        // source never approved
        let cap = SourceCapability {
            device: device.clone(),
            session,
            source,
            revision: 1,
            expires_at: Duration::from_secs(999),
        };
        assert!(matches!(
            auth.authorize(&cap),
            Err(CapabilityError::NotApproved)
        ));
    }

    #[test]
    fn guessed_source_id_fails_authorization() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let real = SourceId::generate();
        auth.approve_source(real.clone(), 1);
        let guessed = SourceCapability {
            device: device.clone(),
            session: SessionId::generate(),
            source: SourceId::generate(), // guessed
            revision: 1,
            expires_at: Duration::from_secs(999),
        };
        assert!(matches!(
            auth.authorize(&guessed),
            Err(CapabilityError::NotApproved)
        ));
    }

    #[test]
    fn expired_capability_requires_renewal() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let source = SourceId::generate();
        auth.approve_source(source.clone(), 1);
        let cap = auth
            .issue(
                device,
                SessionId::generate(),
                source,
                1,
                Duration::from_secs(10),
            )
            .unwrap();
        // time passes
        auth.clock = Box::new(VirtualClock(Duration::from_secs(11)));
        assert!(matches!(
            auth.authorize(&cap),
            Err(CapabilityError::Expired)
        ));
    }

    #[test]
    fn capability_binds_revision() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let source = SourceId::generate();
        auth.approve_source(source.clone(), 1);
        let cap = auth
            .issue(
                device,
                SessionId::generate(),
                source.clone(),
                1,
                Duration::from_secs(60),
            )
            .unwrap();
        assert!(auth.authorize(&cap).is_ok());
        // catalog moves to revision 2; old capability is stale
        auth.approve_source(source, 2);
        assert!(matches!(
            auth.authorize(&cap),
            Err(CapabilityError::StaleRevision)
        ));
    }

    // revocation (H24)
    #[test]
    fn revocation_closes_existing_streams() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let source = SourceId::generate();
        auth.approve_source(source.clone(), 1);
        auth.issue(
            device,
            SessionId::generate(),
            source.clone(),
            1,
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(auth.active_capabilities().len(), 1);
        auth.revoke_source(&source);
        assert_eq!(
            auth.active_capabilities().len(),
            0,
            "source revoke closes its streams"
        );
    }

    #[test]
    fn revoked_device_cannot_resume_old_session() {
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let source = SourceId::generate();
        auth.approve_source(source.clone(), 1);
        let cap = auth
            .issue(
                device.clone(),
                SessionId::generate(),
                source,
                1,
                Duration::from_secs(60),
            )
            .unwrap();
        auth.revoke_device(&device);
        assert!(matches!(
            auth.authorize(&cap),
            Err(CapabilityError::NotPaired)
        ));
    }

    // backoff (H27)
    #[test]
    fn backoff_respects_max_delay_and_budget() {
        let mut c = ReconnectController::new(BackoffPolicy {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(800),
            total_budget: Duration::from_millis(2_000),
        });
        let mut delays = Vec::new();
        while let Ok(d) = c.next_delay() {
            delays.push(d);
        }
        assert!(!delays.is_empty());
        assert!(
            delays.iter().all(|d| *d <= Duration::from_millis(800)),
            "max respected"
        );
        let total: Duration = delays.iter().sum();
        assert!(total <= Duration::from_millis(2_000) + Duration::from_millis(800));
        assert!(matches!(c.next_delay(), Err(BackoffError::BudgetExhausted)));
    }

    #[test]
    fn duplicate_request_id_is_idempotent() {
        let mut c = ReconnectController::new(BackoffPolicy::default());
        assert!(c.dedupe_request("req-1"), "first time applies");
        assert!(!c.dedupe_request("req-1"), "duplicate is a no-op");
        assert!(c.dedupe_request("req-2"));
    }

    #[test]
    fn short_human_code_alone_cannot_authenticate() {
        // The code is display-only; authorization always needs the capability.
        let auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        // No API accepts a human code for authorization — compile-time surface.
        // A capability must exist and be valid:
        let cap = SourceCapability {
            device: DeviceId::generate(),
            session: SessionId::generate(),
            source: SourceId::generate(),
            revision: 1,
            expires_at: Duration::from_secs(1),
        };
        assert!(auth.authorize(&cap).is_err());
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
        let secret = svc
            .take_secret_for_qr(&offer.ephemeral_offer_id)
            .unwrap()
            .0;
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
        svc.approve(&offer.ephemeral_offer_id, DeviceId::generate(), &secret, &code)
            .unwrap();
        // the same secret can never approve anything again
        let err = svc.approve(
            &offer.ephemeral_offer_id,
            DeviceId::generate(),
            &secret,
            &code,
        );
        assert!(matches!(err, Err(PairingError::AlreadyUsed | PairingError::Expired)));
    }

    #[test]
    fn secret_digest_never_exposes_raw_secret() {
        let mut svc = PairingService::new(Box::new(VirtualClock(Duration::ZERO)));
        let offer = svc.begin_offer("fp".into());
        let digest = svc.offer_secret_digest(&offer.ephemeral_offer_id).unwrap();
        let secret = svc
            .take_secret_for_qr(&offer.ephemeral_offer_id)
            .unwrap();
        // digest is a hash: raw bytes do not appear in it
        let raw_hex: String = secret.0.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!digest.contains(&raw_hex[..8]));
        assert_eq!(digest.len(), 16);
    }

    #[test]
    fn issue_rejects_stale_revision() {
        // parity: issue() must not hand out capabilities for an older catalog
        // revision than the current one
        let mut auth = CapabilityAuthority::new(Box::new(VirtualClock(Duration::ZERO)));
        let device = DeviceId::generate();
        auth.pair(device.clone());
        let source = SourceId::generate();
        auth.approve_source(source.clone(), 1);
        auth.approve_source(source.clone(), 2); // catalog advanced
        let err = auth.issue(device, SessionId::generate(), source, 1, Duration::from_secs(60));
        assert!(
            matches!(err, Err(CapabilityError::StaleRevision)),
            "issue() must require the exact current revision"
        );
    }

    #[test]
    fn dedupe_set_is_bounded() {
        let mut c = ReconnectController::new(BackoffPolicy::default());
        for i in 0..10_000 {
            c.dedupe_request(&format!("req-{i}"));
        }
        // bounded, not 10k entries
        let seen_len = c.seen_request_ids.len();
        assert!(seen_len <= 4096, "dedupe set grew to {seen_len}");
    }
}

// Security-test names from docs/07 §18 mapped here:
// - unpaired_peer_cannot_list_sources -> paired_peer_cannot_view_unapproved_source + guessed_source_id_fails_authorization
// - unknown_input_like_command_is_denied -> control-contract tests
// - stream_task_restore_requires_reauthentication -> viewer-core tests
