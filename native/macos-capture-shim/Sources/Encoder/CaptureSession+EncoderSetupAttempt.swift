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

enum EncoderSetupAttempt {
    case installed
    case failed(String)
}

extension CaptureSession {
    func attemptEncoderSetup(
        policy: EncoderSessionPolicy,
        width w: Int32,
        height h: Int32,
        latencyPolicy: EncoderLatencyPolicy,
        averageBitrate avgBitrate: Double,
        requestedExperiment: EncoderExperiment,
        availableEncoders: [EncoderCandidateDescriptor],
        priorAveFallbackReason: String?
    ) -> EncoderSetupAttempt {
        var lastFailure = "encoder setup failed"
        var aveFallbackReason = priorAveFallbackReason
            let requestedEncoderID: String?
            switch policy.mode {
            case .ave:
                guard let encoderID = preferredHardwareEncoderID(
                    codec: policy.codec,
                    candidates: availableEncoders,
                    preferredID: policy.mode == .ave && policy.codec == .h264
                        ? phaseAAVEH264EncoderID
                        : nil
                ) else {
                    lastFailure = "AVE \(policy.codec.rawValue.uppercased()) hardware encoder unavailable"
                    aveFallbackReason = lastFailure
                    NSLog("Leftcar %@", lastFailure)
                    return .failed(lastFailure)
                }
                requestedEncoderID = encoderID
            case .rtvc:
                requestedEncoderID = nil
            }

            guard let specificationPlan = encoderSpecificationPlan(
                mode: policy.mode,
                encoderID: requestedEncoderID
            ) else {
                lastFailure = "\(policy.mode.rawValue.uppercased()) encoder specification unavailable"
                if policy.mode == .ave { aveFallbackReason = lastFailure }
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }
            var encoderSpecificationValues: [String: Any] = [:]
            for entry in specificationPlan {
                switch entry {
                case .requireHardware:
                    encoderSpecificationValues[
                        kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder as String
                    ] = true
                case let .encoderID(encoderID):
                    encoderSpecificationValues[
                        kVTVideoEncoderSpecification_EncoderID as String
                    ] = encoderID
                case .enableLowLatencyRateControl:
                    if #available(macOS 11.3, *) {
                        encoderSpecificationValues[
                            kVTVideoEncoderSpecification_EnableLowLatencyRateControl as String
                        ] = true
                    }
                }
            }

            var s: VTCompressionSession?
            let codecType = policy.codec == .hevc
                ? kCMVideoCodecType_HEVC
                : kCMVideoCodecType_H264
            let status = VTCompressionSessionCreate(
                allocator: nil,
                width: w,
                height: h,
                codecType: codecType,
                encoderSpecification: encoderSpecificationValues as CFDictionary,
                imageBufferAttributes: [
                    kCVPixelBufferPixelFormatTypeKey as String: encoderSourcePixelFormat(),
                ] as CFDictionary,
                compressedDataAllocator: nil,
                outputCallback: nil,
                refcon: nil,
                compressionSessionOut: &s
            )
            guard status == noErr, let s = s else {
                lastFailure = "\(policy.mode.rawValue.uppercased()) \(policy.codec.rawValue.uppercased()) VTCompressionSessionCreate failed: \(status)"
                if policy.mode == .ave { aveFallbackReason = lastFailure }
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }

            var rawSupportedProperties: CFDictionary?
            let supportedPropertyStatus = VTSessionCopySupportedPropertyDictionary(
                s,
                supportedPropertyDictionaryOut: &rawSupportedProperties
            )
            let supportedPropertyKeys: Set<String>
            if supportedPropertyStatus == noErr, let rawSupportedProperties {
                supportedPropertyKeys = Set(
                    (rawSupportedProperties as NSDictionary).allKeys.compactMap { $0 as? String }
                )
            } else {
                supportedPropertyKeys = []
            }

