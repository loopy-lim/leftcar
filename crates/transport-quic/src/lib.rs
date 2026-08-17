//! QUIC transport candidate (ADR-0004).
//!
//! NOT IMPLEMENTED YET by design: ADR-0004 defers the WebRTC-vs-QUIC decision
//! to the Galaxy XR bake-off (H11–H14). This crate will implement
//! `transport_api::Transport` with:
//! - reliable bidirectional stream: handshake, control, codec config, IDR req
//! - QUIC DATAGRAM: video fragments
//! - per-source logical channels on one connection
//!
//! The build below only fixes the transport-selection config surface (H14 Red):
//! a product build must fail when no transport is selected.

/// The transport selection for a product build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedTransport {
    /// Bake-off pending — simulated/in-memory only, never a product default.
    Undecided,
    Quic,
    #[allow(dead_code)]
    WebRtc,
}

/// Product build info fails to construct without a decided transport (H14).
#[derive(Debug, thiserror::Error)]
#[error("no transport selected: run the bake-off and decide (ADR-0004)")]
pub struct TransportUndecided;

pub struct ProductBuildInfo {
    pub transport: SelectedTransport,
}

impl ProductBuildInfo {
    /// A product build requires a decided transport; `Undecided` is rejected.
    pub fn new(transport: SelectedTransport) -> Result<Self, TransportUndecided> {
        match transport {
            SelectedTransport::Undecided => Err(TransportUndecided),
            decided => Ok(Self { transport: decided }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_transport_absent_fails_product_build() {
        // Red test for H14: no selected transport => product build fails.
        assert!(ProductBuildInfo::new(SelectedTransport::Undecided).is_err());
        // a decided transport constructs (the bake-off winner will be one of these)
        assert!(ProductBuildInfo::new(SelectedTransport::Quic).is_ok());
        assert!(ProductBuildInfo::new(SelectedTransport::WebRtc).is_ok());
    }
}
