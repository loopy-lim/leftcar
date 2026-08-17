//! Source lease accounting (docs/03 §3.3, docs/05 §5.2).

use crate::ids::{SourceId, StreamInstanceId};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
#[error("lease not found for release")]
pub struct LeaseNotFoundError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseEvent {
    SourceStarted(SourceId),
    SourceStopped(SourceId),
}

/// Tracks which stream windows hold a lease on which sources.
///
/// Invariants (docs/05 L2):
/// - lease counts never go negative regardless of acquire/release order
/// - first lease on a source emits SourceStarted
/// - last release emits SourceStopped after a debounce interval (caller
///   advances virtual time; the table reports the pending stop deadline)
#[derive(Debug, Default)]
pub struct LeaseTable {
    leases: HashMap<SourceId, HashSet<StreamInstanceId>>,
    /// sources with zero leases awaiting debounce expiry
    pending_stops: HashMap<SourceId, PendingStop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingStop {
    pub source: SourceId,
    pub released_at: Duration,
    pub deadline: Duration,
}

impl LeaseTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire a lease. Returns SourceStarted when this is the first lease.
    pub fn acquire(&mut self, source: SourceId, instance: StreamInstanceId) -> Option<LeaseEvent> {
        // A re-acquire of a pending-stop source cancels the stop.
        self.pending_stops.remove(&source);
        let set = self.leases.entry(source.clone()).or_default();
        let first = set.is_empty();
        set.insert(instance);
        first.then_some(LeaseEvent::SourceStarted(source))
    }

    /// Release a lease. Returns SourceStopped's pending record when the last
    /// lease was released (debounce not yet elapsed).
    pub fn release(
        &mut self,
        source: &SourceId,
        instance: &StreamInstanceId,
        now: Duration,
        debounce: Duration,
    ) -> Result<Option<PendingStop>, LeaseNotFoundError> {
        let Some(set) = self.leases.get_mut(source) else {
            return Ok(None); // release of unknown source is a no-op (docs/05 §5.2)
        };
        if !set.remove(instance) {
            // double release is a no-op
            if set.is_empty() {
                self.leases.remove(source);
            }
            return Ok(None);
        }
        if set.is_empty() {
            self.leases.remove(source);
            let pending = PendingStop {
                source: source.clone(),
                released_at: now,
                deadline: now + debounce,
            };
            self.pending_stops.insert(source.clone(), pending.clone());
            return Ok(Some(pending));
        }
        Ok(None)
    }

    /// Whether a pending stop has elapsed its debounce at `now`.
    pub fn stop_elapsed(&self, source: &SourceId, now: Duration) -> Option<LeaseEvent> {
        self.pending_stops
            .get(source)
            .filter(|p| now >= p.deadline)
            .map(|_| LeaseEvent::SourceStopped(source.clone()))
    }

    pub fn lease_count(&self, source: &SourceId) -> usize {
        self.leases.get(source).map_or(0, |s| s.len())
    }

    pub fn total_leases(&self) -> usize {
        self.leases.values().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(n: &str) -> SourceId {
        SourceId::from_raw(n).unwrap()
    }
    fn iid(n: &str) -> StreamInstanceId {
        StreamInstanceId::from_raw(n).unwrap()
    }

    #[test]
    fn first_lease_starts_source() {
        let mut t = LeaseTable::new();
        assert_eq!(
            t.acquire(sid("s1"), iid("i1")),
            Some(LeaseEvent::SourceStarted(sid("s1")))
        );
        assert_eq!(t.acquire(sid("s1"), iid("i2")), None);
    }

    #[test]
    fn last_lease_stops_after_debounce() {
        let mut t = LeaseTable::new();
        t.acquire(sid("s1"), iid("i1"));
        let pending = t
            .release(
                &sid("s1"),
                &iid("i1"),
                Duration::from_secs(0),
                Duration::from_secs(5),
            )
            .unwrap()
            .expect("pending stop");
        assert_eq!(pending.deadline, Duration::from_secs(5));
        assert_eq!(t.stop_elapsed(&sid("s1"), Duration::from_secs(4)), None);
        assert_eq!(
            t.stop_elapsed(&sid("s1"), Duration::from_secs(5)),
            Some(LeaseEvent::SourceStopped(sid("s1")))
        );
    }

    #[test]
    fn double_release_is_noop() {
        let mut t = LeaseTable::new();
        t.acquire(sid("s1"), iid("i1"));
        t.release(
            &sid("s1"),
            &iid("i1"),
            Duration::ZERO,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(t
            .release(
                &sid("s1"),
                &iid("i1"),
                Duration::ZERO,
                Duration::from_secs(5)
            )
            .is_ok());
        assert_eq!(t.lease_count(&sid("s1")), 0);
    }

    #[test]
    fn release_unknown_source_is_noop() {
        let mut t = LeaseTable::new();
        assert!(t
            .release(&sid("nope"), &iid("i1"), Duration::ZERO, Duration::ZERO)
            .is_ok());
        assert_eq!(t.total_leases(), 0);
    }

    #[test]
    fn reacquire_cancels_pending_stop() {
        let mut t = LeaseTable::new();
        t.acquire(sid("s1"), iid("i1"));
        t.release(
            &sid("s1"),
            &iid("i1"),
            Duration::ZERO,
            Duration::from_secs(5),
        )
        .unwrap();
        t.acquire(sid("s1"), iid("i2"));
        assert_eq!(t.stop_elapsed(&sid("s1"), Duration::from_secs(60)), None);
    }

    proptest::proptest! {
        #[test]
        fn acquire_release_never_negative(ops in proptest::collection::vec(
            proptest::bool::ANY, 0..64
        )) {
            let mut t = LeaseTable::new();
            let source = sid("s");
            let a = iid("a");
            let b = iid("b");
            for acquire in ops {
                if acquire {
                    t.acquire(source.clone(), a.clone());
                } else {
                    let _ = t.release(&source, &b, Duration::ZERO, Duration::ZERO);
                }
                assert!(t.lease_count(&source) <= 2);
            }
            // counts are never negative by construction (usize); ensure no panic
            // and table is consistent.
            assert!(t.total_leases() <= 2);
        }
    }
}
