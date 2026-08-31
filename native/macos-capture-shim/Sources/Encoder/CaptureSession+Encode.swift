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
    // MARK: Frame Processing & VideoToolbox Encoding

    func handleFrame(_ sample: CMSampleBuffer) {
        guard CMSampleBufferIsValid(sample), CMSampleBufferDataIsReady(sample),
              let pixelBuffer = CMSampleBufferGetImageBuffer(sample) else {
            return
        }
        if let attachmentSets = CMSampleBufferGetSampleAttachmentsArray(
            sample,
            createIfNecessary: false
        ) as? [[SCStreamFrameInfo: Any]],
           let attachments = attachmentSets.first,
           let values = attachments[.dirtyRects] as? [NSValue] {
            let motionSample = dirtyRegionMotionSample(
                rects: values.map(\.rectValue),
                frameWidth: CVPixelBufferGetWidth(pixelBuffer),
                frameHeight: CVPixelBufferGetHeight(pixelBuffer)
            )
            observeCaptureMotion(
                motionSample,
                nowNs: DispatchTime.now().uptimeNanoseconds
            )
        }
        let inputPts = CMSampleBufferGetPresentationTimeStamp(sample)
        let inputDuration = CMSampleBufferGetDuration(sample)
        handlePixelBuffer(
            pixelBuffer,
            pts: inputPts,
            duration: inputDuration
        )
    }

    func handlePixelBuffer(_ pixelBuffer: CVPixelBuffer, pts: CMTime, duration: CMTime) {
        let callbackNs = DispatchTime.now().uptimeNanoseconds
        let captureWallMs = UInt64(Date().timeIntervalSince1970 * 1_000.0)
        stateLock.lock()
        let stillRunning = running
        if stillRunning {
            captureCallbacks &+= 1
            rateWindowCaptureCallbacks &+= 1
        }
        if stillRunning, firstCaptureNs == nil {
            firstCaptureNs = callbackNs
            lifecycleState = "encoding_first_frame"
            NSLog(
                "Leftcar first capture frame %@: %dx%d",
                targetLabel,
                CVPixelBufferGetWidth(pixelBuffer),
                CVPixelBufferGetHeight(pixelBuffer)
            )
        }
        if let previous = lastCaptureCallbackNs, callbackNs >= previous {
            appendRollingSample((callbackNs - previous) / 1_000, to: &captureIntervalSamplesUs)
        }
        lastCaptureCallbackNs = callbackNs
        stateLock.unlock()
        guard stillRunning else { return }

        let frame = PendingCaptureFrame(
            pixelBuffer: pixelBuffer,
            pts: pts,
            duration: duration,
            callbackNs: callbackNs,
            captureWallMs: captureWallMs
        )
        captureLock.lock()
        let gateTimedOut = recoveryEncodeInFlight
            && recoveryEncodeGateExpired(
                startedNs: recoveryEncodeGateStartedNs,
                nowNs: callbackNs,
                timeoutNs: 750_000_000
            )
        if gateTimedOut {
            // A VideoToolbox callback can be lost during a hardware reset. Do
            // not let one missing recovery callback freeze the stream forever;
            // the next submitted frame will be another recovery boundary.
            recoveryEncodeInFlight = false
            recoveryEncodeGateStartedNs = 0
        }
        let replaced = enqueuePendingCaptureLocked(frame)
        let shouldSchedule = !encodeScheduled
            && encodeInFlight < maxEncodeInFlight
            && !recoveryEncodeInFlight
        if shouldSchedule {
            encodeScheduled = true
        }
        captureLock.unlock()

        if gateTimedOut {
            NSLog("Leftcar recovery encode gate timed out %@; resuming newest frame", targetLabel)
        }

        if replaced {
            stateLock.lock()
            captureQueueDropped &+= 1
            stateLock.unlock()
        }
        if shouldSchedule {
            encodeQueue.async { [weak self] in
                self?.drainEncodeQueue()
            }
        }
        scheduleSingleEncoderHealthCheckIfNeeded(nowNs: callbackNs)
    }

    func scheduleSingleEncoderHealthCheckIfNeeded(nowNs: UInt64) {
        guard requestedEncoderExperiment != .splitVertical else { return }

        stateLock.lock()
        let generation = encoderSessionGeneration
        let oldestSubmissionNs = singleEncodeSubmissionLedger.oldestSubmissionNs(
            generation: generation
        )
        let oldEnough = oldestSubmissionNs.map {
            nowNs >= $0 && nowNs - $0 >= SingleEncoderHealthState.stallBudgetNs
        } ?? false
        let shouldSchedule = running
            && oldEnough
            && !singleEncoderHealthCheckScheduled
        if shouldSchedule {
            singleEncoderHealthCheckScheduled = true
        }
        stateLock.unlock()

        guard shouldSchedule else { return }
        encodeQueue.async { [weak self] in
            self?.evaluateSingleEncoderHealth()
        }
    }

    func evaluateSingleEncoderHealth() {
        guard requestedEncoderExperiment != .splitVertical else { return }

        captureLock.lock()
        let currentEncodeInFlight = encodeInFlight
        captureLock.unlock()

        let nowNs = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        singleEncoderHealthCheckScheduled = false
        guard running, !stopRequested else {
            stateLock.unlock()
            return
        }
        let generation = encoderSessionGeneration
        let decision = singleEncoderHealthState.evaluate(
            nowNs: nowNs,
            captureCallbackNs: lastCaptureCallbackNs,
            lastValidOutputNs: lastValidSingleEncoderOutputNs,
            encodeInFlight: currentEncodeInFlight,
            oldestSubmissionNs: singleEncodeSubmissionLedger.oldestSubmissionNs(
                generation: generation
            ),
            generation: generation
        )
        let reclaimedSubmissions: [SingleEncodeSubmission]
        if decision == .restart(generation: generation) {
            reclaimedSubmissions = singleEncodeSubmissionLedger.reclaim(
                generation: generation
            )
        } else {
            reclaimedSubmissions = []
        }
        stateLock.unlock()

        handleSingleEncoderHealthDecision(
            decision,
            nowNs: nowNs,
            reclaimedSubmissions: reclaimedSubmissions
        )
    }

     func drainEncodeQueue() {
        while true {
            captureLock.lock()
            guard encodeInFlight < maxEncodeInFlight,
                  !recoveryEncodeInFlight else {
                encodeScheduled = false
                captureLock.unlock()
                return
            }
            let splitLease: SplitFlowLease?
            if requestedEncoderExperiment == .splitVertical {
                guard let admitted = splitFlowState.admit() else {
                    encodeScheduled = false
                    captureLock.unlock()
                    stateLock.lock()
                    splitPreEncodeAdmissionDrops &+= 1
                    stateLock.unlock()
                    return
                }
                splitLease = admitted
            } else {
                splitLease = nil
            }
            guard let next = dequeuePendingCaptureLocked() else {
                if let splitLease {
                    _ = splitFlowState.complete(splitLease)
                }
                encodeScheduled = false
                captureLock.unlock()
                return
            }
            encodeInFlight += 1
            captureLock.unlock()
            if let splitLease {
                encodeSplitFrame(next, lease: splitLease)
            } else {
                encodeFrame(next)
            }
        }
    }
}
