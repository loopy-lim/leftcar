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
     func encodeFrame(_ captured: PendingCaptureFrame) {
        stateLock.lock()
        let stillRunning = running
        let submittedFrames = framesEncoded
        stateLock.unlock()
        guard stillRunning else {
            completeEncodeSlot()
            return
        }

        let pb = captured.pixelBuffer
        let encodeQueueStartNs = DispatchTime.now().uptimeNanoseconds
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else {
            completeEncodeSlot()
            return
        }
        stateLock.lock()
        let submittedEncoderGeneration = encoderSessionGeneration
        stateLock.unlock()
        let inputPreparationStartNs = DispatchTime.now().uptimeNanoseconds
        let encoderInput = encoderInputBuffer(for: pb)
        let inputPreparationUs = (
            DispatchTime.now().uptimeNanoseconds &- inputPreparationStartNs
        ) / 1_000
        stateLock.lock()
        lastInputPreparationUs = inputPreparationUs
        maxInputPreparationUs = max(maxInputPreparationUs, inputPreparationUs)
        appendRollingSample(inputPreparationUs, to: &inputPreparationSamplesUs)
        stateLock.unlock()
        guard let inputBuffer = encoderInput.buffer else {
            stateLock.lock()
            encodeSubmitFailures &+= 1
            stateLock.unlock()
            NSLog(
                "Leftcar encoder input preparation failed %@: status=%d",
                targetLabel,
                encoderInput.status
            )
            if !handleEncoderStartupFailure(
                status: encoderInput.status,
                stage: "input transfer",
                expectedGeneration: submittedEncoderGeneration
            ) {
                requestRecoveryKeyframe()
            }
            completeEncodeSlot()
            return
        }

        let inputPts = captured.pts

        let pts = inputPts.timescale > 0
            ? inputPts
            : CMTime(value: CMTimeValue(submittedFrames), timescale: CMTimeScale(fps))
        let inputDuration = captured.duration
        let duration = inputDuration.timescale > 0
            ? inputDuration
            : CMTime(value: 1, timescale: CMTimeScale(fps))
        stateLock.lock()
        let auId = nextAuId
        nextAuId &+= 1
        captureNsByPts[pts.value] = captured.callbackNs
        captureWallMsByPts[pts.value] = captured.captureWallMs
        encodeAuIdByPts[pts.value] = auId
        let queueWaitUs = (encodeQueueStartNs &- captured.callbackNs) / 1_000
        lastCaptureQueueWaitUs = queueWaitUs
        maxCaptureQueueWaitUs = max(maxCaptureQueueWaitUs, queueWaitUs)
        appendRollingSample(queueWaitUs, to: &captureQueueWaitSamplesUs)
        if captureNsByPts.count > 256 {
            captureNsByPts.removeValue(forKey: captureNsByPts.keys.first!)
        }
        if captureWallMsByPts.count > 256 {
            captureWallMsByPts.removeValue(forKey: captureWallMsByPts.keys.first!)
        }
        if encodeAuIdByPts.count > 256 {
            encodeAuIdByPts.removeValue(forKey: encodeAuIdByPts.keys.first!)
        }
        stateLock.unlock()
        var flags: VTEncodeInfoFlags = []
        stateLock.lock()
        let requestKeyframe = forceKeyframe
        forceKeyframe = false
        let appliedExperiment = appliedEncoderExperimentValue
        let currentBaseFrameQp = adaptiveQpController.currentBaseFrameQp
        stateLock.unlock()
        if requestKeyframe {
            captureLock.lock()
            recoveryEncodeInFlight = true
            recoveryEncodeGateStartedNs = DispatchTime.now().uptimeNanoseconds
            captureLock.unlock()
        }
        let framePlan = encoderFramePropertyPlan(
            appliedExperiment: appliedExperiment,
            requestKeyframe: requestKeyframe,
            currentBaseFrameQp: currentBaseFrameQp
        )
        var framePropertyValues: [String: Any] = [:]
        if framePlan.forceKeyframe {
            framePropertyValues[kVTEncodeFrameOptionKey_ForceKeyFrame as String] = true
        }
        if let baseFrameQp = framePlan.baseFrameQp {
            framePropertyValues[kVTEncodeFrameOptionKey_BaseFrameQP as String] = baseFrameQp
        }
        let frameProperties: CFDictionary? = framePropertyValues.isEmpty
            ? nil
            : framePropertyValues as CFDictionary

        let trackedPts = pts.value
        // Keep this timestamp immediately adjacent to the synchronous
        // VideoToolbox call. The callback captures it directly, which also
        // makes synchronous callbacks safe without a lock/map operation here.
        let completionToken = EncodeSlotCompletionToken()
        let encodeSubmitCallStartNs = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        nextSingleEncodeSubmissionID &+= 1
        let submissionID = nextSingleEncodeSubmissionID
        singleEncodeSubmissionLedger.register(.init(
            id: submissionID,
            generation: submittedEncoderGeneration,
            pts: trackedPts,
            submittedNs: encodeSubmitCallStartNs,
            token: completionToken
        ))
        stateLock.unlock()
        let status = VTCompressionSessionEncodeFrame(
            s,
            imageBuffer: inputBuffer,
            presentationTimeStamp: pts,
            duration: duration,
            frameProperties: frameProperties,
            infoFlagsOut: &flags
        ) { [weak self] status, infoFlags, encodedSample in
            guard let self else { return }
            let encoderCallbackNs = DispatchTime.now().uptimeNanoseconds
            let disposition = encoderCallbackDisposition(
                status: status,
                flags: infoFlags,
                hasSample: encodedSample != nil
            )
            let watchdogOwned = completionToken.completedBy == .watchdog
            self.stateLock.lock()
            let callbackRetirement = self.singleEncodeSubmissionLedger
                .retireCallbackAndRecordHealthProgress(
                    id: submissionID,
                    callbackGeneration: submittedEncoderGeneration,
                    currentGeneration: self.encoderSessionGeneration,
                    callbackNs: encoderCallbackNs,
                    isValidOutput: disposition == .valid && encodedSample != nil,
                    healthState: &self.singleEncoderHealthState,
                    lastValidOutputNs: &self.lastValidSingleEncoderOutputNs
                )
            let retiredSubmission = callbackRetirement.submission
            let generationWasCurrent = callbackRetirement.generationWasCurrent
            if retiredSubmission == nil,
               watchdogOwned || !generationWasCurrent {
                self.encoderLateCallbacks &+= 1
            }
            self.stateLock.unlock()
            guard let retiredSubmission else { return }

            let generationMatches = self.recordEncoderCallbackTiming(
                pts: trackedPts,
                callbackNs: encoderCallbackNs,
                generation: submittedEncoderGeneration,
                submitCallStartNs: encodeSubmitCallStartNs
            )
            if shouldReleaseEncodeSlotBeforePacketization(disposition: disposition),
               retiredSubmission.token.claim(.callback) {
                self.completeEncodeSlot()
            }
            guard generationMatches else {
                self.stateLock.lock()
                self.encoderLateCallbacks &+= 1
                self.stateLock.unlock()
                return
            }

            switch disposition {
            case .dropped:
                self.recordEncoderFrameDrop(pts: trackedPts)
                switch recoveryActionForDroppedFrame(
                    requestedRecoveryKeyframe: requestKeyframe
                ) {
                case .none:
                    break
                case .retryAfterCooldown:
                    self.scheduleRecoveryAfterPacketizationLoss(
                        generation: submittedEncoderGeneration
                    )
                }
                return
            case .failed:
                self.handleEncoderOutputFailure(
                    status: status,
                    requestedKeyframe: requestKeyframe,
                    generation: submittedEncoderGeneration,
                    pts: trackedPts
                )
                return
            case .valid:
                break
            }

            guard let encodedSample else {
                self.discardTrackedFrame(pts: trackedPts)
                return
            }
            guard self.recordValidEncoderOutput(
                pts: trackedPts,
                callbackNs: encoderCallbackNs,
                generation: submittedEncoderGeneration,
                callbackLatencyUs: encoderCallbackLatencyUs(
                    submitCallStartNs: encodeSubmitCallStartNs,
                    callbackNs: encoderCallbackNs
                )
            ) else { return }
            self.confirmEncoderStartup(expectedGeneration: submittedEncoderGeneration)
            self.enqueueEncodedSampleForPacketization(
                encodedSample,
                requestedKeyframe: requestKeyframe,
                callbackNs: encoderCallbackNs,
                generation: submittedEncoderGeneration
            )
        }
        let encodeSubmitCallUs = encodeSubmitCallDurationUs(
            submitCallStartNs: encodeSubmitCallStartNs,
            submitCallEndNs: DispatchTime.now().uptimeNanoseconds
        )
        recordEncodeSubmitCallTiming(encodeSubmitCallUs)

        if status == noErr {
            stateLock.lock()
            framesEncoded &+= 1
            rateWindowFrames &+= 1
            let shouldAdaptBitrate = shouldRunAdaptiveBitrate(
                encodedFrames: framesEncoded,
                fps: fps
            )
            stateLock.unlock()
            if shouldAdaptBitrate {
                adaptBitrateIfNeeded()
            }
            observeAdaptiveQpWindowIfNeeded(
                nowNs: DispatchTime.now().uptimeNanoseconds
            )
        } else {
            NSLog(
                "Leftcar %@ submit failed %@: status=%d",
                codecKind.rawValue.uppercased(),
                targetLabel,
                status
            )
            stateLock.lock()
            encodeSubmitFailures &+= 1
            captureNsByPts.removeValue(forKey: pts.value)
            captureWallMsByPts.removeValue(forKey: pts.value)
            encodeAuIdByPts.removeValue(forKey: pts.value)
            let retiredSubmission = singleEncodeSubmissionLedger.retire(
                id: submissionID
            )
            stateLock.unlock()
            if requestKeyframe {
                clearRecoveryEncodeGate()
            }
            if let retiredSubmission,
               retiredSubmission.token.claim(.submitFailureReturn) {
                completeEncodeSlot()
            }
            if !handleEncoderStartupFailure(
                status: status,
                stage: "submit",
                expectedGeneration: submittedEncoderGeneration
            ) {
                requestRecoveryKeyframe()
            }
        }
    }

    @discardableResult
     func markRecoveryDropRetryPending(generation: UInt64) -> Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return recoveryDropRetryState.register(
            generation: generation,
            currentGeneration: encoderSessionGeneration
        )
    }

    @discardableResult
     func consumeRecoveryDropRetry(generation: UInt64) -> Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return recoveryDropRetryState.consume(generation: generation)
    }

     func establishRegisteredNetworkRecoveryBoundary(
        generation: UInt64
    ) -> Bool {
        // enqueuePacket already establishes networkLock -> stateLock as the
        // repository lock order. Registration released stateLock before this
        // method, so revalidate ownership under that same established order.
        networkLock.lock()
        stateLock.lock()
        let established = recoveryDropRetryState.establishBoundaryIfOwned(
            generation: generation,
            currentGeneration: encoderSessionGeneration,
            boundary: &networkRecoveryBoundary
        )
        if established {
            pendingFrames.removeAll(keepingCapacity: true)
            pendingSplitAccessUnits.removeAll(keepingCapacity: true)
        }
        stateLock.unlock()
        networkLock.unlock()
        return established
    }

     func scheduleRecoveryAfterPacketizationLoss(generation: UInt64) {
        guard markRecoveryDropRetryPending(generation: generation) else {
            return
        }
        guard establishRegisteredNetworkRecoveryBoundary(
            generation: generation
        ) else {
            return
        }
        clearRecoveryEncodeGate()
        scheduleRegisteredRecoveryKeyframeRetry(
            afterNanoseconds: 750_000_000,
            generation: generation
        )
    }
}
