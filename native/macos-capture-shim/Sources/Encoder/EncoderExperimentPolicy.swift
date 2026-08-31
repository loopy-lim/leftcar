import Foundation

enum EncoderExperimentParseResult: Equatable {
    case success(EncoderExperiment)
    case failure(String)
}

func parseEncoderExperimentName(_ raw: String?) -> EncoderExperimentParseResult {
    guard let raw else { return .success(.auto) }
    guard let experiment = EncoderExperiment.parse(raw) else {
        return .failure("unknown encoder experiment: \(raw)")
    }
    return .success(experiment)
}

func parseEncoderExperimentCString(
    _ raw: UnsafePointer<CChar>?
) -> EncoderExperimentParseResult {
    guard let raw else { return .success(.auto) }
    guard let decoded = String(validatingUTF8: raw) else {
        return .failure("unknown encoder experiment: invalid UTF-8")
    }
    return parseEncoderExperimentName(decoded)
}

enum EncoderExperimentStartupDecision: Equatable {
    case success(applied: EncoderExperiment)
    case failure(String)
}

func encoderExperimentStartupDecision(
    requested: EncoderExperiment,
    rtvcHardwareAvailable: Bool,
    supportsBaseFrameQP: Bool,
    hasEncoderPixelBufferPool: Bool
) -> EncoderExperimentStartupDecision {
    guard rtvcHardwareAvailable else {
        return .failure("RTVC hardware encoder is unavailable")
    }
    let applied = appliedEncoderExperiment(requested)
    switch applied {
    case .auto, .rateControl:
        return .success(applied: .rateControl)
    case .adaptiveQp:
        return supportsBaseFrameQP
            ? .success(applied: .adaptiveQp)
            : .failure("adaptiveQp requires SupportsBaseFrameQP")
    case .encoderPool:
        return hasEncoderPixelBufferPool
            ? .success(applied: .encoderPool)
            : .failure("encoderPool requires a VideoToolbox pixel buffer pool")
    case .splitHorizontal, .splitVertical:
        return .failure("unknown encoder experiment: \(requested.rawValue)")
    }
}

func encoderExperimentStartupDecision(
    requested: EncoderExperiment,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    mediaTransport: String,
    hasEncoderPixelBufferPool: Bool,
    splitDiagnosticEnabled: Bool
) -> EncoderExperimentStartupDecision {
    if requested == .splitVertical {
        guard splitDiagnosticEnabled else {
            return .failure("splitVertical diagnostics are disabled")
        }
        guard width == 3_840,
              height == 2_160,
              fps == 60,
              mediaTransport == "udp",
              hasEncoderPixelBufferPool else {
            return .failure(
                "splitVertical requires 3840x2160 at 60fps over direct UDP"
            )
        }
        return .success(applied: .splitVertical)
    }
    return encoderExperimentStartupDecision(
        requested: requested,
        rtvcHardwareAvailable: true,
        supportsBaseFrameQP: true,
        hasEncoderPixelBufferPool: hasEncoderPixelBufferPool
    )
}

func advertisedEncoderExperiments(
    rtvcHardwareAvailable: Bool,
    supportsBaseFrameQP: Bool,
    hasEncoderPixelBufferPool: Bool,
    dualAveTilePairAvailable _: Bool
) -> [EncoderExperiment] {
    guard rtvcHardwareAvailable else { return [] }
    var result: [EncoderExperiment] = [.auto, .rateControl]
    if supportsBaseFrameQP { result.append(.adaptiveQp) }
    if hasEncoderPixelBufferPool { result.append(.encoderPool) }
    return result
}

func encoderExperimentCapabilityEntries(
    verifiedRTVCH264: Bool,
    supportsBaseFrameQP: Bool,
    hasEncoderPixelBufferPool: Bool,
    dualAveTilePairAvailable: Bool
) -> [[String: Any]] {
    advertisedEncoderExperiments(
        rtvcHardwareAvailable: verifiedRTVCH264,
        supportsBaseFrameQP: supportsBaseFrameQP,
        hasEncoderPixelBufferPool: hasEncoderPixelBufferPool,
        dualAveTilePairAvailable: dualAveTilePairAvailable
    ).map { experiment in
        [
            "id": experiment.rawValue,
            "label": encoderExperimentLabel(experiment),
            "hint": encoderExperimentHint(experiment),
            "requiresReconnect": true,
        ]
    }
}

func encoderExperimentStatsFields(
    requested: EncoderExperiment,
    applied: EncoderExperiment,
    fallbackReason: String?,
    baseFrameQp: Int32?,
    baseFrameQpChanges: Int64,
    encoderFrameDrops: Int64,
    encoderFrameDropFps: UInt32,
    validEncodeOutputFps: UInt32,
    encodeSubmitCallP50Us: UInt64,
    encodeSubmitCallP95Us: UInt64,
    encoderCallbackP50Us: UInt64,
    encoderCallbackP95Us: UInt64,
    packetizationInFlight: Int
) -> [String: Any] {
    let reportedFallbackReason: Any = requested == .auto
        ? (fallbackReason.map { $0 as Any } ?? NSNull())
        : NSNull()
    let reportedBaseFrameQp: Any = applied == .adaptiveQp
        ? (baseFrameQp.map { NSNumber(value: $0) } ?? NSNull())
        : NSNull()
    return [
        "encoderExperimentRequested": requested.rawValue,
        "encoderExperimentApplied": applied.rawValue,
        "encoderExperimentFallbackReason": reportedFallbackReason,
        "baseFrameQp": reportedBaseFrameQp,
        "baseFrameQpChanges": baseFrameQpChanges,
        "encoderFrameDrops": encoderFrameDrops,
        "encoderFrameDropFps": encoderFrameDropFps,
        "validEncodeOutputFps": validEncodeOutputFps,
        "encodeSubmitCallP50Us": encodeSubmitCallP50Us,
        "encodeSubmitCallP95Us": encodeSubmitCallP95Us,
        "encoderCallbackP50Us": encoderCallbackP50Us,
        "encoderCallbackP95Us": encoderCallbackP95Us,
        "packetizationInFlight": packetizationInFlight,
    ]
}

func encoderSessionPolicies(for experiment: EncoderExperiment) -> [EncoderSessionPolicy] {
    switch appliedEncoderExperiment(experiment) {
    case .auto, .rateControl, .adaptiveQp, .encoderPool:
        return [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
    case .splitHorizontal, .splitVertical:
        return []
    }
}

struct EncoderFramePropertyPlan: Equatable {
    let forceKeyframe: Bool
    let baseFrameQp: Int32?
}

func encoderFramePropertyPlan(
    appliedExperiment: EncoderExperiment,
    requestKeyframe: Bool,
    currentBaseFrameQp: Int32
) -> EncoderFramePropertyPlan {
    EncoderFramePropertyPlan(
        forceKeyframe: requestKeyframe,
        baseFrameQp: appliedExperiment == .adaptiveQp
            ? min(42, max(26, currentBaseFrameQp))
            : nil
    )
}
