//! Media-plane peer admission check (docs/09 shim boundary).
//!
//! The viewer dials the host for control, so the reverse media connection
//! must come back from that exact address: any other LAN sender could push
//! forged H.264 into the decoder surface. This module is deliberately not
//! android-gated so `cargo test` on the host exercises it (CI runs
//! `cargo test --workspace` for the host target only).

use std::net::{IpAddr, SocketAddr};

/// True when the peer address matches one of the expected hosts exactly
/// (strict IP equality — the viewer dials the host for control, so the media
/// connection must come back from that exact address). A comma-separated host
/// list is used by the automatic USB fallback: the local TCP bridge forwards
/// host packets from 127.0.0.1 while Wi-Fi packets still arrive from the
/// paired Host address. An unparseable or missing expectation denies
/// everything: no paired host, no stream.
///
/// The listener binds 0.0.0.0 (IPv4-only), so accept() peers are plain IPv4;
/// if the bind ever goes dual-stack, to_canonical() normalization would be
/// needed before comparing against a v4 expectation.
pub fn peer_allowed(peer: Option<SocketAddr>, expected_host: &str) -> bool {
    let Some(peer) = peer else { return false };
    expected_host
        .split(',')
        .map(str::trim)
        .filter_map(|host| host.parse::<IpAddr>().ok())
        .any(|expected| peer.ip() == expected)
}

/// True when expected_host is a bare IP literal the accept loop can compare
/// against. Hostnames are rejected — the control plane supplies a dialed IP.
pub fn host_is_valid(expected_host: &str) -> bool {
    expected_host.parse::<IpAddr>().is_ok()
}

/// True when a non-empty comma-separated list contains only bare IP literals.
pub fn hosts_are_valid(expected_hosts: &str) -> bool {
    let hosts: Vec<_> = expected_hosts.split(',').map(str::trim).collect();
    !hosts.is_empty() && hosts.iter().all(|host| host_is_valid(host))
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
        // Automatic USB fallback allows the local bridge in addition to the
        // paired Wi-Fi Host, but does not widen admission to arbitrary peers.
        assert!(peer_allowed(
            Some(v4("127.0.0.1", 5001)),
            "192.168.0.10,127.0.0.1"
        ));
        assert!(peer_allowed(
            Some(v4("192.168.0.10", 5001)),
            "192.168.0.10, 127.0.0.1"
        ));
        assert!(!peer_allowed(
            Some(v4("192.168.0.11", 5001)),
            "192.168.0.10,127.0.0.1"
        ));
    }

    #[test]
    fn host_is_valid_accepts_bare_ip_literals_only() {
        // Valid IPv4 / IPv6.
        assert!(host_is_valid("192.168.0.10"));
        assert!(host_is_valid("fd00::1"));
        // Empty, hostname, addr:port all rejected.
        assert!(!host_is_valid(""));
        assert!(!host_is_valid("foo.local"));
        assert!(!host_is_valid("1.2.3.4:5000"));
    }

    #[test]
    fn hosts_are_valid_accepts_only_non_empty_ip_lists() {
        assert!(hosts_are_valid("192.168.0.10,127.0.0.1"));
        assert!(hosts_are_valid("fd00::1, ::1"));
        assert!(!hosts_are_valid(""));
        assert!(!hosts_are_valid("192.168.0.10,"));
        assert!(!hosts_are_valid("192.168.0.10,host.local"));
    }
}
