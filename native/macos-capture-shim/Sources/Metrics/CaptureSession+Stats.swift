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
    // MARK: Stats

    func statsJSON() -> String {
        let nowNs = DispatchTime.now().uptimeNanoseconds
        captureLock.lock()
        let currentEncodeInFlight = encodeInFlight
        let currentMaxEncodeInFlight = maxEncodeInFlight
        let reportedSplitFlowActiveLeases = splitFlowState.activeCount
        let reportedSplitFlowCapacity = splitFlowState.capacity
        let splitCaptureQueueDepth = pendingSplitCaptures.count
        let splitCaptureOldestCallbackNs = pendingSplitCaptures
            .map(\.callbackNs).min()
        let reportedSplitCaptureQueueOldestUs = splitCaptureOldestCallbackNs.map {
            nowNs >= $0 ? (nowNs - $0) / 1_000 : 0
        } ?? 0
        let reportedSplitRecoveryBoundaryPending =
            splitFlowState.recoveryBoundaryPending
        let reportedSplitRecoveryGatePendingUs: UInt64 = splitRecoveryGateStartedNs != 0
            && nowNs >= splitRecoveryGateStartedNs
            ? (nowNs - splitRecoveryGateStartedNs) / 1_000 : 0
        captureLock.unlock()

        stateLock.lock()
        // roll the 1s rate window
        let now = Date()
        let elapsed = now.timeIntervalSince(rateWindowStart)
        if elapsed >= 1.0 {
            lastCaptureFps = UInt32((Double(rateWindowCaptureCallbacks) / elapsed).rounded())
            lastFps = UInt32((Double(rateWindowFrames) / elapsed).rounded())
            lastEncodeOutputFps = UInt32((Double(rateWindowEncodeOutputCallbacks) / elapsed).rounded())
            lastEncoderFrameDropFps = UInt32((Double(rateWindowEncoderFrameDrops) / elapsed).rounded())
            lastKbps = UInt32((Double(rateWindowBytes) * 8.0 / 1000.0 / elapsed).rounded())
            rateWindowStart = now
            rateWindowCaptureCallbacks = 0
            rateWindowFrames = 0
            rateWindowEncodeOutputCallbacks = 0
            rateWindowEncoderFrameDrops = 0
            rateWindowBytes = 0
        }

        let state = lifecycleState
        let captureToEncodeUs = lastCaptureToEncodeUs
        let maxCaptureToEncodeUs = maxCaptureToEncodeUs
        let captureQueueWaitUs = lastCaptureQueueWaitUs
        let maxCaptureQueueWaitUs = maxCaptureQueueWaitUs
        let inputPreparationUs = lastInputPreparationUs
        let maxInputPreparationUs = maxInputPreparationUs
        let encodeSubmitCallUs = lastEncodeSubmitCallUs
        let maxEncodeSubmitCallUs = maxEncodeSubmitCallUs
        let encoderCallbackUs = lastEncoderCallbackUs
        let maxEncoderCallbackUs = maxEncoderCallbackUs
        let encodeOutputUs = lastEncodeOutputUs
        let maxEncodeOutputUs = maxEncodeOutputUs
        let packetizationInFlight = packetizationAdmissionState.inFlight
        let packetizationAdmissionDrops = packetizationAdmissionDrops
        let packetizationUs = lastPacketizationUs
        let maxPacketizationUs = maxPacketizationUs
        let packetizationQueueWaitUs = lastPacketizationQueueWaitUs
        let maxPacketizationQueueWaitUs = maxPacketizationQueueWaitUs
        let sendBlockUs = lastSendBlockUs
        let maxSendBlockUs = maxSendBlockUs
        let sendPaceUs = lastSendPaceUs
        let maxSendPaceUs = maxSendPaceUs
        let networkDropped = framesDropped
        let networkQueueDropped = networkQueueDropped
        let recoveryFramesDropped = recoveryFramesDropped
        let sentDatagrams = sentDatagrams
        let sentParityDatagrams = sentParityDatagrams
        let lastAuBytes = lastAuBytes
        let lastAuFragments = lastAuFragments
        let lastAuParity = lastAuParity
        let lastAuDatagrams = lastAuDatagrams
        let lastAuExpectedDatagrams = lastAuExpectedDatagrams
        let lastAuSendUs = lastAuSendUs
        let lastAuIsKeyframe = lastAuIsKeyframe
        let maxAuBytes = maxAuBytes
        let maxAuFragments = maxAuFragments
        let udpSendFailures = udpSendFailures
        let udpSendRetries = udpSendRetries
        let recoveryKeyframes = recoveryKeyframes
        let recoveryRequestsSuppressed = recoveryRequestsSuppressed
        let captureQueueDropped = captureQueueDropped
        let captureCallbacks = captureCallbacks
        let encodeOutputCallbacks = encodeOutputCallbacks
        let encoderFrameDrops = encoderFrameDrops
        let encodeSubmitFailures = encodeSubmitFailures
        let reportedEncoderWatchdogRestarts = encoderWatchdogRestarts
        let reportedEncoderWatchdogTerminations = encoderWatchdogTerminations
        let reportedEncoderLateCallbacks = encoderLateCallbacks
        let reportedEncoderWatchdogOldestUs = singleEncodeSubmissionLedger
            .oldestSubmissionNs(generation: encoderSessionGeneration)
            .map { nowNs >= $0 ? (nowNs - $0) / 1_000 : 0 } ?? 0
        let framesDropped = networkDropped + captureQueueDropped
        let framesEncoded = framesEncoded
        let bytesSent = bytesSent
        let reportedCaptureFps = lastCaptureFps
        let reportedFps = lastFps
        let reportedEncodeOutputFps = lastEncodeOutputFps
        let reportedEncoderFrameDropFps = lastEncoderFrameDropFps
        let reportedKbps = lastKbps
        let error = stoppedReason
        let codec = codecKind.rawValue
        let reportedEncoderID = encoderID
        let reportedEncoderHardware: Any = encoderHardwareAccelerated
            .map { NSNumber(value: $0) } ?? NSNull()
        let reportedEncoderPreset = encoderPreset
        let reportedEncoderProfile = encoderProfile
        let reportedEncoderMode = encoderMode
        let reportedEncoderAppliedProperties = encoderAppliedProperties
        let reportedEncoderSuppressedProperties = encoderSuppressedProperties
        let reportedEncoderUnsupportedProperties = encoderUnsupportedProperties
        let reportedEncoderRejectedProperties = encoderRejectedProperties
        let reportedEncoderFallbackReason: Any = encoderFallbackReason ?? NSNull()
        let reportedEncoderExperimentRequested = requestedEncoderExperiment
        let reportedEncoderExperimentApplied = appliedEncoderExperimentValue
        let reportedEncoderExperimentFallbackReason = encoderFallbackReason
        let reportedBaseFrameQp: Int32? = appliedEncoderExperimentValue == .adaptiveQp
            ? adaptiveQpController.currentBaseFrameQp
            : nil
        let reportedBaseFrameQpChanges = baseFrameQpChanges
        let firstCaptureMs = firstCaptureNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let firstEncodeMs = firstEncodeNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let firstSendMs = firstSendNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let currentBitrate = currentAverageBitrate
        let qualityHintValue: Any = currentQualityHint
            .map { NSNumber(value: $0) } ?? NSNull()
        let qualityOverrideValue: Any = manualQualityHint
            .map { NSNumber(value: $0) } ?? NSNull()
        let qualityChecks = qualityAdaptationChecks
        let qualityChanges = qualityAdaptationChanges
        let qualityRejections = qualityAdaptationRejections
        let qualityStatus = qualityAdaptationLastStatus
        let qualityLastEncodeP95Us = qualityAdaptationLastEncodeP95Us
        let qualityLastQueueP95Us = qualityAdaptationLastQueueP95Us
        let qualityLastReceiverLoss = qualityAdaptationLastReceiverLoss
        let captureIntervalP95Us = percentile95(captureIntervalSamplesUs)
        let encodeOutputIntervalP50Us = percentile(
            encodeOutputIntervalSamplesUs,
            quantile: 0.50
        )
        let encodeOutputIntervalP95Us = percentile95(encodeOutputIntervalSamplesUs)
        let captureToEncodeP95Us = percentile95(captureToEncodeSamplesUs)
        let captureQueueWaitP95Us = percentile95(captureQueueWaitSamplesUs)
        let inputPreparationP95Us = percentile95(inputPreparationSamplesUs)
        let encodeSubmitCallP50Us = percentile(
            encodeSubmitCallSamplesUs,
            quantile: 0.50
        )
        let encodeSubmitCallP95Us = percentile95(encodeSubmitCallSamplesUs)
        let encoderCallbackP50Us = percentile(
            encoderCallbackSamplesUs,
            quantile: 0.50
        )
        let encoderCallbackP95Us = percentile95(encoderCallbackSamplesUs)
        let encodeOutputP95Us = percentile95(encodeOutputSamplesUs)
        let packetizationP95Us = percentile95(packetizationSamplesUs)
        let packetizationQueueWaitP95Us = percentile95(packetizationQueueWaitSamplesUs)
        let sendBlockP95Us = percentile95(sendBlockSamplesUs)
        let sendPaceP95Us = percentile95(sendPaceSamplesUs)
        let experimentStatsFields = encoderExperimentStatsFields(
            requested: reportedEncoderExperimentRequested,
            applied: reportedEncoderExperimentApplied,
            fallbackReason: reportedEncoderExperimentFallbackReason,
            baseFrameQp: reportedBaseFrameQp,
            baseFrameQpChanges: reportedBaseFrameQpChanges,
            encoderFrameDrops: encoderFrameDrops,
            encoderFrameDropFps: reportedEncoderFrameDropFps,
            validEncodeOutputFps: reportedEncodeOutputFps,
            encodeSubmitCallP50Us: encodeSubmitCallP50Us,
            encodeSubmitCallP95Us: encodeSubmitCallP95Us,
            encoderCallbackP50Us: encoderCallbackP50Us,
            encoderCallbackP95Us: encoderCallbackP95Us,
            packetizationInFlight: packetizationInFlight
        )
        let receiverStaleInputDropsValue: Any = receiverStaleInputDrops
            .map { NSNumber(value: $0) } ?? NSNull()
        let receiverRttValue: Any = receiverRttMs == .max
            ? NSNull()
            : NSNumber(value: receiverRttMs)
        let receiverWireValue: Any = receiverWireMs == .max
            ? NSNull()
            : NSNumber(value: receiverWireMs)
        let receiverFeedbackAgeValue: Any = receiverFeedbackNs == 0
            ? NSNull()
            : NSNumber(value: (nowNs &- receiverFeedbackNs) / 1_000_000)
        let reportedReceiverFrameGaps = receiverFrameGaps
        let reportedReceiverInputDrops = receiverInputDrops
        let reportedReceiverIncompleteAUs = receiverIncompleteAUs
        let reportedReceiverStaleFrames = receiverStaleFrames
        let reportedReceiverOutputBurstDiscards = receiverOutputBurstDiscards
        let reportedMotionMode = adaptiveMotionState.mode(at: nowNs)
        let reportedDirtyChangedPixelRatio = lastDirtyChangedPixelRatio
        let reportedDirtyRectCount = lastDirtyRectCount
        let reportedMotionTransitions = adaptiveMotionTransitions
        let reportedMotionEvidence = adaptiveMotionEvidence
        let reportedUdpBurstDatagrams = motionAdjustedUdpBurstDatagrams(
            base: activeUdpBurstDatagrams,
            mode: reportedMotionMode
        )
        let reportedUdpPacingRateMultiplier = udpPacingRateMultiplier(
            profile: appliedUdpStability.profile,
            burstDatagrams: activeUdpBurstDatagrams
        )
        let reportedUdpFecParityShards = activeUdpFecParityShards
        let reportedUdpBurstReason = activeUdpBurstReason
        let reportedReceiverMediaDatagrams = receiverMediaDatagrams
        let reportedReceiverDataDatagrams = receiverDataDatagrams
        let reportedReceiverParityDatagrams = receiverParityDatagrams
        let reportedReceiverFecRestoredFragments = receiverFecRestoredFragments
        let reportedReceiverUnrecoverableFecGroups = receiverUnrecoverableFecGroups
        let reportedReceiverMaxMissingDataFragments = receiverMaxMissingDataFragments
        let reportedReceiverOneFrameGapEvents = receiverOneFrameGapEvents
        let reportedReceiverMultiFrameGapEvents = receiverMultiFrameGapEvents
        let reportedReceiverPairedIdrEpisodes = receiverPairedIdrEpisodes
        let reportedReceiverSuppressedRecoveryRequests = receiverSuppressedRecoveryRequests
        let reportedReceiverFecDecodeFailures = receiverFecDecodeFailures
        let receiverRenderedFpsValue: Any = receiverRenderedFps
            .map { NSNumber(value: $0) } ?? NSNull()
        let splitPreparationP50Us = percentile(
            splitPreparationSamplesUs,
            quantile: 0.50
        )
        let splitPreparationP95Us = percentile95(splitPreparationSamplesUs)
        let splitPairCallbackP50Us = percentile(
            splitPairCallbackSamplesUs,
            quantile: 0.50
        )
        let splitPairCallbackP95Us = percentile95(splitPairCallbackSamplesUs)
        let reportedSplitPairAdmissionDrops = splitPairAdmissionDrops
        let reportedSplitPairDrops = splitPairDrops
        let reportedSplitPairTimeouts = splitPairTimeouts
        let reportedSplitLastPairDropReason = splitLastPairDropReason
        let reportedSplitInjectedRightDrops = splitInjectedRightDrops
        let reportedSplitPairsEncoded = splitPairsEncoded
        let reportedSplitLeftOutputs = splitLeftOutputs
        let reportedSplitRightOutputs = splitRightOutputs
        let reportedSplitLeftEncoderID = splitLeftEncoderID
        let reportedSplitRightEncoderID = splitRightEncoderID
        let reportedSplitLeftHardware = splitLeftHardware
        let reportedSplitRightHardware = splitRightHardware
        let reportedSplitLeftReceiverLoss = splitLeftReceiverLoss
        let reportedSplitRightReceiverLoss = splitRightReceiverLoss
        let reportedSplitLeftRenderedFps = splitLeftRenderedFps
        let reportedSplitRightRenderedFps = splitRightRenderedFps
        let reportedSplitJoinedRenderedFps = splitJoinedRenderedFps
        let reportedSplitPairReadyDeltaP95Us = splitPairReadyDeltaP95Us
        let reportedSplitPairReadyDeltaMaxUs = splitPairReadyDeltaMaxUs
        let reportedSplitPairSyncTimeouts = splitPairSyncTimeouts
        let reportedSplitUnmatchedOutputDrops = splitUnmatchedOutputDrops
        let reportedSplitPreEncodeAdmissionDrops = splitPreEncodeAdmissionDrops
        let reportedSplitRecoveryBoundaryDiscards = splitRecoveryBoundaryDiscards
        let reportedSplitPostEncodeDeltaDrops = splitPostEncodeDeltaDrops
        let reportedSplitWirePairsAttempted = splitWirePairsAttempted
        let reportedSplitWirePairSendFailures = splitWirePairSendFailures
        let reportedSplitKeyframeGapRecoveries = splitKeyframeGapRecoveries
        let reportedSplitDeltaGapRecoveries = splitDeltaGapRecoveries
        stateLock.unlock()

        networkLock.lock()
        let reportedSplitEncodedQueueDepth = pendingSplitAccessUnits.count
        let splitOldestQueuedNs = pendingSplitAccessUnits.map(\.queuedNs).min()
        let reportedSplitEncodedQueueOldestUs = splitOldestQueuedNs.map {
            nowNs >= $0 ? (nowNs - $0) / 1_000 : 0
        } ?? 0
        let queueSnapshot = networkQueueSnapshot(
            frames: pendingFrames,
            splitAccessUnits: pendingSplitAccessUnits,
            nowNs: nowNs
        )
        networkLock.unlock()

        stateLock.lock()
        let shouldLogPerf = nowNs >= lastPerfLogNs
            && nowNs - lastPerfLogNs >= 1_000_000_000
        if shouldLogPerf { lastPerfLogNs = nowNs }
        stateLock.unlock()

        if shouldLogPerf {
            let perfLogLine = leftcarPerfLogLine(
                captureCallbacks: captureCallbacks,
                encodeOutputCallbacks: encodeOutputCallbacks,
                captureFps: reportedCaptureFps,
                encodeOutputFps: reportedEncodeOutputFps,
                encodeOutputIntervalP50Us: encodeOutputIntervalP50Us,
                encodeOutputIntervalP95Us: encodeOutputIntervalP95Us,
                encodeOutputP95Us: encodeOutputP95Us,
                inputPreparationP95Us: inputPreparationP95Us,
                queueOldestUs: queueSnapshot.oldestAgeUs,
                encoderWatchdogRestarts: reportedEncoderWatchdogRestarts,
                encoderWatchdogTerminations: reportedEncoderWatchdogTerminations,
                encoderLateCallbacks: reportedEncoderLateCallbacks,
                encoderWatchdogOldestUs: reportedEncoderWatchdogOldestUs,
                encoderMode: reportedEncoderMode,
                encoderID: reportedEncoderID
            )
            leftcarPerformanceLogger.notice("\(perfLogLine, privacy: .public)")
            if reportedEncoderMode == "splitVertical" {
                leftcarPerformanceLogger.notice(
                    "LeftcarSplit pairs=\(reportedSplitPairsEncoded) captureQueueDrops=\(captureQueueDropped) admissionDrops=\(reportedSplitPairAdmissionDrops) pairDrops=\(reportedSplitPairDrops) pairTimeouts=\(reportedSplitPairTimeouts) lastPairDropReason=\(reportedSplitLastPairDropReason, privacy: .public) inFlight=\(currentEncodeInFlight)/\(currentMaxEncodeInFlight) preparationP95Us=\(splitPreparationP95Us) callbackP95Us=\(splitPairCallbackP95Us) captureQueueDepth=\(splitCaptureQueueDepth) captureQueueOldestUs=\(reportedSplitCaptureQueueOldestUs) recoveryGatePending=\(reportedSplitRecoveryBoundaryPending) recoveryGatePendingUs=\(reportedSplitRecoveryGatePendingUs) preEncodeAdmissionDrops=\(reportedSplitPreEncodeAdmissionDrops)"
                )
            }
        }

        var obj: [String: Any] = [
            "frames": framesEncoded,
            "dropped": framesDropped,
            "networkDropped": networkDropped,
            "networkQueueDropped": networkQueueDropped,
            "recoveryFramesDropped": recoveryFramesDropped,
            "udpSendFailures": udpSendFailures,
            "udpSendRetries": udpSendRetries,
            "recoveryKeyframes": recoveryKeyframes,
            "recoveryRequestsSuppressed": recoveryRequestsSuppressed,
            "captureQueueDropped": captureQueueDropped,
            "bytes": bytesSent,
            "state": state,
            "fps": reportedFps,
            "captureFps": reportedCaptureFps,
            "encodeSubmitFps": reportedFps,
            "encodeOutputFps": reportedEncodeOutputFps,
            "kbps": reportedKbps,
            "fpsTarget": self.fps,
            "captureCallbacks": captureCallbacks,
            "encodeOutputCallbacks": encodeOutputCallbacks,
            "encodeSubmitFailures": encodeSubmitFailures,
            "encodeInFlight": currentEncodeInFlight,
            "encoderWatchdogRestarts": reportedEncoderWatchdogRestarts,
            "encoderWatchdogTerminations": reportedEncoderWatchdogTerminations,
            "encoderLateCallbacks": reportedEncoderLateCallbacks,
            "encoderWatchdogOldestUs": reportedEncoderWatchdogOldestUs,
            "packetizationAdmissionDrops": packetizationAdmissionDrops,
            "codec": codec,
            "encoderID": reportedEncoderID,
            "encoderHardwareAccelerated": reportedEncoderHardware,
            "encoderPreset": reportedEncoderPreset,
            "encoderProfile": reportedEncoderProfile,
            "encoderMode": reportedEncoderMode,
            "encoderAppliedProperties": reportedEncoderAppliedProperties,
            "encoderSuppressedProperties": reportedEncoderSuppressedProperties,
            "encoderUnsupportedProperties": reportedEncoderUnsupportedProperties,
            "encoderRejectedProperties": reportedEncoderRejectedProperties,
            "encoderFallbackReason": reportedEncoderFallbackReason,
            "captureBackend": backend.rawValue,
            "mediaTransport": mediaTransport.rawValue,
            "udpStabilityProfile": appliedUdpStability.profile.rawValue,
            "udpBurstDatagrams": reportedUdpBurstDatagrams,
            "udpPacingRateMultiplier": reportedUdpPacingRateMultiplier,
            "udpFecParityShards": reportedUdpFecParityShards,
            "udpAdaptivePacing": appliedUdpStability.adaptivePacing,
            "udpBurstReason": reportedUdpBurstReason,
            "adaptiveMotionMode": reportedMotionMode.rawValue,
            "adaptiveMotionEvidence": reportedMotionEvidence,
            "adaptiveMotionTransitions": reportedMotionTransitions,
            "dirtyChangedPixelRatio": reportedDirtyChangedPixelRatio,
            "dirtyRectCount": reportedDirtyRectCount,
            "firstCaptureMs": firstCaptureMs,
            "firstEncodeMs": firstEncodeMs,
            "firstSendMs": firstSendMs,
            "currentBitrate": currentBitrate,
            "bitrateFloorCollapseCount": bitrateFloorCollapseCount,
            "bitrateFloorCollapseLastReason": bitrateFloorCollapseLastReason,
            "qualityHint": qualityHintValue,
            "qualityOverride": qualityOverrideValue,
            "qualityAdaptationChecks": qualityChecks,
            "qualityAdaptationChanges": qualityChanges,
            "qualityAdaptationRejections": qualityRejections,
            "qualityAdaptationLastStatus": qualityStatus,
            "qualityAdaptationLastEncodeP95Us": qualityLastEncodeP95Us,
            "qualityAdaptationLastQueueP95Us": qualityLastQueueP95Us,
            "qualityAdaptationLastReceiverLoss": qualityLastReceiverLoss,
            "captureIntervalP95Us": captureIntervalP95Us,
            "encodeOutputIntervalP50Us": encodeOutputIntervalP50Us,
            "encodeOutputIntervalP95Us": encodeOutputIntervalP95Us,
            "captureToEncodeP95Us": captureToEncodeP95Us,
            "captureQueueWaitP95Us": captureQueueWaitP95Us,
            "inputPreparationP95Us": inputPreparationP95Us,
            "encodeOutputP95Us": encodeOutputP95Us,
            "packetizationP95Us": packetizationP95Us,
            "packetizationQueueWaitP95Us": packetizationQueueWaitP95Us,
            "sendBlockP95Us": sendBlockP95Us,
            "sendPaceP95Us": sendPaceP95Us,
            "lastAuBytes": lastAuBytes,
            "lastAuFragments": lastAuFragments,
            "lastAuParity": lastAuParity,
            "lastAuDatagrams": lastAuDatagrams,
            "lastAuExpectedDatagrams": lastAuExpectedDatagrams,
            "lastAuSendUs": lastAuSendUs,
            "lastAuIsKeyframe": lastAuIsKeyframe,
            "maxAuBytes": maxAuBytes,
            "maxAuFragments": maxAuFragments,
            "sentDatagrams": sentDatagrams,
            "sentParityDatagrams": sentParityDatagrams,
            "captureToEncodeUs": captureToEncodeUs,
            "maxCaptureToEncodeUs": maxCaptureToEncodeUs,
            "captureQueueWaitUs": captureQueueWaitUs,
            "maxCaptureQueueWaitUs": maxCaptureQueueWaitUs,
            "inputPreparationUs": inputPreparationUs,
            "maxInputPreparationUs": maxInputPreparationUs,
            "encodeSubmitCallUs": encodeSubmitCallUs,
            "maxEncodeSubmitCallUs": maxEncodeSubmitCallUs,
            "encoderCallbackUs": encoderCallbackUs,
            "maxEncoderCallbackUs": maxEncoderCallbackUs,
            "encodeOutputUs": encodeOutputUs,
            "maxEncodeOutputUs": maxEncodeOutputUs,
            "packetizationUs": packetizationUs,
            "maxPacketizationUs": maxPacketizationUs,
            "packetizationQueueWaitUs": packetizationQueueWaitUs,
            "maxPacketizationQueueWaitUs": maxPacketizationQueueWaitUs,
            "sendBlockUs": sendBlockUs,
            "maxSendBlockUs": maxSendBlockUs,
            "sendPaceUs": sendPaceUs,
            "maxSendPaceUs": maxSendPaceUs,
            "receiverFrameGaps": reportedReceiverFrameGaps,
            "receiverInputDrops": reportedReceiverInputDrops,
            "receiverIncompleteAus": reportedReceiverIncompleteAUs,
            "receiverStaleFrames": reportedReceiverStaleFrames,
            "receiverStaleInputDrops": receiverStaleInputDropsValue,
            "receiverOutputBurstDiscards": reportedReceiverOutputBurstDiscards,
            "receiverRenderedFps": receiverRenderedFpsValue,
            "receiverRttMs": receiverRttValue,
            "receiverWireMs": receiverWireValue,
            "receiverFeedbackAgeMs": receiverFeedbackAgeValue,
            "receiverMediaDatagrams": reportedReceiverMediaDatagrams,
            "receiverDataDatagrams": reportedReceiverDataDatagrams,
            "receiverParityDatagrams": reportedReceiverParityDatagrams,
            "receiverFecRestoredFragments": reportedReceiverFecRestoredFragments,
            "receiverUnrecoverableFecGroups": reportedReceiverUnrecoverableFecGroups,
            "receiverMaxMissingDataFragments": reportedReceiverMaxMissingDataFragments,
            "receiverOneFrameGapEvents": reportedReceiverOneFrameGapEvents,
            "receiverMultiFrameGapEvents": reportedReceiverMultiFrameGapEvents,
            "receiverPairedIdrEpisodes": reportedReceiverPairedIdrEpisodes,
            "receiverSuppressedRecoveryRequests": reportedReceiverSuppressedRecoveryRequests,
            "receiverFecDecodeFailures": reportedReceiverFecDecodeFailures,
            "pendingFrame": queueSnapshot.count,
            "pendingFrameBytes": queueSnapshot.bytes,
            "pendingFrameOldestAgeUs": queueSnapshot.oldestAgeUs,
            "splitPreparationP50Us": splitPreparationP50Us,
            "splitPreparationP95Us": splitPreparationP95Us,
            "splitPairCallbackP50Us": splitPairCallbackP50Us,
            "splitPairCallbackP95Us": splitPairCallbackP95Us,
            "splitPairAdmissionDrops": reportedSplitPairAdmissionDrops,
            "splitPairDrops": reportedSplitPairDrops,
            "splitPairTimeouts": reportedSplitPairTimeouts,
            "splitLastPairDropReason": reportedSplitLastPairDropReason,
            "splitInjectedRightDrops": reportedSplitInjectedRightDrops,
            "splitPairsEncoded": reportedSplitPairsEncoded,
            "splitLeftOutputs": reportedSplitLeftOutputs,
            "splitRightOutputs": reportedSplitRightOutputs,
            "splitLeftOutputFps": reportedFps,
            "splitRightOutputFps": reportedFps,
            "splitJoinedOutputFps": reportedFps,
            "splitLeftEncoderID": reportedSplitLeftEncoderID,
            "splitRightEncoderID": reportedSplitRightEncoderID,
            "splitLeftHardware": reportedSplitLeftHardware,
            "splitRightHardware": reportedSplitRightHardware,
            "splitAggregateBitrate": currentBitrate,
            "splitPerTileBitrate": currentBitrate / 2,
            "splitDirection": reportedEncoderExperimentApplied == .splitVertical
                ? "vertical"
                : NSNull(),
            "encodedPairCallbackP50Us": splitPairCallbackP50Us,
            "encodedPairCallbackP95Us": splitPairCallbackP95Us,
            "encodedPairTimeouts": reportedSplitPairTimeouts,
            "encodedPairDrops": reportedSplitPairDrops,
            "leftValidEncodeOutputFps": reportedFps,
            "rightValidEncodeOutputFps": reportedFps,
            "leftEncoderFrameDrops": 0,
            "rightEncoderFrameDrops": 0,
            "leftBitrateBps": currentBitrate / 2,
            "rightBitrateBps": currentBitrate / 2,
            "aggregateBitrateBps": currentBitrate,
            "leftReceiverLoss": reportedSplitLeftReceiverLoss,
            "rightReceiverLoss": reportedSplitRightReceiverLoss,
            "leftRenderedFps": reportedSplitLeftRenderedFps,
            "rightRenderedFps": reportedSplitRightRenderedFps,
            "joinedRenderedFps": reportedSplitJoinedRenderedFps,
            "pairReadyDeltaP95Us": reportedSplitPairReadyDeltaP95Us,
            "pairReadyDeltaMaxUs": reportedSplitPairReadyDeltaMaxUs,
            "pairSyncTimeouts": reportedSplitPairSyncTimeouts,
            "unmatchedOutputDrops": reportedSplitUnmatchedOutputDrops,
            "pairedRecoveryRequests": recoveryKeyframes,
            "pairedRecoveryKeyframes": recoveryKeyframes,
            "splitTestInjectedDrops": reportedSplitInjectedRightDrops,
            "splitFlowActiveLeases": reportedSplitFlowActiveLeases,
            "splitFlowCapacity": reportedSplitFlowCapacity,
            "splitPreEncodeAdmissionDrops": reportedSplitPreEncodeAdmissionDrops,
            "splitCaptureQueueDepth": splitCaptureQueueDepth,
            "splitCaptureQueueOldestUs": reportedSplitCaptureQueueOldestUs,
            "splitRecoveryBoundaryPending": reportedSplitRecoveryBoundaryPending,
            "splitRecoveryGatePendingUs": reportedSplitRecoveryGatePendingUs,
            "splitEncodedQueueDepth": reportedSplitEncodedQueueDepth,
            "splitEncodedQueueOldestUs": reportedSplitEncodedQueueOldestUs,
            "splitRecoveryBoundaryDiscards": reportedSplitRecoveryBoundaryDiscards,
            "splitPostEncodeDeltaDrops": reportedSplitPostEncodeDeltaDrops,
            "splitWirePairsAttempted": reportedSplitWirePairsAttempted,
            "splitWirePairSendFailures": reportedSplitWirePairSendFailures,
            "splitKeyframeGapRecoveries": reportedSplitKeyframeGapRecoveries,
            "splitDeltaGapRecoveries": reportedSplitDeltaGapRecoveries,
            "error": error,
        ]
        obj.merge(experimentStatsFields) { _, testedValue in testedValue }
        if let data = try? JSONSerialization.data(withJSONObject: obj),
           let s = String(data: data, encoding: .utf8) {
            return s
        }
        return "{\"state\":\"\(state)\"}"
    }
}