            var supportsBaseFrameQP = false
            var hasEncoderPixelBufferPool = false
            if #available(macOS 12.0, *) {
                var rawSupportsBaseFrameQP: Unmanaged<CFTypeRef>?
                let supportsStatus = VTSessionCopyProperty(
                    s,
                    key: kVTCompressionPropertyKey_SupportsBaseFrameQP,
                    allocator: nil,
                    valueOut: &rawSupportsBaseFrameQP
                )
                supportsBaseFrameQP = supportsStatus == noErr
                    && (rawSupportsBaseFrameQP?.takeRetainedValue() as? NSNumber)?.boolValue == true
            }
            var supportedPresets: NSDictionary?
            if #available(macOS 26.0, *) {
                var rawPresets: Unmanaged<CFTypeRef>?
                if VTSessionCopyProperty(
                    s,
                    key: kVTCompressionPropertyKey_SupportedPresetDictionaries,
                    allocator: nil,
                    valueOut: &rawPresets
                ) == noErr, let rawPresets {
                    supportedPresets = rawPresets.takeRetainedValue() as? NSDictionary
                }
            }

            var report = EncoderConfigurationReport(mode: policy.mode)
            stateLock.lock()
            let aveRetryStagingEnabled = aveInputStagingEnabled
            stateLock.unlock()
            let inputSurfacePolicy = encoderInputSurfacePolicy(
                experiment: appliedEncoderExperiment(requestedExperiment),
                mode: policy.mode,
                width: UInt32(w),
                height: UInt32(h),
                captureBackend: backend.rawValue,
                aveRetryStagingEnabled: aveRetryStagingEnabled
            )
            switch inputSurfacePolicy {
            case .direct:
                break
            case .pixelTransfer:
                report.recordApplied("IOSurfacePixelTransferStaging")
            case .cpuCopy:
                report.recordApplied("IOSurfaceCPUCopyStaging")
            }
            let presetKind: EncoderPresetKind = policy.mode == .ave
                ? .highSpeed
                : .videoConferencing
            let presetApplied: Bool
            if #available(macOS 26.0, *) {
                presetApplied = applyEncoderPresetIfAvailable(
                    to: s,
                    preset: presetKind,
                    supportedPresets: supportedPresets,
                    report: &report
                )
            } else {
                report.recordUnsupported(
                    presetKind == .highSpeed ? "HighSpeed" : "VideoConferencing"
                )
                presetApplied = false
            }

            let realTimeStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_RealTime,
                value: true as CFBoolean
            )
            let noReorderStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_AllowFrameReordering,
                value: false as CFBoolean
            )
            let profileStatus: OSStatus
            let entropyStatus: OSStatus
            switch policy.codec {
            case .h264:
                let profile = requiredH264Profile(mode: policy.mode)
                profileStatus = VTSessionSetProperty(
                    s,
                    key: kVTCompressionPropertyKey_ProfileLevel,
                    value: profile == .main
                        ? kVTProfileLevel_H264_Main_AutoLevel
                        : kVTProfileLevel_H264_High_AutoLevel
                )
                let entropyMode = requiredH264EntropyMode(mode: policy.mode)
                entropyStatus = VTSessionSetProperty(
                    s,
                    key: kVTCompressionPropertyKey_H264EntropyMode,
                    value: entropyMode == .cabac
                        ? kVTH264EntropyMode_CABAC
                        : kVTH264EntropyMode_CAVLC
                )
            case .hevc:
                profileStatus = VTSessionSetProperty(
                    s,
                    key: kVTCompressionPropertyKey_ProfileLevel,
                    value: kVTProfileLevel_HEVC_Main_AutoLevel
                )
                entropyStatus = noErr
            }
            let expectedFrameRateStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_ExpectedFrameRate,
                value: Int32(fps) as CFNumber
            )
            guard realTimeStatus == noErr,
                  noReorderStatus == noErr,
                  profileStatus == noErr,
                  entropyStatus == noErr,
                  expectedFrameRateStatus == noErr else {
                VTCompressionSessionInvalidate(s)
                lastFailure = "\(policy.mode.rawValue.uppercased()) mandatory \(policy.codec.rawValue.uppercased()) setup failed: realtime=\(realTimeStatus) noReorder=\(noReorderStatus) profile=\(profileStatus) entropy=\(entropyStatus) expectedFps=\(expectedFrameRateStatus)"
                if policy.mode == .ave { aveFallbackReason = lastFailure }
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }

            applyOptionalEncoderProperties(
                to: s,
                policy: policy,
                supportedPropertyKeys: supportedPropertyKeys,
                latencyPolicy: latencyPolicy,
                width: w,
                height: h,
                report: &report
            )

            let averageBitrateStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_AverageBitRate,
                value: Int(avgBitrate) as CFNumber
            )
            if averageBitrateStatus != noErr {
                NSLog("Leftcar average bitrate rejected: status=%d", averageBitrateStatus)
            }
            // DataRateLimits is expressed as [bytes, seconds], while
            // AverageBitRate is expressed in bits per second. Keep a small
            // 1-second headroom without allowing multi-second bursts.
            let hardLimitBytes = max(1, Int(avgBitrate / 8.0 * 1.25))
            let dataRateStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_DataRateLimits,
                value: [hardLimitBytes, 1] as CFArray
            )
            if dataRateStatus != noErr {
                NSLog("Leftcar data rate limit rejected: status=%d", dataRateStatus)
            }
            // Recovery happens through the authenticated IDR request path
            // (viewer IDR datagram -> kVTEncodeFrameOptionKey_ForceKeyFrame).
            // A periodic UDP IDR would re-introduce the one-second resync ceiling
            // this recovery redesign removes. 3600 frames is effectively an
            // infinite GOP for an interactive 60fps session.
            let nominalKeyframeInterval = mediaTransport.usesTCP
                ? max(1, fps * 60)
                : 3600
            let keyframeIntervalStatus = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_MaxKeyFrameInterval,
                value: nominalKeyframeInterval as CFNumber
            )
            if keyframeIntervalStatus != noErr {
                NSLog("Leftcar keyframe interval rejected: status=%d", keyframeIntervalStatus)
            }

            switch encoderPrepareStrategy(mode: policy.mode) {
            case .eager:
                let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(s)
                guard prepareStatus == noErr else {
                    VTCompressionSessionInvalidate(s)
                    lastFailure = "\(policy.mode.rawValue.uppercased()) prepare failed: \(prepareStatus)"
                    if policy.mode == .ave { aveFallbackReason = lastFailure }
                    NSLog("Leftcar %@", lastFailure)
                    return .failed(lastFailure)
                }
                hasEncoderPixelBufferPool = VTCompressionSessionGetPixelBufferPool(s) != nil
            case .deferToFirstFrame:
                // Apple documents PrepareToEncodeFrames as optional. The
                // exact AVE session returns kVTSessionMalfunctionErr here on
                // the target Mac even though creation and property selection
                // succeed, so let the first real frame allocate resources.
                report.recordApplied("PrepareDeferredToFirstFrame")
                NSLog(
                    "Leftcar %@ prepare deferred to first frame %@",
                    policy.mode.rawValue.uppercased(),
                    targetLabel
                )
            }

            var selectedEncoderID: String?
            var encoderIDValue: Unmanaged<CFTypeRef>?
            let encoderIDStatus = VTSessionCopyProperty(
                s,
                key: kVTCompressionPropertyKey_EncoderID,
                allocator: nil,
                valueOut: &encoderIDValue
            )
            if encoderIDStatus == noErr, let encoderIDValue {
                selectedEncoderID = encoderIDValue.takeRetainedValue() as? String
            }
            var selectedHardware: Bool?
            var hardwareStatus: OSStatus = kVTPropertyNotSupportedErr
            if #available(macOS 10.9, *) {
                var hardwareValue: Unmanaged<CFTypeRef>?
                hardwareStatus = VTSessionCopyProperty(
                    s,
                    key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
                    allocator: nil,
                    valueOut: &hardwareValue
                )
                if hardwareStatus == noErr, let hardwareValue {
                    selectedHardware = (hardwareValue.takeRetainedValue() as? NSNumber)?.boolValue
                }
            }
            let verifiedEncoder = encoderSessionVerified(
                policy: policy,
                encoderIDStatus: encoderIDStatus,
                encoderID: selectedEncoderID,
                expectedEncoderID: requestedEncoderID ?? (
                    policy.mode == .rtvc ? phaseARTVCH264EncoderID : nil
                ),
                hardwareStatus: hardwareStatus,
                hardware: selectedHardware
            )
            guard let selectedEncoderID, verifiedEncoder else {
                VTCompressionSessionInvalidate(s)
                lastFailure = "\(policy.mode.rawValue.uppercased()) encoder verification failed: encoderIDStatus=\(encoderIDStatus) hardwareStatus=\(hardwareStatus) hardware=\(selectedHardware.map(String.init(describing:)) ?? "unknown")"
                if policy.mode == .ave { aveFallbackReason = lastFailure }
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }
            let experimentDecision: EncoderExperimentStartupDecision
            if policy.mode == .ave {
                experimentDecision = .success(
                    applied: appliedEncoderExperiment(requestedExperiment)
                )
            } else {
                experimentDecision = encoderExperimentStartupDecision(
                    requested: requestedExperiment,
                    rtvcHardwareAvailable: verifiedEncoder,
                    supportsBaseFrameQP: supportsBaseFrameQP,
                    hasEncoderPixelBufferPool: hasEncoderPixelBufferPool
                )
            }
            guard case .success(let applied) = experimentDecision else {
                let reason: String
                if case let .failure(message) = experimentDecision {
                    reason = message
                } else {
                    reason = "encoder experiment setup failed"
                }
                VTCompressionSessionInvalidate(s)
                lastFailure = reason
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }
            if policy.mode == .ave, selectedEncoderID != requestedEncoderID {
                VTCompressionSessionInvalidate(s)
                lastFailure = "AVE encoder identity mismatch: requested=\(requestedEncoderID ?? "unknown") actual=\(selectedEncoderID)"
                aveFallbackReason = lastFailure
                NSLog("Leftcar %@", lastFailure)
                return .failed(lastFailure)
            }

            let presetLabel: String
            if presetApplied {
                presetLabel = presetKind == .highSpeed ? "high-speed" : "video-conferencing"
            } else {
                presetLabel = "default"
            }
            let startupInFlightLimit = encoderStartupInFlightLimit(
                mode: policy.mode,
                outputConfirmed: false,
                configuredLimit: configuredEncodeInFlightLimit
            )
            stateLock.lock()
            currentAverageBitrate = Int(avgBitrate)
            currentQualityHint = initialEncoderQuality(
                mode: policy.mode,
                width: UInt32(w),
                height: UInt32(h)
            ) ?? 0.50
            encoderID = selectedEncoderID
            encoderHardwareAccelerated = selectedHardware
            encoderPreset = presetLabel
            encoderProfile = "main"
            encoderMode = policy.mode.rawValue
            appliedEncoderExperimentValue = applied
            encoderAppliedProperties = report.applied
            encoderSuppressedProperties = report.suppressed
            encoderUnsupportedProperties = report.unsupported
            encoderRejectedProperties = report.rejected
            encoderFallbackReason = requestedExperiment == .auto
                ? aveFallbackReason
                : nil
            if applied == .encoderPool {
                encoderInputPool = VTCompressionSessionGetPixelBufferPool(s)
            } else {
                encoderInputPool = nil
            }
            adaptiveQpController = AdaptiveQpController()
            adaptiveQpWindowStartNs = 0
            adaptiveQpWindowSubmittedFrames = 0
            adaptiveQpWindowEncoderDrops = 0
            adaptiveQpWindowValidOutputFrames = 0
            adaptiveQpWindowSubmitSamplesUs.removeAll(keepingCapacity: true)
            adaptiveQpWindowCallbackSamplesUs.removeAll(keepingCapacity: true)
            baseFrameQpChanges = 0
            encoderSessionGeneration &+= 1
            encoderSessionOutputCallbacks = 0
            recoveryDropRetryState.clear()
            stateLock.unlock()
            updateEncoderStartupInFlightLimit(
                mode: policy.mode,
                outputConfirmed: false
            )
            codecKind = policy.codec
            NSLog(
                "Leftcar hardware %@ %@ encoder ready %@: %dx%d bitrate=%d startupInFlight=%d configuredInFlight=%d encoderID=%@ hardware=%@ profile=main entropy=%@ preset=%@",
                policy.codec.rawValue.uppercased(),
                policy.mode.rawValue.uppercased(),
                targetLabel,
                w,
                h,
                Int(avgBitrate),
                startupInFlightLimit,
                latencyPolicy.maxEncodeInFlight,
                selectedEncoderID,
                selectedHardware.map(String.init(describing:)) ?? "unknown",
                policy.codec == .h264 ? "cabac" : "n/a",
                presetLabel
            )
            session = s
            return .installed
    }
}

