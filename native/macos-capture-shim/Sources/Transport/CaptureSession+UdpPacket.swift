import Foundation

extension CaptureSession {
    // Keep the macOS shim self-contained: its C ABI dylib is loaded by the
    // Tauri host independently from the Rust host crate. This is the same
    // GF(256) RS(8,10) layout used by fec-core for Windows and Android.
     static let fecMultiplyTable: [UInt8] = {
        var table = [UInt8](repeating: 0, count: 256 * 256)
        for left in 0..<256 {
            for right in 0..<256 {
                var a = UInt8(left)
                var b = UInt8(right)
                var result: UInt8 = 0
                for _ in 0..<8 {
                    if b & 1 != 0 { result ^= a }
                    let carry = a & 0x80 != 0
                    a <<= 1
                    if carry { a ^= 0x1d }
                    b >>= 1
                }
                table[(left << 8) | right] = result
            }
        }
        return table
    }()

     func fecMultiply(_ left: UInt8, _ right: UInt8) -> UInt8 {
        Self.fecMultiplyTable[(Int(left) << 8) | Int(right)]
    }

     func fecPower(_ base: UInt8, _ exponent: Int) -> UInt8 {
        if exponent == 0 { return 1 }
        var result: UInt8 = 1
        for _ in 0..<exponent { result = fecMultiply(result, base) }
        return result
    }

     func fecParityDatagrams(
        auID: UInt16,
        totalFragments: Int,
        wallMs: UInt64,
        payloads: [Data],
        reducedParity: Bool = false,
        recoveryParity: Bool = false,
        parityOverride: Int? = nil
    ) -> [Data] {
        guard !payloads.isEmpty else { return [] }
        let selectedParity = appliedUdpStability.profile == .legacy
            ? nil
            : (parityOverride ?? currentUdpParityCount())
        var output = [Data]()
        for base in stride(from: 0, to: payloads.count, by: 8) {
            let end = min(payloads.count, base + 8)
            let group = Array(payloads[base..<end])
            let k = group.count
            let parityCount = selectedUdpParityCount(
                dataCount: k,
                reducedLegacyParity: reducedParity,
                recovery: recoveryParity,
                parityOverride: selectedParity
            )
            guard parityCount > 0 else { continue }
            let width = (group.map(\.count).max() ?? 0) + 2
            let shards = group.map { payload -> [UInt8] in
                let bytes = Array(payload)
                var shard = [UInt8](repeating: 0, count: width)
                var length = UInt16(bytes.count).bigEndian
                withUnsafeBytes(of: &length) { shard.replaceSubrange(0..<2, with: $0) }
                shard.replaceSubrange(2..<(2 + bytes.count), with: bytes)
                return shard
            }
            let coefficients = (0..<parityCount).map { parityIndex in
                let rowBase = UInt8(1 << parityIndex)
                return (0..<k).map { column in
                    fecPower(rowBase, column)
                }
            }
            for parityIndex in 0..<parityCount {
                var parity = [UInt8](repeating: 0, count: width)
                for byteIndex in 0..<width {
                    for column in 0..<k {
                        let coefficient = coefficients[parityIndex][column]
                        parity[byteIndex] ^= fecMultiply(coefficient, shards[column][byteIndex])
                    }
                }
                var datagram = Data([0x50]) // P
                datagram.append(UInt8(auID & 0xff))
                datagram.append(UInt8(auID >> 8))
                datagram.append(UInt8(k))
                datagram.append(UInt8(parityIndex))
                var baseBE = UInt16(base).bigEndian
                var totalBE = UInt16(totalFragments).bigEndian
                withUnsafeBytes(of: &baseBE) { datagram.append(contentsOf: $0) }
                withUnsafeBytes(of: &totalBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: [0x4c, 0x54]) // LT
                var wallBE = wallMs.bigEndian
                withUnsafeBytes(of: &wallBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: parity)
                if datagram.count <= udpMediaDatagramBytes {
                    output.append(datagram)
                }
            }
        }
        return output
    }

     func shouldProtectUdpAccessUnit(
        fragmentCount: Int,
        isKeyframe: Bool
    ) -> Bool {
        // One parity shard protects a short AU from one lost fragment; full
        // groups use two shards so a transient Wi-Fi burst does not promote
        // the whole H.264 reference chain into an IDR recovery burst.
        isKeyframe || fragmentCount >= 2
    }

