import Foundation
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Darwin

enum VideoCodecKind: String {
    case h264
    case hevc

    var id: UInt8 {
        switch self {
        case .h264: return 1
        case .hevc: return 2
        }
    }

    var parameterSetCount: Int {
        self == .hevc ? 3 : 2
    }
}

enum H264ProfileKind: Equatable {
    case high
    case main
}

enum H264EntropyModeKind: Equatable {
    case cavlc
    case cabac
}

enum EncoderPresetKind: Equatable {
    case highSpeed
    case videoConferencing
}

struct EncoderQualityPolicy: Equatable {
    let initialHint: Float?
    let supportsQualityProperty: Bool
}

struct VideoBitrateBounds: Equatable {
    let minimum: Int
    let maximum: Int
}

func isUltraHdDimensions(width: UInt32, height: UInt32) -> Bool {
    max(width, height) >= 3_840 && min(width, height) >= 2_160
}

func videoBitrateBounds(width: UInt32, height: UInt32, activeCount: Int) -> VideoBitrateBounds {
    if isUltraHdDimensions(width: width, height: height) {
        return VideoBitrateBounds(
            minimum: activeCount > 1 ? 18_000_000 : 24_000_000,
            maximum: activeCount > 1 ? 56_000_000 : 80_000_000
        )
    }
    return VideoBitrateBounds(
        minimum: activeCount > 1 ? 7_000_000 : 8_000_000,
        maximum: activeCount > 1 ? 20_000_000 : 28_000_000
    )
}

enum EncoderExperiment: String, Equatable {
    case auto
    case rateControl
    case adaptiveQp
    case encoderPool
    case splitHorizontal
    case splitVertical

    static func parse(_ raw: String?) -> EncoderExperiment? {
        guard let raw else { return .auto }
        guard let value = EncoderExperiment(rawValue: raw) else { return nil }
        switch value {
        case .auto, .rateControl, .adaptiveQp, .encoderPool, .splitVertical:
            return value
        case .splitHorizontal:
            return nil
        }
    }
}

enum EncoderMode: String, Hashable {
    case ave
    case rtvc
}

enum EncoderInputSurfacePolicy: Equatable {
    case direct
    case pixelTransfer
    case cpuCopy
}

func encoderInputSurfacePolicy(
    experiment: EncoderExperiment = .auto,
    mode: EncoderMode,
    width: UInt32,
    height: UInt32,
    captureBackend: String,
    aveRetryStagingEnabled: Bool
) -> EncoderInputSurfacePolicy {
    if experiment == .encoderPool {
        return .pixelTransfer
    }
    if mode == .ave {
        return aveRetryStagingEnabled ? .pixelTransfer : .direct
    }
    let isAtLeast1440p = max(width, height) >= 2_560 && min(width, height) >= 1_440
    let isUltraHD = max(width, height) >= 3_840 && min(width, height) >= 2_160
    return isAtLeast1440p && !isUltraHD
        && captureBackend.lowercased() == "cgdisplaystream"
        ? .cpuCopy
        : .direct
}

func appliedEncoderExperiment(_ requested: EncoderExperiment) -> EncoderExperiment {
    requested == .auto ? .rateControl : requested
}

func baseFrameQp(forSliderPercent percent: Int32) -> Int32? {
    guard (25...50).contains(percent) else { return nil }
    let ratio = Double(percent - 25) / 25.0
    return Int32((42.0 - ratio * 16.0).rounded())
}

func nextAdaptiveBaseFrameQp(
    current: Int32,
    pressured: Bool,
    stableWindows: Int
) -> Int32 {
    let clampedCurrent = min(42, max(26, current))
    if pressured { return min(42, clampedCurrent + 2) }
    if stableWindows >= 3 { return max(26, clampedCurrent - 1) }
    return clampedCurrent
}

func encoderCallbackDisposition(
    status: OSStatus,
    flags: VTEncodeInfoFlags,
    hasSample: Bool
) -> EncoderCallbackDisposition {
    if status == noErr, flags.contains(.frameDropped) { return .dropped }
    if status == noErr, hasSample { return .valid }
    return .failed
}

