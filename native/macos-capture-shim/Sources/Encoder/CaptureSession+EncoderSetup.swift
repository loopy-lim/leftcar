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
        // Parsec-equivalent sharpness on a healthy link starts near the
        // policy ceiling: 0.13 puts 1440p60 at ~29Mbps and 1080p60 at
        // ~16Mbps, and the 1Hz ABR cuts fast when the link disagrees. The
        // old 0.07 started half that and spent the first ~8s ramping while
        // every frame the user saw was mushy.
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.13 * streamFactor
        let minRate: Int
        let maxRate: Int
        if contentMode == .video {
            let bounds = videoBitrateBounds(
                width: UInt32(w),
                height: UInt32(h),
                activeCount: activeCount
            )
            minRate = bounds.minimum
            maxRate = bounds.maximum
        } else {
            minRate = activeCount > 1 ? 4_000_000 : 6_000_000
            maxRate = activeCount > 1 ? 24_000_000 : 60_000_000
        }
        let avgBitrate = min(max(idealBits, Double(minRate)), Double(maxRate))

        stateLock.lock()
        let requestedExperiment = requestedEncoderExperiment
        stateLock.unlock()
        let policies = encoderSessionPolicies(
            for: requestedExperiment,
            width: UInt32(w),
            height: UInt32(h),
            contentMode: contentMode.rawValue
        )
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
