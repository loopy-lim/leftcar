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
     func requestRecoveryKeyframe(scheduledRetry: Bool = false) {
        if requestedEncoderExperiment == .splitVertical {
            beginSplitTransportRecovery(
                reason: scheduledRetry
                    ? "scheduled split recovery retry"
                    : "viewer or transport requested IDR"
            )
            return
        }
        stateLock.lock()
        let now = DispatchTime.now().uptimeNanoseconds
        // Keep one recovery request outstanding. If its keyframe never reaches
        // the viewer, retry after 750ms instead of creating a 5Hz IDR storm.
        let cooldownElapsed = now &- lastKeyframeRequestNs >= 750_000_000
        let decision = recoveryKeyframeRequestDecision(
            delayedRetryPending: recoveryDropRetryState.hasPendingRetry,
            scheduledRetry: scheduledRetry,
            recoveryKeyframePending: recoveryKeyframePending,
            cooldownElapsed: cooldownElapsed
        )
        if decision == .requestNow {
            lastKeyframeRequestNs = now
            recoveryKeyframePending = true
            forceKeyframe = true
            csdSent = false
            recoveryKeyframes &+= 1
        } else {
            recoveryRequestsSuppressed &+= 1
        }
        stateLock.unlock()
    }

    /// Start one network recovery episode. A viewer can repeat IDR requests
    /// while the same recovery keyframe is still being encoded or drained;
    /// those requests must not repeatedly erase the recovery boundary or
    /// churn the pending delta queue.
     func beginNetworkRecovery() {
        if requestedEncoderExperiment == .splitVertical {
            beginSplitTransportRecovery(reason: "network recovery requested")
            return
        }
        networkLock.lock()
        let keyframeQueued = pendingFrames.contains(where: { $0.isKeyframe })
        let shouldStart = shouldStartNetworkRecovery(
            awaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
            keyframeInFlight: networkKeyframeInFlight,
            keyframeQueued: keyframeQueued
        )
        if shouldStart {
            pendingFrames.removeAll(keepingCapacity: true)
            networkRecoveryBoundary.establishAwaitingKeyframe()
        }
        let shouldRetry = networkRecoveryBoundary.awaitingKeyframe
            && !networkKeyframeInFlight
            && !keyframeQueued
        networkLock.unlock()

        if shouldStart || shouldRetry {
            requestRecoveryKeyframe()
        }
    }

    func beginSplitTransportRecovery(
        reason: String,
        invalidatePendingBoundary: Bool = false,
        failedLease: SplitFlowLease? = nil
    ) {
        captureLock.lock()
        let episode = beginSplitRecovery(
            flow: &splitFlowState,
            invalidatePendingBoundary: invalidatePendingBoundary,
            failedLease: failedLease,
            pendingCaptures: &pendingSplitCaptures,
            carrier: splitRecoveryCarrier
        )
        captureLock.unlock()

        guard let generation = episode else {
            stateLock.lock()
            recoveryRequestsSuppressed &+= 1
            stateLock.unlock()
            return
        }

        // Drain uses this same network -> capture order. Revalidate the episode
        // while holding both owners: an older operation must never clean a newer
        // IDR or reserve an encode task it will later abandon. No platform or
        // encoder callback runs inside this transaction.
        networkLock.lock()
        captureLock.lock()
        guard let discarded = finishSplitRecoveryCleanup(
            generation: generation,
            flow: splitFlowState,
            pending: &pendingSplitAccessUnits,
            lease: { $0.lease }
        ) else {
            captureLock.unlock()
            networkLock.unlock()
            return
        }
        let needsBoundarySubmission = splitFlowState.activeCount == 0
        if needsBoundarySubmission {
            // Reuse retained capture immediately on an idle screen. A boundary
            // already admitted/queued owns its carrier and needs no duplicate.
            seedSplitRecoveryCarrierLocked()
        }
        splitRecoveryGateStartedNs = DispatchTime.now().uptimeNanoseconds
        nextUdpSendNs = splitRecoveryGateStartedNs
        networkRecoveryBoundary.establishAwaitingKeyframe()
        let shouldSchedule = needsBoundarySubmission
            && hasPendingCaptureLocked()
            && !encodeScheduled
            && encodeInFlight < maxEncodeInFlight
            && !recoveryEncodeInFlight
        if shouldSchedule {
            encodeScheduled = true
        }
        captureLock.unlock()
        networkLock.unlock()

        stateLock.lock()
        recoveryKeyframes &+= 1
        splitRecoveryBoundaryDiscards &+= Int64(discarded)
        framesDropped &+= Int64(discarded)
        stateLock.unlock()

        // The admitted recovery lease forces both encoder requests to IDR.
        // A separate delayed request flag could otherwise spill into the next
        // admission after this episode's boundary was already encoded.
        NSLog("Leftcar split recovery %@: %@", targetLabel, reason)
        if shouldSchedule {
            encodeQueue.async { [weak self] in
                self?.drainEncodeQueue()
            }
        }
    }

     func recoveryKeyframeDidSend() {
        stateLock.lock()
        recoveryKeyframePending = false
        recoveryDropRetryState.clear()
        forceKeyframe = false
        stateLock.unlock()
        clearRecoveryEncodeGate()
        schedulePendingEncodeIfPossible()
    }

     func discardTrackedFrame(pts: Int64) {
        stateLock.lock()
        captureNsByPts.removeValue(forKey: pts)
        captureWallMsByPts.removeValue(forKey: pts)
        encodeAuIdByPts.removeValue(forKey: pts)
        stateLock.unlock()
    }

     func recordEncodeSubmitCallTiming(_ elapsedUs: UInt64) {
        stateLock.lock()
        lastEncodeSubmitCallUs = elapsedUs
        maxEncodeSubmitCallUs = max(maxEncodeSubmitCallUs, elapsedUs)
        appendRollingSample(elapsedUs, to: &encodeSubmitCallSamplesUs)
        if appliedEncoderExperimentValue == .adaptiveQp {
            appendRollingSample(elapsedUs, to: &adaptiveQpWindowSubmitSamplesUs)
        }
        stateLock.unlock()
    }

    @discardableResult
     func recordEncoderCallbackTiming(
        pts: Int64,
        callbackNs: UInt64,
        generation: UInt64,
        submitCallStartNs: UInt64
    ) -> Bool {
        stateLock.lock()
        guard generation == encoderSessionGeneration else {
            stateLock.unlock()
            return false
        }
        let elapsedUs = encoderCallbackLatencyUs(
            submitCallStartNs: submitCallStartNs,
            callbackNs: callbackNs
        )
        lastEncoderCallbackUs = elapsedUs
        maxEncoderCallbackUs = max(maxEncoderCallbackUs, elapsedUs)
        appendRollingSample(elapsedUs, to: &encoderCallbackSamplesUs)
        if appliedEncoderExperimentValue == .adaptiveQp {
            appendRollingSample(elapsedUs, to: &adaptiveQpWindowCallbackSamplesUs)
        }
        stateLock.unlock()
        return true
    }

     func recordEncoderFrameDrop(pts: Int64) {
        stateLock.lock()
        encoderFrameDrops &+= 1
        rateWindowEncoderFrameDrops &+= 1
        if appliedEncoderExperimentValue == .adaptiveQp {
            adaptiveQpWindowEncoderDrops &+= 1
        }
        captureNsByPts.removeValue(forKey: pts)
        captureWallMsByPts.removeValue(forKey: pts)
        encodeAuIdByPts.removeValue(forKey: pts)
        stateLock.unlock()
    }
}
