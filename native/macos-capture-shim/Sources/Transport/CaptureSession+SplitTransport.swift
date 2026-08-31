import Foundation

struct SplitDatagramTransmission {
    let side: TileSide
    let datagram: Data
}

struct PreparedUdpAccessUnit {
    let datagrams: [Data]
    let payloadBytes: Int
    let fragmentCount: Int
    let parityCount: Int
}

struct SplitPairSendResult {
    let auID: UInt16
    let succeeded: Bool
    let attemptedDatagrams: Int
}

func interleaveSplitTransmissions(
    left: [Data],
    right: [Data]
) -> [SplitDatagramTransmission] {
    var output = [SplitDatagramTransmission]()
    output.reserveCapacity(left.count + right.count)

    let sharedCount = min(left.count, right.count)
    let leftPrefixCount = left.count - sharedCount
    let rightPrefixCount = right.count - sharedCount
    for datagram in left.prefix(leftPrefixCount) {
        output.append(.init(side: .left, datagram: datagram))
    }
    for datagram in right.prefix(rightPrefixCount) {
        output.append(.init(side: .right, datagram: datagram))
    }
    for offset in 0..<sharedCount {
        let leftIndex = leftPrefixCount + offset
        let rightIndex = rightPrefixCount + offset
        if left.count >= right.count {
            output.append(.init(side: .left, datagram: left[leftIndex]))
            output.append(.init(side: .right, datagram: right[rightIndex]))
        } else {
            output.append(.init(side: .right, datagram: right[rightIndex]))
            output.append(.init(side: .left, datagram: left[leftIndex]))
        }
    }
    return output
}

/// Reed-Solomon needs at least two data shards in the current wire format.
/// Every AU whose fragment count is `8n + 1` therefore has an unprotected
/// singleton tail group. Duplicate that tail once so one isolated Wi-Fi loss
/// cannot destroy the H.264 reference frame while FEC reports no unrecoverable
/// group. `Data` is copy-on-write, so this adds only one `sendto`.
func addSingletonFecTailUdpRedundancy(
    primary: [Data],
    assembled: [Data]
) -> [Data] {
    guard primary.count % 8 == 1, let datagram = primary.last else {
        return assembled
    }
    var protected = assembled
    protected.append(datagram)
    return protected
}

/// Keep an unprotected singleton tail and its duplicate out of the same paced
/// Wi-Fi burst. Split interleaving places one datagram per tile together, so a
/// two-datagram burst preserves pair locality while moving each tile's backup
/// into the following burst. Fault-injected single-tile sends must pace every
/// datagram independently to provide the same separation.
func splitPacingBurstLimit(
    configured: Int,
    leftFragmentCount: Int,
    rightFragmentCount: Int?
) -> Int {
    let boundedConfigured = max(1, configured)
    let hasSingletonTail = leftFragmentCount % 8 == 1
        || rightFragmentCount.map { $0 % 8 == 1 } == true
    guard hasSingletonTail else { return boundedConfigured }
    return min(boundedConfigured, rightFragmentCount == nil ? 1 : 2)
}

extension CaptureSession {
    func prepareUdpAccessUnit(
        _ data: Data,
        isKeyframe: Bool,
        parityOverride: Int? = nil
    ) -> PreparedUdpAccessUnit? {
        guard data.count > 21,
              data[0] == 0x47,
              data[3...4] == Data([0x4C, 0x32]) else {
            return nil
        }
        let maxPayload = udpMediaFragmentPayloadBytes
        let payloadCount = data.count - 21
        let fragmentCount = max(1, (payloadCount + maxPayload - 1) / maxPayload)
        guard fragmentCount <= Int(UInt16.max) else { return nil }

        var sendWallMs = UInt64(Date().timeIntervalSince1970 * 1_000.0).bigEndian
        var primary = [Data]()
        primary.reserveCapacity(fragmentCount)
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
            withUnsafeBytes(of: &sendWallMs) { datagram.append(contentsOf: $0) }
            datagram.append(contentsOf: data[start..<end])
            primary.append(datagram)
        }

