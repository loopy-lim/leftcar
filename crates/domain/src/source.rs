//! Source descriptors and catalog revisioning (docs/04 §4, docs/03 §5).

use crate::ids::SourceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Display,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub id: SourceId,
    pub kind: SourceKind,
    /// UI display name; redacted in diagnostics by default.
    pub display_name: String,
    pub application_name: Option<String>,
    pub width_px: u32,
    pub height_px: u32,
    pub is_approved: bool,
    pub is_available: bool,
    pub revision: u64,
}

/// Catalog snapshot with compare-and-set revision for mutations (docs/04 §9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCatalogSnapshot {
    pub revision: u64,
    pub sources: Vec<SourceDescriptor>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("stale revision: expected newer than {expected}")]
    StaleRevision { expected: u64 },
}

/// In-memory approved-source registry with revision CAS.
#[derive(Debug, Default)]
pub struct SourceRegistry {
    snapshot: SourceCatalogSnapshot,
}

impl SourceRegistry {
    pub fn new(sources: Vec<SourceDescriptor>) -> Self {
        Self {
            snapshot: SourceCatalogSnapshot { revision: 1, sources },
        }
    }

    pub fn snapshot(&self) -> &SourceCatalogSnapshot {
        &self.snapshot
    }

    /// Mutate with compare-and-set on revision. A stale revision is rejected so
    /// stale mutations never clobber the latest sources (docs/05 L2).
    pub fn mutate<F>(&mut self, expected_revision: u64, f: F) -> Result<u64, CatalogError>
    where
        F: FnOnce(&mut Vec<SourceDescriptor>),
    {
        if expected_revision != self.snapshot.revision {
            return Err(CatalogError::StaleRevision { expected: expected_revision });
        }
        f(&mut self.snapshot.sources);
        self.snapshot.revision += 1;
        Ok(self.snapshot.revision)
    }

    pub fn approved_sources(&self) -> Vec<&SourceDescriptor> {
        self.snapshot.sources.iter().filter(|s| s.is_approved).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(n: &str, approved: bool) -> SourceDescriptor {
        SourceDescriptor {
            id: SourceId::from_raw(n).unwrap(),
            kind: SourceKind::Window,
            display_name: format!("Window {n}"),
            application_name: Some("App".into()),
            width_px: 1920,
            height_px: 1080,
            is_approved: approved,
            is_available: true,
            revision: 0,
        }
    }

    #[test]
    fn stale_revision_mutation_rejected() {
        let mut r = SourceRegistry::new(vec![src("a", true)]);
        let rev = r.snapshot().revision;
        r.mutate(rev, |s| s[0].is_approved = false).unwrap();
        // retry with the old revision: rejected
        let err = r.mutate(rev, |s| s[0].is_approved = true);
        assert!(matches!(err, Err(CatalogError::StaleRevision { .. })));
        assert!(!r.snapshot().sources[0].is_approved, "stale write must not apply");
    }

    #[test]
    fn revision_monotonic() {
        let mut r = SourceRegistry::new(vec![]);
        let a = r.mutate(1, |_| {}).unwrap();
        let b = r.mutate(a, |_| {}).unwrap();
        assert!(b > a);
    }
}
