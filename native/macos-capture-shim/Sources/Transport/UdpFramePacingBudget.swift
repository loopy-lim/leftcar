/// Single-stream delta frames need service time below their arrival interval.
/// Charge actual fragment headers, FEC, singleton duplicates and encryption,
/// then reserve 20% of the interval for preparation and scheduler jitter.
/// This changes pacing only: encoded references and packet contents are intact.
struct UdpFramePacingBudget {
    static let datagramOverheadBytes = MediaWireFormat.counterLen + MediaWireFormat.tagLen + 28
    let bitrate: Int

    init?(targetBitrate: Int, payloadBytes: Int, plaintextBytes: Int,
          datagramCount: Int, fps: UInt32) {
        guard targetBitrate > 0, payloadBytes > 0, plaintextBytes >= payloadBytes,
              datagramCount > 0, fps > 0 else { return nil }
        let wireBytes = Double(plaintextBytes)
            + Double(datagramCount) * Double(Self.datagramOverheadBytes)
        let wireTarget = Double(targetBitrate) * wireBytes / Double(payloadBytes)
        let sendBudgetUs = Double(frameBudgetUs(fps: fps)) * 0.8
        let budgetRate = wireBytes * 8_000_000 / sendBudgetUs
        // Keep the existing high-motion cap. A frame too large for that cap
        // may exceed the time budget; never hide overload with unbounded bursts.
        let selected = max(wireTarget, min(budgetRate, 120_000_000))
        bitrate = selected >= Double(Int.max) ? Int.max : max(1, Int(selected.rounded(.up)))
    }
}
