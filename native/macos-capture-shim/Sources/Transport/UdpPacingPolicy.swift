/// Pure FEC and UDP pacing policy shared by the single-stream and split
/// transports. Keeping it outside EncoderPolicy makes transport burst limits
/// independently testable without growing the VideoToolbox policy module.

/// Direct-LAN media packets stay below a 1,500-byte Ethernet IP MTU after the
/// 20-byte IPv4 and 8-byte UDP headers. The previous 1,200-byte QUIC-safe
/// envelope created hundreds of sendto calls for one 4K recovery frame even
/// though Leftcar's negotiated direct-LAN path does not traverse the public
/// Internet.
let udpMediaDatagramBytes = 1_400
let udpMediaFrameHeaderBytes = 33
let udpMediaFragmentPayloadBytes = udpMediaDatagramBytes - udpMediaFrameHeaderBytes

/// The wall-clock budget for one frame at the requested stream rate. Round up
/// so a 60fps budget is never reported as shorter than the actual 16.67ms
/// interval.
func frameBudgetUs(fps: UInt32) -> UInt64 {
    let safeFps = UInt64(max(1, fps))
    return (1_000_000 + safeFps - 1) / safeFps
}

/// Group a short run of datagrams under one pacing deadline. Sleeping once
/// per ~1.2KiB packet asks the macOS scheduler for sub-millisecond wakeups and
/// turns an intended 16.67ms frame into a 40-120ms send. Eight datagrams keep
/// each microburst below 10KiB while reducing scheduler wakeups by up to 8x.
func udpPacingBurstRanges(
    datagramCount: Int,
    maxDatagrams: Int = 8
) -> [Range<Int>] {
    guard datagramCount > 0 else { return [] }
    let boundedMaximum = max(1, maxDatagrams)
    var ranges = [Range<Int>]()
    ranges.reserveCapacity((datagramCount + boundedMaximum - 1) / boundedMaximum)
    var start = 0
    while start < datagramCount {
        let end = min(datagramCount, start + boundedMaximum)
        ranges.append(start..<end)
        start = end
    }
    return ranges
}

/// The explicitly selected 16-datagram custom profile is the clean-LAN
/// latency experiment. It drains ordinary delta access units at twice the
/// encoded media rate while preserving the bounded recovery-keyframe path.
/// Smaller and canonical profiles retain the established pacing rate.
func udpPacingRateMultiplier(
    profile: UdpStabilityProfile,
    burstDatagrams: Int
) -> Int {
    profile == .custom && burstDatagrams >= 16 ? 2 : 1
}

/// Derive a two-frame recovery target from the observed AU while preserving a
/// 64Mbps burst cap. A 160Mbps split A/B overflowed the MediaCodec input path
/// even with enlarged receive sockets, so recovery reliability wins here.
func recoveryPacingBitrate(
    targetBitrate: Int,
    bytes: Int,
    fps: UInt32
) -> Int {
    let recoveryBudgetUs = max(frameBudgetUs(fps: fps) * 2, 25_000)
    let safeBytes = UInt64(max(1, bytes))
    let requiredBitrate = Int(
        min(
            UInt64(Int.max),
            (safeBytes * 8 * 1_000_000 + recoveryBudgetUs - 1) / recoveryBudgetUs
        )
    )
    let boundedRecoveryBitrate = min(max(24_000_000, requiredBitrate), 64_000_000)
    return max(max(1, targetBitrate), boundedRecoveryBitrate)
}

func fecParityCount(
    dataCount: Int,
    reduced: Bool = false,
    recovery: Bool = false
) -> Int {
    guard dataCount > 1 else { return 0 }
    if recovery {
        return dataCount == 8 ? 2 : 1
    }
    if reduced { return 1 }
    return dataCount == 8 ? 2 : 1
}

