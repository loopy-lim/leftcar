import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Security
import Darwin
import OSLog

extension CaptureSession {
    // MARK: Packetization & TCP Transmission

    @discardableResult
     func recordValidEncoderOutput(
        pts: Int64,
        callbackNs: UInt64,
        generation: UInt64,
        callbackLatencyUs: UInt64
    ) -> Bool {
        stateLock.lock()
        guard generation == encoderSessionGeneration else {
            stateLock.unlock()
            return false
        }
        encodeOutputCallbacks &+= 1
        encoderSessionOutputCallbacks &+= 1
        rateWindowEncodeOutputCallbacks &+= 1
        if let previous = lastEncodeOutputCallbackNs, callbackNs >= previous {
            appendRollingSample(
                (callbackNs - previous) / 1_000,
                to: &encodeOutputIntervalSamplesUs
            )
        }
        lastEncodeOutputCallbackNs = callbackNs
        if appliedEncoderExperimentValue == .adaptiveQp {
            adaptiveQpWindowValidOutputFrames &+= 1
        }
        if firstEncodeNs == nil {
            firstEncodeNs = callbackNs
            lifecycleState = "waiting_first_send"
            NSLog("Leftcar first encoded frame %@", targetLabel)
        }
        if let captureNs = captureNsByPts[pts] {
            let elapsedUs = (callbackNs &- captureNs) / 1_000
            lastCaptureToEncodeUs = elapsedUs
            maxCaptureToEncodeUs = max(maxCaptureToEncodeUs, elapsedUs)
            appendRollingSample(elapsedUs, to: &captureToEncodeSamplesUs)
        }
        lastEncodeOutputUs = callbackLatencyUs
        maxEncodeOutputUs = max(maxEncodeOutputUs, callbackLatencyUs)
        appendRollingSample(callbackLatencyUs, to: &encodeOutputSamplesUs)
        stateLock.unlock()
        return true
    }

     func recordPacketizationQueueWait(startNs: UInt64, callbackNs: UInt64) {
        guard startNs >= callbackNs else { return }
        let elapsedUs = (startNs - callbackNs) / 1_000
        stateLock.lock()
        lastPacketizationQueueWaitUs = elapsedUs
        maxPacketizationQueueWaitUs = max(maxPacketizationQueueWaitUs, elapsedUs)
        appendRollingSample(elapsedUs, to: &packetizationQueueWaitSamplesUs)
        stateLock.unlock()
    }

     func recordPacketizationDuration(startNs: UInt64) {
        let nowNs = DispatchTime.now().uptimeNanoseconds
        guard nowNs >= startNs else { return }
        let elapsedUs = (nowNs - startNs) / 1_000
        stateLock.lock()
        lastPacketizationUs = elapsedUs
        maxPacketizationUs = max(maxPacketizationUs, elapsedUs)
        appendRollingSample(elapsedUs, to: &packetizationSamplesUs)
        stateLock.unlock()
    }

     func recoveryBoundaryPendingForPacketization() -> Bool {
        stateLock.lock()
        let statePending = recoveryKeyframePending
            || recoveryDropRetryState.hasPendingRetry
        stateLock.unlock()

        networkLock.lock()
        let networkPending = networkRecoveryBoundary.awaitingKeyframe
            || networkKeyframeInFlight
            || pendingFrames.contains(where: { $0.isKeyframe })
            || pendingSplitAccessUnits.contains(where: { $0.isKeyframe })
        networkLock.unlock()

        return statePending || networkPending
    }

     func beginPacketizationWork(
        recoveryBoundaryPending: Bool
    ) -> PacketizationAdmissionDecision {
        stateLock.lock()
        let decision = packetizationAdmissionState.admit(
            recoveryBoundaryPending: recoveryBoundaryPending
        )
        stateLock.unlock()
        return decision
    }

     func finishPacketizationWork() {
        stateLock.lock()
        packetizationAdmissionState.finish()
        stateLock.unlock()
    }

