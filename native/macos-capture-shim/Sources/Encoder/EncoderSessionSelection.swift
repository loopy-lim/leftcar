import Foundation
import VideoToolbox
import CoreMedia

enum EncoderPrepareStrategy: Equatable {
    case eager
    case deferToFirstFrame
}

enum EncoderStartupFailureAction: Equatable {
    case retryWithStagingInput
    case fallbackToNextPolicy
    case recoverCurrentEncoder
}

struct EncoderSessionPolicy: Equatable {
    let mode: EncoderMode
    let codec: VideoCodecKind
}

let phaseARTVCH264EncoderID = "com.apple.videotoolbox.videoencoder.h264.rtvc"

func phaseARTVCH264EncoderVerified(
    policy: EncoderSessionPolicy,
    encoderIDStatus: OSStatus,
    encoderID: String?,
    hardwareStatus: OSStatus,
    hardware: Bool?
) -> Bool {
    policy.mode == .rtvc
        && policy.codec == .h264
        && encoderIDStatus == noErr
        && encoderID == phaseARTVCH264EncoderID
        && hardwareEncoderVerified(
            queryStatus: hardwareStatus,
            queriedHardware: hardware,
            requireHardware: true
        )
}

func encoderPrepareStrategy(mode: EncoderMode) -> EncoderPrepareStrategy {
    mode == .ave ? .deferToFirstFrame : .eager
}

func encoderStartupInFlightLimit(
    mode: EncoderMode,
    outputConfirmed: Bool,
    configuredLimit: Int
) -> Int {
    mode == .ave && !outputConfirmed ? 1 : max(1, configuredLimit)
}

func encoderStartupFailureAction(
    mode: EncoderMode,
    encodedOutputCount: Int64,
    inputStagingEnabled: Bool,
    hasNextPolicy: Bool
) -> EncoderStartupFailureAction {
    guard encodedOutputCount == 0 else {
        return .recoverCurrentEncoder
    }
    if mode == .ave && !inputStagingEnabled {
        return .retryWithStagingInput
    }
    return hasNextPolicy ? .fallbackToNextPolicy : .recoverCurrentEncoder
}

struct EncoderCandidateDescriptor: Equatable {
    let id: String
    let codec: VideoCodecKind
    let hardware: Bool
    let performanceRating: Int
}

enum EncoderSpecificationEntry: Equatable {
    case requireHardware
    case encoderID(String)
    case enableLowLatencyRateControl
}

enum EncoderOptionalProperty: String, CaseIterable {
    case prioritizeSpeed = "PrioritizeEncodingSpeedOverQuality"
    case maximumRealTimeFrameRate = "MaximumRealTimeFrameRate"
    case suggestedLookAheadFrameCount = "SuggestedLookAheadFrameCount"
    case maximizePowerEfficiency = "MaximizePowerEfficiency"
    case quality = "Quality"
}

func encoderSessionPolicies(
    width: UInt32,
    height: UInt32,
    contentMode: String
) -> [EncoderSessionPolicy] {
    let isUltraHD = max(width, height) >= 3_840 && min(width, height) >= 2_160
    if isUltraHD && contentMode.lowercased() == StreamContentMode.video.rawValue {
        return [
            EncoderSessionPolicy(mode: .rtvc, codec: .h264),
            EncoderSessionPolicy(mode: .ave, codec: .h264),
        ]
    }
    return [EncoderSessionPolicy(mode: .rtvc, codec: .h264)]
}

func eligibleEncoderSessionPolicies(
    _ policies: [EncoderSessionPolicy],
    unavailableModes: Set<EncoderMode>
) -> [EncoderSessionPolicy] {
    policies.filter { !unavailableModes.contains($0.mode) }
}

func preferredHardwareEncoderID(
    codec: VideoCodecKind,
    candidates: [EncoderCandidateDescriptor]
) -> String? {
    candidates
        .filter { $0.codec == codec && $0.hardware }
        .sorted {
            if $0.performanceRating == $1.performanceRating { return $0.id < $1.id }
            return $0.performanceRating > $1.performanceRating
        }
        .first?.id
}

func encoderSpecificationPlan(
    mode: EncoderMode,
    encoderID: String?
) -> [EncoderSpecificationEntry]? {
    switch mode {
    case .ave:
        guard let encoderID else { return nil }
        return [.requireHardware, .encoderID(encoderID)]
    case .rtvc:
        return [.requireHardware, .enableLowLatencyRateControl]
    }
}

