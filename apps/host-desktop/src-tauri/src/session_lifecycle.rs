//! In-memory operation identity. ControlServer owns locking and resources;
//! this policy decides whether an asynchronous result may still be published.
#[derive(Clone, Copy)]
pub(super) struct Operation(u64);

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Lifecycle {
    revision: u64,
    stopped: bool,
}

impl Lifecycle {
    pub(super) fn begin(&mut self) -> Operation {
        self.invalidate();
        Operation(self.revision)
    }

    pub(super) fn invalidate(&mut self) {
        self.revision += 1;
    }

    pub(super) fn stop(&mut self) {
        self.stopped = true;
        self.invalidate();
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub(super) fn accepts(&self, operation: Operation, backend_released: bool) -> bool {
        !self.stopped && !backend_released && self.revision == operation.0
    }
}

use std::collections::{HashMap, HashSet};

type Endpoint = (Option<String>, String);
#[derive(Default)]
struct StartEntry {
    newest: u64,
    registered_owner: Option<u64>,
    transport_busy: bool,
    transport: Option<OwnedTransport>,
}

struct OwnedTransport {
    owner: u64,
    kind: String,
    port: u16,
    cleanup_pending: bool,
}

/// Keep endpoint generations while older requests can still finish or a
/// registered session owns transport resources. Released owners are pruned.
#[derive(Default)]
pub(super) struct Starts {
    sequence: u64,
    active: HashSet<u64>,
    endpoints: HashMap<Endpoint, StartEntry>,
}

#[derive(Clone)]
pub(super) struct StartAttempt {
    request: u64,
    endpoint: Endpoint,
}

impl Starts {
    pub(super) fn begin(&mut self) -> u64 {
        self.sequence += 1;
        self.active.insert(self.sequence);
        self.sequence
    }

    pub(super) fn finish(&mut self, request: u64) {
        self.active.remove(&request);
        self.prune();
    }

    fn prune(&mut self) {
        let oldest = self.active.iter().min().copied();
        self.endpoints.retain(|_, entry| {
            entry.transport_busy
                || entry.registered_owner.is_some()
                || entry.transport.is_some()
                || oldest.is_some_and(|oldest| entry.newest >= oldest)
        });
    }

    pub(super) fn register(&mut self, attempt: &StartAttempt) {
        debug_assert!(self.accepts(attempt));
        self.endpoints
            .get_mut(&attempt.endpoint)
            .unwrap()
            .registered_owner = Some(attempt.request);
    }

    /// Called inside the setup lease, before creating any external resource.
    /// The successor inherits the actual predecessor resource even if its
    /// Session has already been removed by a delayed backend callback.
    pub(super) fn replace_transport(
        &mut self,
        attempt: &StartAttempt,
        kind: &str,
        port: u16,
    ) -> Option<(String, u16)> {
        let entry = self.endpoints.get_mut(&attempt.endpoint).unwrap();
        debug_assert!(entry.transport_busy && entry.newest == attempt.request);
        entry
            .transport
            .replace(OwnedTransport {
                owner: attempt.request,
                kind: kind.to_owned(),
                port,
                cleanup_pending: false,
            })
            .map(|old| (old.kind, old.port))
    }

    pub(super) fn begin_owned_cleanup(&mut self, attempt: &StartAttempt) -> Option<(String, u16)> {
        let entry = self.endpoints.get_mut(&attempt.endpoint)?;
        let resource = entry
            .transport
            .as_mut()
            .filter(|r| r.owner == attempt.request)?;
        if entry.transport_busy {
            // The owning setup lease must either inherit and clean this
            // resource or drain this request before releasing its lease.
            resource.cleanup_pending = true;
            return None;
        }
        entry.transport_busy = true;
        Some((resource.kind.clone(), resource.port))
    }

    pub(super) fn release_transport(&mut self, attempt: &StartAttempt) {
        if let Some(entry) = self.endpoints.get_mut(&attempt.endpoint) {
            if entry
                .transport
                .as_ref()
                .is_some_and(|r| r.owner == attempt.request)
            {
                entry.transport = None;
            }
        }
        self.prune();
    }

    pub(super) fn release_registered(&mut self, attempt: &StartAttempt) {
        if let Some(entry) = self.endpoints.get_mut(&attempt.endpoint) {
            if entry.registered_owner == Some(attempt.request) {
                entry.registered_owner = None;
            }
        }
        self.prune();
    }

