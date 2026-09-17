import Foundation

@main struct PeerCongestionMemoryTests {
    static func main() {
        let ttl: UInt64 = 10 * 60 * 1_000_000_000
        let peer: UInt32 = 0xC0A8_00F9 // 192.168.0.249 host order
        let other: UInt32 = 0x13_00_A8_C0

        // Zero ceiling (no confirmed congestion) remembers nothing: healthy
        // links keep the full Parsec-style ladder start.
        rememberPeerCongestionCeiling(addressHostOrder: peer, ceiling: 0, nowNs: 100)
        precondition(peerCongestionStartCeiling(addressHostOrder: peer, nowNs: 200, ttlNs: ttl) == nil)

        // A remembered ceiling surfaces while fresh and is per-peer.
        rememberPeerCongestionCeiling(addressHostOrder: peer, ceiling: 6_200_000, nowNs: 1_000)
        precondition(peerCongestionStartCeiling(addressHostOrder: peer, nowNs: 2_000, ttlNs: ttl) == 6_200_000)
        precondition(peerCongestionStartCeiling(addressHostOrder: other, nowNs: 2_000, ttlNs: ttl) == nil)

        // Expired memory is gone: a returning device gets the full ladder.
        precondition(peerCongestionStartCeiling(addressHostOrder: peer, nowNs: 1_000 + ttl, ttlNs: ttl) == nil)

        // The latest collapse overwrites the previous one.
        rememberPeerCongestionCeiling(addressHostOrder: peer, ceiling: 4_000_000, nowNs: 5_000)
        precondition(peerCongestionStartCeiling(addressHostOrder: peer, nowNs: 5_001, ttlNs: ttl) == 4_000_000)

        // The start clamp never violates the profile floor: a remembered
        // ceiling below minRate still starts at minRate.
        let minRate = 6_000_000
        var avgBitrate = 28_700_000.0
        if let remembered = peerCongestionStartCeiling(addressHostOrder: peer, nowNs: 5_001, ttlNs: ttl),
           remembered < Int(avgBitrate) {
            avgBitrate = Double(max(remembered, minRate))
        }
        precondition(avgBitrate == 6_000_000.0)

        print("PeerCongestionMemoryTests: TTL, per-peer keying, overwrite, floor clamp PASS")
    }
}