     func recordPacketizationAdmissionDrop(
        pts: Int64,
        recoveryBoundaryPending: Bool
    ) {
        stateLock.lock()
        packetizationAdmissionDrops &+= 1
        framesDropped &+= 1
        networkQueueDropped &+= 1
        if recoveryBoundaryPending {
            recoveryFramesDropped &+= 1
        }
        captureNsByPts.removeValue(forKey: pts)
        captureWallMsByPts.removeValue(forKey: pts)
        encodeAuIdByPts.removeValue(forKey: pts)
        stateLock.unlock()
    }

     func enqueueEncodedSampleForPacketization(
        _ encodedSample: CMSampleBuffer,
        requestedKeyframe: Bool,
        callbackNs: UInt64,
        generation: UInt64
    ) {
        let recoveryBoundaryPending = recoveryBoundaryPendingForPacketization()
        let admission = beginPacketizationWork(
            recoveryBoundaryPending: recoveryBoundaryPending
        )
        guard admission == .admit else {
            let encodedPts = CMSampleBufferGetPresentationTimeStamp(encodedSample).value
            let dropDuringRecovery = admission == .dropDuringRecovery
                || requestedKeyframe
            recordPacketizationAdmissionDrop(
                pts: encodedPts,
                recoveryBoundaryPending: dropDuringRecovery
            )
            if requestedKeyframe {
                scheduleRecoveryAfterPacketizationLoss(
                    generation: generation
                )
            } else if admission == .dropAndBeginRecovery {
                beginNetworkRecovery()
            }
            return
        }
        let process = { [weak self] in
            guard let self else { return }
            defer { self.finishPacketizationWork() }
            guard self.isCurrentEncoderGeneration(generation) else { return }
            let packetizationStartNs = DispatchTime.now().uptimeNanoseconds
            self.recordPacketizationQueueWait(
                startNs: packetizationStartNs,
                callbackNs: callbackNs
            )
            self.handleEncoded(
                encodedSample,
                requestedKeyframe: requestedKeyframe,
                packetizationStartNs: packetizationStartNs,
                generation: generation
            )
        }
        if shouldOffloadEncodedSample(width: outWidth, height: outHeight) {
            packetizationQueue.async(execute: process)
        } else {
            process()
        }
    }

