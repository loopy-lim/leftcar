import Foundation

@main struct RecoveryPacingTests {
    static func main() {
        // Healthy link (target ≥ 6Mbps): the two-frame 24–64Mbps burst window
        // is preserved bit-for-bit with the pre-collapse policy.
        precondition(recoveryPacingBitrate(targetBitrate: 14_000_000, bytes: 300_000, fps: 60) == 64_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 29_000_000, bytes: 300_000, fps: 60) == 64_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 14_000_000, bytes: 40_000, fps: 60) == 24_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 6_000_000, bytes: 300_000, fps: 60) == 64_000_000)

        // Collapsed link: the burst ceiling scales with the measured rate
        // (4x, at least 8Mbps) instead of re-congesting at 24Mbps.
        precondition(recoveryPacingBitrate(targetBitrate: 3_000_000, bytes: 117_000, fps: 60) == 12_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 5_900_000, bytes: 117_000, fps: 60) == 23_600_000)
        precondition(recoveryPacingBitrate(targetBitrate: 1_000_000, bytes: 117_000, fps: 60) == 8_000_000)
        // Small recovery AUs never pace below the measured rate.
        precondition(recoveryPacingBitrate(targetBitrate: 3_000_000, bytes: 4_800, fps: 60) == 12_000_000)

        // Degraded-but-fast link (jitter episode armed by slow sendto):
        // the same weak-link ceiling applies even though the measured rate
        // would normally unlock the 64Mbps burst window. The measured-rate
        // floor still applies, so a 29Mbps stream paces its IDR at its own
        // rate instead of the 64Mbps burst window.
        precondition(recoveryPacingBitrate(targetBitrate: 14_000_000, bytes: 300_000, fps: 60, linkDegraded: true) == 24_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 29_000_000, bytes: 300_000, fps: 60, linkDegraded: true) == 29_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 6_000_000, bytes: 300_000, fps: 60, linkDegraded: true) == 24_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 29_000_000, bytes: 40_000, fps: 60, linkDegraded: true) == 29_000_000)
        precondition(recoveryPacingBitrate(targetBitrate: 14_000_000, bytes: 4_800, fps: 60, linkDegraded: true) == 24_000_000)

        print("RecoveryPacingTests: healthy burst window preserved, collapsed ceiling scaled, degraded ceiling clamped PASS")
    }
}
