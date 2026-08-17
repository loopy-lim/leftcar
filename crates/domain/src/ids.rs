//! Opaque entity IDs (docs/04 §3).
//!
//! IDs are opaque strings; titles, bundle IDs, and native handles must never
//! be encoded into them.

use serde::{Deserialize, Serialize};

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn generate() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            pub fn from_raw(raw: impl Into<String>) -> Result<Self, IdError> {
                let raw = raw.into();
                if raw.trim().is_empty() {
                    Err(IdError::Empty)
                } else {
                    Ok(Self(raw))
                }
            }
        }
    };
}

opaque_id!(HostId);
opaque_id!(DeviceId);
opaque_id!(SourceId);
opaque_id!(StreamInstanceId);
opaque_id!(SessionId);

#[derive(Debug, thiserror::Error)]
#[error("id must not be empty")]
pub enum IdError {
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_unique() {
        let a = SourceId::generate();
        let b = SourceId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn id_rejects_empty() {
        assert!(SourceId::from_raw("").is_err());
        assert!(SourceId::from_raw("   ").is_err());
        assert!(SourceId::from_raw("window-42").is_ok());
    }
}
