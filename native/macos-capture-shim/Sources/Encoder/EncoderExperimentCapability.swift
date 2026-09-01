import Foundation
import CoreVideo
import VideoToolbox

private struct BaseEncoderExperimentCapability {
    let verifiedRTVCH264: Bool
    let supportsBaseFrameQP: Bool
    let hasEncoderPixelBufferPool: Bool
}

private let encoderExperimentCapabilityLock = NSLock()
private var encoderExperimentCapabilityJSONCache: String?

private func probeBaseEncoderExperimentCapability() -> BaseEncoderExperimentCapability {
    let unavailable = BaseEncoderExperimentCapability(
        verifiedRTVCH264: false,
        supportsBaseFrameQP: false,
        hasEncoderPixelBufferPool: false
    )
    let specification: [String: Any] = [
        kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder as String: true,
        kVTVideoEncoderSpecification_EnableLowLatencyRateControl as String: true,
    ]
    var session: VTCompressionSession?
    let createStatus = VTCompressionSessionCreate(
        allocator: nil,
        width: 3_840,
        height: 2_160,
        codecType: kCMVideoCodecType_H264,
        encoderSpecification: specification as CFDictionary,
        imageBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: encoderSourcePixelFormat(),
        ] as CFDictionary,
        compressedDataAllocator: nil,
        outputCallback: nil,
        refcon: nil,
        compressionSessionOut: &session
    )
    guard createStatus == noErr, let session else { return unavailable }
    defer { VTCompressionSessionInvalidate(session) }

    let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(session)
    guard prepareStatus == noErr else { return unavailable }

    var encoderIDValue: Unmanaged<CFTypeRef>?
    let encoderIDStatus = VTSessionCopyProperty(
        session,
        key: kVTCompressionPropertyKey_EncoderID,
        allocator: nil,
        valueOut: &encoderIDValue
    )
    let encoderID = encoderIDStatus == noErr
        ? encoderIDValue?.takeRetainedValue() as? String
        : nil

    var hardwareValue: Unmanaged<CFTypeRef>?
    let hardwareStatus = VTSessionCopyProperty(
        session,
        key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
        allocator: nil,
        valueOut: &hardwareValue
    )
    let hardware = hardwareStatus == noErr
        ? (hardwareValue?.takeRetainedValue() as? NSNumber)?.boolValue
        : nil
    let verifiedRTVCH264 = phaseARTVCH264EncoderVerified(
        policy: EncoderSessionPolicy(mode: .rtvc, codec: .h264),
        encoderIDStatus: encoderIDStatus,
        encoderID: encoderID,
        hardwareStatus: hardwareStatus,
        hardware: hardware
    )

    let supportsBaseFrameQP: Bool
    if #available(macOS 12.0, *) {
        var value: Unmanaged<CFTypeRef>?
        let status = VTSessionCopyProperty(
            session,
            key: kVTCompressionPropertyKey_SupportsBaseFrameQP,
            allocator: nil,
            valueOut: &value
        )
        supportsBaseFrameQP = status == noErr
            && (value?.takeRetainedValue() as? NSNumber)?.boolValue == true
    } else {
        supportsBaseFrameQP = false
    }

    return BaseEncoderExperimentCapability(
        verifiedRTVCH264: verifiedRTVCH264,
        supportsBaseFrameQP: supportsBaseFrameQP,
        hasEncoderPixelBufferPool: VTCompressionSessionGetPixelBufferPool(session) != nil
    )
}

func encoderExperimentCapabilityJSON() -> String {
    encoderExperimentCapabilityLock.lock()
    defer { encoderExperimentCapabilityLock.unlock() }
    if let cached = encoderExperimentCapabilityJSONCache { return cached }

    let base = probeBaseEncoderExperimentCapability()
    let capabilities = encoderExperimentCapabilityEntries(
        verifiedRTVCH264: base.verifiedRTVCH264,
        supportsBaseFrameQP: base.supportsBaseFrameQP,
        hasEncoderPixelBufferPool: base.hasEncoderPixelBufferPool,
        dualAveTilePairAvailable: dualAveTileEncoderPairAvailable()
    )
    let json: String
    if let data = try? JSONSerialization.data(withJSONObject: capabilities),
       let encoded = String(data: data, encoding: .utf8) {
        json = encoded
    } else {
        json = "[]"
    }
    encoderExperimentCapabilityJSONCache = json
    return json
}

func encoderExperimentLabel(_ experiment: EncoderExperiment) -> String {
    switch experiment {
    case .auto: return "Automatic"
    case .rateControl: return "Rate control"
    case .adaptiveQp: return "Adaptive QP"
    case .encoderPool: return "Encoder pool"
    case .splitHorizontal: return "Split horizontal"
    case .splitVertical: return "Split vertical"
    }
}

func encoderExperimentHint(_ experiment: EncoderExperiment) -> String {
    switch experiment {
    case .auto: return "Host-selected encoder policy"
    case .rateControl: return "Fixed rate-control encoder"
    case .adaptiveQp: return "Adaptive base-frame quantizer"
    case .encoderPool: return "Pooled encoder input buffers"
    case .splitVertical:
        return "4K over two hardware encoders and two consecutive UDP ports"
    case .splitHorizontal: return "Reserved"
    }
}

@_cdecl("leftcar_capture_encoder_experiments_v1")
public func leftcarCaptureEncoderExperimentsV1() -> UnsafeMutablePointer<CChar> {
    let json = encoderExperimentCapabilityJSON()
    setLastError("")
    return UnsafeMutablePointer<CChar>(strdup(json))
}