/// Canonical negotiated parity policy. Tail groups cannot spend as many
/// parity rows as data rows; the recovery flag is retained in the interface
/// because recovery affects pacing, not the reconnect-scoped FEC strength.
func fecParityCount(
    dataCount: Int,
    selectedParity: Int,
    recovery _: Bool = false
) -> Int {
    guard dataCount > 1 else { return 0 }
    return min(dataCount - 1, min(4, max(1, selectedParity)))
}

func udpFecOverheadScale(
    dataFragmentCount: Int,
    contentMode: String,
    isKeyframe: Bool,
    selectedParity: Int? = nil
) -> Double {
    guard dataFragmentCount > 1 else { return 1.0 }
    let reducedParity = contentMode.lowercased() == StreamContentMode.video.rawValue
        && !isKeyframe
    var dataCount = 0
    var parityCount = 0
    for base in stride(from: 0, to: dataFragmentCount, by: 8) {
        let groupCount = min(dataFragmentCount - base, 8)
        dataCount += groupCount
        parityCount += selectedParity.map {
            fecParityCount(
                dataCount: groupCount,
                selectedParity: $0,
                recovery: isKeyframe
            )
        } ?? fecParityCount(
            dataCount: groupCount,
            reduced: reducedParity,
            recovery: isKeyframe
        )
    }
    guard dataCount > 0 else { return 1.0 }
    return Double(dataCount + parityCount) / Double(dataCount)
}

func udpPacingBitrate(
    targetBitrate: Int,
    dataFragmentCount: Int,
    contentMode: String,
    isKeyframe: Bool,
    selectedParity: Int? = nil
) -> Int {
    let safeTarget = max(1, targetBitrate)
    let scale = udpFecOverheadScale(
        dataFragmentCount: dataFragmentCount,
        contentMode: contentMode,
        isKeyframe: isKeyframe,
        selectedParity: selectedParity
    )
    let scaled = (Double(safeTarget) * scale).rounded(.up)
    guard scaled < Double(Int.max) else { return Int.max }
    return max(safeTarget, Int(scaled))
}

func highMotionPacingBitrate(
    targetBitrate: Int,
    accessUnitBytes: Int,
    fps: UInt32,
    fecOverheadScale: Double = 1.25
) -> Int {
    let safeBytes = UInt64(max(1, accessUnitBytes))
    let safeFps = UInt64(max(1, fps))
    let safeScale = max(1.0, fecOverheadScale)
    let requiredBitrate = UInt64(
        (Double(safeBytes) * 8.0 * Double(safeFps) * safeScale).rounded(.up)
    )
    let burstFloor = UInt64(max(max(1, targetBitrate), 24_000_000))
    return Int(max(burstFloor, min(requiredBitrate, 120_000_000)))
}

func shouldUseHighMotionPacing(
    accessUnitBytes: Int,
    targetBitrate: Int,
    fps: UInt32,
    fecOverheadScale: Double = 1.25
) -> Bool {
    guard accessUnitBytes > 0, targetBitrate > 0, fps > 0 else { return false }
    let requiredBitrate = UInt64(
        (Double(accessUnitBytes) * 8.0 * Double(fps) * max(1.0, fecOverheadScale)).rounded(.up)
    )
    return requiredBitrate > UInt64(targetBitrate)
}

