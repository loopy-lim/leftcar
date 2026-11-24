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
        // Hard evidence (a refused send / AU deadline abort): −40% per window
        // under the same floor and survival rules. A 4K video overshoot
        // descends 48 → 28.8 → 24 (floor clamps one step) → 14.4 → 8.64 →
        // 5.18 → 3.11 → 3.0 inside six windows instead of ten or more at the
        // soft −20% shape.
        precondition(adaptiveCongestionCutTarget(currentBitrate: 48_000_000, floorBitrate: 24_000_000, activeCount: 1, retainFraction: 0.60) == 28_800_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 28_800_000, floorBitrate: 24_000_000, activeCount: 1, retainFraction: 0.60) == 24_000_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 24_000_000, floorBitrate: 24_000_000, activeCount: 1, retainFraction: 0.60) == 14_400_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 14_400_000, floorBitrate: 24_000_000, activeCount: 1, retainFraction: 0.60) == 8_640_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 3_100_000, floorBitrate: 24_000_000, activeCount: 1, retainFraction: 0.60) == 3_000_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 30_000_000, floorBitrate: 18_000_000, activeCount: 2, retainFraction: 0.60) == 18_000_000)
        // Retain fractions outside the established band collapse to the soft shape.
        precondition(adaptiveCongestionCutTarget(currentBitrate: 10_000_000, floorBitrate: 8_000_000, activeCount: 1, retainFraction: 0.95) == 8_000_000)
        precondition(adaptiveCongestionCutTarget(currentBitrate: 10_000_000, floorBitrate: 4_000_000, activeCount: 1, retainFraction: 0.05) == 4_000_000)
        // Control-plane retry backoff: doubling 3→192ms (≈380ms of coverage,
        // inside one probe interval) then nil once the budget is spent.
        precondition(controlSendRetryDelayMs(attempt: 0) == 3)
        precondition(controlSendRetryDelayMs(attempt: 3) == 24)
        precondition(controlSendRetryDelayMs(attempt: 6) == 192)
        precondition(controlSendRetryDelayMs(attempt: 7) == nil)
        precondition(controlSendRetryDelayMs(attempt: -1) == nil)
        print("AdaptiveCongestionCutTests: floor survival descent + hard-evidence cut + recovery spacing + control retry PASS")
    }
}
