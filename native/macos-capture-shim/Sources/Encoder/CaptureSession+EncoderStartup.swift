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
     func updateEncoderStartupInFlightLimit(
        mode: EncoderMode,
        outputConfirmed: Bool
    ) {
        let limit = encoderStartupInFlightLimit(
            mode: mode,
            outputConfirmed: outputConfirmed,
            configuredLimit: configuredEncodeInFlightLimit
        )
        captureLock.lock()
        let didIncrease = limit > maxEncodeInFlight
        maxEncodeInFlight = limit
        let shouldSchedule = didIncrease
            && hasPendingCaptureLocked()
            && !encodeScheduled
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
    }

    // Call on encodeQueue. The generation check prevents a delayed callback
    // from an invalidated VT session from changing the replacement session.
     func currentEncoderStartupFailureAction(
        expectedGeneration: UInt64
    ) -> (
        mode: EncoderMode,
        action: EncoderStartupFailureAction
    )? {
        stateLock.lock()
        let mode = EncoderMode(rawValue: encoderMode)
        let generationMatches = expectedGeneration == encoderSessionGeneration
        let outputCount = encoderSessionOutputCallbacks
        let inputStagingEnabled = aveInputStagingEnabled
        stateLock.unlock()
        guard generationMatches, let mode else { return nil }
        let policies = eligibleEncoderSessionPolicies(
            encoderSessionPolicies(for: requestedEncoderExperiment),
            unavailableModes: unavailableEncoderModes
        )
        let hasNextPolicy: Bool
        if let currentIndex = policies.firstIndex(where: { $0.mode == mode }) {
            hasNextPolicy = policies.index(after: currentIndex) < policies.endIndex
        } else {
            hasNextPolicy = false
        }
        return (
            mode,
            encoderStartupFailureAction(
                mode: mode,
                encodedOutputCount: outputCount,
                inputStagingEnabled: inputStagingEnabled,
                hasNextPolicy: hasNextPolicy
            )
        )
    }

     func confirmEncoderStartup(expectedGeneration: UInt64) {
        stateLock.lock()
        let mode = expectedGeneration == encoderSessionGeneration
            ? EncoderMode(rawValue: encoderMode)
            : nil
        stateLock.unlock()
        guard let mode else { return }
        updateEncoderStartupInFlightLimit(
            mode: mode,
            outputConfirmed: true
        )
    }

     func isCurrentEncoderGeneration(_ generation: UInt64) -> Bool {
        stateLock.lock()
        let matches = generation == encoderSessionGeneration
        stateLock.unlock()
        return matches
    }

    /// Called on encodeQueue after the first actual AVE submission or callback
    /// fails with the direct capture surface. Recreate the same AVE policy and
    /// retry through a pool-backed NV12 surface before giving up on AVE.
    @discardableResult
     func retryEncoderStartupWithStagingInput(
        status: OSStatus,
        stage: String,
        expectedGeneration: UInt64
    ) -> Bool {
        guard let decision = currentEncoderStartupFailureAction(
            expectedGeneration: expectedGeneration
        ),
              decision.action == .retryWithStagingInput else {
            return false
        }
        if let s = session {
            VTCompressionSessionInvalidate(s)
            session = nil
        }
        if let pixelTransferSession {
            VTPixelTransferSessionInvalidate(pixelTransferSession)
            self.pixelTransferSession = nil
        }
        encoderInputPool = nil
        updateEncoderStartupInFlightLimit(
            mode: decision.mode,
            outputConfirmed: false
        )
        let reason = "\(decision.mode.rawValue.uppercased()) first \(stage) failed: \(status); retrying staged input"
        stateLock.lock()
        aveInputStagingEnabled = true
        encoderMode = "staging_retry_pending"
        encoderFallbackReason = reason
        encoderSessionGeneration &+= 1
        encoderSessionOutputCallbacks = 0
        recoveryDropRetryState.clear()
        csdSent = false
        forceKeyframe = false
        recoveryKeyframePending = false
        stateLock.unlock()
        NSLog("Leftcar %@", reason)
        return true
    }

    /// Called on encodeQueue when an encoder attempt fails before its first
    /// output and another policy is available. Once a session has produced
    /// output, later failures stay on it and use the IDR recovery path.
    @discardableResult
     func fallbackAfterEncoderStartupFailure(
        status: OSStatus,
        stage: String,
        expectedGeneration: UInt64
    ) -> Bool {
        guard let decision = currentEncoderStartupFailureAction(
            expectedGeneration: expectedGeneration
        ),
              decision.action == .fallbackToNextPolicy else {
            return false
        }
        unavailableEncoderModes.insert(decision.mode)
        if let s = session {
            VTCompressionSessionInvalidate(s)
            session = nil
        }
        updateEncoderStartupInFlightLimit(
            mode: decision.mode,
            outputConfirmed: false
        )
        let reason = "\(decision.mode.rawValue.uppercased()) first \(stage) failed: \(status)"
        stateLock.lock()
        encoderMode = "fallback_pending"
        encoderFallbackReason = reason
        encoderSessionGeneration &+= 1
        encoderSessionOutputCallbacks = 0
        recoveryDropRetryState.clear()
        csdSent = false
        forceKeyframe = false
        recoveryKeyframePending = false
        stateLock.unlock()
        NSLog("Leftcar %@; trying next encoder policy", reason)
        return true
    }

    @discardableResult
     func handleEncoderStartupFailure(
        status: OSStatus,
        stage: String,
        expectedGeneration: UInt64
    ) -> Bool {
        guard let decision = currentEncoderStartupFailureAction(
            expectedGeneration: expectedGeneration
        ) else {
            return false
        }
        switch decision.action {
        case .retryWithStagingInput:
            return retryEncoderStartupWithStagingInput(
                status: status,
                stage: stage,
                expectedGeneration: expectedGeneration
            )
        case .fallbackToNextPolicy:
            return fallbackAfterEncoderStartupFailure(
                status: status,
                stage: stage,
                expectedGeneration: expectedGeneration
            )
        case .recoverCurrentEncoder:
            return false
        }
    }

     func completeEncodeSlot() {
        completeEncodeSlots(1)
    }

     func completeEncodeSlots(_ count: Int) {
        captureLock.lock()
        encodeInFlight = max(0, encodeInFlight - max(0, count))
        let shouldSchedule = hasPendingCaptureLocked()
            && !encodeScheduled
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
    }

     func clearRecoveryEncodeGate() {
        captureLock.lock()
        recoveryEncodeInFlight = false
        recoveryEncodeGateStartedNs = 0
        captureLock.unlock()
    }

     func schedulePendingEncodeIfPossible() {
        captureLock.lock()
        let shouldSchedule = hasPendingCaptureLocked()
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
    }
}

