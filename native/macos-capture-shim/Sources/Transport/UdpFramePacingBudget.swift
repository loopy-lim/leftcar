import Foundation

/// Single-stream delta frames need service time below their arrival interval.
/// Charge actual fragment headers, FEC, singleton duplicates and encryption,
/// then reserve part of the interval for preparation and scheduler jitter.
/// This changes pacing only: encoded references and packet contents are intact.
struct UdpFramePacingBudget {
    static let datagramOverheadBytes = MediaWireFormat.counterLen + MediaWireFormat.tagLen + 28
    /// Share of the frame interval the AU may occupy. 0.8 = shipped default
    /// (80% spent sending, 20% reserved). Tighter values front-load the AU so
    /// the last fragment — and therefore decode start — lands earlier; the
    /// 120Mbps cap below still bounds the instantaneous rate.
    /// LEFTCAR_PACING_BUDGET_PCT (30-100, %) overrides for A/B runs.
    static var reservedShare: Double {
        let env = Foundation.ProcessInfo.processInfo.environment["LEFTCAR_PACING_BUDGET_PCT"]
            .flatMap(Double.init)
            .map { min(max($0, 5), 100) / 100 }
        return env ?? 0.2
    }

    let bitrate: Int

    init?(targetBitrate: Int, payloadBytes: Int, plaintextBytes: Int,
          datagramCount: Int, fps: UInt32) {
        guard targetBitrate > 0, payloadBytes > 0, plaintextBytes >= payloadBytes,
              datagramCount > 0, fps > 0 else { return nil }
        let wireBytes = Double(plaintextBytes)
            + Double(datagramCount) * Double(Self.datagramOverheadBytes)
        let wireTarget = Double(targetBitrate) * wireBytes / Double(payloadBytes)
        let sendBudgetUs = Double(frameBudgetUs(fps: fps)) * (1 - Self.reservedShare)
        let budgetRate = wireBytes * 8_000_000 / sendBudgetUs
        // Keep the existing high-motion cap. A frame too large for that cap
        // may exceed the time budget; never hide overload with unbounded bursts.
        let selected = max(wireTarget, min(budgetRate, 120_000_000))
        bitrate = selected >= Double(Int.max) ? Int.max : max(1, Int(selected.rounded(.up)))
    }
}