func recoveryActionForDroppedFrame(
    requestedRecoveryKeyframe: Bool
) -> RecoveryDroppedFrameAction {
    requestedRecoveryKeyframe ? .retryAfterCooldown : .none
}

func shouldReleaseEncodeSlotBeforePacketization(
    disposition: EncoderCallbackDisposition
) -> Bool {
    switch disposition {
    case .valid, .dropped, .failed:
        return true
    }
}

func packetizationInFlightLimit(width: UInt32, height: UInt32) -> Int {
    shouldOffloadEncodedSample(width: width, height: height) ? 2 : 1
}

func packetizationAdmissionDecision(
    inFlight: Int,
    limit: Int,
    recoveryBoundaryPending: Bool
) -> PacketizationAdmissionDecision {
    if inFlight < max(1, limit) {
        return .admit
    }
    return recoveryBoundaryPending ? .dropDuringRecovery : .dropAndBeginRecovery
}

func encodeSubmitCallDurationUs(
    submitCallStartNs: UInt64,
    submitCallEndNs: UInt64
) -> UInt64 {
    guard submitCallEndNs >= submitCallStartNs else { return 0 }
    return (submitCallEndNs - submitCallStartNs) / 1_000
}

func encoderCallbackLatencyUs(
    submitCallStartNs: UInt64,
    callbackNs: UInt64
) -> UInt64 {
    guard callbackNs >= submitCallStartNs else { return 0 }
    return (callbackNs - submitCallStartNs) / 1_000
}

func recoveryKeyframeRequestDecision(
    delayedRetryPending: Bool,
    scheduledRetry: Bool,
    recoveryKeyframePending: Bool,
    cooldownElapsed: Bool
) -> RecoveryKeyframeRequestDecision {
    if delayedRetryPending && !scheduledRetry {
        return .suppressForDelayedRetry
    }
    if (!recoveryKeyframePending || cooldownElapsed) && cooldownElapsed {
        return .requestNow
    }
    return .suppressForCooldown
}

func copyPlanarPixelBufferPlanes(
    from source: CVPixelBuffer,
    to destination: CVPixelBuffer
) -> OSStatus {
    guard CVPixelBufferGetPixelFormatType(source) == CVPixelBufferGetPixelFormatType(destination),
          CVPixelBufferGetWidth(source) == CVPixelBufferGetWidth(destination),
          CVPixelBufferGetHeight(source) == CVPixelBufferGetHeight(destination),
          CVPixelBufferGetPlaneCount(source) == CVPixelBufferGetPlaneCount(destination) else {
        return kCVReturnInvalidArgument
    }
    let sourceLock = CVPixelBufferLockBaseAddress(source, .readOnly)
    guard sourceLock == kCVReturnSuccess else { return sourceLock }
    defer { CVPixelBufferUnlockBaseAddress(source, .readOnly) }
    let destinationLock = CVPixelBufferLockBaseAddress(destination, [])
    guard destinationLock == kCVReturnSuccess else { return destinationLock }
    defer { CVPixelBufferUnlockBaseAddress(destination, []) }

    for plane in 0..<CVPixelBufferGetPlaneCount(source) {
        guard let sourceBase = CVPixelBufferGetBaseAddressOfPlane(source, plane),
              let destinationBase = CVPixelBufferGetBaseAddressOfPlane(destination, plane) else {
            return kCVReturnInvalidArgument
        }
        let sourceStride = CVPixelBufferGetBytesPerRowOfPlane(source, plane)
        let destinationStride = CVPixelBufferGetBytesPerRowOfPlane(destination, plane)
        let rowBytes = min(sourceStride, destinationStride)
        let rows = min(
            CVPixelBufferGetHeightOfPlane(source, plane),
            CVPixelBufferGetHeightOfPlane(destination, plane)
        )
        for row in 0..<rows {
            memcpy(
                destinationBase.advanced(by: row * destinationStride),
                sourceBase.advanced(by: row * sourceStride),
                rowBytes
            )
        }
    }
    CVBufferPropagateAttachments(source, destination)
    return noErr
}


