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

        // Capacity limits apply during the TTL, and updating an existing peer
        // preserves unrelated entries while refreshing the eviction order.
        var bounded = PeerCongestionMemory(capacity: 2)
        bounded.remember(address: 1, ceiling: 1_000, nowNs: 100)
        bounded.remember(address: 2, ceiling: 2_000, nowNs: 200)
        bounded.remember(address: 1, ceiling: 3_000, nowNs: 300)
        bounded.remember(address: 3, ceiling: 4_000, nowNs: 400)
        precondition(bounded.count == 2)
        precondition(bounded.startCeiling(address: 2, nowNs: 401) == nil)
        precondition(bounded.startCeiling(address: 1, nowNs: 401) == 3_000)
        precondition(bounded.startCeiling(address: 3, nowNs: 401) == 4_000)
        // A lookup for a new peer also reclaims all expired entries.
        precondition(bounded.startCeiling(address: 99, nowNs: ttl + 400) == nil)
        precondition(bounded.count == 0)

        // Insertion reclaims expired peers without requiring a prior lookup.
        bounded.remember(address: 1, ceiling: 1_000, nowNs: ttl + 500)
        bounded.remember(address: 2, ceiling: 2_000, nowNs: ttl * 2 + 500)
        precondition(bounded.count == 1)
        precondition(bounded.startCeiling(address: 1, nowNs: ttl * 2 + 501) == nil)
        // A clock reset cannot keep a ceiling from a future time alive.
        precondition(bounded.startCeiling(address: 2, nowNs: 1) == nil)
        precondition(bounded.count == 0)

        var churn = PeerCongestionMemory()
        for address in 1...1_000 as ClosedRange<UInt32> {
            churn.remember(address: address, ceiling: 1_000, nowNs: UInt64(address))
            precondition(churn.count <= 128)
        }
        precondition(churn.count == 128)
        precondition(churn.startCeiling(address: 1, nowNs: 1_001) == nil)
        precondition(churn.startCeiling(address: 1_000, nowNs: 1_001) == 1_000)

        print("PeerCongestionMemoryTests: TTL reclamation, capacity, churn, overwrite, clock reset, floor clamp PASS")
    }
}
