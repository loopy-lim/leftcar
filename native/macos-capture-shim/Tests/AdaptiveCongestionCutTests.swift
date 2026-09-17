import Foundation

@main struct AdaptiveCongestionCutTests {
    static func main() {
        // Above the mode floor the −20% cut shape is unchanged.
        precondition(adaptiveCongestionCutTarget(currentBitrate: 28_700_000, floorBitrate: 8_000_000, activeCount: 1)
            == 22_960_000)
        // At the video-mode floor the cut now descends THROUGH it…
        precondition(adaptiveCongestionCutTarget(currentBitrate: 8_000_000, floorBitrate: 8_000_000, activeCount: 1)
            == 6_400_000)
        // …and settles at the survival floor instead of pinning at 8Mbps on a
        // link that sustains ~3Mbps (2026-09-17 XR storm).
        precondition(adaptiveCongestionCutTarget(currentBitrate: 3_750_000, floorBitrate: 8_000_000, activeCount: 1)
            == 3_000_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 3_000_000, floorBitrate: 8_000_000, activeCount: 1)
            == 3_000_000)
        // Interactive mode (4Mbps floor) shares the same survival bottom.
        precondition(adaptiveCongestionCutTarget(currentBitrate: 4_000_000, floorBitrate: 4_000_000, activeCount: 1)
            == 3_200_000)
        // Multi-stream halves the survival floor.
        precondition(adaptiveCongestionCutTarget(currentBitrate: 2_500_000, floorBitrate: 7_000_000, activeCount: 2)
            == 2_000_000)
        // Deep-collapse recovery IDR spacing: 750ms normally, 2.5s at ≤6Mbps.
        precondition(recoveryRequestCooldownNs(currentBitrate: 14_000_000) == 750_000_000)
        precondition(recoveryRequestCooldownNs(currentBitrate: 6_000_000) == 2_500_000_000)
        precondition(recoveryRequestCooldownNs(currentBitrate: 3_000_000) == 2_500_000_000)
        print("AdaptiveCongestionCutTests: floor survival descent + recovery spacing PASS")
    }
}