func captureMinimumFrameTimeSeconds(fps: UInt32) -> Double {
    let targetFps = Double(max(1, fps))
    // CGDisplayStream treats this as a minimum interval, so asking for exactly
    // 1/60 leaves no scheduling margin and repeatedly settles near 57 fps.
    let captureCeilingFps = targetFps >= 60 ? targetFps + 2 : targetFps
    return 1.0 / captureCeilingFps
}

func captureQueueDepth(experiment: EncoderExperiment) -> Int {
    // A split frame remains backed by the ScreenCaptureKit IOSurface until
    // both hardware encoders have accepted their tile. Three framework
    // surfaces are not enough for two app-pending frames plus the measured
    // 2-3 encoder submissions in flight, which makes replayd throttle capture
    // to roughly 40 fps. Eight is ScreenCaptureKit's practical upper cushion;
    // the app queue remains bounded to two and latest-wins on overflow.
    experiment == .splitVertical ? 8 : 3
}

func encoderQualityPolicy(
    codec: VideoCodecKind,
    width: UInt32,
    height: UInt32
) -> EncoderQualityPolicy {
    let isUltraHd = isUltraHdDimensions(width: width, height: height)
    guard isUltraHd, codec == .h264 || codec == .hevc else {
        return EncoderQualityPolicy(initialHint: nil, supportsQualityProperty: false)
    }
    // Keep one quality state for both codecs. VideoToolbox may reject the
    // generic quality property on a particular hardware encoder; callers
    // still apply the corresponding bitrate scale as a real fallback.
    return EncoderQualityPolicy(initialHint: 0.5, supportsQualityProperty: true)
}

func preferredH264Profile(lowLatency: Bool) -> H264ProfileKind {
    lowLatency ? .high : .main
}

func preferredH264Profile(
    lowLatency: Bool,
    width: UInt32,
    height: UInt32,
    contentMode: String
) -> H264ProfileKind {
    let isUltraHd = max(width, height) >= 3_840 && min(width, height) >= 2_160
    if lowLatency,
       isUltraHd,
       contentMode.lowercased() == StreamContentMode.video.rawValue {
        // High adds tools that are useful for offline compression but costly
        // on the 4K real-time path. Main keeps CABAC and the low-latency
        // reference model while avoiding the High-only transform workload.
        return .main
    }
    return preferredH264Profile(lowLatency: lowLatency)
}

func preferredH264EntropyMode(
    width: UInt32,
    height: UInt32,
    contentMode: String
) -> H264EntropyModeKind {
    // The M1 Max hardware encoder was more stable with its CABAC path in the
    // 4K moving-wallpaper A/B. Keep this explicit so a future OS default
    // change cannot silently switch the production profile.
    _ = width
    _ = height
    _ = contentMode
    return .cabac
}

func preferredEncoderPreset(contentMode: String) -> EncoderPresetKind {
    contentMode.lowercased() == StreamContentMode.video.rawValue
        ? .highSpeed
        : .videoConferencing
}

func encoderCandidateOrder(
    width: UInt32,
    height: UInt32,
    contentMode: String
) -> [VideoCodecKind] {
    _ = width
    _ = height
    _ = contentMode
    return [.h264]
}

func preferredVideoCodec(width: UInt32, height: UInt32, contentMode: String) -> VideoCodecKind {
    encoderCandidateOrder(width: width, height: height, contentMode: contentMode).first ?? .h264
}

func codecParameterSetCount(_ codec: VideoCodecKind) -> Int {
    codec.parameterSetCount
}

func recoveryEncodeGateExpired(startedNs: UInt64, nowNs: UInt64, timeoutNs: UInt64) -> Bool {
    startedNs > 0 && nowNs >= startedNs && nowNs - startedNs >= timeoutNs
}

struct EncoderLatencyPolicy {
    let maxEncodeInFlight: Int
    let usesLowLatencyRateControl: Bool
    let allowFrameReordering: Bool
    let maxFrameDelayCount: Int
    let suggestedLookAheadFrameCount: Int
    let qualityHint: Float?
    let maximumRealTimeFrameRate: UInt32
}