func udpDatagramIntervalUs(
    bytes: Int,
    isKeyframe: Bool,
    accessUnitBytes: Int = 0,
    targetBitrate: Int,
    fps: UInt32,
    contentMode: String = StreamContentMode.interactive.rawValue,
    dataFragmentCount: Int = 0,
    selectedParity: Int? = nil,
    pacingRateMultiplier: Int = 1
) -> UInt64 {
    let hasFragmentCount = dataFragmentCount > 0
    let fecScale = hasFragmentCount
        ? udpFecOverheadScale(
            dataFragmentCount: dataFragmentCount,
            contentMode: contentMode,
            isKeyframe: isKeyframe,
            selectedParity: selectedParity
        )
        : 1.25
    let wireTargetBitrate = hasFragmentCount
        ? udpPacingBitrate(
            targetBitrate: targetBitrate,
            dataFragmentCount: dataFragmentCount,
            contentMode: contentMode,
            isKeyframe: isKeyframe,
            selectedParity: selectedParity
        )
        : max(1, targetBitrate)
    let pacingBitrate: Int
    if isKeyframe {
        let recoveryBytes = Int(
            min(
                Double(Int.max),
                (Double(max(1, max(bytes, accessUnitBytes))) * fecScale).rounded(.up)
            )
        )
        pacingBitrate = recoveryPacingBitrate(
            targetBitrate: wireTargetBitrate,
            bytes: recoveryBytes,
            fps: fps
        )
    } else if shouldUseHighMotionPacing(
        accessUnitBytes: accessUnitBytes,
        targetBitrate: wireTargetBitrate,
        fps: fps,
        fecOverheadScale: fecScale
    ) {
        pacingBitrate = highMotionPacingBitrate(
            targetBitrate: wireTargetBitrate,
            accessUnitBytes: accessUnitBytes,
            fps: fps,
            fecOverheadScale: fecScale
        )
    } else {
        pacingBitrate = wireTargetBitrate
    }
    let safeBytes = UInt64(max(1, bytes))
    let baseBitrate = UInt64(max(1, pacingBitrate))
    let multiplier = UInt64(isKeyframe ? 1 : max(1, pacingRateMultiplier))
    let multiplied = baseBitrate.multipliedReportingOverflow(by: multiplier)
    let safeBitrate = multiplied.overflow ? UInt64.max : multiplied.partialValue
    let interval = (safeBytes * 8 * 1_000_000 + safeBitrate - 1) / safeBitrate
    let minimum: UInt64 = isKeyframe ? 50 : 100
    return max(minimum, min(4_000, interval))
}

/// Wall-clock budget for sending one access unit over UDP.
///
/// During Wi-Fi airtime collapse a non-blocking `sendto` does not fail — it
/// SUCCEEDS slowly (observed 13-80ms per datagram, one 2.24s outlier), so the
/// serial network queue crawls behind a degraded link for hundreds of
/// milliseconds per frame with zero send failures. Every newer frame then
/// piles up behind the stale one and the whole capture→encode pipeline stalls
/// ("sudden 4fps"). Aborting an access unit that overran its budget turns
/// that crawl into an ordinary loss event: the existing failure path drops
/// the stale chain and requests a recovery keyframe, so the encoder and
/// capture keep running and the stream recovers the moment the link does.
///
/// The budget must be AU-SIZE AWARE: a legitimate high-motion delta (40+ KB)
/// spends its entire send window in pacing sleeps — 4ms per burst range,
/// ~44ms for a 43-fragment AU — and a flat two-frame budget (33ms at 60fps)
/// aborted every large frame even on a healthy link, flooding the stream
/// with recovery IDRs (observed 300 keyframes/10min at RTT 9ms). Scale the
/// budget with the AU's own paced duration: twice the pacing floor plus a
/// fixed 20ms slack, never below two frame budgets. Recovery keyframes keep
/// the IDR retry cadence instead.
func udpAccessUnitSendDeadlineUs(
    isKeyframe: Bool,
    fps: UInt32,
    dataFragmentCount: Int,
    burstDatagrams: Int
) -> UInt64 {
    if isKeyframe {
        return 750_000
    }
    let boundedBurst = max(1, burstDatagrams)
    let fragmentCount = max(1, dataFragmentCount)
    let pacingRanges = udpPacingBurstRanges(
        datagramCount: fragmentCount,
        maxDatagrams: boundedBurst
    ).count
    let pacedUs = UInt64(pacingRanges) * 4_000
    return max(frameBudgetUs(fps: fps) * 2, pacedUs * 2 + 20_000)
}
