import Foundation
import CoreGraphics
import CoreVideo
import VideoToolbox

@main
struct EncodePolicyTests {
    static func main() {
        let managedModeTestDisplayID = CGMainDisplayID()
        precondition(registerManagedDisplayMode(
            displayID: managedModeTestDisplayID,
            generation: 1,
            logicalWidth: 1600,
            logicalHeight: 1000,
            pixelWidth: 3200,
            pixelHeight: 2000
        ))
        precondition(managedPixelSize(for: managedModeTestDisplayID, logicalWidth: 1600, logicalHeight: 1000)
            == NativePixelSize(width: 3200, height: 2000))
        precondition(managedPixelSize(for: managedModeTestDisplayID, logicalWidth: 1000, logicalHeight: 1600)
            == NativePixelSize(width: 2000, height: 3200))
        clearManagedDisplayMode(displayID: managedModeTestDisplayID, generation: 1)
        precondition(managedPixelSize(for: managedModeTestDisplayID, logicalWidth: 1600, logicalHeight: 1000) == nil)
        precondition(registerManagedDisplayMode(
            displayID: managedModeTestDisplayID,
            generation: 2,
            logicalWidth: 1920,
            logicalHeight: 1200,
            pixelWidth: 1920,
            pixelHeight: 1200
        ))
        precondition(registerManagedDisplayMode(
            displayID: managedModeTestDisplayID,
            generation: 3,
            logicalWidth: 1600,
            logicalHeight: 1000,
            pixelWidth: 3200,
            pixelHeight: 2000
        ))
        clearManagedDisplayMode(displayID: managedModeTestDisplayID, generation: 2)
        precondition(managedPixelSize(for: managedModeTestDisplayID, logicalWidth: 1600, logicalHeight: 1000)
            == NativePixelSize(width: 3200, height: 2000))
        clearManagedDisplayMode(displayID: managedModeTestDisplayID, generation: 3)

        let overlappingDirtyRegions = dirtyRegionMotionSample(
            rects: [
                CGRect(x: 0, y: 0, width: 50, height: 50),
                CGRect(x: 25, y: 0, width: 50, height: 50),
            ],
            frameWidth: 100,
            frameHeight: 100
        )
        precondition(overlappingDirtyRegions.rectCount == 2)
        precondition(abs(overlappingDirtyRegions.changedPixelRatio - 0.375) < 0.000_001)
        let clippedDirtyRegions = dirtyRegionMotionSample(
            rects: [
                CGRect(x: -25, y: -25, width: 50, height: 50),
                CGRect(x: 95, y: 95, width: 20, height: 20),
                CGRect(x: 0, y: 0, width: CGFloat.infinity, height: 20),
            ],
            frameWidth: 100,
            frameHeight: 100
        )
        precondition(clippedDirtyRegions.rectCount == 2)
        precondition(abs(clippedDirtyRegions.changedPixelRatio - 0.065) < 0.000_001)
        precondition(
            dirtyRegionMotionSample(
                rects: [CGRect(x: 0, y: 0, width: 10, height: 10)],
                frameWidth: 0,
                frameHeight: 100
            ) == DirtyRegionMotionSample(rectCount: 0, changedPixelRatio: 0)
        )

        var motion = AdaptiveMotionState()
        precondition(
            motion.observe(
                nowNs: 100,
                changedPixelRatio: 0.05,
                accessUnitPressure: false
            ).mode == .interactive
        )
        let broadChange = motion.observe(
            nowNs: 1_000,
            changedPixelRatio: 0.30,
            accessUnitPressure: false
        )
        precondition(broadChange.mode == .video)
        precondition(broadChange.enteredVideo)
        precondition(
            motion.observe(
                nowNs: 700_000_000,
                changedPixelRatio: 0.01,
                accessUnitPressure: false
            ).mode == .video
        )
        let localizedAgain = motion.observe(
            nowNs: 801_001_000,
            changedPixelRatio: 0.01,
            accessUnitPressure: false
        )
        precondition(localizedAgain.mode == .interactive)
        precondition(localizedAgain.enteredInteractive)
        let accessUnitPressure = motion.observe(
            nowNs: 900_000_000,
            changedPixelRatio: nil,
            accessUnitPressure: true
        )
        precondition(accessUnitPressure.mode == .video)
        precondition(
            motion.mode(at: 2_899_999_999) == .video
        )
        precondition(
            motion.mode(at: 2_900_000_000) == .interactive
        )
        var motionAfterIdle = AdaptiveMotionState()
        _ = motionAfterIdle.observe(
            nowNs: 10_000,
            changedPixelRatio: 0.40,
            accessUnitPressure: false
        )
        let motionReentry = motionAfterIdle.observe(
            nowNs: 900_010_000,
            changedPixelRatio: 0.40,
            accessUnitPressure: false
        )
        precondition(motionReentry.mode == .video)
        precondition(motionReentry.enteredVideo)
        precondition(
            motionAdjustedUdpBurstDatagrams(base: 4, mode: .interactive) == 4
        )
        precondition(
            motionAdjustedUdpBurstDatagrams(base: 4, mode: .video) == 8
        )
        precondition(
            motionAdjustedUdpBurstDatagrams(base: 16, mode: .video) == 16
        )
        precondition(
            highMotionBitrateTarget(
                current: 42_000_000,
                floor: 14_000_000,
                ceiling: 60_000_000,
                motionFloor: 36_000_000
            ) == 42_000_000
        )
        precondition(
            highMotionBitrateTarget(
                current: 24_000_000,
                floor: 14_000_000,
                ceiling: 60_000_000,
                motionFloor: 36_000_000
            ) == 36_000_000
        )
        precondition(
            highMotionBitrateTarget(
                current: 72_000_000,
                floor: 14_000_000,
                ceiling: 60_000_000,
                motionFloor: 48_000_000
            ) == 60_000_000
        )
        let regionMotionSession = CaptureSession(
            targetAddr: sockaddr_in(),
            targetPort: 5_002,
            targetLabel: "motion-region-test",
            width: 3_840,
            height: 2_160,
            fps: 60,
            backend: .screenCaptureKit,
            mediaTransport: .udp,
            contentMode: .interactive,
            requestedEncoderExperiment: .splitVertical,
            udpStability: .init(
                profile: .auto,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: true
            )
        )
        precondition(regionMotionSession.currentUdpBurstLimit() == 4)
        regionMotionSession.observeCaptureMotion(
            DirtyRegionMotionSample(rectCount: 3, changedPixelRatio: 0.35),
            nowNs: DispatchTime.now().uptimeNanoseconds
        )
        precondition(regionMotionSession.currentUdpBurstLimit() == 8)

        let accessUnitMotionSession = CaptureSession(
            targetAddr: sockaddr_in(),
            targetPort: 5_003,
            targetLabel: "motion-au-test",
            width: 3_840,
            height: 2_160,
            fps: 60,
            backend: .cgDisplayStream,
            mediaTransport: .udp,
            contentMode: .interactive,
            requestedEncoderExperiment: .auto,
            udpStability: .init(
                profile: .auto,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: true
            )
        )
        accessUnitMotionSession.stateLock.lock()
        accessUnitMotionSession.currentAverageBitrate = 24_000_000
        accessUnitMotionSession.stateLock.unlock()
        accessUnitMotionSession.recordAccessUnitShape(
            bytes: 96 * 1_024,
            isKeyframe: false,
            sendUs: 24_000
        )
        precondition(accessUnitMotionSession.currentUdpBurstLimit() == 8)

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
        let portraitUltraHdPolicy = encoderLatencyPolicy(width: 2_160, height: 3_840)
        precondition(portraitUltraHdPolicy.maxEncodeInFlight == ultraHdPolicy.maxEncodeInFlight)
        precondition(portraitUltraHdPolicy.qualityHint == ultraHdPolicy.qualityHint)
        precondition(isUltraHdDimensions(width: 2_160, height: 3_840))
        precondition(isUltraHdDimensions(width: 3_840, height: 2_160))
        precondition(!isUltraHdDimensions(width: 2_560, height: 1_440))
        precondition(
            videoBitrateBounds(width: 2_160, height: 3_840, activeCount: 1)
                == VideoBitrateBounds(minimum: 24_000_000, maximum: 80_000_000)
        )
        precondition(
            videoBitrateBounds(width: 3_840, height: 2_160, activeCount: 1)
                == videoBitrateBounds(width: 2_160, height: 3_840, activeCount: 1)
        )
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
        precondition(
            encoderBitrateApplicationRoute(
                hasSingleSession: true,
                hasSplitPipeline: false
            ) == .single
        )
        precondition(
            encoderBitrateApplicationRoute(
                hasSingleSession: false,
                hasSplitPipeline: true
            ) == .split
        )
        precondition(
            encoderBitrateApplicationRoute(
                hasSingleSession: false,
                hasSplitPipeline: false
            ) == .unavailable
        )
        precondition(!shouldRunAdaptiveBitrate(encodedFrames: 59, fps: 60))
        precondition(shouldRunAdaptiveBitrate(encodedFrames: 60, fps: 60))
        precondition(shouldRunAdaptiveBitrate(encodedFrames: 120, fps: 60))
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
        precondition(captureMinimumFrameTimeSeconds(fps: 60) == 1.0 / 62.0)
        precondition(captureMinimumFrameTimeSeconds(fps: 30) == 1.0 / 30.0)
        precondition(captureMinimumFrameTimeSeconds(fps: 0) == 1.0)
        precondition(captureQueueDepth(experiment: .auto) == 3)
        precondition(captureQueueDepth(experiment: .splitVertical) == 8)
        precondition(shouldOffloadEncodedSample(width: 3_840, height: 2_160))
        precondition(shouldOffloadEncodedSample(width: 2_560, height: 1_440))
        precondition(!shouldOffloadEncodedSample(width: 2_560, height: 1_439))
        precondition(!shouldOffloadEncodedSample(width: 1_920, height: 1_080))
        let currentHiDPIMode = NativePixelModeCandidate(
            logicalWidth: 1_920,
            logicalHeight: 1_080,
            pixelWidth: 3_840,
            pixelHeight: 2_160
        )
        let appKitHiDPIMode = nativePixelModeCandidate(
            logicalWidth: 1_920,
            logicalHeight: 1_080,
            backingWidth: 3_840,
            backingHeight: 2_160
        )
        precondition(appKitHiDPIMode?.pixelWidth == 3_840)
        precondition(appKitHiDPIMode?.pixelHeight == 2_160)
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [appKitHiDPIMode!]
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: .infinity,
                backingHeight: 2_160
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: 3_840,
                backingHeight: .nan
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: 0,
                backingHeight: 2_160
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: 3_840,
                backingHeight: -2_160
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: Double(Int.max),
                backingHeight: 2_160
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: 2_160,
                backingHeight: 3_840
            ) == nil
        )
        precondition(
            nativePixelModeCandidate(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                backingWidth: 4_096,
                backingHeight: 2_160
            ) == nil
        )
        let appKitNativeMode = nativePixelModeCandidate(
            logicalWidth: 3_840,
            logicalHeight: 2_160,
            backingWidth: 3_840,
            backingHeight: 2_160
        )
        precondition(appKitNativeMode?.pixelWidth == 3_840)
        precondition(appKitNativeMode?.pixelHeight == 2_160)
        precondition(
            nativePixelSize(
                logicalWidth: 3_840,
                logicalHeight: 2_160,
                currentMode: nil,
                candidates: [appKitNativeMode!]
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: currentHiDPIMode,
                candidates: []
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 1_920,
                        logicalHeight: 1_080,
                        pixelWidth: 2_160,
                        pixelHeight: 3_840
                    ),
                ]
            ) == NativePixelSize(width: 1_920, height: 1_080)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 1_920,
                        logicalHeight: 1_080,
                        pixelWidth: 4_096,
                        pixelHeight: 2_160
                    ),
                ]
            ) == NativePixelSize(width: 1_920, height: 1_080)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 1_920,
                        logicalHeight: 1_080,
                        pixelWidth: 2_560,
                        pixelHeight: 1_440
                    ),
                ]
            ) == NativePixelSize(width: 2_560, height: 1_440)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 1_920,
                        logicalHeight: 1_080,
                        pixelWidth: 1_920,
                        pixelHeight: 1_080
                    ),
                    currentHiDPIMode,
                ]
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: NativePixelModeCandidate(
                    logicalWidth: 1_920,
                    logicalHeight: 1_080,
                    pixelWidth: 2_560,
                    pixelHeight: 1_440
                ),
                candidates: [currentHiDPIMode]
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: nil,
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 3_840,
                        logicalHeight: 2_160,
                        pixelWidth: 3_840,
                        pixelHeight: 2_160
                    ),
                ]
            ) == NativePixelSize(width: 1_920, height: 1_080)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 1_920,
                logicalHeight: 1_080,
                currentMode: NativePixelModeCandidate(
                    logicalWidth: 1_920,
                    logicalHeight: 1_080,
                    pixelWidth: 0,
                    pixelHeight: 0
                ),
                candidates: [
                    NativePixelModeCandidate(
                        logicalWidth: 1_920,
                        logicalHeight: 1_080,
                        pixelWidth: 1_920,
                        pixelHeight: 1_080
                    ),
                ]
            ) == NativePixelSize(width: 1_920, height: 1_080)
        )
        precondition(
            nativePixelSize(
                logicalWidth: 3_840,
                logicalHeight: 2_160,
                currentMode: NativePixelModeCandidate(
                    logicalWidth: 3_840,
                    logicalHeight: 2_160,
                    pixelWidth: 3_840,
                    pixelHeight: 2_160
                ),
                candidates: []
            ) == NativePixelSize(width: 3_840, height: 2_160)
        )
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
            EncoderSessionPolicy(mode: .rtvc, codec: .h264),
            EncoderSessionPolicy(mode: .ave, codec: .h264),
        ])
        precondition(
            encoderSessionPolicies(width: 3_840, height: 2_160, contentMode: "interactive")
                == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
        )
        precondition(
            encoderSessionPolicies(width: 2_560, height: 1_440, contentMode: "video")
                == [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
        )
        precondition(EncoderExperiment.parse(nil) == .auto)
        precondition(EncoderExperiment.parse("rateControl") == .rateControl)
        precondition(EncoderExperiment.parse("adaptiveQp") == .adaptiveQp)
        precondition(EncoderExperiment.parse("encoderPool") == .encoderPool)
        precondition(EncoderExperiment.parse("splitHorizontal") == nil)
        precondition(appliedEncoderExperiment(.auto) == .rateControl)
        precondition(
            adaptiveQpWindowIsPressured(
                encoderDrops: 1,
                validOutputFps: 60,
                submitP95Us: 10_000
            )
        )
        precondition(
            adaptiveQpWindowIsPressured(
                encoderDrops: 0,
                validOutputFps: 54,
                submitP95Us: 10_000
            )
        )
        precondition(
            adaptiveQpWindowIsPressured(
                encoderDrops: 0,
                validOutputFps: 60,
                submitP95Us: 16_668
            )
        )
        precondition(
            !adaptiveQpWindowIsPressured(
                encoderDrops: 0,
                validOutputFps: 60,
                submitP95Us: 16_000
            )
        )
        precondition(
            adaptiveQpStableWindowIsReady(
                encoderDrops: 0,
                validOutputFps: 59,
                callbackP95Us: 18_500,
                networkOldestAgeUs: 16_667
            )
        )
        precondition(
            !adaptiveQpStableWindowIsReady(
                encoderDrops: 0,
                validOutputFps: 59,
                callbackP95Us: 18_501,
                networkOldestAgeUs: 16_667
            )
        )

        var adaptiveController = AdaptiveQpController()
        precondition(adaptiveController.currentBaseFrameQp == 32)
        precondition(
            encoderSessionPolicies(for: .adaptiveQp) == [
                EncoderSessionPolicy(mode: .rtvc, codec: .h264),
            ]
        )
        precondition(
            encoderSessionPolicies(for: .auto) == [
                EncoderSessionPolicy(mode: .rtvc, codec: .h264),
            ]
        )
        precondition(
            encoderSessionPolicies(
                for: .auto,
                width: 3_840,
                height: 2_160,
                contentMode: "video"
            ) == video4K
        )
        precondition(encoderSessionPolicies(for: .splitVertical).isEmpty)
        precondition(
            adaptiveController.observeWindow(
                encoderDrops: 1,
                validOutputFps: 60,
                submitP95Us: 10_000,
                callbackP95Us: 10_000,
                networkOldestAgeUs: 1_000
            ) == 34
        )
        precondition(
            adaptiveController.observeWindow(
                encoderDrops: 0,
                validOutputFps: 60,
                submitP95Us: 10_000,
                callbackP95Us: 10_000,
                networkOldestAgeUs: 1_000
            ) == 34
        )
        precondition(
            adaptiveController.observeWindow(
                encoderDrops: 0,
                validOutputFps: 59,
                submitP95Us: 10_000,
                callbackP95Us: 18_500,
                networkOldestAgeUs: 16_667
            ) == 34
        )
        precondition(
            adaptiveController.observeWindow(
                encoderDrops: 0,
                validOutputFps: 59,
                submitP95Us: 10_000,
                callbackP95Us: 18_500,
                networkOldestAgeUs: 16_667
            ) == 33
        )
        precondition(adaptiveController.applyManualSlider(percent: 25) == 42)
        precondition(adaptiveController.applyManualSlider(percent: 50) == 26)
        precondition(adaptiveController.applyManualSlider(percent: 0) == nil)
        precondition(adaptiveController.currentBaseFrameQp == 33)

        let deltaProperties = encoderFramePropertyPlan(
            appliedExperiment: .adaptiveQp,
            requestKeyframe: false,
            currentBaseFrameQp: 32
        )
        precondition(!deltaProperties.forceKeyframe)
        precondition(deltaProperties.baseFrameQp == 32)
        let idrProperties = encoderFramePropertyPlan(
            appliedExperiment: .adaptiveQp,
            requestKeyframe: true,
            currentBaseFrameQp: 34
        )
        precondition(idrProperties.forceKeyframe)
        precondition(idrProperties.baseFrameQp == 34)
        precondition(
            encoderFramePropertyPlan(
                appliedExperiment: .rateControl,
                requestKeyframe: true,
                currentBaseFrameQp: 34
            ).baseFrameQp == nil
        )

        precondition(parseEncoderExperimentName(nil) == .success(.auto))
        precondition(parseEncoderExperimentName("rateControl") == .success(.rateControl))
        precondition(
            parseEncoderExperimentName("splitHorizontal")
                == .failure("unknown encoder experiment: splitHorizontal")
        )
        precondition(
            parseEncoderExperimentName("bogus")
                == .failure("unknown encoder experiment: bogus")
        )
        precondition(parseEncoderExperimentCString(nil) == .success(.auto))
        precondition(
            "adaptiveQp".withCString {
                parseEncoderExperimentCString($0)
            } == .success(.adaptiveQp)
        )
        let invalidExperimentUTF8: [CChar] = [CChar(bitPattern: 0xFF), 0]
        precondition(
            invalidExperimentUTF8.withUnsafeBufferPointer {
                parseEncoderExperimentCString($0.baseAddress)
            } == .failure("unknown encoder experiment: invalid UTF-8")
        )
        let invalidV6Start = "127.0.0.1".withCString { ip in
            "cg".withCString { backend in
                "udp".withCString { transport in
                    "video".withCString { contentMode in
                        invalidExperimentUTF8.withUnsafeBufferPointer { experiment in
                            leftcarCaptureStartV6(
                                ip: ip,
                                port: 9,
                                displayIndex: 0,
                                width: 3_840,
                                height: 2_160,
                                fps: 60,
                                backendName: backend,
                                transportName: transport,
                                contentModeName: contentMode,
                                encoderExperimentName: experiment.baseAddress
                            )
                        }
                    }
                }
            }
        }
        precondition(invalidV6Start == 0)
        precondition(
            String(cString: leftcarCaptureLastErrorV2())
                == "unknown encoder experiment: invalid UTF-8"
        )
        precondition(
            phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: "com.apple.videotoolbox.videoencoder.h264.gva",
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: noErr,
                hardware: false
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: noErr,
                hardware: nil
            )
        )
        precondition(
            phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: kVTPropertyNotSupportedErr,
                hardware: nil
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: -12_345,
                hardware: nil
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .rtvc, codec: .hevc),
                encoderIDStatus: noErr,
                encoderID: "com.apple.videotoolbox.videoencoder.hevc.rtvc",
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            !phaseARTVCH264EncoderVerified(
                policy: EncoderSessionPolicy(mode: .ave, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            encoderSessionVerified(
                policy: EncoderSessionPolicy(mode: .ave, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                expectedEncoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            !encoderSessionVerified(
                policy: EncoderSessionPolicy(mode: .ave, codec: .h264),
                encoderIDStatus: noErr,
                encoderID: phaseARTVCH264EncoderID,
                expectedEncoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                hardwareStatus: noErr,
                hardware: true
            )
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .auto,
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: false
            ) == .success(applied: .rateControl)
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .rateControl,
                rtvcHardwareAvailable: false,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: false
            ) == .failure("RTVC hardware encoder is unavailable")
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .adaptiveQp,
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: false
            ) == .failure("adaptiveQp requires SupportsBaseFrameQP")
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .encoderPool,
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: false
            ) == .failure("encoderPool requires a VideoToolbox pixel buffer pool")
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .encoderPool,
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: true
            ) == .success(applied: .encoderPool)
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .splitVertical,
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                splitDiagnosticEnabled: false
            ) == .failure("splitVertical diagnostics are disabled")
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .splitVertical,
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                splitDiagnosticEnabled: true
            ) == .success(applied: .splitVertical)
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .splitVertical,
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "tcp",
                hasEncoderPixelBufferPool: true,
                splitDiagnosticEnabled: true
            ) == .failure("splitVertical requires 3840x2160 at 60fps over direct UDP")
        )
        precondition(
            advertisedEncoderExperiments(
                rtvcHardwareAvailable: false,
                supportsBaseFrameQP: true,
                hasEncoderPixelBufferPool: true,
                dualAveTilePairAvailable: true
            ).isEmpty
        )
        precondition(
            advertisedEncoderExperiments(
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: false,
                hasEncoderPixelBufferPool: false,
                dualAveTilePairAvailable: false
            ) == [.auto, .rateControl]
        )
        precondition(
            advertisedEncoderExperiments(
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: true,
                hasEncoderPixelBufferPool: true,
                dualAveTilePairAvailable: false
            ) == [.auto, .rateControl, .adaptiveQp, .encoderPool]
        )
        precondition(
            advertisedEncoderExperiments(
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: true,
                hasEncoderPixelBufferPool: true,
                dualAveTilePairAvailable: true
            ) == [.auto, .rateControl, .adaptiveQp, .encoderPool, .splitVertical]
        )
        precondition(
            !advertisedEncoderExperiments(
                rtvcHardwareAvailable: true,
                supportsBaseFrameQP: true,
                hasEncoderPixelBufferPool: true,
                dualAveTilePairAvailable: true
            ).contains(.splitHorizontal)
        )
        precondition(
            encoderExperimentCapabilityEntries(
                verifiedRTVCH264: false,
                supportsBaseFrameQP: true,
                hasEncoderPixelBufferPool: true,
                dualAveTilePairAvailable: true
            ).isEmpty
        )
        let capabilityEntries = encoderExperimentCapabilityEntries(
            verifiedRTVCH264: true,
            supportsBaseFrameQP: true,
            hasEncoderPixelBufferPool: true,
            dualAveTilePairAvailable: true
        )
        let capabilityData = try! JSONSerialization.data(withJSONObject: capabilityEntries)
        let decodedCapabilities = try! JSONSerialization.jsonObject(with: capabilityData)
            as! [[String: Any]]
        precondition(
            decodedCapabilities.compactMap { $0["id"] as? String }
                == ["auto", "rateControl", "adaptiveQp", "encoderPool", "splitVertical"]
        )
        precondition(
            decodedCapabilities.allSatisfy {
                Set($0.keys) == Set(["id", "label", "hint", "requiresReconnect"])
                    && ($0["requiresReconnect"] as? Bool) == true
            }
        )
        precondition(
            decodedCapabilities.allSatisfy {
                $0["id"] as? String != "splitHorizontal"
            }
        )

        let explicitStats = encoderExperimentStatsFields(
            requested: .rateControl,
            applied: .rateControl,
            fallbackReason: "must not leak for explicit profiles",
            baseFrameQp: 32,
            baseFrameQpChanges: 4,
            encoderFrameDrops: 5,
            encoderFrameDropFps: 6,
            validEncodeOutputFps: 57,
            encodeSubmitCallP50Us: 8,
            encodeSubmitCallP95Us: 9,
            encoderCallbackP50Us: 10,
            encoderCallbackP95Us: 11,
            packetizationInFlight: 2
        )
        let expectedExperimentStatsKeys: Set<String> = [
            "encoderExperimentRequested",
            "encoderExperimentApplied",
            "encoderExperimentFallbackReason",
            "baseFrameQp",
            "baseFrameQpChanges",
            "encoderFrameDrops",
            "encoderFrameDropFps",
            "validEncodeOutputFps",
            "encodeSubmitCallP50Us",
            "encodeSubmitCallP95Us",
            "encoderCallbackP50Us",
            "encoderCallbackP95Us",
            "packetizationInFlight",
        ]
        precondition(Set(explicitStats.keys) == expectedExperimentStatsKeys)
        precondition(explicitStats["encoderExperimentRequested"] as? String == "rateControl")
        precondition(explicitStats["encoderExperimentApplied"] as? String == "rateControl")
        precondition(explicitStats["encoderExperimentFallbackReason"] is NSNull)
        precondition(explicitStats["baseFrameQp"] is NSNull)
        let explicitStatsData = try! JSONSerialization.data(withJSONObject: explicitStats)
        precondition(
            try! JSONSerialization.jsonObject(with: explicitStatsData) is [String: Any]
        )

        let adaptiveStats = encoderExperimentStatsFields(
            requested: .adaptiveQp,
            applied: .adaptiveQp,
            fallbackReason: "must not leak for explicit profiles",
            baseFrameQp: 34,
            baseFrameQpChanges: 2,
            encoderFrameDrops: 0,
            encoderFrameDropFps: 0,
            validEncodeOutputFps: 60,
            encodeSubmitCallP50Us: 1_000,
            encodeSubmitCallP95Us: 2_000,
            encoderCallbackP50Us: 3_000,
            encoderCallbackP95Us: 4_000,
            packetizationInFlight: 1
        )
        precondition(Set(adaptiveStats.keys) == expectedExperimentStatsKeys)
        precondition((adaptiveStats["baseFrameQp"] as? NSNumber)?.int32Value == 34)
        precondition(adaptiveStats["encoderExperimentFallbackReason"] is NSNull)

        let automaticStats = encoderExperimentStatsFields(
            requested: .auto,
            applied: .rateControl,
            fallbackReason: "auto selected exact RTVC rate control",
            baseFrameQp: nil,
            baseFrameQpChanges: 0,
            encoderFrameDrops: 0,
            encoderFrameDropFps: 0,
            validEncodeOutputFps: 60,
            encodeSubmitCallP50Us: 1_000,
            encodeSubmitCallP95Us: 2_000,
            encoderCallbackP50Us: 3_000,
            encoderCallbackP95Us: 4_000,
            packetizationInFlight: 0
        )
        precondition(
            automaticStats["encoderExperimentFallbackReason"] as? String
                == "auto selected exact RTVC rate control"
        )

        let capabilityPointer = leftcarCaptureEncoderExperimentsV1()
        let exportedCapabilityJSON = String(validatingUTF8: capabilityPointer)
        leftcarCaptureFreeString(s: capabilityPointer)
        precondition(exportedCapabilityJSON != nil)
        let exportedCapabilityValue = try! JSONSerialization.jsonObject(
            with: exportedCapabilityJSON!.data(using: .utf8)!
        )
        precondition(exportedCapabilityValue is [Any])
        precondition(baseFrameQp(forSliderPercent: 25) == 42)
        precondition(baseFrameQp(forSliderPercent: 50) == 26)
        precondition(baseFrameQp(forSliderPercent: 0) == nil)
        precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: true, stableWindows: 0) == 34)
        precondition(nextAdaptiveBaseFrameQp(current: 41, pressured: true, stableWindows: 0) == 42)
        precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: false, stableWindows: 2) == 32)
        precondition(nextAdaptiveBaseFrameQp(current: 32, pressured: false, stableWindows: 3) == 31)
        precondition(nextAdaptiveBaseFrameQp(current: 26, pressured: false, stableWindows: 3) == 26)
        precondition(nextAdaptiveBaseFrameQp(current: 50, pressured: false, stableWindows: 0) == 42)
        precondition(nextAdaptiveBaseFrameQp(current: 10, pressured: true, stableWindows: 0) == 28)
        precondition(
            encoderCallbackDisposition(status: noErr, flags: [.frameDropped], hasSample: false) == .dropped
        )
        precondition(
            encoderCallbackDisposition(status: noErr, flags: [], hasSample: true) == .valid
        )
        precondition(
            encoderCallbackDisposition(status: -1, flags: [], hasSample: false) == .failed
        )
        precondition(
            recoveryActionForDroppedFrame(requestedRecoveryKeyframe: false) == .none
        )
        precondition(
            recoveryActionForDroppedFrame(requestedRecoveryKeyframe: true) == .retryAfterCooldown
        )
        precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .valid))
        precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .dropped))
        precondition(shouldReleaseEncodeSlotBeforePacketization(disposition: .failed))

        var belowStallBudget = SingleEncoderHealthState()
        precondition(
            belowStallBudget.evaluate(
                nowNs: 250_000_999,
                captureCallbackNs: 250_000_999,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1_000,
                generation: 1
            ) == .healthy
        )
        var atStallBudget = SingleEncoderHealthState()
        let firstRestartDecision = atStallBudget.evaluate(
            nowNs: 250_001_000,
            captureCallbackNs: 250_001_000,
            lastValidOutputNs: nil,
            encodeInFlight: 1,
            oldestSubmissionNs: 1_000,
            generation: 1
        )
        precondition(firstRestartDecision == .restart(generation: 1))

        var missingEvidence = SingleEncoderHealthState()
        precondition(
            missingEvidence.evaluate(
                nowNs: 500_000_000,
                captureCallbackNs: nil,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1,
                generation: 2
            ) == .healthy
        )
        precondition(
            missingEvidence.evaluate(
                nowNs: 500_000_001,
                captureCallbackNs: 100,
                lastValidOutputNs: nil,
                encodeInFlight: 0,
                oldestSubmissionNs: 1,
                generation: 2
            ) == .healthy
        )
        precondition(
            missingEvidence.evaluate(
                nowNs: 500_000_002,
                captureCallbackNs: 101,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: nil,
                generation: 2
            ) == .healthy
        )
        precondition(
            missingEvidence.evaluate(
                nowNs: 500_000_003,
                captureCallbackNs: 101,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1,
                generation: 2
            ) == .healthy
        )
        precondition(
            missingEvidence.evaluate(
                nowNs: 500_000_004,
                captureCallbackNs: 100,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1,
                generation: 2
            ) == .healthy
        )

        var outputProgress = SingleEncoderHealthState()
        outputProgress.recordValidOutput(at: 260_000_000)
        precondition(
            outputProgress.evaluate(
                nowNs: 500_000_000,
                captureCallbackNs: 500_000_000,
                lastValidOutputNs: 260_000_000,
                encodeInFlight: 1,
                oldestSubmissionNs: 250_000_000,
                generation: 3
            ) == .healthy
        )
        precondition(
            outputProgress.evaluate(
                nowNs: 501_000_000,
                captureCallbackNs: 501_000_000,
                lastValidOutputNs: 260_000_000,
                encodeInFlight: 1,
                oldestSubmissionNs: 250_000_000,
                generation: 3
            ) == .restart(generation: 3)
        )

        precondition(
            atStallBudget.evaluate(
                nowNs: 500_001_000,
                captureCallbackNs: 500_001_000,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1_000,
                generation: 1
            ) == .healthy
        )
        atStallBudget.recordRestart(at: 250_001_000, generation: 1)
        precondition(
            atStallBudget.evaluate(
                nowNs: 1_250_000_999,
                captureCallbackNs: 1_250_000_999,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 900_000_000,
                generation: 2
            ) == .healthy
        )

        var restartBudget = SingleEncoderHealthState()
        restartBudget.recordRestart(at: 1_000_000_000, generation: 10)
        precondition(
            restartBudget.evaluate(
                nowNs: 2_000_000_000,
                captureCallbackNs: 2_000_000_000,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 1_700_000_000,
                generation: 11
            ) == .restart(generation: 11)
        )
        restartBudget.recordRestart(at: 2_000_000_000, generation: 11)
        precondition(
            restartBudget.evaluate(
                nowNs: 3_000_000_000,
                captureCallbackNs: 3_000_000_000,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 2_700_000_000,
                generation: 12
            ) == .terminate
        )

        var prunedRestartBudget = SingleEncoderHealthState()
        prunedRestartBudget.recordRestart(at: 1_000_000_000, generation: 20)
        prunedRestartBudget.recordRestart(at: 2_000_000_000, generation: 21)
        precondition(
            prunedRestartBudget.evaluate(
                nowNs: 12_000_000_001,
                captureCallbackNs: 12_000_000_001,
                lastValidOutputNs: nil,
                encodeInFlight: 1,
                oldestSubmissionNs: 11_700_000_000,
                generation: 22
            ) == .restart(generation: 22)
        )

        let callbackFirstCompletion = EncodeSlotCompletionToken()
        precondition(callbackFirstCompletion.claim(.callback))
        precondition(!callbackFirstCompletion.claim(.submitFailureReturn))
        precondition(!callbackFirstCompletion.claim(.watchdog))
        precondition(callbackFirstCompletion.completedBy == .callback)
        precondition(callbackFirstCompletion.completionCount == 1)

        let submitFailureFirstCompletion = EncodeSlotCompletionToken()
        precondition(submitFailureFirstCompletion.claim(.submitFailureReturn))
        precondition(!submitFailureFirstCompletion.claim(.callback))
        precondition(submitFailureFirstCompletion.completedBy == .submitFailureReturn)
        precondition(submitFailureFirstCompletion.completionCount == 1)

        let watchdogFirstCompletion = EncodeSlotCompletionToken()
        precondition(watchdogFirstCompletion.claim(.watchdog))
        precondition(!watchdogFirstCompletion.claim(.callback))
        precondition(watchdogFirstCompletion.completedBy == .watchdog)
        precondition(watchdogFirstCompletion.completionCount == 1)

        for _ in 0..<100 {
            let competingCompletion = EncodeSlotCompletionToken()
            let competitors = DispatchGroup()
            competitors.enter()
            DispatchQueue.global(qos: .userInitiated).async {
                _ = competingCompletion.claim(.callback)
                competitors.leave()
            }
            competitors.enter()
            DispatchQueue.global(qos: .userInitiated).async {
                _ = competingCompletion.claim(.watchdog)
                competitors.leave()
            }
            precondition(competitors.wait(timeout: .now() + .seconds(1)) == .success)
            precondition(competingCompletion.completionCount == 1)
            precondition(
                competingCompletion.completedBy == .callback
                    || competingCompletion.completedBy == .watchdog
            )
        }

        let generationOneOldest = EncodeSlotCompletionToken()
        let generationOneNewest = EncodeSlotCompletionToken()
        let generationTwo = EncodeSlotCompletionToken()
        var submissionLedger = SingleEncodeSubmissionLedger()
        submissionLedger.register(.init(
            id: 1,
            generation: 31,
            pts: 101,
            submittedNs: 10_000,
            token: generationOneOldest
        ))
        submissionLedger.register(.init(
            id: 2,
            generation: 31,
            pts: 102,
            submittedNs: 20_000,
            token: generationOneNewest
        ))
        submissionLedger.register(.init(
            id: 3,
            generation: 32,
            pts: 103,
            submittedNs: 5_000,
            token: generationTwo
        ))
        precondition(submissionLedger.oldestSubmissionNs(generation: 31) == 10_000)
        precondition(submissionLedger.oldestSubmissionNs(generation: 32) == 5_000)
        let reclaimedGeneration = submissionLedger.reclaim(generation: 31)
        precondition(reclaimedGeneration.map(\.id) == [1, 2])
        precondition(submissionLedger.retire(id: 1) == nil)
        precondition(submissionLedger.retire(id: 2) == nil)
        precondition(submissionLedger.retire(id: 3)?.generation == 32)
        precondition(submissionLedger.oldestSubmissionNs(generation: 31) == nil)
        precondition(submissionLedger.oldestSubmissionNs(generation: 32) == nil)

        let stalledInterleavingToken = EncodeSlotCompletionToken()
        let validInterleavingToken = EncodeSlotCompletionToken()
        var interleavingLedger = SingleEncodeSubmissionLedger()
        interleavingLedger.register(.init(
            id: 40,
            generation: 41,
            pts: 140,
            submittedNs: 1_000,
            token: stalledInterleavingToken
        ))
        interleavingLedger.register(.init(
            id: 41,
            generation: 41,
            pts: 141,
            submittedNs: 200_000_000,
            token: validInterleavingToken
        ))
        var interleavingHealth = SingleEncoderHealthState()
        var interleavingLastValidOutputNs: UInt64? = 2_000
        interleavingHealth.recordValidOutput(at: 2_000)
        precondition(
            interleavingHealth.evaluate(
                nowNs: 250_001_000,
                captureCallbackNs: 250_001_000,
                lastValidOutputNs: interleavingLastValidOutputNs,
                encodeInFlight: 2,
                oldestSubmissionNs: interleavingLedger.oldestSubmissionNs(
                    generation: 41
                ),
                generation: 41
            ) == .healthy
        )
        let validCallbackRetirement = interleavingLedger
            .retireCallbackAndRecordHealthProgress(
                id: 41,
                callbackGeneration: 41,
                currentGeneration: 41,
                callbackNs: 260_000_000,
                isValidOutput: true,
                healthState: &interleavingHealth,
                lastValidOutputNs: &interleavingLastValidOutputNs
            )
        precondition(validCallbackRetirement.submission?.id == 41)
        precondition(validCallbackRetirement.generationWasCurrent)
        precondition(interleavingLastValidOutputNs == 260_000_000)
        precondition(
            interleavingLedger.oldestSubmissionNs(generation: 41) == 1_000
        )
        precondition(
            interleavingHealth.evaluate(
                nowNs: 260_000_001,
                captureCallbackNs: 260_000_001,
                lastValidOutputNs: interleavingLastValidOutputNs,
                encodeInFlight: 1,
                oldestSubmissionNs: interleavingLedger.oldestSubmissionNs(
                    generation: 41
                ),
                generation: 41
            ) == .healthy
        )

        let firstPostRestartFramePlan = encoderFramePropertyPlan(
            appliedExperiment: .rateControl,
            requestKeyframe: firstRestartDecision == .restart(generation: 1),
            currentBaseFrameQp: 32
        )
        precondition(firstPostRestartFramePlan.forceKeyframe)

        let submissionTiming = EncoderSubmissionTimingContext()
        submissionTiming.recordInputPreparation(startNs: 1_000, endNs: 5_000)
        submissionTiming.beginSubmitCall(at: 7_000)
        precondition(submissionTiming.inputPreparationUs == 4)
        precondition(submissionTiming.submitCallDurationUs(at: 9_000) == 2)
        precondition(submissionTiming.callbackLatencyUs(at: 11_000) == 4)

        var recoveryBoundary = NetworkRecoveryBoundaryState()
        precondition(recoveryBoundary.admission(isKeyframe: false) == .admit)
        recoveryBoundary.establishAwaitingKeyframe()
        precondition(recoveryBoundary.admission(isKeyframe: false) == .dropDelta)
        precondition(recoveryBoundary.admission(isKeyframe: true) == .admit)

        var recoveryLossBoundary = NetworkRecoveryBoundaryState()
        recoveryLossBoundary.establishAwaitingKeyframe()
        precondition(recoveryLossBoundary.admission(isKeyframe: false) == .dropDelta)
        recoveryLossBoundary.clearAfterSuccessfulKeyframe()
        precondition(recoveryLossBoundary.admission(isKeyframe: false) == .admit)

        var retryRegistration = RecoveryDropRetryState()
        precondition(
            retryRegistration.register(
                generation: 41,
                currentGeneration: 41
            )
        )
        precondition(retryRegistration.hasPendingRetry)
        precondition(
            recoveryKeyframeRequestDecision(
                delayedRetryPending: retryRegistration.hasPendingRetry,
                scheduledRetry: false,
                recoveryKeyframePending: false,
                cooldownElapsed: true
            ) == .suppressForDelayedRetry
        )
        precondition(recoveryBoundary.admission(isKeyframe: false) == .dropDelta)
        precondition(
            !retryRegistration.register(
                generation: 40,
                currentGeneration: 41
            )
        )
        precondition(retryRegistration.pendingGeneration == 41)
        retryRegistration.clear()
        recoveryBoundary.clearAfterSuccessfulKeyframe()
        precondition(!retryRegistration.hasPendingRetry)
        precondition(recoveryBoundary.admission(isKeyframe: false) == .admit)
        precondition(
            retryRegistration.register(
                generation: 42,
                currentGeneration: 42
            )
        )
        precondition(retryRegistration.pendingGeneration == 42)

        var currentGenerationRetry = RecoveryDropRetryState()
        var currentGenerationBoundary = NetworkRecoveryBoundaryState()
        precondition(
            currentGenerationRetry.register(
                generation: 51,
                currentGeneration: 51
            )
        )
        precondition(
            currentGenerationRetry.establishBoundaryIfOwned(
                generation: 51,
                currentGeneration: 51,
                boundary: &currentGenerationBoundary
            )
        )
        precondition(currentGenerationBoundary.awaitingKeyframe)

        var staleGenerationRetry = RecoveryDropRetryState()
        var staleGenerationBoundary = NetworkRecoveryBoundaryState()
        precondition(
            !staleGenerationRetry.register(
                generation: 60,
                currentGeneration: 61
            )
        )
        precondition(
            !staleGenerationRetry.establishBoundaryIfOwned(
                generation: 60,
                currentGeneration: 61,
                boundary: &staleGenerationBoundary
            )
        )
        precondition(!staleGenerationBoundary.awaitingKeyframe)
        precondition(!staleGenerationRetry.hasPendingRetry)

        var replacedGenerationRetry = RecoveryDropRetryState()
        var replacedGenerationBoundary = NetworkRecoveryBoundaryState()
        precondition(
            replacedGenerationRetry.register(
                generation: 70,
                currentGeneration: 70
            )
        )
        precondition(
            !replacedGenerationRetry.establishBoundaryIfOwned(
                generation: 70,
                currentGeneration: 71,
                boundary: &replacedGenerationBoundary
            )
        )
        precondition(!replacedGenerationBoundary.awaitingKeyframe)
        precondition(!replacedGenerationRetry.hasPendingRetry)

        var delayedRetry = RecoveryDropRetryState()
        var delayedRetryBoundary = NetworkRecoveryBoundaryState()
        precondition(
            delayedRetry.register(
                generation: 80,
                currentGeneration: 80
            )
        )
        precondition(
            delayedRetry.establishBoundaryIfOwned(
                generation: 80,
                currentGeneration: 80,
                boundary: &delayedRetryBoundary
            )
        )
        precondition(delayedRetry.consume(generation: 80))
        precondition(!delayedRetry.consume(generation: 80))
        precondition(!delayedRetry.hasPendingRetry)

        var packetizationState = PacketizationAdmissionState(limit: 2)
        precondition(packetizationState.admit(recoveryBoundaryPending: false) == .admit)
        precondition(packetizationState.inFlight == 1)
        precondition(packetizationState.admit(recoveryBoundaryPending: false) == .admit)
        precondition(packetizationState.inFlight == 2)
        precondition(
            packetizationState.admit(recoveryBoundaryPending: false) == .dropAndBeginRecovery
        )
        precondition(packetizationState.inFlight == 2)
        packetizationState.finish()
        precondition(packetizationState.inFlight == 1)
        precondition(packetizationState.admit(recoveryBoundaryPending: true) == .admit)
        precondition(packetizationState.inFlight == 2)
        precondition(
            packetizationState.admit(recoveryBoundaryPending: true) == .dropDuringRecovery
        )
        precondition(packetizationState.inFlight == 2)
        packetizationState.finish()
        packetizationState.finish()
        packetizationState.finish()
        precondition(packetizationState.inFlight == 0)

        let packetizationLimit4K = packetizationInFlightLimit(width: 3_840, height: 2_160)
        precondition(packetizationLimit4K == 2)
        precondition(
            packetizationAdmissionDecision(
                inFlight: packetizationLimit4K - 1,
                limit: packetizationLimit4K,
                recoveryBoundaryPending: false
            ) == .admit
        )
        precondition(
            packetizationAdmissionDecision(
                inFlight: packetizationLimit4K,
                limit: packetizationLimit4K,
                recoveryBoundaryPending: false
            ) == .dropAndBeginRecovery
        )
        precondition(
            packetizationAdmissionDecision(
                inFlight: packetizationLimit4K + 3,
                limit: packetizationLimit4K,
                recoveryBoundaryPending: true
            ) == .dropDuringRecovery
        )
        precondition(
            encoderCallbackLatencyUs(submitCallStartNs: 5_000, callbackNs: 9_000) == 4
        )
        precondition(
            encoderCallbackLatencyUs(submitCallStartNs: 1_000, callbackNs: 9_000) == 8
        )
        precondition(
            encodeSubmitCallDurationUs(submitCallStartNs: 5_000, submitCallEndNs: 8_000) == 3
        )
        precondition(
            recoveryKeyframeRequestDecision(
                delayedRetryPending: true,
                scheduledRetry: false,
                recoveryKeyframePending: false,
                cooldownElapsed: true
            ) == .suppressForDelayedRetry
        )
        precondition(
            recoveryKeyframeRequestDecision(
                delayedRetryPending: true,
                scheduledRetry: true,
                recoveryKeyframePending: false,
                cooldownElapsed: true
            ) == .requestNow
        )
        precondition(
            recoveryKeyframeRequestDecision(
                delayedRetryPending: false,
                scheduledRetry: false,
                recoveryKeyframePending: false,
                cooldownElapsed: true
            ) == .requestNow
        )
        precondition(
            recoveryKeyframeRequestDecision(
                delayedRetryPending: false,
                scheduledRetry: false,
                recoveryKeyframePending: true,
                cooldownElapsed: false
            ) == .suppressForCooldown
        )
        precondition(encoderPrepareStrategy(mode: .ave) == .deferToFirstFrame)
        precondition(encoderPrepareStrategy(mode: .rtvc) == .eager)
        precondition(
            encoderStartupInFlightLimit(
                mode: .ave,
                outputConfirmed: false,
                configuredLimit: 3
            ) == 1
        )
        precondition(
            encoderStartupInFlightLimit(
                mode: .ave,
                outputConfirmed: true,
                configuredLimit: 3
            ) == 3
        )
        precondition(
            encoderStartupInFlightLimit(
                mode: .rtvc,
                outputConfirmed: false,
                configuredLimit: 3
            ) == 3
        )
        precondition(
            encoderInputSurfacePolicy(
                experiment: .encoderPool,
                mode: .rtvc,
                width: 3_840,
                height: 2_160,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: false
            ) == .pixelTransfer
        )
        precondition(
            encoderInputSurfacePolicy(
                experiment: .rateControl,
                mode: .rtvc,
                width: 3_840,
                height: 2_160,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: false
            ) == .direct
        )
        precondition(
            encoderInputSurfacePolicy(
                experiment: .adaptiveQp,
                mode: .rtvc,
                width: 3_840,
                height: 2_160,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: false
            ) == .direct
        )
        precondition(
            encoderInputSurfacePolicy(
                mode: .rtvc,
                width: 3_840,
                height: 2_160,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: false
            ) == .direct
        )
        precondition(
            encoderInputSurfacePolicy(
                mode: .rtvc,
                width: 3_840,
                height: 2_160,
                captureBackend: "screenCaptureKit",
                aveRetryStagingEnabled: false
            ) == .direct
        )
        precondition(
            encoderInputSurfacePolicy(
                mode: .rtvc,
                width: 2_560,
                height: 1_440,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: false
            ) == .cpuCopy
        )
        precondition(
            encoderInputSurfacePolicy(
                mode: .ave,
                width: 3_840,
                height: 2_160,
                captureBackend: "cgDisplayStream",
                aveRetryStagingEnabled: true
            ) == .pixelTransfer
        )
        precondition(
            encoderStartupFailureAction(
                mode: .ave,
                encodedOutputCount: 0,
                inputStagingEnabled: false,
                hasNextPolicy: true
            ) == .retryWithStagingInput
        )
        precondition(
            encoderStartupFailureAction(
                mode: .ave,
                encodedOutputCount: 0,
                inputStagingEnabled: true,
                hasNextPolicy: true
            ) == .fallbackToNextPolicy
        )
        precondition(
            encoderStartupFailureAction(
                mode: .ave,
                encodedOutputCount: 1,
                inputStagingEnabled: true,
                hasNextPolicy: true
            )
                == .recoverCurrentEncoder
        )
        precondition(
            encoderStartupFailureAction(
                mode: .rtvc,
                encodedOutputCount: 0,
                inputStagingEnabled: false,
                hasNextPolicy: true
            )
                == .fallbackToNextPolicy
        )
        precondition(
            encoderStartupFailureAction(
                mode: .rtvc,
                encodedOutputCount: 0,
                inputStagingEnabled: false,
                hasNextPolicy: false
            )
                == .recoverCurrentEncoder
        )
        precondition(
            eligibleEncoderSessionPolicies(video4K, unavailableModes: [.ave]) == [
                EncoderSessionPolicy(mode: .rtvc, codec: .h264),
            ]
        )
        precondition(
            eligibleEncoderSessionPolicies(video4K, unavailableModes: []) == video4K
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
                inputPreparationP95Us: 2_000,
                queueOldestUs: 2_100,
                encoderWatchdogRestarts: 0,
                encoderWatchdogTerminations: 0,
                encoderLateCallbacks: 0,
                encoderWatchdogOldestUs: 0,
                encoderMode: "rtvc",
                encoderID: "com.apple.videotoolbox.videoencoder.h264.rtvc"
            ) == "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=118 captureFps=60 encodeOutputFps=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 inputPreparationP95Us=2000 queueOldestUs=2100 encoderWatchdogRestarts=0 encoderWatchdogTerminations=0 encoderLateCallbacks=0 encoderWatchdogOldestUs=0 encoderMode=rtvc encoderID=com.apple.videotoolbox.videoencoder.h264.rtvc"
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
            .suggestedLookAheadFrameCount,
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
        report.recordSuppressed("SuggestedLookAheadFrameCount")
        report.recordUnsupported("MissingProperty")
        report.recordRejected("Quality", status: -12_900)
        precondition(report.applied == ["HighSpeed"])
        precondition(report.suppressed == ["SuggestedLookAheadFrameCount"])
        precondition(report.unsupported == ["MissingProperty"])
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
        precondition(tcpMediaFrameLimitBytes == 16 * 1024 * 1024)
        precondition(isValidTcpMediaFrameLength(16 * 1024 * 1024))
        precondition(!isValidTcpMediaFrameLength(16 * 1024 * 1024 + 1))
        precondition(
            viewerControlSide(sourcePort: 54_321, targetPort: 5_001, split: false) == .left
        )
        precondition(
            viewerControlSide(sourcePort: 5_001, targetPort: 5_001, split: true) == .left
        )
        precondition(
            viewerControlSide(sourcePort: 5_002, targetPort: 5_001, split: true) == .right
        )
        precondition(
            viewerControlSide(sourcePort: 54_321, targetPort: 5_001, split: true) == nil
        )
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 7_700_000,
                bytes: 100 * 1024,
                fps: 60
            ) >= 24_000_000
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
            ) <= 64_000_000
        )
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 24_000_000,
                bytes: 1024 * 1024,
                fps: 60
            ) == 64_000_000
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
        let pacingBursts = udpPacingBurstRanges(datagramCount: 17)
        precondition(pacingBursts == [0..<8, 8..<16, 16..<17])
        precondition(udpPacingBurstRanges(datagramCount: 0).isEmpty)
        precondition(udpMediaDatagramBytes == 1_400)
        precondition(udpMediaFragmentPayloadBytes == 1_367)
        precondition(
            udpPacingBurstRanges(datagramCount: 9, maxDatagrams: 4)
                == [0..<4, 4..<8, 8..<9]
        )
        precondition(
            udpPacingRateMultiplier(
                profile: .responsive,
                burstDatagrams: 8
            ) == 1
        )
        precondition(
            udpPacingRateMultiplier(
                profile: .custom,
                burstDatagrams: 16
            ) == 2
        )
        let standardDeltaInterval = udpDatagramIntervalUs(
            bytes: 16 * udpMediaDatagramBytes,
            isKeyframe: false,
            accessUnitBytes: 75 * 1024,
            targetBitrate: 60_000_000,
            fps: 60,
            dataFragmentCount: 56,
            selectedParity: 2,
            pacingRateMultiplier: 1
        )
        let cleanLanDeltaInterval = udpDatagramIntervalUs(
            bytes: 16 * udpMediaDatagramBytes,
            isKeyframe: false,
            accessUnitBytes: 75 * 1024,
            targetBitrate: 60_000_000,
            fps: 60,
            dataFragmentCount: 56,
            selectedParity: 2,
            pacingRateMultiplier: 2
        )
        precondition(cleanLanDeltaInterval < standardDeltaInterval)
        precondition(cleanLanDeltaInterval * 2 <= standardDeltaInterval + 1)
        let standardKeyframeInterval = udpDatagramIntervalUs(
            bytes: 16 * udpMediaDatagramBytes,
            isKeyframe: true,
            accessUnitBytes: 500 * 1024,
            targetBitrate: 60_000_000,
            fps: 60,
            dataFragmentCount: 375,
            selectedParity: 2,
            pacingRateMultiplier: 1
        )
        let cleanLanKeyframeInterval = udpDatagramIntervalUs(
            bytes: 16 * udpMediaDatagramBytes,
            isKeyframe: true,
            accessUnitBytes: 500 * 1024,
            targetBitrate: 60_000_000,
            fps: 60,
            dataFragmentCount: 375,
            selectedParity: 2,
            pacingRateMultiplier: 2
        )
        precondition(cleanLanKeyframeInterval == standardKeyframeInterval)
        let autoUdp = AppliedUdpStability(
            profile: .auto,
            burstDatagrams: 4,
            fecParityShards: 2,
            adaptivePacing: true
        )
        var autoBurst = UdpBurstPolicyState(applied: autoUdp)
        precondition(autoBurst.currentBurstDatagrams == 4)
        precondition(autoBurst.currentFecParityShards == 2)
        let receiverLossDecision = autoBurst.observe(.init(
            nowNs: 1_000,
            incompleteAccessUnits: 1,
            oneFrameGapEvents: 0,
            multiFrameGapEvents: 0,
            recoveryBoundarySent: false
        ))
        precondition(receiverLossDecision.burstDatagrams == 4)
        precondition(receiverLossDecision.fecParityShards == 3)
        let parityHoldDecision = autoBurst.observe(.init(
            nowNs: 4_999_999_999,
            incompleteAccessUnits: 1,
            oneFrameGapEvents: 0,
            multiFrameGapEvents: 0,
            recoveryBoundarySent: false
        ))
        precondition(parityHoldDecision.burstDatagrams == 4)
        precondition(parityHoldDecision.fecParityShards == 3)
        let parityCleanDecision = autoBurst.observe(.init(
            nowNs: 5_000_001_000,
            incompleteAccessUnits: 1,
            oneFrameGapEvents: 0,
            multiFrameGapEvents: 0,
            recoveryBoundarySent: false
        ))
        precondition(parityCleanDecision.burstDatagrams == 4)
        precondition(parityCleanDecision.fecParityShards == 2)
        precondition(
            autoBurst.observe(.init(
                nowNs: 31_000_000_000,
                incompleteAccessUnits: 1,
                oneFrameGapEvents: 0,
                multiFrameGapEvents: 0,
                recoveryBoundarySent: true
            )).fecParityShards == 3
        )
        let recoveryHoldDecision = autoBurst.observe(.init(
            nowNs: 32_999_999_999,
            incompleteAccessUnits: 1,
            oneFrameGapEvents: 0,
            multiFrameGapEvents: 0,
            recoveryBoundarySent: false
        ))
        precondition(recoveryHoldDecision.burstDatagrams == 4)
        precondition(recoveryHoldDecision.fecParityShards == 3)
        precondition(
            autoBurst.observe(.init(
                nowNs: 36_000_001_000,
                incompleteAccessUnits: 1,
                oneFrameGapEvents: 0,
                multiFrameGapEvents: 0,
                recoveryBoundarySent: false
            )).fecParityShards == 2
        )
        var oneFrameLossBurst = UdpBurstPolicyState(applied: autoUdp)
        let oneFrameLossDecision = oneFrameLossBurst.observe(.init(
            nowNs: 1_000,
            incompleteAccessUnits: 0,
            oneFrameGapEvents: 1,
            multiFrameGapEvents: 0,
            recoveryBoundarySent: false
        ))
        precondition(oneFrameLossDecision.reason == .receiverLoss)
        precondition(oneFrameLossDecision.burstDatagrams == 4)
        precondition(oneFrameLossDecision.fecParityShards == 3)
        var stableBurst = UdpBurstPolicyState(applied: .init(
            profile: .stable,
            burstDatagrams: 2,
            fecParityShards: 4,
            adaptivePacing: false
        ))
        precondition(
            stableBurst.observe(.init(
                nowNs: 40_000_000_000,
                incompleteAccessUnits: 0,
                oneFrameGapEvents: 0,
                multiFrameGapEvents: 0,
                recoveryBoundarySent: false
            )).fecParityShards == 4
        )
        var responsiveBurst = UdpBurstPolicyState(applied: .init(
            profile: .responsive,
            burstDatagrams: 8,
            fecParityShards: 2,
            adaptivePacing: false
        ))
        precondition(
            responsiveBurst.observe(.init(
                nowNs: 40_000_000_000,
                incompleteAccessUnits: 10,
                oneFrameGapEvents: 10,
                multiFrameGapEvents: 10,
                recoveryBoundarySent: true
            )).fecParityShards == 2
        )
        precondition(fecParityCount(dataCount: 8, selectedParity: 4) == 4)
        precondition(fecParityCount(dataCount: 3, selectedParity: 4) == 2)
        precondition(
            AppliedUdpStability.validated(
                profileName: "custom",
                burstDatagrams: 16,
                fecParityShards: 2,
                adaptivePacing: false
            )?.burstDatagrams == 16
        )
        precondition(
            AppliedUdpStability.validated(
                profileName: "stable",
                burstDatagrams: 2,
                fecParityShards: 4,
                adaptivePacing: false
            )?.profile == .stable
        )
        precondition(
            AppliedUdpStability.validated(
                profileName: "stable",
                burstDatagrams: 8,
                fecParityShards: 2,
                adaptivePacing: false
            ) == nil
        )
        let strongFecSession = CaptureSession(
            targetAddr: sockaddr_in(),
            targetPort: 5_001,
            targetLabel: "test",
            width: 3_840,
            height: 2_160,
            fps: 60,
            backend: .cgDisplayStream,
            mediaTransport: .udp,
            requestedEncoderExperiment: .auto,
            udpStability: .init(
                profile: .stable,
                burstDatagrams: 2,
                fecParityShards: 4,
                adaptivePacing: false
            )
        )
        let strongParity = strongFecSession.fecParityDatagrams(
            auID: 7,
            totalFragments: 8,
            wallMs: 9,
            payloads: (0..<8).map { Data(repeating: UInt8($0), count: 64) }
        )
        precondition(strongParity.count == 4)
        precondition(strongParity.map { $0[4] } == [0, 1, 2, 3])
        let fullWidthParity = strongFecSession.fecParityDatagrams(
            auID: 8,
            totalFragments: 8,
            wallMs: 10,
            payloads: (0..<8).map {
                Data(repeating: UInt8($0), count: udpMediaFragmentPayloadBytes)
            },
            parityOverride: 2
        )
        precondition(fullWidthParity.count == 2)
        precondition(fullWidthParity.allSatisfy { $0.count <= udpMediaDatagramBytes })
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