func encoderLatencyPolicy(
    width: UInt32,
    height: UInt32,
    fps: UInt32 = 60
) -> EncoderLatencyPolicy {
    let highResolution = max(width, height) >= 2_560 && min(width, height) >= 1_440
    return EncoderLatencyPolicy(
        // Three outstanding 4K jobs preserve enough parallelism for a 60fps
        // hardware encoder while bounding the amount of pre-display work to
        // roughly one 50ms latency target. Two is sufficient below 1440p.
        maxEncodeInFlight: highResolution ? 3 : 2,
        usesLowLatencyRateControl: true,
        allowFrameReordering: false,
        maxFrameDelayCount: 0,
        suggestedLookAheadFrameCount: 0,
        // Keep the high-change 4K path inside the frame budget. This is a
        // VideoToolbox quality hint, not a bitrate change; the configured
        // bitrate still controls the wire size and quality floor.
        qualityHint: isUltraHdDimensions(width: width, height: height) ? 0.5 : nil,
        maximumRealTimeFrameRate: max(1, min(fps, 90))
    )
}

// Both capture backends deliver an IOSurface-backed bi-planar 4:2:0 surface.
// Tell VideoToolbox that this is the source format at session creation time so
// the hardware encoder can keep the native surface path on high-change 4K
// frames instead of negotiating a format on the first submission.
func encoderSourcePixelFormat() -> OSType {
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
}

func adaptiveEncoderQualityHint(
    current: Float,
    encodeOutputP95Us: UInt64,
    captureQueueWaitP95Us: UInt64,
    receiverLoss: UInt64,
    renderedFps: UInt32?,
    targetFps: UInt32
) -> Float {
    let floor: Float = 0.25
    let ceiling: Float = 0.5
    let renderBelowTarget = renderedFps.map {
        $0 > 0 && $0 * 10 < max(1, targetFps) * 9
    } ?? false
    let overloaded = encodeOutputP95Us > 20_000
        || captureQueueWaitP95Us > 8_000
        || receiverLoss > 0
        || renderBelowTarget
    if overloaded {
        // Drop gradually. The old one-shot -0.25 cut floored quality on a
        // single bad second (one receiver gap), and recovery needed a
        // PERFECT window to start — a 55-of-60fps viewer never qualified,
        // so long sessions spent the rest of their lifetime at 0.55x bitrate.
        return max(floor, current - 0.10)
    }

    // Recovery uses the same 90% render band as the overload detector so a
    // slightly-under-target viewer can still climb back after a cut.
    let renderAtOrAboveBand = renderedFps.map {
        $0 > 0 && $0 * 10 >= max(1, targetFps) * 9
    } ?? false
    let hasHeadroom = encodeOutputP95Us > 0
        && encodeOutputP95Us <= 13_000
        && captureQueueWaitP95Us <= 2_000
        && receiverLoss == 0
        && renderAtOrAboveBand
    guard hasHeadroom else { return current }
    let rounded = (Double(current) + 0.05) * 100.0
    return min(ceiling, Float(rounded.rounded() / 100.0))
}

// VideoToolbox quality is not a runtime-mutable property on every hardware
// encoder. Keep a deterministic bitrate mapping alongside the hint so a
// rejected quality-property update still produces a real spatial-quality
// reduction on the wire. Normal quality (0.50) preserves the existing target;
// the low band (0.25) spends 55% of that target and intermediate steps are
// restored gradually as headroom returns.
func adaptiveQualityBitrateScale(qualityHint: Float) -> Double {
    let normalized = max(0.0, min(1.0, (Double(qualityHint) - 0.25) / 0.25))
    return 0.55 + (normalized * 0.45)
}

/// Bitrate level below the one that just collapsed the link. After a confirmed
/// congestion cut the controller may raise back toward this level but not
/// beyond it — without the memory it accelerated straight back into the same
/// wall and the link sawtoothed raise→collapse every few seconds while the
/// content kept demanding more (observed on a marginal Wi-Fi link 2026-09-08).
func adaptiveRaiseCeilingAfterCongestion(failingBitrate: Int) -> Int {
    max(1, Int(Double(max(1, failingBitrate)) * 0.90))
}