     func handleEncoded(
        _ sample: CMSampleBuffer,
        requestedKeyframe: Bool,
        packetizationStartNs: UInt64,
        generation: UInt64
    ) {
        defer {
            recordPacketizationDuration(startNs: packetizationStartNs)
        }
        guard isRunning else {
            if requestedKeyframe {
                clearRecoveryEncodeGate()
            }
            return
        }
        let encodedPts = CMSampleBufferGetPresentationTimeStamp(sample).value
        stateLock.lock()
        let auId = encodeAuIdByPts.removeValue(forKey: encodedPts)
        let captureWallMs = captureWallMsByPts.removeValue(forKey: encodedPts)
        captureNsByPts.removeValue(forKey: encodedPts)
        stateLock.unlock()
        guard let auId, let captureWallMs else {
            // An output callback arriving after its bookkeeping window is
            // still safer to drop than to emit a duplicate AU id. The next
            // frame will carry a forced IDR and restore decoder continuity.
            stateLock.lock()
            forceKeyframe = true
            csdSent = false
            stateLock.unlock()
            if requestedKeyframe {
                clearRecoveryEncodeGate()
            }
            return
        }

        // Send parameter sets (csd: SPS/PPS) periodically so a viewer that
        // joins late (or restarted its decoder) can configure before the
        // next keyframe.
        let attachments = CMSampleBufferGetSampleAttachmentsArray(
            sample,
            createIfNecessary: false
        ) as? [[String: Any]]
        let notSync = attachments?.first?[kCMSampleAttachmentKey_NotSync as String] as? Bool
        let isKeyframe = notSync != true

        stateLock.lock()
        let shouldSendConfig = shouldSendCodecConfig(csdSent: csdSent)
        if shouldSendConfig {
            // Reserve this config send while holding the lock so concurrent
            // VideoToolbox callbacks do not all enqueue the same CSD packet.
            csdSent = true
        }
        stateLock.unlock()

        if shouldSendConfig, let fd = sample.formatDescription {
            var cfg = codecKind == .hevc
                ? Data([0x43, 0x46, 0x32, codecKind.id]) // "CF2", codec id
                : Data([0x43, 0x46, 0x47]) // legacy "CFG" = H.264
            var idx = 0
            while idx < codecKind.parameterSetCount {
                var ptr: UnsafePointer<UInt8>? = nil
                var size = 0
                let status: OSStatus
                switch codecKind {
                case .h264:
                    status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                        fd,
                        parameterSetIndex: idx,
                        parameterSetPointerOut: &ptr,
                        parameterSetSizeOut: &size,
                        parameterSetCountOut: nil,
                        nalUnitHeaderLengthOut: nil
                    )
                case .hevc:
                    status = CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                        fd,
                        parameterSetIndex: idx,
                        parameterSetPointerOut: &ptr,
                        parameterSetSizeOut: &size,
                        parameterSetCountOut: nil,
                        nalUnitHeaderLengthOut: nil
                    )
                }
                if status != noErr { break }
                guard let ptr, size > 0 else { break }
                var lenBE = UInt32(size + 4).bigEndian
                withUnsafeBytes(of: &lenBE) { cfg.append(contentsOf: $0) }
                cfg.append(contentsOf: [0, 0, 0, 1])
                cfg.append(contentsOf: UnsafeBufferPointer(start: ptr, count: size))
                idx += 1
            }
            if idx == codecKind.parameterSetCount {
                enqueuePacket(config: cfg)
            } else {
                stateLock.lock()
                csdSent = false
                stateLock.unlock()
            }
        }

        guard let bb = CMSampleBufferGetDataBuffer(sample) else {
            if requestedKeyframe {
                scheduleRecoveryAfterPacketizationLoss(generation: generation)
            }
            return
        }
        var lengthAtOffset = 0
        var totalLength = 0
        var dataPointer: UnsafeMutablePointer<Int8>? = nil
        CMBlockBufferGetDataPointer(
            bb,
            atOffset: 0,
            lengthAtOffsetOut: &lengthAtOffset,
            totalLengthOut: &totalLength,
            dataPointerOut: &dataPointer
        )
        guard let ptr = dataPointer else {
            if requestedKeyframe {
                scheduleRecoveryAfterPacketizationLoss(generation: generation)
            }
            return
        }
        let bytes = UnsafeRawBufferPointer(start: ptr, count: totalLength)

        var pkt = Data([0x41, 0x55]) // "AU"
        let pts = CMSampleBufferGetPresentationTimeStamp(sample).value
        var ptsBE = UInt64(pts).bigEndian
        withUnsafeBytes(of: &ptsBE) { pkt.append(contentsOf: $0) }

        var offset = 0
        while offset < totalLength {
            var length = 0
            for j in 0..<4 {
                length = (length << 8) | Int(bytes[offset + j])
            }
            pkt.append(contentsOf: [0, 0, 0, 1])
            pkt.append(contentsOf: bytes[(offset + 4)..<(offset + 4 + length)])
            offset += 4 + length
        }

        // The network writer splits this logical G access unit into 1,200-byte
        // UDP datagrams. Its base envelope carries the AU id and stage times;
        // each emitted fragment receives its own index/count fields.
        // Logical L2 header: G, AU id (LE), "L2", capture wall ms,
        // encoded-output wall ms. The network writer adds send wall ms.
        var p2 = Data([0x47, UInt8(auId & 0xFF), UInt8(auId >> 8), 0x4C, 0x32])
        var captureWallMsBE = captureWallMs.bigEndian
        var encodeWallMsBE = UInt64(Date().timeIntervalSince1970 * 1_000.0).bigEndian
        withUnsafeBytes(of: &captureWallMsBE) { p2.append(contentsOf: $0) }
        withUnsafeBytes(of: &encodeWallMsBE) { p2.append(contentsOf: $0) }
        p2.append(pkt.dropFirst(10))
        let isRecoveryKeyframe = requestedKeyframe && isKeyframe
        if requestedKeyframe && !isKeyframe {
            // VideoToolbox did not honor the recovery request. Do not leave
            // the encoder paused forever; ask again after the normal cooldown.
            scheduleRecoveryAfterPacketizationLoss(generation: generation)
            return
        }
        enqueuePacket(
            frame: p2,
            isKeyframe: isKeyframe,
            isRecoveryKeyframe: isRecoveryKeyframe
        )
    }
}
