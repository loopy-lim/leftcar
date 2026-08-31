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
     func observeAdaptiveQpWindowIfNeeded(nowNs: UInt64) {
        networkLock.lock()
        let networkOldestAgeUs = networkQueueSnapshot(
            frames: pendingFrames,
            splitAccessUnits: pendingSplitAccessUnits,
            nowNs: nowNs
        ).oldestAgeUs
        networkLock.unlock()

        stateLock.lock()
        guard appliedEncoderExperimentValue == .adaptiveQp else {
            stateLock.unlock()
            return
        }
        adaptiveQpWindowSubmittedFrames &+= 1
        if adaptiveQpWindowStartNs == 0 {
            adaptiveQpWindowStartNs = nowNs
            stateLock.unlock()
            return
        }
        let elapsedNs = nowNs >= adaptiveQpWindowStartNs
            ? nowNs - adaptiveQpWindowStartNs
            : 0
        guard elapsedNs >= 1_000_000_000 else {
            stateLock.unlock()
            return
        }
        let elapsed = Double(elapsedNs) / 1_000_000_000.0
        let validOutputFps = UInt32(
            max(0, (Double(adaptiveQpWindowValidOutputFrames) / elapsed).rounded())
        )
        let previousQp = adaptiveQpController.currentBaseFrameQp
        let next = adaptiveQpController.observeWindow(
            encoderDrops: adaptiveQpWindowEncoderDrops,
            validOutputFps: validOutputFps,
            submitP95Us: percentile95(adaptiveQpWindowSubmitSamplesUs),
            callbackP95Us: percentile95(adaptiveQpWindowCallbackSamplesUs),
            networkOldestAgeUs: networkOldestAgeUs
        )
        if next != previousQp {
            baseFrameQpChanges &+= 1
        }
        adaptiveQpWindowStartNs = nowNs
        adaptiveQpWindowSubmittedFrames = 0
        adaptiveQpWindowEncoderDrops = 0
        adaptiveQpWindowValidOutputFrames = 0
        adaptiveQpWindowSubmitSamplesUs.removeAll(keepingCapacity: true)
        adaptiveQpWindowCallbackSamplesUs.removeAll(keepingCapacity: true)
        stateLock.unlock()
    }

     func scheduleRegisteredRecoveryKeyframeRetry(
        afterNanoseconds cooldownNs: UInt64,
        generation: UInt64
    ) {
        let delay = DispatchTimeInterval.nanoseconds(Int(cooldownNs))
        encodeQueue.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self else { return }
            guard self.isRunning else {
                _ = self.consumeRecoveryDropRetry(generation: generation)
                return
            }
            guard self.isCurrentEncoderGeneration(generation) else {
                _ = self.consumeRecoveryDropRetry(generation: generation)
                return
            }
            self.networkLock.lock()
            let awaitingKeyframe = self.networkRecoveryBoundary.awaitingKeyframe
            self.networkLock.unlock()
            guard self.consumeRecoveryDropRetry(generation: generation) else {
                return
            }
            guard awaitingKeyframe else { return }
            self.requestRecoveryKeyframe(scheduledRetry: true)
        }
    }

     func handleEncoderOutputFailure(
        status: OSStatus,
        requestedKeyframe: Bool,
        generation: UInt64,
        pts: Int64
    ) {
        NSLog(
            "Leftcar %@ output failed %@: status=%d",
            codecKind.rawValue.uppercased(),
            targetLabel,
            status
        )
        discardTrackedFrame(pts: pts)
        if requestedKeyframe {
            clearRecoveryEncodeGate()
        }
        encodeQueue.async { [weak self] in
            guard let self else { return }
            guard self.isCurrentEncoderGeneration(generation) else { return }
            if !self.handleEncoderStartupFailure(
                status: status,
                stage: "output",
                expectedGeneration: generation
            ) {
                self.requestRecoveryKeyframe()
            }
        }
    }

     func recordAccessUnitShape(bytes: UInt64, isKeyframe: Bool, sendUs: UInt64) {
        guard !isKeyframe else { return }
        let nowNs = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        let sample = Double(bytes)
        recentAuBytesEwma = recentAuBytesEwma == 0
            ? sample
            : (recentAuBytesEwma * 0.75) + (sample * 0.25)
        let expectedBytes = Double(max(1, currentAverageBitrate))
            / 8.0
            / Double(max(1, fps))
        // A sustained delta above both a practical 48KiB floor and the current
        // frame budget indicates a genuinely high-change scene. This remains
        // the fallback when a capture backend has no dirty-region metadata.
        let highMotion = sample >= max(48_000.0, expectedBytes * 1.35)
            || recentAuBytesEwma >= max(48_000.0, expectedBytes * 1.20)
            || sendUs >= max(20_000, 700_000 / UInt64(max(1, fps)))
        stateLock.unlock()
        _ = observeAccessUnitMotion(nowNs: nowNs, highMotion: highMotion)
    }

     func adaptQualityIfNeeded(
        session: VTCompressionSession,
        receiverLoss: UInt64
    ) {
        stateLock.lock()
        qualityAdaptationChecks &+= 1
        qualityAdaptationLastReceiverLoss = receiverLoss
        stateLock.unlock()
        guard encoderQualityPolicy(
            codec: codecKind,
            width: outWidth,
            height: outHeight
        ).initialHint != nil else {
            stateLock.lock()
            qualityAdaptationLastStatus = "unsupported"
            stateLock.unlock()
            return
        }
        stateLock.lock()
        guard let current = currentQualityHint else {
            qualityAdaptationLastStatus = "no_hint"
            stateLock.unlock()
            return
        }
        if manualQualityHint != nil {
            qualityAdaptationLastStatus = "manual"
            stateLock.unlock()
            return
        }
        let currentBitrate = currentAverageBitrate
        let canApplyGenericQuality = encoderAppliedProperties.contains(
            EncoderOptionalProperty.quality.rawValue
        )
        let encodeP95Us = percentile95(encodeOutputSamplesUs)
        let queueP95Us = percentile95(captureQueueWaitSamplesUs)
        qualityAdaptationLastEncodeP95Us = encodeP95Us
        qualityAdaptationLastQueueP95Us = queueP95Us
        NSLog(
            "Leftcar %@ adaptive quality check: current=%.2f encodeP95=%d queueP95=%d receiverLoss=%d render=%d",
            codecKind.rawValue.uppercased(),
            current,
            encodeP95Us,
            queueP95Us,
            receiverLoss,
            receiverRenderedFps ?? 0
        )
        let next = adaptiveEncoderQualityHint(
            current: current,
            encodeOutputP95Us: encodeP95Us,
            captureQueueWaitP95Us: queueP95Us,
            receiverLoss: receiverLoss,
            renderedFps: receiverRenderedFps,
            targetFps: fps
        )
        stateLock.unlock()
        guard next != current else {
            stateLock.lock()
            qualityAdaptationLastStatus = "stable"
            stateLock.unlock()
            return
        }

        let qualityStatus: OSStatus
        if canApplyGenericQuality {
            qualityStatus = VTSessionSetProperty(
                session,
                key: kVTCompressionPropertyKey_Quality,
                value: next as CFNumber
            )
        } else {
            qualityStatus = -1
        }
        let currentScale = adaptiveQualityBitrateScale(qualityHint: current)
        let nextScale = adaptiveQualityBitrateScale(qualityHint: next)
        let targetBitrate = max(
            1_000_000,
            Int((Double(max(1, currentBitrate)) * nextScale / currentScale).rounded())
        )
        var bitrateStatus: OSStatus = noErr
        if targetBitrate != currentBitrate {
            bitrateStatus = VTSessionSetProperty(
                session,
                key: kVTCompressionPropertyKey_AverageBitRate,
                value: targetBitrate as CFNumber
            )
            if bitrateStatus == noErr {
                let hardLimitBytes = max(1, Int(Double(targetBitrate) / 8.0 * 1.25))
                _ = VTSessionSetProperty(
                    session,
                    key: kVTCompressionPropertyKey_DataRateLimits,
                    value: [hardLimitBytes, 1] as CFArray
                )
            }
        }
        guard qualityStatus == noErr || bitrateStatus == noErr else {
            stateLock.lock()
            qualityAdaptationRejections &+= 1
            qualityAdaptationLastStatus = "rejected"
            stateLock.unlock()
            NSLog(
                "Leftcar %@ adaptive quality rejected: quality=%d bitrate=%d",
                codecKind.rawValue.uppercased(),
                qualityStatus,
                bitrateStatus
            )
            return
        }
        stateLock.lock()
        currentQualityHint = next
        if bitrateStatus == noErr {
            currentAverageBitrate = targetBitrate
        }
        qualityAdaptationChanges &+= 1
        qualityAdaptationLastStatus = qualityStatus == noErr
            ? "changed"
            : "changed_bitrate_fallback"
        if qualityStatus != noErr {
            qualityAdaptationRejections &+= 1
        }
        stateLock.unlock()
        NSLog(
            "Leftcar %@ adaptive quality: %.2f -> %.2f bitrate=%d",
            codecKind.rawValue.uppercased(),
            current,
            next
        )
    }
}