/// Relax the congestion-cut ceiling by 10% per 30 consecutive clean windows,
/// never above the policy's global ceiling. An inactive ceiling (0) stays
/// inactive; the streak accounting belongs to the caller.
func nextAdaptiveRaiseCeiling(
    currentCeiling: Int,
    cleanStreak: Int,
    globalCeiling: Int
) -> Int {
    guard currentCeiling > 0 else { return 0 }
    guard cleanStreak >= 30 else { return currentCeiling }
    return min(globalCeiling, Int(Double(currentCeiling) * 1.10))
}

enum EncoderBitrateApplicationRoute: Equatable {
    case unavailable
    case single
    case split
}

/// Select the live encoder that owns the bitrate budget. A split pipeline is
/// authoritative when present because its aggregate target must be divided
/// between both tile encoders rather than applied to a stale single session.
func encoderBitrateApplicationRoute(
    hasSingleSession: Bool,
    hasSplitPipeline: Bool
) -> EncoderBitrateApplicationRoute {
    if hasSplitPipeline { return .split }
    if hasSingleSession { return .single }
    return .unavailable
}

/// Keep single and split encoders on the same one-second adaptation cadence.
func shouldRunAdaptiveBitrate(encodedFrames: Int64, fps: UInt32) -> Bool {
    encodedFrames > 0 && encodedFrames % Int64(max(1, fps)) == 0
}

/// Convert the Host's intervention slider into the same quality band used by
/// the automatic controller. Zero releases the session back to Auto; the
/// manual range intentionally stops at the normal 0.50 hint so an operator
/// can trade spatial quality for frame continuity without asking this encoder
/// path to exceed its proven low-latency budget.
func manualQualityHintFromSliderPercent(_ percent: Int32) -> Float? {
    guard percent == 0 || (25...50).contains(percent) else { return nil }
    return percent == 0 ? nil : Float(percent) / 100.0
}

func encodeInFlightLimit(width: UInt32, height: UInt32) -> Int {
    encoderLatencyPolicy(width: width, height: height).maxEncodeInFlight
}

func shouldStartNetworkRecovery(
    awaitingKeyframe: Bool,
    keyframeInFlight: Bool,
    keyframeQueued: Bool
) -> Bool {
    !awaitingKeyframe && !keyframeInFlight && !keyframeQueued
}

func shouldRecoverAfterNetworkOverflow(
    incomingIsKeyframe: Bool,
    keyframeQueued: Bool,
    keyframeInFlight: Bool = false
) -> Bool {
    !incomingIsKeyframe && !keyframeQueued && !keyframeInFlight
}

func shouldPrioritizeNetworkKeyframe(isKeyframe: Bool) -> Bool {
    isKeyframe
}

/// Keep the media queue behind an encoded keyframe until that boundary has
/// actually been written. VideoToolbox callbacks can finish older deltas
/// after the keyframe callback; allowing those deltas into the queue early
/// sends an older reference chain after the recovery boundary.
func networkAwaitingKeyframeAfterEnqueue(
    currentAwaitingKeyframe: Bool,
    isKeyframe: Bool,
    isRecoveryKeyframe: Bool
) -> Bool {
    // Periodic encoder IDRs are normal video traffic. Only a requested
    // recovery IDR opens a send-side boundary; making every GOP keyframe set
    // this flag would discard the following deltas at every GOP interval.
    currentAwaitingKeyframe || isRecoveryKeyframe
}

func networkAwaitingKeyframeAfterSend(
    currentAwaitingKeyframe: Bool,
    isKeyframe: Bool,
    isRecoveryKeyframe: Bool,
    sendSucceeded: Bool
) -> Bool {
    if (isKeyframe || isRecoveryKeyframe) && sendSucceeded {
        return false
    }
    return currentAwaitingKeyframe
}

/// Codec configuration is a decoder setup boundary, not a per-GOP marker.
/// Recovery requests clear `csdSent`, so the next recovery IDR still carries
/// SPS/PPS without forcing the viewer to discard deltas for every normal GOP.
func shouldSendCodecConfig(csdSent: Bool) -> Bool {
    !csdSent
}

func shouldOffloadEncodedSample(width: UInt32, height: UInt32) -> Bool {
    max(width, height) >= 2_560 && min(width, height) >= 1_440
}
