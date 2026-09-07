import Foundation
import CoreVideo
import CoreMedia
import VideoToolbox

func splitPipelineStartupDecision(
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    mediaTransport: String,
    hasEncoderPixelBufferPool: Bool,
    dualAveCapabilityAvailable: Bool = false,
    environment: [String: String] = ProcessInfo.processInfo.environment
) -> EncoderExperimentStartupDecision {
    encoderExperimentStartupDecision(
        requested: .splitVertical,
        width: width,
        height: height,
        fps: fps,
        mediaTransport: mediaTransport,
        hasEncoderPixelBufferPool: hasEncoderPixelBufferPool,
        splitDiagnosticEnabled: splitDiagnosticEnabled(environment: environment)
            || dualAveCapabilityAvailable
    )
}

extension CaptureSession {
    func encodeSplitFrame(
        _ captured: PendingCaptureFrame,
        lease: SplitFlowLease
    ) {
        guard requestedEncoderExperiment == .splitVertical else {
            _ = finishSplitFlowLease(lease)
            completeEncodeSlot()
            return
        }
        if splitPipeline == nil {
            let decision = splitPipelineStartupDecision(
                width: outWidth,
                height: outHeight,
                fps: fps,
                mediaTransport: mediaTransport.rawValue,
                hasEncoderPixelBufferPool: splitTilePixelBufferPoolAvailable(),
                dualAveCapabilityAvailable: dualAveTileEncoderPairAvailable()
            )
            guard case .success = decision else {
                _ = finishSplitFlowLease(lease)
                completeEncodeSlot()
                let reason: String
                if case let .failure(message) = decision {
                    reason = "splitVertical startup denied: \(message)"
                } else {
                    reason = "splitVertical startup denied"
                }
                setLastError(reason)
                markStopped(reason)
                return
            }
            do {
                let aggregateBitrate = initialSplitAggregateBitrate()
                let strategy = SplitEncoderStrategy.parse()
                splitPipeline = try DualEncoderPipeline(
                    queue: encodeQueue,
                    fps: fps,
                    aggregateBitrate: aggregateBitrate,
                    strategy: strategy
                ) { [weak self] event in
                    self?.handleSplitPipelineEvent(event)
                }
                stateLock.lock()
                maxEncodeInFlight = splitEncoderInFlightLimit(fps: fps)
                currentAverageBitrate = aggregateBitrate
                appliedEncoderExperimentValue = .splitVertical
                encoderMode = strategy.encoderMode
                encoderProfile = "H264_Main_AutoLevel_CAVLC"
                encoderPreset = "VideoConferencing"
                stateLock.unlock()
                if strategy.isDiagnostic {
                    NSLog(
                        "Leftcar split encoder diagnostic mode %@",
                        strategy.rawValue
                    )
                }
            } catch {
                _ = finishSplitFlowLease(lease)
                completeEncodeSlot()
                let reason = "splitVertical encoder setup failed: \(error)"
                setLastError(reason)
                markStopped(reason)
                return
            }
        }
        guard let splitPipeline else {
            _ = finishSplitFlowLease(lease)
            completeEncodeSlot()
            return
        }
        if splitPipeline.submit(captured: captured, lease: lease) == .dropPair {
            stateLock.lock()
            splitPairAdmissionDrops &+= 1
            framesDropped &+= 1
            stateLock.unlock()
            _ = finishSplitFlowLease(lease)
            completeEncodeSlot()
        }
    }

    @discardableResult
    func finishSplitFlowLease(_ lease: SplitFlowLease) -> Bool {
        captureLock.lock()
        let completed = splitFlowState.complete(lease)
        if completed, !splitFlowState.recoveryBoundaryPending {
            splitRecoveryGateStartedNs = 0
        }
        let shouldSchedule = completed
            && hasPendingCaptureLocked()
            && !encodeScheduled
            && encodeInFlight < maxEncodeInFlight
            && !recoveryEncodeInFlight
        if shouldSchedule {
            encodeScheduled = true
        }
        captureLock.unlock()
        if shouldSchedule {
            encodeQueue.async { [weak self] in
                self?.drainEncodeQueue()
            }
        }
        return completed
    }