    pub(super) fn claim(
        &mut self,
        request: u64,
        device: Option<&str>,
        address: &str,
    ) -> Option<StartAttempt> {
        let endpoint = (device.map(str::to_owned), address.to_owned());
        let entry = self.endpoints.entry(endpoint.clone()).or_default();
        if entry.transport_busy || entry.newest > request {
            return None;
        }
        entry.newest = request;
        Some(StartAttempt { request, endpoint })
    }

    pub(super) fn accepts(&self, attempt: &StartAttempt) -> bool {
        self.endpoints
            .get(&attempt.endpoint)
            .is_some_and(|entry| entry.newest == attempt.request && !entry.transport_busy)
    }

    pub(super) fn begin_transport_action(&mut self, attempt: &StartAttempt) -> bool {
        if !self.accepts(attempt) {
            return false;
        }
        self.endpoints
            .get_mut(&attempt.endpoint)
            .unwrap()
            .transport_busy = true;
        true
    }

    /// Keep the lease while draining deferred teardown. Checking for pending
    /// work and unlocking are atomic, so cleanup cannot fall between them.
    pub(super) fn end_transport_action(&mut self, attempt: &StartAttempt) -> Option<(String, u16)> {
        if let Some(entry) = self.endpoints.get_mut(&attempt.endpoint) {
            if entry.transport.as_ref().is_some_and(|r| r.cleanup_pending) {
                return entry.transport.take().map(|r| (r.kind, r.port));
            }
            entry.transport_busy = false;
        }
        self.prune();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(starts: &mut Starts) -> StartAttempt {
        let request = starts.begin();
        let attempt = starts
            .claim(request, Some("viewer"), "127.0.0.1:5001")
            .unwrap();
        assert!(starts.begin_transport_action(&attempt));
        starts.replace_transport(&attempt, "adbTcp", 5001);
        assert!(starts.end_transport_action(&attempt).is_none());
        starts.register(&attempt);
        starts.finish(request);
        attempt
    }

    #[test]
    fn pending_transport_owns_resource_before_registration() {
        let mut starts = Starts::default();
        let old = prepared(&mut starts);
        let request = starts.begin();
        let new = starts
            .claim(request, Some("viewer"), "127.0.0.1:5001")
            .unwrap();
        assert!(starts.begin_transport_action(&new));
        assert_eq!(
            starts.replace_transport(&new, "tcp", 5001),
            Some(("adbTcp".into(), 5001))
        );
        assert!(starts.end_transport_action(&new).is_none());
        assert!(starts.begin_owned_cleanup(&old).is_none());
        starts.release_registered(&old);
        assert_eq!(starts.begin_owned_cleanup(&new), Some(("tcp".into(), 5001)));
    }

    #[test]
    fn newer_claim_without_setup_cannot_orphan_old_resource() {
        let mut starts = Starts::default();
        let old = prepared(&mut starts);
        let request = starts.begin();
        starts
            .claim(request, Some("viewer"), "127.0.0.1:5001")
            .unwrap();
        starts.finish(request); // The successor aborts before resource setup.
        assert_eq!(
            starts.begin_owned_cleanup(&old),
            Some(("adbTcp".into(), 5001))
        );
        starts.release_transport(&old);
        assert!(starts.end_transport_action(&old).is_none());
        starts.release_registered(&old);
        assert!(starts.endpoints.is_empty());
    }

    #[test]
    fn aborted_lease_drains_cleanup_before_unlocking() {
        let mut starts = Starts::default();
        let old = prepared(&mut starts);
        let request = starts.begin();
        let new = starts
            .claim(request, Some("viewer"), "127.0.0.1:5001")
            .unwrap();
        assert!(starts.begin_transport_action(&new));
        assert!(starts.begin_owned_cleanup(&old).is_none());
        starts.release_registered(&old);
        assert_eq!(
            starts.end_transport_action(&new),
            Some(("adbTcp".into(), 5001))
        );
        assert!(!starts.accepts(&new)); // External cleanup still owns the lease.
        assert!(starts.end_transport_action(&new).is_none());
        starts.finish(request);
        assert!(starts.endpoints.is_empty());
    }
}