    /// Send one config datagram or one fragmented H.264 AU. Datagram payloads
    /// stay below 1,200 bytes to avoid IP fragmentation on Wi-Fi and Tailscale.
    /// On a local queue overflow, recover from a fresh IDR instead of blocking
    /// subsequent video behind a lost packet.
    @discardableResult
     func writePacket(
        _ data: Data,
        isFrame: Bool = false,
        isKeyframe: Bool = false,
        isRecoveryKeyframe: Bool = false,
        tileSide: TileSide? = nil
    ) -> Bool {
        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else {
            if isRecoveryKeyframe {
                clearRecoveryEncodeGate()
            }
            return false
        }

        let sendStart = DispatchTime.now().uptimeNanoseconds
        var sentBytes = 0
        var sentDatagramCount = 0
        var sentParityDatagramCount = 0
        var expectedDatagramCount = 0
        var auBytes: UInt64 = 0
        var auFragmentCount: UInt32 = 0
        var auParityCount: UInt32 = 0
        var sendSyscallUs: UInt64 = 0
        // Worst SINGLE sendto duration in this AU. The AU total grows with
        // fragment count (a clean 165-fragment 4K frame easily sums past
        // 8ms of fast syscalls), so only the per-datagram worst says whether
        // the socket buffer actually blocked the sender.
        var worstDatagramSyscallUs: UInt64 = 0
        var ok = true
        if isFrame {
            // 논리 L2 헤더 검사부터 프래그먼트·FEC 조립까지는 split 전송 경로와
            // 같은 prepareUdpAccessUnit 하나로 수행한다(바이트 동일). 이 함수는
            // 메트릭·데드라인·페이싱·sendto만 담당한다.
            guard let prepared = prepareUdpAccessUnit(data, isKeyframe: isKeyframe) else {
                if isRecoveryKeyframe {
                    clearRecoveryEncodeGate()
                }
                requestRecoveryKeyframe()
                return false
            }
            if isRecoveryKeyframe {
                observeUdpRecoveryBoundary()
            }
            let selectedParity = appliedUdpStability.profile == .legacy
                ? nil
                : currentUdpParityCount()
            let fragmentCount = prepared.fragmentCount
            auBytes = UInt64(prepared.payloadBytes)
            auFragmentCount = UInt32(prepared.fragmentCount)
            auParityCount = UInt32(prepared.parityCount)
            // Send each FEC group before its parity. This keeps parity close
            // to the fragments it protects and avoids losing an entire AU's
            // recovery budget to one large primary burst. The shared UDP
            // pacer spreads both ordinary frames and recovery IDRs.
            var transmissions = prepared.datagrams
            expectedDatagramCount = transmissions.count
            if isKeyframe {
                NSLog(
                    "Leftcar recovery AU %@: bytes=%d fragments=%d parity=%d transmissions=%d",
                    targetLabel,
                    data.count,
                    fragmentCount,
                    prepared.parityCount,
                    transmissions.count
                )
            }
            // Slow-but-successful sends during link collapse must not hold
            // newer frames hostage: abort the access unit once it overran its
            // size-aware budget (see udpAccessUnitSendDeadlineUs). Checked per
            // pacing range so the deadline covers pacing sleeps and sendto
            // time together.
            let sendDeadlineBudgetUs = udpAccessUnitSendDeadlineUs(
                isKeyframe: isKeyframe,
                fps: fps,
                dataFragmentCount: fragmentCount,
                burstDatagrams: currentUdpBurstLimit()
            )
            primary: for range in udpPacingBurstRanges(
                datagramCount: transmissions.count,
                maxDatagrams: currentUdpBurstLimit()
            ) {
                let elapsedUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
                if elapsedUs > sendDeadlineBudgetUs {
                    NSLog(
                        "Leftcar %@ AU send deadline exceeded %@: elapsed=%lluus fragments=%d isKeyframe=%@",
                        codecKind.rawValue.uppercased(),
                        targetLabel,
                        elapsedUs,
                        fragmentCount,
                        isKeyframe ? "true" : "false"
                    )
                    ok = false
                    break primary
                }
                let burstBytes = range.reduce(into: 0) { total, index in
                    total += transmissions[index].count
                }
                paceUdpDatagram(
                    bytes: burstBytes,
                    isKeyframe: isKeyframe,
                    // Pacing protects the socket regardless of profile. The
                    // fast interactive profile can still carry a moving
                    // video window, and its 4.4Mbps target was previously
                    // allowed to drain 20-25KiB AUs over several frame
                    // periods because this value was passed as zero.
                    accessUnitBytes: data.count,
                    dataFragmentCount: fragmentCount,
                    selectedParity: selectedParity
                )
                for index in range {
                    let datagram = transmissions[index]
                    let syscallStart = DispatchTime.now().uptimeNanoseconds
                    let sent = sendMediaDatagram(datagram, fd: fd, tileSide: tileSide)
                    let datagramSyscallUs = (
                        DispatchTime.now().uptimeNanoseconds &- syscallStart
                    ) / 1_000
                    sendSyscallUs &+= datagramSyscallUs
                    worstDatagramSyscallUs = max(worstDatagramSyscallUs, datagramSyscallUs)
                    if sent != datagram.count {
                        ok = false
                        break primary
                    }
                    sentBytes += sent
                    sentDatagramCount += 1
                    if datagram.first == 0x50 {
                        sentParityDatagramCount += 1
                    }
                }
            }
        } else {
            let syscallStart = DispatchTime.now().uptimeNanoseconds
            let sent = sendMediaDatagram(data, fd: fd, tileSide: tileSide)
            sendSyscallUs = (DispatchTime.now().uptimeNanoseconds &- syscallStart) / 1_000
            worstDatagramSyscallUs = sendSyscallUs
            ok = sent == data.count
            if ok {
                sentBytes = sent
            }
        }
        if isFrame {
            let auSendUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
            stateLock.lock()
            sentDatagrams &+= Int64(sentDatagramCount)
            sentParityDatagrams &+= Int64(sentParityDatagramCount)
            lastAuBytes = auBytes
            lastAuFragments = auFragmentCount
            lastAuParity = auParityCount
            lastAuDatagrams = UInt32(sentDatagramCount)
            lastAuExpectedDatagrams = UInt32(expectedDatagramCount)
            lastAuSendUs = auSendUs
            lastAuIsKeyframe = isKeyframe
            maxAuBytes = max(maxAuBytes, auBytes)
            maxAuFragments = max(maxAuFragments, auFragmentCount)
            stateLock.unlock()
            recordAccessUnitShape(bytes: auBytes, isKeyframe: isKeyframe, sendUs: auSendUs)
        }
        if !ok {
            networkLock.lock()
            pendingFrames.removeAll(keepingCapacity: true)
            pendingSplitAccessUnits.removeAll(keepingCapacity: true)
            networkRecoveryBoundary.establishAwaitingKeyframe()
            networkLock.unlock()
            stateLock.lock()
            framesDropped &+= isFrame ? 1 : 0
            stateLock.unlock()
            if isRecoveryKeyframe {
                clearRecoveryEncodeGate()
            }
            requestRecoveryKeyframe()
            return false
        }
        let sendPaceUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
        stateLock.lock()
        if isFrame, firstSendNs == nil {
            firstSendNs = DispatchTime.now().uptimeNanoseconds
            lifecycleState = "running"
            NSLog(
                "Leftcar first media frame sent %@: bytes=%d transport=%@",
                targetLabel,
                sentBytes,
                mediaTransport.rawValue
            )
        }
        bytesSent &+= Int64(sentBytes)
        rateWindowBytes &+= Int64(sentBytes)
        // Congestion votes read the per-datagram worst, not the AU total
        // (see worstDatagramSyscallUs above). The AU total keeps feeding the
        // rolling p95 diagnostics.
        lastSendBlockUs = worstDatagramSyscallUs
        maxSendBlockUs = max(maxSendBlockUs, worstDatagramSyscallUs)
        appendRollingSample(sendSyscallUs, to: &sendBlockSamplesUs)
        lastSendPaceUs = sendPaceUs
        maxSendPaceUs = max(maxSendPaceUs, sendPaceUs)
        appendRollingSample(sendPaceUs, to: &sendPaceSamplesUs)
        if isFrame && isKeyframe {
            lastRecoverySendNs = DispatchTime.now().uptimeNanoseconds
        }
        stateLock.unlock()
        if isFrame && isRecoveryKeyframe {
            recoveryKeyframeDidSend()
        }
        return true
    }

}
