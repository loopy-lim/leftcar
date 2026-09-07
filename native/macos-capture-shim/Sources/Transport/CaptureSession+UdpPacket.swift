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
            // Logical L2 header: marker + AU id LE + capture/encode clocks.
            guard data.count > 21, data[0] == 0x47, data[3...4] == Data([0x4C, 0x32]) else {
                if isRecoveryKeyframe {
                    clearRecoveryEncodeGate()
                }
                requestRecoveryKeyframe()
                return false
            }
            let maxPayload = udpMediaFragmentPayloadBytes
            let payloadCount = data.count - 21
            let fragmentCount = max(1, (payloadCount + maxPayload - 1) / maxPayload)
            guard fragmentCount <= Int(UInt16.max) else {
                if isRecoveryKeyframe {
                    clearRecoveryEncodeGate()
                }
                requestRecoveryKeyframe()
                return false
            }
            var sendWallMsBE = UInt64(Date().timeIntervalSince1970 * 1_000.0).bigEndian
            var primaryDatagrams = [Data]()
            primaryDatagrams.reserveCapacity(fragmentCount)
            for index in 0..<fragmentCount {
                let start = 21 + index * maxPayload
                let end = min(data.count, start + maxPayload)
                var datagram = Data(capacity: 33 + end - start)
                datagram.append(0x47)
                var indexBE = UInt16(index).bigEndian
                var countBE = UInt16(fragmentCount).bigEndian
                withUnsafeBytes(of: &indexBE) { datagram.append(contentsOf: $0) }
                withUnsafeBytes(of: &countBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: data[1...20])
                withUnsafeBytes(of: &sendWallMsBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: data[start..<end])
                primaryDatagrams.append(datagram)
            }
            let auID = UInt16(data[1]) | (UInt16(data[2]) << 8)
            if isRecoveryKeyframe {
                observeUdpRecoveryBoundary()
            }
            let selectedParity = appliedUdpStability.profile == .legacy
                ? nil
                : currentUdpParityCount()
            // Protect every multi-fragment UDP AU. A single lost fragment
            // otherwise invalidates the whole H.264 access unit and starts an
            // IDR recovery loop. The parity math uses a lookup table and
            // precomputed row coefficients so this stays off the capture and
            // encoder queues while remaining cheap enough for 60fps deltas.
            let parityDatagrams = mediaTransport == .udp
                && shouldProtectUdpAccessUnit(
                    fragmentCount: fragmentCount,
                    isKeyframe: isKeyframe
                )
                ? fecParityDatagrams(
                    auID: auID,
                    totalFragments: fragmentCount,
                    wallMs: UInt64(Date().timeIntervalSince1970 * 1_000.0),
                    payloads: primaryDatagrams.map { Data($0.dropFirst(33)) },
                    reducedParity: contentMode == .video && !isKeyframe,
                    recoveryParity: isKeyframe,
                    parityOverride: selectedParity
                )
                : []
            auBytes = UInt64(payloadCount)
            auFragmentCount = UInt32(fragmentCount)
            auParityCount = UInt32(parityDatagrams.count)
            // Send each FEC group before its parity. This keeps parity close
            // to the fragments it protects and avoids losing an entire AU's
            // recovery budget to one large primary burst. The shared UDP
            // pacer spreads both ordinary frames and recovery IDRs.
            var transmissions = [Data]()
            transmissions.reserveCapacity(fragmentCount + parityDatagrams.count)
            var parityOffset = 0
            for base in stride(from: 0, to: fragmentCount, by: 8) {
                let end = min(fragmentCount, base + 8)
                transmissions.append(contentsOf: primaryDatagrams[base..<end])
                let parityCount = selectedUdpParityCount(
                    dataCount: end - base,
                    reducedLegacyParity: contentMode == .video && !isKeyframe,
                    recovery: isKeyframe,
                    parityOverride: selectedParity
                )
                if !mediaTransport.usesTCP && parityCount > 0 {
                    // A parity datagram may be omitted when it cannot fit the
                    // MTU-safe envelope. Never let a malformed/oversized FEC
                    // group abort the serial network queue with an array
                    // bounds trap; the protected primary fragments are still
                    // useful and the next IDR can recover the decoder.
                    let available = max(0, parityDatagrams.count - parityOffset)
                    let appendCount = min(parityCount, available)
                    if appendCount > 0 {
                        transmissions.append(contentsOf: parityDatagrams[parityOffset..<(parityOffset + appendCount)])
                        parityOffset += appendCount
                    }
                }
            }
            transmissions = addSingletonFecTailUdpRedundancy(
                primary: primaryDatagrams,
                assembled: transmissions
            )
            expectedDatagramCount = transmissions.count
            if isKeyframe {
                NSLog(
                    "Leftcar recovery AU %@: bytes=%d fragments=%d parity=%d transmissions=%d",
                    targetLabel,
                    data.count,
                    fragmentCount,
                    parityDatagrams.count,
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