func hardwareEncoderVerified(
    queryStatus: OSStatus,
    queriedHardware: Bool?,
    requireHardware: Bool
) -> Bool {
    if queryStatus == noErr {
        return queriedHardware == true
    }
    return queryStatus == kVTPropertyNotSupportedErr && requireHardware
}

func leftcarPerfLogLine(
    captureCallbacks: Int64,
    encodeOutputCallbacks: Int64,
    captureFps: UInt32,
    encodeOutputFps: UInt32,
    encodeOutputIntervalP50Us: UInt64,
    encodeOutputIntervalP95Us: UInt64,
    encodeOutputP95Us: UInt64,
    inputPreparationP95Us: UInt64,
    queueOldestUs: UInt64,
    encoderWatchdogRestarts: Int64,
    encoderWatchdogTerminations: Int64,
    encoderLateCallbacks: Int64,
    encoderWatchdogOldestUs: UInt64,
    encoderMode: String,
    encoderID: String
) -> String {
    "LeftcarPerf captureCallbacks=\(captureCallbacks) "
        + "encodeOutputCallbacks=\(encodeOutputCallbacks) "
        + "captureFps=\(captureFps) encodeOutputFps=\(encodeOutputFps) "
        + "encodeOutputIntervalP50Us=\(encodeOutputIntervalP50Us) "
        + "encodeOutputIntervalP95Us=\(encodeOutputIntervalP95Us) "
        + "encodeOutputP95Us=\(encodeOutputP95Us) "
        + "inputPreparationP95Us=\(inputPreparationP95Us) queueOldestUs=\(queueOldestUs) "
        + "encoderWatchdogRestarts=\(encoderWatchdogRestarts) "
        + "encoderWatchdogTerminations=\(encoderWatchdogTerminations) "
        + "encoderLateCallbacks=\(encoderLateCallbacks) "
        + "encoderWatchdogOldestUs=\(encoderWatchdogOldestUs) "
        + "encoderMode=\(encoderMode) encoderID=\(encoderID)"
}

func optionalPropertyPlan(
    mode: EncoderMode,
    supported: Set<EncoderOptionalProperty>
) -> [EncoderOptionalProperty] {
    guard mode == .ave else { return [] }
    // AVE advertises and accepts this key, but the first 4K H.264 encode then
    // fails with kVTSessionMalfunctionErr. Keep it out of the AVE plan.
    return EncoderOptionalProperty.allCases.filter {
        supported.contains($0) && $0 != .suggestedLookAheadFrameCount
    }
}

struct EncoderConfigurationReport {
    let mode: EncoderMode
    private(set) var applied: [String] = []
    private(set) var suppressed: [String] = []
    private(set) var unsupported: [String] = []
    private(set) var rejected: [String] = []

    mutating func recordApplied(_ key: String) { applied.append(key) }
    mutating func recordSuppressed(_ key: String) { suppressed.append(key) }
    mutating func recordUnsupported(_ key: String) { unsupported.append(key) }
    mutating func recordRejected(_ key: String, status: OSStatus) {
        rejected.append("\(key)=\(status)")
    }
}

func requiredH264Profile(mode: EncoderMode) -> H264ProfileKind {
    _ = mode
    return .main
}

func requiredH264EntropyMode(mode: EncoderMode) -> H264EntropyModeKind {
    _ = mode
    return .cabac
}

func initialEncoderQuality(
    mode: EncoderMode,
    width: UInt32,
    height: UInt32
) -> Float? {
    let isUltraHD = max(width, height) >= 3_840 && min(width, height) >= 2_160
    return mode == .ave && isUltraHD ? 0.25 : nil
}

 func availableEncoderDescriptors() -> [EncoderCandidateDescriptor] {
    var rawList: CFArray?
    guard VTCopyVideoEncoderList(nil, &rawList) == noErr,
          let rows = rawList as? [[String: Any]] else {
        return []
    }
    return rows.compactMap { row in
        guard let id = row[kVTVideoEncoderList_EncoderID as String] as? String,
              let codecNumber = row[kVTVideoEncoderList_CodecType as String] as? NSNumber else {
            return nil
        }
        let codecType = CMVideoCodecType(codecNumber.uint32Value)
        let codec: VideoCodecKind
        switch codecType {
        case kCMVideoCodecType_H264: codec = .h264
        case kCMVideoCodecType_HEVC: codec = .hevc
        default: return nil
        }
        return EncoderCandidateDescriptor(
            id: id,
            codec: codec,
            hardware: (row[kVTVideoEncoderList_IsHardwareAccelerated as String] as? NSNumber)?.boolValue ?? false,
            performanceRating: (row[kVTVideoEncoderList_PerformanceRating as String] as? NSNumber)?.intValue ?? 0
        )
    }
}
