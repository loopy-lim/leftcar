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
    func handleSingleEncoderHealthDecision(
        _ decision: SingleEncoderHealthDecision,
        nowNs: UInt64,
        reclaimedSubmissions: [SingleEncodeSubmission]
    ) {
        switch decision {
        case .healthy:
            return
        case let .restart(generation):
            restartSingleEncoderAfterWatchdog(
                generation: generation,
                nowNs: nowNs,
                reclaimedSubmissions: reclaimedSubmissions
            )
        case .terminate:
            stateLock.lock()
            let shouldTerminate = running && !stopRequested
            if shouldTerminate {
                encoderWatchdogTerminations &+= 1
            }
            stateLock.unlock()
            if shouldTerminate {
                markStopped("encoder_stalled")
            }
        }
    }

    func restartSingleEncoderAfterWatchdog(
        generation: UInt64,
        nowNs: UInt64,
        reclaimedSubmissions: [SingleEncodeSubmission]
    ) {
        precondition(DispatchQueue.getSpecific(key: encodeQueueKey) != nil)
        guard requestedEncoderExperiment != .splitVertical else { return }

        let claimedSlots = reclaimedSubmissions.reduce(into: 0) { count, submission in
            if submission.token.claim(.watchdog) {
                count += 1
            }
        }

        stateLock.lock()
        let shouldRestart = running
            && !stopRequested
            && generation == encoderSessionGeneration
        stateLock.unlock()

        if shouldRestart {
            invalidateEncoderOnEncodeQueue()
        }

        stateLock.lock()
        for submission in reclaimedSubmissions {
            captureNsByPts.removeValue(forKey: submission.pts)
            captureWallMsByPts.removeValue(forKey: submission.pts)
            encodeAuIdByPts.removeValue(forKey: submission.pts)
        }
        stateLock.unlock()

        completeEncodeSlots(claimedSlots)
        clearRecoveryEncodeGate()

        guard shouldRestart else { return }

        stateLock.lock()
        forceKeyframe = true
        csdSent = false
        recoveryKeyframePending = true
        lastValidSingleEncoderOutputNs = nil
        singleEncoderHealthState.recordRestart(at: nowNs, generation: generation)
        encoderWatchdogRestarts &+= 1
        stateLock.unlock()

        schedulePendingEncodeIfPossible()
    }

     func invalidateEncoderOnEncodeQueue() {
        let invalidate = { [weak self] in
            guard let self else { return }
            let staleSession = self.session
            self.session = nil
            let stalePixelTransferSession = self.pixelTransferSession
            self.pixelTransferSession = nil
            self.encoderInputPool = nil
            self.stateLock.lock()
            let invalidatedGeneration = self.encoderSessionGeneration
            let detachedSubmissions = self.singleEncodeSubmissionLedger.reclaim(
                generation: invalidatedGeneration
            )
            for submission in detachedSubmissions {
                self.captureNsByPts.removeValue(forKey: submission.pts)
                self.captureWallMsByPts.removeValue(forKey: submission.pts)
                self.encodeAuIdByPts.removeValue(forKey: submission.pts)
            }
            self.encoderSessionGeneration &+= 1
            self.encoderSessionOutputCallbacks = 0
            self.recoveryDropRetryState.clear()
            self.stateLock.unlock()

            let claimedSlots = detachedSubmissions.reduce(into: 0) { count, submission in
                if submission.token.claim(.watchdog) {
                    count += 1
                }
            }
            if let staleSession {
                VTCompressionSessionInvalidate(staleSession)
            }
            if let stalePixelTransferSession {
                VTPixelTransferSessionInvalidate(stalePixelTransferSession)
            }
            self.completeEncodeSlots(claimedSlots)
        }
        if DispatchQueue.getSpecific(key: encodeQueueKey) != nil {
            invalidate()
        } else {
            encodeQueue.sync(execute: invalidate)
        }
    }

    @available(macOS 26.0, *)
     func applyEncoderPresetIfAvailable(
        to session: VTCompressionSession,
        preset: EncoderPresetKind,
        supportedPresets: NSDictionary?,
        report: inout EncoderConfigurationReport
    ) -> Bool {
        let presetKey: String
        let reportKey: String
        switch preset {
        case .highSpeed:
            presetKey = kVTCompressionPreset_HighSpeed as String
            reportKey = "HighSpeed"
        case .videoConferencing:
            presetKey = kVTCompressionPreset_VideoConferencing as String
            reportKey = "VideoConferencing"
        }
        guard let settings = supportedPresets?.object(forKey: presetKey) as? NSDictionary else {
            report.recordUnsupported(reportKey)
            return false
        }
        let status = VTSessionSetProperties(
            session,
            propertyDictionary: settings as CFDictionary
        )
        if status != noErr {
            report.recordRejected(reportKey, status: status)
            return false
        }
        report.recordApplied(reportKey)
        return true
    }
}