        let auID = UInt16(data[1]) | (UInt16(data[2]) << 8)
        let selectedParity = appliedUdpStability.profile == .legacy
            ? nil
            : (parityOverride ?? currentUdpParityCount())
        let parity = mediaTransport == .udp
            && shouldProtectUdpAccessUnit(
                fragmentCount: fragmentCount,
                isKeyframe: isKeyframe
            )
            ? fecParityDatagrams(
                auID: auID,
                totalFragments: fragmentCount,
                wallMs: UInt64(Date().timeIntervalSince1970 * 1_000.0),
                payloads: primary.map { Data($0.dropFirst(33)) },
                reducedParity: contentMode == .video && !isKeyframe,
                recoveryParity: isKeyframe,
                parityOverride: selectedParity
            )
            : []

        var datagrams = [Data]()
        datagrams.reserveCapacity(primary.count + parity.count)
        var parityOffset = 0
        for base in stride(from: 0, to: fragmentCount, by: 8) {
            let end = min(fragmentCount, base + 8)
            datagrams.append(contentsOf: primary[base..<end])
            let requestedParity = selectedUdpParityCount(
                dataCount: end - base,
                reducedLegacyParity: contentMode == .video && !isKeyframe,
                recovery: isKeyframe,
                parityOverride: selectedParity
            )
            let available = max(0, parity.count - parityOffset)
            let appendCount = min(requestedParity, available)
            if appendCount > 0 {
                datagrams.append(contentsOf: parity[parityOffset..<(parityOffset + appendCount)])
                parityOffset += appendCount
            }
        }
        return PreparedUdpAccessUnit(
            datagrams: addSingletonFecTailUdpRedundancy(
                primary: primary,
                assembled: datagrams
            ),
            payloadBytes: payloadCount,
            fragmentCount: fragmentCount,
            parityCount: parity.count
        )
    }

    @discardableResult
    func writeSplitPacketPair(
        _ accessUnit: PendingSplitAccessUnit
    ) -> SplitPairSendResult {
        let auID = splitWireSequence.allocate()
        stateLock.lock()
        splitWirePairsAttempted &+= 1
        stateLock.unlock()

        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else {
            stateLock.lock()
            splitWirePairSendFailures &+= 1
            stateLock.unlock()
            return .init(auID: auID, succeeded: false, attemptedDatagrams: 0)
        }

        if let config = accessUnit.left.config,
           !writePacket(config, tileSide: .left) {
            stateLock.lock()
            splitWirePairSendFailures &+= 1
            stateLock.unlock()
            return .init(auID: auID, succeeded: false, attemptedDatagrams: 0)
        }
        if !accessUnit.dropRightForTest,
           let config = accessUnit.right.config,
           !writePacket(config, tileSide: .right) {
            stateLock.lock()
            splitWirePairSendFailures &+= 1
            stateLock.unlock()
            return .init(auID: auID, succeeded: false, attemptedDatagrams: 0)
        }

        let leftFrame = splitWireFrame(payload: accessUnit.left, auID: auID)
        let rightFrame = splitWireFrame(payload: accessUnit.right, auID: auID)
        if accessUnit.isRecoveryKeyframe {
            observeUdpRecoveryBoundary()
        }
        let selectedParity = appliedUdpStability.profile == .legacy
            ? nil
            : currentUdpParityCount()
        guard let leftUnit = prepareUdpAccessUnit(
            leftFrame,
            isKeyframe: accessUnit.isKeyframe,
            parityOverride: selectedParity
        ) else {
            stateLock.lock()
            splitWirePairSendFailures &+= 1
            stateLock.unlock()
            return .init(auID: auID, succeeded: false, attemptedDatagrams: 0)
        }
        let rightUnit: PreparedUdpAccessUnit?
        if accessUnit.dropRightForTest {
            rightUnit = nil
        } else {
            rightUnit = prepareUdpAccessUnit(
                rightFrame,
                isKeyframe: accessUnit.isKeyframe,
                parityOverride: selectedParity
            )
            guard rightUnit != nil else {
                stateLock.lock()
                splitWirePairSendFailures &+= 1
                stateLock.unlock()
                return .init(auID: auID, succeeded: false, attemptedDatagrams: 0)
            }
        }

        let startedNs = DispatchTime.now().uptimeNanoseconds
        let keyframe = accessUnit.isKeyframe
        let recoveryKeyframe = accessUnit.isRecoveryKeyframe
        let aggregateBytes = leftUnit.payloadBytes + (rightUnit?.payloadBytes ?? 0)
        let aggregateFragments = leftUnit.fragmentCount + (rightUnit?.fragmentCount ?? 0)
        let transmissions = rightUnit.map {
            interleaveSplitTransmissions(
                left: leftUnit.datagrams,
                right: $0.datagrams
            )
        } ?? leftUnit.datagrams.map {
            SplitDatagramTransmission(side: .left, datagram: $0)
        }
        if keyframe {
            NSLog(
                "Leftcar split recovery pair %@: bytes=%d leftFragments=%d rightFragments=%d transmissions=%d",
                targetLabel,
                aggregateBytes,
                leftUnit.fragmentCount,
                rightUnit?.fragmentCount ?? 0,
                transmissions.count
            )
        }

        var sentBytes = 0
        var sentDatagrams = 0
        var sentParity = 0
        var sendSyscallUs: UInt64 = 0
        var succeeded = true
        let burstLimit = splitPacingBurstLimit(
            configured: currentUdpBurstLimit(),
            leftFragmentCount: leftUnit.fragmentCount,
            rightFragmentCount: rightUnit?.fragmentCount
        )
        sendLoop: for range in udpPacingBurstRanges(
            datagramCount: transmissions.count,
            maxDatagrams: burstLimit
        ) {
            let burstBytes = range.reduce(into: 0) { total, index in
                total += transmissions[index].datagram.count
            }
            paceUdpDatagram(
                bytes: burstBytes,
                isKeyframe: keyframe,
                accessUnitBytes: aggregateBytes,
                dataFragmentCount: aggregateFragments,
                selectedParity: selectedParity
            )
            for index in range {
                let transmission = transmissions[index]
                let syscallStart = DispatchTime.now().uptimeNanoseconds
                let sent = sendMediaDatagram(
                    transmission.datagram,
                    fd: fd,
                    tileSide: transmission.side
                )
                sendSyscallUs &+= (
                    DispatchTime.now().uptimeNanoseconds &- syscallStart
                ) / 1_000
                guard sent == transmission.datagram.count else {
                    succeeded = false
                    break sendLoop
                }
                sentBytes += sent
                sentDatagrams += 1
                if transmission.datagram.first == 0x50 { sentParity += 1 }
            }
        }

        let sendUs = (DispatchTime.now().uptimeNanoseconds &- startedNs) / 1_000
        stateLock.lock()
        self.sentDatagrams &+= Int64(sentDatagrams)
        sentParityDatagrams &+= Int64(sentParity)
        lastAuBytes = UInt64(aggregateBytes)
        lastAuFragments = UInt32(aggregateFragments)
        lastAuParity = UInt32(leftUnit.parityCount + (rightUnit?.parityCount ?? 0))
        lastAuDatagrams = UInt32(sentDatagrams)
        lastAuExpectedDatagrams = UInt32(transmissions.count)
        lastAuSendUs = sendUs
        lastAuIsKeyframe = keyframe
        maxAuBytes = max(maxAuBytes, UInt64(aggregateBytes))
        maxAuFragments = max(maxAuFragments, UInt32(aggregateFragments))
        stateLock.unlock()
        recordAccessUnitShape(
            bytes: UInt64(aggregateBytes),
            isKeyframe: keyframe,
            sendUs: sendUs
        )

        guard succeeded else {
            stateLock.lock()
            framesDropped &+= 1
            splitWirePairSendFailures &+= 1
            stateLock.unlock()
            return .init(
                auID: auID,
                succeeded: false,
                attemptedDatagrams: transmissions.count
            )
        }

        stateLock.lock()
        if firstSendNs == nil {
            firstSendNs = DispatchTime.now().uptimeNanoseconds
            lifecycleState = "running"
            NSLog(
                "Leftcar first split media pair sent %@: bytes=%d",
                targetLabel,
                sentBytes
            )
        }
        bytesSent &+= Int64(sentBytes)
        rateWindowBytes &+= Int64(sentBytes)
        lastSendBlockUs = sendSyscallUs
        maxSendBlockUs = max(maxSendBlockUs, sendSyscallUs)
        appendRollingSample(sendSyscallUs, to: &sendBlockSamplesUs)
        lastSendPaceUs = sendUs
        maxSendPaceUs = max(maxSendPaceUs, sendUs)
        appendRollingSample(sendUs, to: &sendPaceSamplesUs)
        if keyframe { lastRecoverySendNs = DispatchTime.now().uptimeNanoseconds }
        stateLock.unlock()
        if recoveryKeyframe { recoveryKeyframeDidSend() }
        return .init(
            auID: auID,
            succeeded: true,
            attemptedDatagrams: transmissions.count
        )
    }
}
