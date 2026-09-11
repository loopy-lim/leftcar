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
        // R4: read the shared-link congestion signal BEFORE taking stateLock.
        // The registry lock is never taken while stateLock is held here (other
        // paths run withRegistry and then call into sessions), so every
        // registry touch in this function stays outside the stateLock
        // critical sections and no lock-order dependency is created.
        let peerSampleNs = DispatchTime.now().uptimeNanoseconds
        let (activeCountSnapshot, peerCongested) = withRegistry { registry in
            let count = max(1, registry.count)
            let peerFresh = count > 1
                && registry.values.contains { session in
                    session !== self
                        && sharedCongestionMarkFresh(
                            markNs: session.sharedCongestionMarkNs,
                            nowNs: peerSampleNs
                        )
                }
            return (count, peerFresh)
        }
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
        // Latency only counts as congestion while it is WORSENING. A steady
        // high RTT/wire baseline (Wi-Fi power-save viewers sit there for the
        // whole session) previously voted congested in every window, so
        // `stableBitrateWindows` never reached eight and the bitrate could
        // never rise again after one cut — the stream degraded monotonically
        // over a long session while every cut made latency no better.
        let latencyWorsening: Bool
        if feedbackFresh, !recoveryBurstGrace {
            let rttKnown = receiverRttMs != .max
            let wireKnown = receiverWireMs != .max
            let rttWorse = rttKnown
                && lastAdaptedRttMs != .max
                && receiverRttMs >= 50
                && Int(receiverRttMs) - Int(lastAdaptedRttMs) >= 30
            let wireWorse = wireKnown
                && lastAdaptedWireMs != .max
                && receiverWireMs >= 40
                && Int(receiverWireMs) - Int(lastAdaptedWireMs) >= 30
            latencyWorsening = rttWorse || wireWorse
        } else {
            latencyWorsening = false
        }
        if feedbackFresh {
            lastAdaptedRttMs = receiverRttMs
            lastAdaptedWireMs = receiverWireMs
        }
        // A viewer that keeps its control feedback flowing but reports ZERO
        // rendered frames is stalled on its own side (app foregrounding,
        // surface recreation — observed 3-8s on a headset). Frames dropped
        // while nothing is being rendered are the stall's consequence, not
        // link congestion; letting them vote cut the bitrate after every app
        // access even though RTT and loss were clean.
        let viewerRenderStalled = feedbackFresh && receiverRenderedFps == nil
        let ownCongested = !viewerRenderStalled
            && (newDrops > 0
                || (!recoveryBurstGrace && lastSendBlockUs > 8_000)
                || (!recoveryBurstGrace && newReceiverLoss > 0)
                || latencyWorsening)
        // R4: a congestion mark broadcast by another session on the shared
        // link contributes exactly ONE vote, weighted like any local detector
        // and governed by the same two-consecutive-windows rule, so a single
        // transient elsewhere can only confirm the second window — it can
        // never cut on its own.
        let congested = ownCongested || peerCongested
        let current = currentAverageBitrate
        let qualityHintForBitrate = manualQualityHint ?? currentQualityHint
        let highMotion = adaptiveMotionState.mode(at: nowNs) == .video
        // R5: ramp eligibility is snapshotted before this window mutates the
        // ramp state; an eligible window spends one of the at-most-three
        // accelerated raises.
        let rampRaiseEligible = recoveryRamp.raiseEligible
        if congested {
            stableBitrateWindows = 0
            consecutiveCongestedWindows += 1
            consecutiveRaiseSteps = 0
            adaptiveCeilingCleanStreak = 0
            // R5: any congestion vote — including one shared from a peer —
            // cancels the ramp and re-arms the normal ladder logic.
            recoveryRamp = recoveryRamp.cancelled()
            if peerCongested {
                crossSessionCongestionPeerVotes &+= 1
            }
        } else {
            stableBitrateWindows += 1
            adaptiveCeilingCleanStreak += 1
            if stableBitrateWindows >= 8 {
                consecutiveCongestedWindows = 0
            }
            let advanced = recoveryRamp.advancedAfterCleanWindow()
            recoveryRamp = advanced.state
            if advanced.restartLadderClock {
                stableBitrateWindows = 0
            }
        }
        if ownCongested {
            crossSessionCongestionMarks &+= 1
        }
        let raiseCeilingSnapshot = adaptiveRaiseCeilingBitrate
        let ceilingCleanStreakSnapshot = adaptiveCeilingCleanStreak
        // Only treat sustained congestion (two consecutive windows) as real.
        // A single lost datagram or one RTT spike is normal Wi-Fi behavior
        // and must not cut the bitrate.
        let congestionConfirmed = consecutiveCongestedWindows >= 2
        // R5: while the recovery ramp has an accelerated raise pending it
        // takes precedence over the normal ladder for this window.
        let canRaise = !congested && stableBitrateWindows >= 8 && !rampRaiseEligible
        if canRaise {
            stableBitrateWindows = 0
            consecutiveRaiseSteps += 1
        }
        stateLock.unlock()

        // R4: broadcast own-detector congestion to the other sessions on the
        // shared link. Peer-echoed votes (ownCongested == false) deliberately
        // do not refresh the mark — see CaptureSession.sharedCongestionMarkNs.
        if ownCongested {
            withRegistry { _ in sharedCongestionMarkNs = nowNs }
        }

        if let singleSession {
            adaptQualityIfNeeded(
                session: singleSession,
                receiverLoss: newReceiverLoss
            )
        }

        guard current > 0 else { return }
        // R4: the active session count was snapshotted together with the peer
        // congestion flag before stateLock was taken (same withRegistry pass).
        let activeCount = activeCountSnapshot
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
            let ultraHd = isUltraHdDimensions(width: outWidth, height: outHeight)
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
        // Relax the congestion-cut ceiling only after sustained health so a
        // marginal link stops sawtoothing raise→collapse (see
        // adaptiveRaiseCeilingAfterCongestion).
        var raiseCeiling = raiseCeilingSnapshot
        if raiseCeiling > 0 {
            let relaxed = nextAdaptiveRaiseCeiling(
                currentCeiling: raiseCeiling,
                cleanStreak: ceilingCleanStreakSnapshot,
                globalCeiling: ceilingBitrate
            )
            if relaxed != raiseCeiling {
                raiseCeiling = relaxed
                stateLock.lock()
                adaptiveRaiseCeilingBitrate = relaxed
                adaptiveCeilingCleanStreak = 0
                stateLock.unlock()
            }
        }
        let effectiveCeiling = raiseCeiling > 0 ? min(raiseCeiling, ceilingBitrate) : ceilingBitrate
        let target: Int
        if congestionConfirmed {
            target = max(floorBitrate, Int(Double(current) * 0.80))
            stateLock.lock()
            adaptiveRaiseCeilingBitrate = adaptiveRaiseCeilingAfterCongestion(
                failingBitrate: current
            )
            adaptiveCeilingCleanStreak = 0
            // R5: the cut arms the bounded recovery ramp. The pre-cut level is
            // already remembered by the congestion-cut memory ceiling above
            // (90% of the failing bitrate), so the ramp reuses that value as
            // its cap instead of keeping a separate lastCutFromValue.
            recoveryRamp = recoveryRamp.rearmedAfterCut()
            stateLock.unlock()
            // The congestion floor has consumed the 4K bitrate budget: the
            // rate controller cannot restore the frame rate on its own.
            // This monotonic counter records detected floor pressure, not a
            // successfully applied bitrate transition. The Viewer deliberately
            // requires pressure in two status windows before changing resolution,
            // so target==current and encoder rejection remain valid evidence that
            // bitrate control cannot relieve congestion at this resolution.
            if adaptiveBitrateFloorDecision(
                activeWidth: Int(outWidth),
                activeHeight: Int(outHeight),
                floorBitrate: floorBitrate,
                currentBitrate: target
            ) == .downshiftTo1440p {
                stateLock.lock()
                bitrateFloorCollapseCount &+= 1
                bitrateFloorCollapseLastReason = "resolution_fallback_floor_reached"
                stateLock.unlock()
                NSLog(
                    "Leftcar %@ adaptive bitrate floor collapse: resolution fallback requested at %d bps",
                    codecKind.rawValue.uppercased(),
                    target
                )
            }
        } else if highMotion {
            // Raise the budget as soon as a sustained high-change scene is
            // observed. This avoids waiting through eight stable windows,
            // which is too slow for the first seconds of a video. The
            // congestion-cut ceiling still applies: high motion must not push
            // the stream back into a level that just collapsed the link.
            let ultraHd = isUltraHdDimensions(width: outWidth, height: outHeight)
            let motionFloor = ultraHd
                ? (activeCount > 1 ? 36_000_000 : 48_000_000)
                : (activeCount > 1 ? 10_000_000 : 14_000_000)
            target = highMotionBitrateTarget(
                current: current,
                floor: floorBitrate,
                ceiling: effectiveCeiling,
                motionFloor: min(motionFloor, effectiveCeiling)
            )
        } else if rampRaiseEligible {
            // R5 bounded recovery ramp: two clean windows after the cut armed
            // at most three accelerated raises of +30% per clean window. The
            // congestion-cut memory ceiling (90% of the pre-cut bitrate)
            // still bounds every step, so the ramp can never climb back into
            // the level that just collapsed the link; once the ramp is spent
            // the normal eight-clean-window ladder resumes.
            let capped = recoveryRampRaiseTarget(
                current: current,
                ceiling: effectiveCeiling
            )
            guard capped > current else { return }
            target = capped
        } else if canRaise {
            // Accelerating recovery: 4% → 8% → 16% per stable window so a
            // ratchet-down to the floor recovers in a few seconds while an
            // early overshoot is still corrected by the next cut.
            let raiseFactor = min(0.04 * pow(2.0, Double(min(consecutiveRaiseSteps - 1, 5))), 0.30)
            let capped = min(effectiveCeiling, Int(Double(current) * (1.0 + raiseFactor)))
            guard capped > current else { return }
            target = capped
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
                let hardLimitBytes = vtHardLimitBytes(bitrate: target)
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
