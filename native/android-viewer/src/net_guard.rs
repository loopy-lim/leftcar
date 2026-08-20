//! Media-plane peer admission check (docs/09 shim boundary).
//!
//! The viewer dials the host for control, so the reverse media connection
//! must come back from that exact address: any other LAN sender could push
//! forged H.264 into the decoder surface. This module is deliberately not
//! android-gated so `cargo test` on the host exercises it (CI runs
//! `cargo test --workspace` for the host target only).

use std::net::{IpAddr, SocketAddr};

/// True when the peer address matches the expected host exactly (strict IP
/// equality — the viewer dials the host for control, so the media connection
/// must come back from that exact address). An unparseable or missing
/// expectation denies everything: no paired host, no stream.
pub fn peer_allowed(peer: Option<SocketAddr>, expected_host: &str) -> bool {
    match (peer, expected_host.parse::<IpAddr>()) {
        (Some(addr), Ok(expected)) => addr.ip() == expected,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(addr: &str, port: u16) -> SocketAddr {
        format!("{addr}:{port}").parse().unwrap()
    }

    #[test]
    fn peer_allowed_matches_exact_ip_only() {
        // Same IPv4 allowed (any port on the host).
        assert!(peer_allowed(Some(v4("192.168.0.10", 5001)), "192.168.0.10"));
        assert!(peer_allowed(
            Some(v4("192.168.0.10", 65535)),
            "192.168.0.10"
        ));
        // Different IPv4 denied.
        assert!(!peer_allowed(
            Some(v4("192.168.0.11", 5001)),
            "192.168.0.10"
        ));
        // None peer denied.
        assert!(!peer_allowed(None, "192.168.0.10"));
        // Unparseable expected_host denied (hostname, empty, addr:port).
        assert!(!peer_allowed(Some(v4("192.168.0.10", 5001)), ""));
        assert!(!peer_allowed(
            Some(v4("192.168.0.10", 5001)),
            "macbook.local"
        ));
        assert!(!peer_allowed(
            Some(v4("192.168.0.10", 5001)),
            "192.168.0.10:7777"
        ));
        // IPv6 expected works.
        assert!(peer_allowed(
            Some("[fd00::1]:5001".parse().unwrap()),
            "fd00::1"
        ));
        assert!(!peer_allowed(
            Some("[fd00::2]:5001".parse().unwrap()),
            "fd00::1"
        ));
        // v4 peer against v6 expectation (and vice versa) denied.
        assert!(!peer_allowed(Some(v4("192.168.0.10", 5001)), "::1"));
    }
}
