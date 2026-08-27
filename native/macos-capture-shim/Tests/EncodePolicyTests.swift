import Foundation
import CoreVideo
import VideoToolbox

@main
struct EncodePolicyTests {
    static func main() {
        precondition(encodeInFlightLimit(width: 3_840, height: 2_160) == 3)
        precondition(encodeInFlightLimit(width: 2_560, height: 1_440) == 3)
        precondition(encodeInFlightLimit(width: 1_920, height: 1_080) == 2)
        let ultraHdPolicy = encoderLatencyPolicy(width: 3_840, height: 2_160)
        precondition(ultraHdPolicy.maxEncodeInFlight == 3)
        precondition(ultraHdPolicy.usesLowLatencyRateControl)
        precondition(!ultraHdPolicy.allowFrameReordering)
        precondition(ultraHdPolicy.maxFrameDelayCount == 0)
        precondition(ultraHdPolicy.suggestedLookAheadFrameCount == 0)
        precondition(ultraHdPolicy.qualityHint == 0.5)
        precondition(ultraHdPolicy.maximumRealTimeFrameRate == 60)
        precondition(encoderLatencyPolicy(width: 2_560, height: 1_440).qualityHint == nil)
        precondition(
            encoderQualityPolicy(codec: .h264, width: 3_840, height: 2_160)
                == EncoderQualityPolicy(initialHint: 0.5, supportsQualityProperty: true)
        )
        precondition(
            encoderQualityPolicy(codec: .hevc, width: 3_840, height: 2_160)
                == EncoderQualityPolicy(initialHint: 0.5, supportsQualityProperty: true)
        )
        precondition(
            encoderQualityPolicy(codec: .hevc, width: 2_560, height: 1_440)
                == EncoderQualityPolicy(initialHint: nil, supportsQualityProperty: false)
        )
        precondition(
            adaptiveEncoderQualityHint(
                current: 0.5,
                encodeOutputP95Us: 22_000,
                captureQueueWaitP95Us: 9_000,
                receiverLoss: 0,
                renderedFps: 60,
                targetFps: 60
            ) == 0.25
        )
        precondition(
            adaptiveEncoderQualityHint(
                current: 0.25,
                encodeOutputP95Us: 11_000,
                captureQueueWaitP95Us: 1_000,
                receiverLoss: 0,
                renderedFps: 60,
                targetFps: 60
            ) == 0.3
        )
        precondition(
            adaptiveEncoderQualityHint(
                current: 0.5,
                encodeOutputP95Us: 14_000,
                captureQueueWaitP95Us: 3_000,
                receiverLoss: 0,
                renderedFps: 60,
                targetFps: 60
            ) == 0.5
        )
        precondition(adaptiveQualityBitrateScale(qualityHint: 0.25) == 0.55)
        precondition(adaptiveQualityBitrateScale(qualityHint: 0.5) == 1.0)
        precondition(adaptiveQualityBitrateScale(qualityHint: 0.375) == 0.775)
        precondition(manualQualityHintFromSliderPercent(0) == nil)
        precondition(manualQualityHintFromSliderPercent(25) == 0.25)
        precondition(manualQualityHintFromSliderPercent(50) == 0.5)
        precondition(manualQualityHintFromSliderPercent(24) == nil)
        precondition(manualQualityHintFromSliderPercent(51) == nil)
        precondition(
            encoderLatencyPolicy(width: 3_840, height: 2_160, fps: 90).maximumRealTimeFrameRate == 90
        )
        precondition(
            encoderSourcePixelFormat() == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        )
        precondition(captureMinimumFrameTimeSeconds(fps: 60) == 1.0 / 60.0)
        precondition(captureMinimumFrameTimeSeconds(fps: 0) == 1.0)
        precondition(captureQueueDepth() == 3)
        precondition(shouldOffloadEncodedSample(width: 3_840, height: 2_160))
        precondition(shouldOffloadEncodedSample(width: 2_560, height: 1_440))
        precondition(!shouldOffloadEncodedSample(width: 2_560, height: 1_439))
        precondition(!shouldOffloadEncodedSample(width: 1_920, height: 1_080))
        precondition(
            preferredVideoCodec(width: 3_840, height: 2_160, contentMode: "video") == .h264
        )
        precondition(
            preferredVideoCodec(width: 1_920, height: 1_080, contentMode: "video") == .h264
        )
        precondition(
            preferredVideoCodec(width: 3_840, height: 2_160, contentMode: "interactive") == .h264
        )
        precondition(
            encoderCandidateOrder(width: 3_840, height: 2_160, contentMode: "video") == [.h264]
        )
        precondition(
            encoderCandidateOrder(width: 2_560, height: 1_440, contentMode: "video") == [.h264]
        )
        let video4K = encoderSessionPolicies(width: 3_840, height: 2_160, contentMode: "video")
        precondition(video4K == [
            EncoderSessionPolicy(mode: .ave, codec: .h264),
            EncoderSessionPolicy(mode: .rtvc, codec: .h264),
        ])
        precondition(
            encoderSessionPolicies(width: 3_840, height: 2_160, contentMode: "interactive")
                == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
        )
        precondition(
            encoderSessionPolicies(width: 2_560, height: 1_440, contentMode: "video")
                == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
        )

        let preferredAVE = preferredHardwareEncoderID(
            codec: .h264,
            candidates: [
                EncoderCandidateDescriptor(id: "software", codec: .h264, hardware: false, performanceRating: 900),
                EncoderCandidateDescriptor(id: "ave-slow", codec: .h264, hardware: true, performanceRating: 200),
                EncoderCandidateDescriptor(id: "ave-fast", codec: .h264, hardware: true, performanceRating: 400),
                EncoderCandidateDescriptor(id: "hevc-fast", codec: .hevc, hardware: true, performanceRating: 500),
            ]
        )
        precondition(preferredAVE == "ave-fast")

        precondition(
            encoderSpecificationPlan(mode: .ave, encoderID: "ave-fast") == [
                .requireHardware,
                .encoderID("ave-fast"),
            ]
        )
        precondition(encoderSpecificationPlan(mode: .ave, encoderID: nil) == nil)
        precondition(
            encoderSpecificationPlan(mode: .rtvc, encoderID: nil) == [
                .requireHardware,
                .enableLowLatencyRateControl,
            ]
        )
        precondition(
            hardwareEncoderVerified(
                queryStatus: noErr,
                queriedHardware: true,
                requireHardware: false
            )
        )
        precondition(
            !hardwareEncoderVerified(
                queryStatus: noErr,
                queriedHardware: false,
                requireHardware: true
            )
        )
        precondition(
            !hardwareEncoderVerified(
                queryStatus: noErr,
                queriedHardware: nil,
                requireHardware: true
            )
        )
        precondition(
            hardwareEncoderVerified(
                queryStatus: kVTPropertyNotSupportedErr,
                queriedHardware: nil,
                requireHardware: true
            )
        )
        precondition(
            !hardwareEncoderVerified(
                queryStatus: kVTPropertyNotSupportedErr,
                queriedHardware: nil,
                requireHardware: false
            )
        )
        precondition(
            !hardwareEncoderVerified(
                queryStatus: -12_901,
                queriedHardware: true,
                requireHardware: true
            )
        )
        precondition(
            leftcarPerfLogLine(
                captureCallbacks: 120,
                encodeOutputCallbacks: 118,
                captureFps: 60,
                encodeOutputFps: 59,
                encodeOutputIntervalP50Us: 16_600,
                encodeOutputIntervalP95Us: 18_000,
                encodeOutputP95Us: 24_000,
                queueOldestUs: 2_100,
                encoderMode: "rtvc",
                encoderID: "com.apple.videotoolbox.videoencoder.h264.rtvc"
            ) == "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=118 captureFps=60 encodeOutputFps=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100 encoderMode=rtvc encoderID=com.apple.videotoolbox.videoencoder.h264.rtvc"
        )

        let tickerSignal = DispatchSemaphore(value: 0)
        let tickerCountLock = NSLock()
        var tickerCount = 0
        let ticker = PerformanceLogTicker(
            interval: .milliseconds(10),
            queue: DispatchQueue(label: "leftcar.performance.test", qos: .utility)
        ) {
            tickerCountLock.lock()
            tickerCount += 1
            tickerCountLock.unlock()
            tickerSignal.signal()
        }
        precondition(ticker.start())
        precondition(!ticker.start())
        for _ in 0..<3 {
            precondition(tickerSignal.wait(timeout: .now() + .seconds(1)) == .success)
        }
        ticker.stop()
        tickerCountLock.lock()
        let tickerCountAfterStop = tickerCount
        tickerCountLock.unlock()
        _ = DispatchSemaphore(value: 0).wait(timeout: .now() + .milliseconds(60))
        tickerCountLock.lock()
        let tickerCountAfterQuietPeriod = tickerCount
        tickerCountLock.unlock()
        precondition(tickerCountAfterStop >= 3)
        precondition(tickerCountAfterQuietPeriod == tickerCountAfterStop)

        let supportedAVE: Set<EncoderOptionalProperty> = [
            .prioritizeSpeed,
            .maximumRealTimeFrameRate,
            .quality,
        ]
        precondition(
            optionalPropertyPlan(mode: .ave, supported: supportedAVE) == [
                .prioritizeSpeed,
                .maximumRealTimeFrameRate,
                .quality,
            ]
        )
        precondition(optionalPropertyPlan(mode: .rtvc, supported: supportedAVE).isEmpty)
        precondition(!EncoderOptionalProperty.allCases.map(\.rawValue).contains("MaxFrameDelayCount"))
        var report = EncoderConfigurationReport(mode: .ave)
        report.recordApplied("HighSpeed")
        report.recordUnsupported("SuggestedLookAheadFrameCount")
        report.recordRejected("Quality", status: -12_900)
        precondition(report.applied == ["HighSpeed"])
        precondition(report.unsupported == ["SuggestedLookAheadFrameCount"])
        precondition(report.rejected == ["Quality=-12900"])
        precondition(requiredH264Profile(mode: .ave) == .main)
        precondition(requiredH264EntropyMode(mode: .ave) == .cabac)
        precondition(initialEncoderQuality(mode: .ave, width: 3_840, height: 2_160) == 0.25)
        precondition(initialEncoderQuality(mode: .rtvc, width: 3_840, height: 2_160) == nil)
        precondition(
            preferredH264Profile(lowLatency: true) == .high
        )
        precondition(
            preferredH264Profile(
                lowLatency: true,
                width: 3_840,
                height: 2_160,
                contentMode: "video"
            ) == .main
        )
        precondition(
            preferredH264EntropyMode(
                width: 3_840,
                height: 2_160,
                contentMode: "video"
            ) == .cabac
        )
        precondition(
            preferredH264EntropyMode(
                width: 1_920,
                height: 1_080,
                contentMode: "interactive"
            ) == .cabac
        )
        precondition(
            preferredEncoderPreset(contentMode: "video") == .highSpeed
        )
        precondition(
            preferredEncoderPreset(contentMode: "interactive") == .videoConferencing
        )
        precondition(codecParameterSetCount(.h264) == 2)
        precondition(codecParameterSetCount(.hevc) == 3)
        precondition(fecParityCount(dataCount: 8, reduced: true) == 1)
        precondition(fecParityCount(dataCount: 6, reduced: true) == 1)
        precondition(fecParityCount(dataCount: 8, recovery: true) == 2)
        precondition(fecParityCount(dataCount: 2, recovery: true) == 1)
        precondition(fecParityCount(dataCount: 3, recovery: true) == 1)
        precondition(
            udpPacingBitrate(
                targetBitrate: 8_000_000,
                dataFragmentCount: 8,
                contentMode: "video",
                isKeyframe: false
            ) == 9_000_000
        )
        precondition(
            udpPacingBitrate(
                targetBitrate: 8_000_000,
                dataFragmentCount: 8,
                contentMode: "interactive",
                isKeyframe: false
            ) == 10_000_000
        )
        precondition(
            udpPacingBitrate(
                targetBitrate: 8_000_000,
                dataFragmentCount: 6,
                contentMode: "video",
                isKeyframe: false
            ) == 9_333_334
        )
        precondition(
            shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: true,
                keyframeInFlight: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: true,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: false,
                keyframeQueued: true
            )
        )
        precondition(
            shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: false,
                keyframeQueued: true
            )
        )
        precondition(
            !shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: false,
                keyframeQueued: false,
                keyframeInFlight: true
            )
        )
        precondition(
            !shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: true,
                keyframeQueued: false
            )
        )
        precondition(shouldPrioritizeNetworkKeyframe(isKeyframe: true))
        precondition(!shouldPrioritizeNetworkKeyframe(isKeyframe: false))
        // A recovery keyframe is a wire-order boundary. Deltas that finish
        // encoding while that keyframe is still queued must remain blocked
        // until the boundary has actually been sent.
        precondition(
            networkAwaitingKeyframeAfterEnqueue(
                currentAwaitingKeyframe: false,
                isKeyframe: true,
                isRecoveryKeyframe: false
            ) == false
        )
        precondition(
            networkAwaitingKeyframeAfterEnqueue(
                currentAwaitingKeyframe: false,
                isKeyframe: true,
                isRecoveryKeyframe: true
            )
        )
        precondition(
            networkAwaitingKeyframeAfterEnqueue(
                currentAwaitingKeyframe: true,
                isKeyframe: false,
                isRecoveryKeyframe: false
            )
        )
        precondition(
            !networkAwaitingKeyframeAfterSend(
                currentAwaitingKeyframe: true,
                isKeyframe: true,
                isRecoveryKeyframe: false,
                sendSucceeded: true
            )
        )
        precondition(
            networkAwaitingKeyframeAfterSend(
                currentAwaitingKeyframe: true,
                isKeyframe: true,
                isRecoveryKeyframe: true,
                sendSucceeded: false
            )
        )
        precondition(shouldSendCodecConfig(csdSent: false))
        precondition(!shouldSendCodecConfig(csdSent: true))
        precondition(
            recoveryEncodeGateExpired(
                startedNs: 1_000,
                nowNs: 751_001_000,
                timeoutNs: 750_000_000
            )
        )
        precondition(
            !recoveryEncodeGateExpired(
                startedNs: 1_000,
                nowNs: 750_000_999,
                timeoutNs: 750_000_000
            )
        )
        precondition(frameBudgetUs(fps: 60) == 16_667)
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 7_700_000,
                bytes: 100 * 1024,
                fps: 60
            ) >= 40_000_000
        )
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 80_000_000,
                bytes: 100 * 1024,
                fps: 60
            ) == 80_000_000
        )
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 24_000_000,
                bytes: 202 * 1024,
                fps: 60
            ) >= 120_000_000
        )
        precondition(
            udpDatagramIntervalUs(
                bytes: 1_200,
                isKeyframe: true,
                accessUnitBytes: 140 * 1024,
                targetBitrate: 7_700_000,
                fps: 60
            ) <= 245
        )
        precondition(
            udpDatagramIntervalUs(
                bytes: 1_200,
                isKeyframe: false,
                accessUnitBytes: 26 * 1024,
                targetBitrate: 4_500_000,
                fps: 60
            ) >= 300 && udpDatagramIntervalUs(
                bytes: 1_200,
                isKeyframe: false,
                accessUnitBytes: 26 * 1024,
                targetBitrate: 4_500_000,
                fps: 60
            ) <= 700
        )
        precondition(
            udpDatagramIntervalUs(
                bytes: 1_200,
                isKeyframe: false,
                accessUnitBytes: 75 * 1024,
                targetBitrate: 24_000_000,
                fps: 60
            ) >= 180 && udpDatagramIntervalUs(
                bytes: 1_200,
                isKeyframe: false,
                accessUnitBytes: 75 * 1024,
                targetBitrate: 24_000_000,
                fps: 60
            ) <= 260
        )
        // A 23KiB AU at 4.4Mbps cannot fit inside one 60fps frame period at
        // the ordinary bitrate. It must use the bounded high-motion pacing
        // path; the same AU at 24Mbps already fits without that boost.
        precondition(
            shouldUseHighMotionPacing(
                accessUnitBytes: 23 * 1024,
                targetBitrate: 4_400_000,
                fps: 60
            )
        )
        precondition(
            !shouldUseHighMotionPacing(
                accessUnitBytes: 23 * 1024,
                targetBitrate: 24_000_000,
                fps: 60
            )
        )
        let queuedFrames = [
            PendingEncodedFrame(
                data: Data(repeating: 0, count: 120),
                isKeyframe: false,
                isRecoveryKeyframe: false,
                queuedNs: 1_000
            ),
            PendingEncodedFrame(
                data: Data(repeating: 0, count: 80),
                isKeyframe: true,
                isRecoveryKeyframe: true,
                queuedNs: 2_000
            ),
        ]
        let snapshot = networkQueueSnapshot(frames: queuedFrames, nowNs: 2_500)
        precondition(snapshot.count == 2)
        precondition(snapshot.bytes == 200)
        precondition(snapshot.oldestAgeUs == 1)
    }
}
