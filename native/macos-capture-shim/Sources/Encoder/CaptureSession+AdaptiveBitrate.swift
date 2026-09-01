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
     func adaptBitrateIfNeeded() {
        let singleSession = session
        let activeSplitPipeline = splitPipeline
        let applicationRoute = encoderBitrateApplicationRoute(
            hasSingleSession: singleSession != nil,
            hasSplitPipeline: activeSplitPipeline != nil
        )
        guard applicationRoute != .unavailable else { return }
        stateLock.lock()
        guard appliedEncoderExperimentValue != .adaptiveQp else {
            stateLock.unlock()
            return
        }
        let congestionDrops = max(0, framesDropped - recoveryFramesDropped) + captureQueueDropped
        let newDrops = congestionDrops - lastAdaptedDropped
        lastAdaptedDropped = congestionDrops
        let nowNs = DispatchTime.now().uptimeNanoseconds
        let feedbackFresh = receiverFeedbackNs > 0
            && nowNs &- receiverFeedbackNs <= 3_000_000_000
        let recoveryBurstGrace = lastRecoverySendNs != 0
            && nowNs &- lastRecoverySendNs < 1_000_000_000
        let receiverLoss = UInt64(receiverFrameGaps)
            + UInt64(receiverInputDrops)
            + UInt64(receiverIncompleteAUs)
            + UInt64(receiverStaleInputDrops ?? receiverStaleFrames)
        let newReceiverLoss: UInt64
        if feedbackFresh {
            newReceiverLoss = receiverLoss >= lastAdaptedReceiverLoss
                ? receiverLoss - lastAdaptedReceiverLoss
                : receiverLoss
            lastAdaptedReceiverLoss = receiverLoss
        } else {
            newReceiverLoss = 0
        }
        let receiverLatencyHigh = feedbackFresh
            && !recoveryBurstGrace
            && ((receiverRttMs != .max && receiverRttMs >= 50)
                || (receiverWireMs != .max && receiverWireMs >= 40))
        let congested = newDrops > 0
            || (!recoveryBurstGrace && lastSendBlockUs > 8_000)
            || (!recoveryBurstGrace && newReceiverLoss > 0)
            || receiverLatencyHigh
        let current = currentAverageBitrate
        let qualityHintForBitrate = manualQualityHint ?? currentQualityHint
        let highMotion = adaptiveMotionState.mode(at: nowNs) == .video
        if congested {
            stableBitrateWindows = 0
            consecutiveCongestedWindows += 1
            consecutiveRaiseSteps = 0
        } else {
            stableBitrateWindows += 1
            if stableBitrateWindows >= 8 {
                consecutiveCongestedWindows = 0
            }
        }
        // Only treat sustained congestion (two consecutive windows) as real.
        // A single lost datagram or one RTT spike is normal Wi-Fi behavior
        // and must not cut the bitrate.
        let congestionConfirmed = consecutiveCongestedWindows >= 2
        let canRaise = !congested && stableBitrateWindows >= 8
        if canRaise {
            stableBitrateWindows = 0
            consecutiveRaiseSteps += 1
        }
        stateLock.unlock()

        if let singleSession {
            adaptQualityIfNeeded(
                session: singleSession,
                receiverLoss: newReceiverLoss
            )
        }

        guard current > 0 else { return }
        let activeCount = max(1, withRegistry { $0.count })
        let streamFactor = activeCount > 1 ? (1.0 / Double(activeCount) * 1.3) : 1.0
        let pixelsPerSecond = Double(outWidth) * Double(outHeight) * Double(fps) * streamFactor
        // A hard 8Mbps single-stream floor prevented the controller from
        // escaping congestion even while receiver loss kept rising. Text is
        // still readable at the 4-6Mbps recovery band; quality climbs again
        // only after eight stable windows.
        let minFloor: Int
        let maxFloor: Int
        let minCeiling: Int
        let maxCeiling: Int
        if contentMode == .video {
            // 4K60 needs a materially larger spatial-quality budget. Keep
            // the frame rate fixed at 60 and spend the available capacity on
            // bits per frame instead of letting moving pictures collapse into
            // the old 8-28Mbps 1080p band.
            let ultraHd = outWidth >= 3_840 && outHeight >= 2_160
            if ultraHd {
                minFloor = activeCount > 1 ? 18_000_000 : 24_000_000
                maxFloor = activeCount > 1 ? 28_000_000 : 36_000_000
                minCeiling = activeCount > 1 ? 32_000_000 : 44_000_000
                maxCeiling = activeCount > 1 ? 56_000_000 : 80_000_000
            } else {
                minFloor = activeCount > 1 ? 7_000_000 : 8_000_000
                maxFloor = activeCount > 1 ? 10_000_000 : 12_000_000
                minCeiling = activeCount > 1 ? 10_000_000 : 12_000_000
                maxCeiling = activeCount > 1 ? 20_000_000 : 28_000_000
            }
        } else {
            minFloor = activeCount > 1 ? 3_000_000 : 4_000_000
            maxFloor = activeCount > 1 ? 10_000_000 : 14_000_000
            minCeiling = activeCount > 1 ? 8_000_000 : 24_000_000
            maxCeiling = activeCount > 1 ? 28_000_000 : 60_000_000
        }
        let qualityScale = qualityHintForBitrate
            .map(adaptiveQualityBitrateScale)
            ?? 1.0
        let floorBitrate = Int(min(
            max(pixelsPerSecond * 0.035 * qualityScale, Double(minFloor) * qualityScale),
            Double(maxFloor) * qualityScale
        ))
        let ceilingBitrate = Int(min(
            max(pixelsPerSecond * 0.14 * qualityScale, Double(minCeiling) * qualityScale),
            Double(maxCeiling) * qualityScale
        ))
        let target: Int
        if congestionConfirmed {
            target = max(floorBitrate, Int(Double(current) * 0.80))
            // The congestion floor has consumed the 4K bitrate budget: the
            // rate controller cannot restore the frame rate on its own.
            // Record the hand-off so the Viewer's resolution policy sees a
            // floor-collapse signal instead of an ordinary bitrate window.
            if adaptiveBitrateFloorDecision(
                activeWidth: Int(outWidth),
                activeHeight: Int(outHeight),
                floorBitrate: floorBitrate,
                currentBitrate: target
            ) == .downshiftTo1440p {
                stateLock.lock()
                bitrateFloorCollapseCount &+= 1
                bitrateFloorCollapseLastReason = "floor_reached_4k"
                stateLock.unlock()
                NSLog(
                    "Leftcar %@ adaptive bitrate floor collapse: 4K fallback requested at %d bps",
                    codecKind.rawValue.uppercased(),
                    target
                )
            }
        } else if highMotion {
            // Raise the budget as soon as a sustained high-change scene is
            // observed. This avoids waiting through eight stable windows,
            // which is too slow for the first seconds of a video.
            let ultraHd = outWidth >= 3_840 && outHeight >= 2_160
            let motionFloor = ultraHd
                ? (activeCount > 1 ? 36_000_000 : 48_000_000)
                : (activeCount > 1 ? 10_000_000 : 14_000_000)
            target = highMotionBitrateTarget(
                current: current,
                floor: floorBitrate,
                ceiling: ceilingBitrate,
                motionFloor: motionFloor
            )
        } else if canRaise {
            // Accelerating recovery: 4% → 8% → 16% per stable window so a
            // ratchet-down to the floor recovers in a few seconds while an
            // early overshoot is still corrected by the next cut.
            let raiseFactor = min(0.04 * pow(2.0, Double(min(consecutiveRaiseSteps - 1, 5))), 0.30)
            target = min(ceilingBitrate, Int(Double(current) * (1.0 + raiseFactor)))
        } else {
            return
        }
        guard target != current else { return }
        let applied: Bool
        switch applicationRoute {
        case .split:
            applied = activeSplitPipeline?.updateAggregateBitrate(target) == true
        case .single:
            guard let singleSession else { return }
            let status = VTSessionSetProperty(
                singleSession,
                key: kVTCompressionPropertyKey_AverageBitRate,
                value: target as CFNumber
            )
            if status == noErr {
                let hardLimitBytes = max(1, Int(Double(target) / 8.0 * 1.25))
                _ = VTSessionSetProperty(
                    singleSession,
                    key: kVTCompressionPropertyKey_DataRateLimits,
                    value: [hardLimitBytes, 1] as CFArray
                )
                applied = true
            } else {
                applied = false
            }
        case .unavailable:
            applied = false
        }
        guard applied else { return }
        stateLock.lock()
        currentAverageBitrate = target
        stateLock.unlock()
        NSLog(
            "Leftcar %@ adaptive bitrate: route=%@ current=%d target=%d congested=%@",
            codecKind.rawValue.uppercased(),
            String(describing: applicationRoute),
            current,
            target,
            congestionConfirmed ? "true" : "false"
        )
    }
}