    func splitFlowAccepts(_ lease: SplitFlowLease) -> Bool {
        captureLock.lock()
        let accepted = splitFlowState.accepts(lease)
        captureLock.unlock()
        return accepted
    }

    func requestSplitPairedKeyframe(reason: String) {
        encodeQueue.async { [weak self] in
            self?.splitPipeline?.requestPairedKeyframe(reason: reason)
        }
    }

    func invalidateSplitPipeline() {
        let invalidate = { [weak self] in
            self?.splitPipeline?.invalidate()
            self?.splitPipeline = nil
        }
        if DispatchQueue.getSpecific(key: encodeQueueKey) != nil {
            invalidate()
        } else {
            encodeQueue.sync(execute: invalidate)
        }
    }

    private func initialSplitAggregateBitrate() -> Int {
        let pixelsPerSecond = Double(outWidth) * Double(outHeight) * Double(fps)
        let ideal = Int((pixelsPerSecond * 0.085).rounded())
        return min(60_000_000, max(30_000_000, ideal))
    }

    private func splitTilePixelBufferPoolAvailable() -> Bool {
        let attributes: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String:
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferWidthKey as String: 1_920,
            kCVPixelBufferHeightKey as String: 2_160,
            kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
            kCVPixelBufferMetalCompatibilityKey as String: true,
        ]
        var pool: CVPixelBufferPool?
        guard CVPixelBufferPoolCreate(
            kCFAllocatorDefault,
            nil,
            attributes as CFDictionary,
            &pool
        ) == kCVReturnSuccess, let pool else {
            return false
        }
        var pixelBuffer: CVPixelBuffer?
        return CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            pool,
            &pixelBuffer
        ) == kCVReturnSuccess && pixelBuffer != nil
    }

    private func handleSplitPipelineEvent(_ event: DualEncoderPipelineEvent) {
        switch event {
        case let .configured(
            leftEncoderID,
            rightEncoderID,
            leftHardware,
            rightHardware
        ):
            stateLock.lock()
            splitLeftEncoderID = leftEncoderID
            splitRightEncoderID = rightEncoderID
            splitLeftHardware = leftHardware
            splitRightHardware = rightHardware
            encoderID = "left=\(leftEncoderID),right=\(rightEncoderID)"
            encoderHardwareAccelerated = leftHardware && rightHardware
            stateLock.unlock()
        case let .prepared(microseconds):
            stateLock.lock()
            appendRollingSample(microseconds, to: &splitPreparationSamplesUs)
            // Populate the generic input-preparation gauges for split mode;
            // Metal NV12 splitting is the split pipeline's input preparation.
            lastInputPreparationUs = microseconds
            maxInputPreparationUs = max(maxInputPreparationUs, microseconds)
            appendRollingSample(microseconds, to: &inputPreparationSamplesUs)
            stateLock.unlock()
        case let .encodedPair(lease, left, right):
            recordSplitPair(left: left, right: right)
            completeEncodeSlot()
            guard let lease else {
                beginSplitTransportRecovery(
                    reason: "encoded split pair missing flow lease",
                    invalidatePendingBoundary: true
                )
                return
            }
            packetizationQueue.async { [weak self] in
                self?.packetizeSplitPair(lease: lease, left: left, right: right)
            }
        case let .injectedRightDrop(lease, left):
            stateLock.lock()
            splitInjectedRightDrops &+= 1
            splitPairDrops &+= 1
            framesDropped &+= 1
            stateLock.unlock()
            completeEncodeSlot()
            guard let lease else {
                beginSplitTransportRecovery(
                    reason: "injected split pair missing flow lease",
                    invalidatePendingBoundary: true
                )
                return
            }
            packetizationQueue.async { [weak self] in
                guard let self else { return }
                if let payload = self.packetizeSplitTile(left) {
                    self.enqueueSplitFaultLeftOnly(
                        lease: lease,
                        left: left,
                        payload: payload
                    )
                } else {
                    self.beginSplitTransportRecovery(
                        reason: "injected split packetization failed",
                        invalidatePendingBoundary: true
                    )
                }
            }
        case let .dropped(reason, releasedPairs, releasedLeases):
            stateLock.lock()
            splitPairDrops &+= Int64(max(1, releasedPairs))
            framesDropped &+= Int64(max(1, releasedPairs))
            splitLastPairDropReason = reason
            if reason.contains("timeout") {
                splitPairTimeouts &+= 1
            }
            stateLock.unlock()
            if releasedPairs > 0 {
                completeEncodeSlots(releasedPairs)
            }
            if !releasedLeases.isEmpty {
                beginSplitTransportRecovery(
                    reason: reason,
                    invalidatePendingBoundary: true
                )
            }
            NSLog("Leftcar split pair dropped %@: %@", targetLabel, reason)
        }
    }

    private func recordSplitPair(
        left: TileEncodedSample,
        right: TileEncodedSample
    ) {
        let nowNs = DispatchTime.now().uptimeNanoseconds
        let pairLatencyUs = nowNs >= left.captureNs
            ? (nowNs - left.captureNs) / 1_000
            : 0
        stateLock.lock()
        splitPairsEncoded &+= 1
        splitLeftOutputs &+= 1
        splitRightOutputs &+= 1
        framesEncoded &+= 1
        encodeOutputCallbacks &+= 1
        rateWindowFrames &+= 1
        rateWindowEncodeOutputCallbacks &+= 1
        let shouldAdaptBitrate = shouldRunAdaptiveBitrate(
            encodedFrames: framesEncoded,
            fps: fps
        )
        appendRollingSample(pairLatencyUs, to: &splitPairCallbackSamplesUs)
        // Populate the generic encode-output gauges for split mode: paired
        // output callbacks are the split pipeline's encoder output stream.
        if let previous = lastEncodeOutputCallbackNs, nowNs >= previous {
            appendRollingSample(
                (nowNs - previous) / 1_000,
                to: &encodeOutputIntervalSamplesUs
            )
        }
        lastEncodeOutputCallbackNs = nowNs
        appendRollingSample(pairLatencyUs, to: &encodeOutputSamplesUs)
        if firstEncodeNs == nil {
            firstEncodeNs = nowNs
            lifecycleState = "waiting_first_send"
        }
        stateLock.unlock()
        if shouldAdaptBitrate {
            adaptBitrateIfNeeded()
        }
    }

    private func packetizeSplitPair(
        lease: SplitFlowLease,
        left: TileEncodedSample,
        right: TileEncodedSample
    ) {
        guard splitFlowAccepts(lease) else {
            stateLock.lock()
            splitRecoveryBoundaryDiscards &+= 1
            stateLock.unlock()
            return
        }
        guard let leftPayload = packetizeSplitTile(left),
              let rightPayload = packetizeSplitTile(right) else {
            stateLock.lock()
            splitPairDrops &+= 1
            framesDropped &+= 1
            stateLock.unlock()
            beginSplitTransportRecovery(
                reason: "split packetization failed",
                invalidatePendingBoundary: true
            )
            return
        }
        let bothKeyframes = left.isKeyframe && right.isKeyframe
        enqueueSplitAccessUnit(
            PendingSplitAccessUnit(
                sequence: left.frameSequence,
                generation: lease.generation,
                lease: lease,
                left: leftPayload,
                right: rightPayload,
                isKeyframe: bothKeyframes,
                isRecoveryKeyframe: lease.isRecoveryBoundary && bothKeyframes,
                queuedNs: DispatchTime.now().uptimeNanoseconds
            )
        )
    }

    private func packetizeSplitTile(
        _ encoded: TileEncodedSample
    ) -> SplitEncodedPayload? {
        let sample = encoded.sampleBuffer
        // Configuration is stable for the lifetime of each tile encoder.
        // Re-send it at every paired IDR/recovery boundary, not on all 60
        // samples per second where it would add avoidable allocations and
        // two extra datagrams to the network queue.
        let config = encoded.isKeyframe ? splitCodecConfig(sample: sample) : nil
        guard let block = CMSampleBufferGetDataBuffer(sample) else { return nil }
        let totalLength = CMBlockBufferGetDataLength(block)
        guard totalLength > 0 else { return nil }
        var avcc = Data(count: totalLength)
        let copyStatus = avcc.withUnsafeMutableBytes { raw in
            CMBlockBufferCopyDataBytes(
                block,
                atOffset: 0,
                dataLength: totalLength,
                destination: raw.baseAddress!
            )
        }
        guard copyStatus == noErr else { return nil }

        var annexB = Data()
        var offset = 0
        while offset + 4 <= avcc.count {
            let length = (Int(avcc[offset]) << 24)
                | (Int(avcc[offset + 1]) << 16)
                | (Int(avcc[offset + 2]) << 8)
                | Int(avcc[offset + 3])
            guard length >= 0, offset + 4 + length <= avcc.count else { return nil }
            annexB.append(contentsOf: [0, 0, 0, 1])
            annexB.append(avcc[(offset + 4)..<(offset + 4 + length)])
            offset += 4 + length
        }
        guard offset == avcc.count else { return nil }

        return SplitEncodedPayload(
            config: config,
            annexB: annexB,
            captureWallMs: encoded.captureWallMs,
            encodeWallMs: UInt64(Date().timeIntervalSince1970 * 1_000.0)
        )
    }

    private func splitCodecConfig(sample: CMSampleBuffer) -> Data? {
        guard let format = sample.formatDescription else { return nil }
        var config = Data([0x43, 0x46, 0x47])
        for index in 0..<2 {
            var pointer: UnsafePointer<UInt8>?
            var size = 0
            let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format,
                parameterSetIndex: index,
                parameterSetPointerOut: &pointer,
                parameterSetSizeOut: &size,
                parameterSetCountOut: nil,
                nalUnitHeaderLengthOut: nil
            )
            guard status == noErr, let pointer, size > 0 else { return nil }
            var length = UInt32(size + 4).bigEndian
            withUnsafeBytes(of: &length) { config.append(contentsOf: $0) }
            config.append(contentsOf: [0, 0, 0, 1])
            config.append(UnsafeBufferPointer(start: pointer, count: size))
        }
        return config
    }

    private func enqueueSplitFaultLeftOnly(
        lease: SplitFlowLease,
        left: TileEncodedSample,
        payload: SplitEncodedPayload
    ) {
        guard splitFlowAccepts(lease) else {
            stateLock.lock()
            splitRecoveryBoundaryDiscards &+= 1
            stateLock.unlock()
            return
        }
        enqueueSplitAccessUnit(
            PendingSplitAccessUnit(
                sequence: left.frameSequence,
                generation: lease.generation,
                lease: lease,
                left: payload,
                right: payload,
                isKeyframe: left.isKeyframe,
                isRecoveryKeyframe: lease.isRecoveryBoundary && left.isKeyframe,
                queuedNs: DispatchTime.now().uptimeNanoseconds,
                dropRightForTest: true
            )
        )
    }

    private func enqueueSplitAccessUnit(_ accessUnit: PendingSplitAccessUnit) {
        guard splitFlowAccepts(accessUnit.lease) else {
            stateLock.lock()
            splitRecoveryBoundaryDiscards &+= 1
            stateLock.unlock()
            return
        }
        captureLock.lock()
        let capacity = splitFlowState.capacity
        captureLock.unlock()

        networkLock.lock()
        pendingSplitAccessUnits.append(accessUnit)
        pendingSplitAccessUnits.sort { $0.sequence < $1.sequence }
        let invariantFailed = pendingSplitAccessUnits.count > capacity
        if invariantFailed {
            pendingSplitAccessUnits.removeAll(keepingCapacity: true)
        }
        let schedule = !networkDrainScheduled && !invariantFailed
        if schedule { networkDrainScheduled = true }
        networkLock.unlock()

        if invariantFailed {
            stateLock.lock()
            splitPostEncodeDeltaDrops &+= 1
            framesDropped &+= 1
            stateLock.unlock()
            let reason = "split encoded queue exceeded flow capacity"
            setLastError(reason)
            markStopped(reason)
            return
        }
        if schedule {
            networkQueue.async { [weak self] in self?.drainNetwork() }
        }
    }
}
