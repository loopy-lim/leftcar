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
     func setupEncoder(for imageBuffer: CVImageBuffer) {
        let w = Int32(CVPixelBufferGetWidth(imageBuffer))
        let h = Int32(CVPixelBufferGetHeight(imageBuffer))
        let latencyPolicy = encoderLatencyPolicy(width: UInt32(w), height: UInt32(h))

        let activeCount = max(1, withRegistry { $0.count })
        let streamFactor = activeCount > 1 ? (1.0 / Double(activeCount) * 1.3) : 1.0
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.07 * streamFactor
        let minRate: Int
        let maxRate: Int
        if contentMode == .video {
            let ultraHd = w >= 3_840 && h >= 2_160
            if ultraHd {
                minRate = activeCount > 1 ? 18_000_000 : 24_000_000
                maxRate = activeCount > 1 ? 56_000_000 : 80_000_000
            } else {
                minRate = activeCount > 1 ? 7_000_000 : 8_000_000
                maxRate = activeCount > 1 ? 20_000_000 : 28_000_000
            }
        } else {
            minRate = activeCount > 1 ? 4_000_000 : 6_000_000
            maxRate = activeCount > 1 ? 24_000_000 : 60_000_000
        }
        let avgBitrate = min(max(idealBits, Double(minRate)), Double(maxRate))

        stateLock.lock()
        let requestedExperiment = requestedEncoderExperiment
        stateLock.unlock()
        let policies = encoderSessionPolicies(for: requestedExperiment)
        let availableEncoders = availableEncoderDescriptors()
        var lastFailure = "no encoder session policy"
        stateLock.lock()
        var aveFallbackReason = encoderFallbackReason
        stateLock.unlock()

        for policy in policies {
            switch attemptEncoderSetup(
                policy: policy,
                width: w,
                height: h,
                latencyPolicy: latencyPolicy,
                averageBitrate: avgBitrate,
                requestedExperiment: requestedExperiment,
                availableEncoders: availableEncoders,
                priorAveFallbackReason: aveFallbackReason
            ) {
            case .installed:
                return
            case let .failed(reason):
                lastFailure = reason
                if policy.mode == .ave {
                    aveFallbackReason = reason
                }
            }
        }

        stateLock.lock()
        encoderFallbackReason = requestedExperiment == .auto
            ? aveFallbackReason
            : nil
        stateLock.unlock()
        setLastError(lastFailure)
        markStopped(lastFailure)
    }

}


