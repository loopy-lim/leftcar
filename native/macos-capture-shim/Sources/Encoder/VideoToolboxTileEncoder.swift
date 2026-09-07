import Foundation
import CoreMedia
import CoreVideo
import VideoToolbox

struct TileEncodeRequest {
    let side: TileSide
    let frameSequence: UInt64
    let pts: CMTime
    let duration: CMTime
    let captureNs: UInt64
    let captureWallMs: UInt64
    let recoveryGeneration: UInt64
    let forceKeyframe: Bool
    let pixelBuffer: CVPixelBuffer
}

struct TileEncodedSample {
    let side: TileSide
    let frameSequence: UInt64
    let pts: CMTime
    let captureNs: UInt64
    let captureWallMs: UInt64
    let sampleBuffer: CMSampleBuffer
    let isKeyframe: Bool
    let recoveryGeneration: UInt64
    let requestedKeyframe: Bool
}

enum TileEncoderError: Error {
    case create(OSStatus)
    case property(String, OSStatus)
    case prepare(OSStatus)
    case submit(OSStatus)
    case callback(OSStatus)
    case dropped
    case missingSample
}

enum SplitTileEntropyMode: Equatable {
    case cabac
    case cavlc
}

enum SplitTileEncoderBackend: String, Equatable {
    case rtvc
    case ave

    var usesLowLatencyRateControl: Bool { self == .rtvc }
    var requestedEncoderID: String? {
        self == .ave ? "com.apple.videotoolbox.videoencoder.ave.avc" : nil
    }
    var preparesEagerly: Bool { self == .rtvc }
}

func splitTileEntropyMode(prioritizeSpeed: Bool) -> SplitTileEntropyMode {
    prioritizeSpeed ? .cavlc : .cabac
}

func splitTileHardwareEncoderVerified(
    queryStatus: OSStatus,
    queriedHardware: Bool?
) -> Bool {
    hardwareEncoderVerified(
        queryStatus: queryStatus,
        queriedHardware: queriedHardware,
        requireHardware: true
    )
}

/// Submission PTS for one `VTCompressionSessionEncodeFrame` call.
///
/// Apple's VideoToolbox contract (VTCompressionSessionEncodeFrame,
/// developer.apple.com/documentation/videotoolbox) requires every
/// presentation timestamp in a session to be strictly greater than the
/// previous one. Split recovery replays the newest retained capture frame as
/// the paired-IDR carrier, so its source PTS can repeat or undercut a PTS the
/// same live session already encoded (recovery keeps the pipeline's encoder
/// sessions). The submission clock must therefore stay strictly monotonic for
/// the replay AND for every subsequent normal capture whose source PTS did
/// not clear the replayed clock. The request keeps the original
/// `captureNs`/`captureWallMs` so age accounting stays honest; only the PTS
/// handed to VideoToolbox is adjusted. A fresh encoder instance owns a fresh
/// VTCompressionSession, so its clock starts empty.
func nextStrictlyMonotonicSubmissionPTS(
    sourcePTS: CMTime,
    lastSubmittedPTS: CMTime?
) -> CMTime {
    guard let last = lastSubmittedPTS, last.isValid else {
        return sourcePTS
    }
    guard CMTimeCompare(sourcePTS, last) > 0 else {
        // Replayed carrier (duplicate PTS) or a later frame whose source
        // clock did not clear the replayed submission: step exactly one tick
        // forward in the same timescale/epoch so the next real capture stays
        // as close to its source PTS as possible.
        var stepped = last
        stepped.value &+= 1
        return stepped
    }
    return sourcePTS
}

final class VideoToolboxTileEncoder {
    let side: TileSide
    let encoderID: String
    let hardwareAccelerated: Bool
    let backend: SplitTileEncoderBackend

    private let session: VTCompressionSession
    private let lock = NSLock()
    private var invalidated = false
    // Strictly increasing encode submission clock for this one
    // VTCompressionSession. A recreated DualEncoderPipeline creates fresh
    // encoder instances, which resets this clock per encoder session.
    private var lastSubmittedPTS: CMTime?

    init(
        side: TileSide,
        width: Int32 = 1_920,
        height: Int32 = 2_160,
        fps: UInt32 = 60,
        bitrate: Int,
        backend: SplitTileEncoderBackend = .rtvc
    ) throws {
        self.side = side
        self.backend = backend
        var compressionSession: VTCompressionSession?
        var specification: [String: Any] = [
            kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder as String: true,
        ]
        if #available(macOS 11.3, *), backend.usesLowLatencyRateControl {
            specification[
                kVTVideoEncoderSpecification_EnableLowLatencyRateControl as String
            ] = true
        }
        if let requestedEncoderID = backend.requestedEncoderID {
            specification[kVTVideoEncoderSpecification_EncoderID as String] = requestedEncoderID
        }
        let createStatus = VTCompressionSessionCreate(
            allocator: nil,
            width: width,
            height: height,
            codecType: kCMVideoCodecType_H264,
            encoderSpecification: specification as CFDictionary,
            imageBufferAttributes: [
                kCVPixelBufferPixelFormatTypeKey as String:
                    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                kCVPixelBufferWidthKey as String: Int(width),
                kCVPixelBufferHeightKey as String: Int(height),
                kCVPixelBufferMetalCompatibilityKey as String: true,
                kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
            ] as CFDictionary,
            compressedDataAllocator: nil,
            outputCallback: nil,
            refcon: nil,
            compressionSessionOut: &compressionSession
        )
        guard createStatus == noErr, let compressionSession else {
            throw TileEncoderError.create(createStatus)
        }
        self.session = compressionSession

        do {
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_RealTime,
                    value: true as CFBoolean
                ),
                "RealTime"
            )
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_AllowFrameReordering,
                    value: false as CFBoolean
                ),
                "AllowFrameReordering"
            )
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_ProfileLevel,
                    value: kVTProfileLevel_H264_Main_AutoLevel
                ),
                "ProfileLevel"
            )
            let entropyMode = splitTileEntropyMode(prioritizeSpeed: true)
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_H264EntropyMode,
                    value: entropyMode == .cavlc
                        ? kVTH264EntropyMode_CAVLC
                        : kVTH264EntropyMode_CABAC
                ),
                "H264EntropyMode"
            )
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_ExpectedFrameRate,
                    value: Int32(fps) as CFNumber
                ),
                "ExpectedFrameRate"
            )
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_AverageBitRate,
                    value: max(1_000_000, bitrate) as CFNumber
                ),
                "AverageBitRate"
            )
            let hardLimitBytes = max(1, Int(Double(max(1_000_000, bitrate)) / 8.0 * 1.25))
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_DataRateLimits,
                    value: [hardLimitBytes, 1] as CFArray
                ),
                "DataRateLimits"
            )
            try Self.require(
                VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    value: 3_600 as CFNumber
                ),
                "MaxKeyFrameInterval"
            )
            if #available(macOS 11.0, *) {
                _ = VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
                    value: true as CFBoolean
                )
            }
            if #available(macOS 15.0, *) {
                _ = VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_MaximumRealTimeFrameRate,
                    value: Int32(fps) as CFNumber
                )
                if backend != .ave {
                    _ = VTSessionSetProperty(
                        compressionSession,
                        key: kVTCompressionPropertyKey_SuggestedLookAheadFrameCount,
                        value: 0 as CFNumber
                    )
                }
            }
            if backend.preparesEagerly {
                let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(compressionSession)
                guard prepareStatus == noErr else {
                    throw TileEncoderError.prepare(prepareStatus)
                }
            }
        } catch {
            VTCompressionSessionInvalidate(compressionSession)
            throw error
        }

        var idValue: Unmanaged<CFTypeRef>?
        let idStatus = VTSessionCopyProperty(
            compressionSession,
            key: kVTCompressionPropertyKey_EncoderID,
            allocator: nil,
            valueOut: &idValue
        )
        encoderID = idStatus == noErr
            ? (idValue?.takeRetainedValue() as? String ?? "unknown")
            : "unknown"
        var hardwareValue: Unmanaged<CFTypeRef>?
        let hardwareStatus = VTSessionCopyProperty(
            compressionSession,
            key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
            allocator: nil,
            valueOut: &hardwareValue
        )
        let queriedHardware = hardwareStatus == noErr
            ? (hardwareValue?.takeRetainedValue() as? NSNumber)?.boolValue
            : nil
        hardwareAccelerated = splitTileHardwareEncoderVerified(
            queryStatus: hardwareStatus,
            queriedHardware: queriedHardware
        )
        guard hardwareAccelerated else {
            VTCompressionSessionInvalidate(compressionSession)
            throw TileEncoderError.property("UsingHardwareAcceleratedVideoEncoder", hardwareStatus)
        }
    }

    func submit(
        _ request: TileEncodeRequest,
        completion: @escaping (Result<TileEncodedSample, TileEncoderError>) -> Void
    ) {
        lock.lock()
        let usable = !invalidated
        // Enforce the VTCompressionSession strict-monotonic PTS contract here,
        // at the single point every tile submission crosses (normal captures
        // and replayed recovery carriers alike).
        let submissionPTS = nextStrictlyMonotonicSubmissionPTS(
            sourcePTS: request.pts,
            lastSubmittedPTS: lastSubmittedPTS
        )
        if usable {
            lastSubmittedPTS = submissionPTS
        }
        lock.unlock()
        guard usable else {
            completion(.failure(.submit(kVTInvalidSessionErr)))
            return
        }

        var properties: [String: Any] = [:]
        if request.forceKeyframe {
            properties[kVTEncodeFrameOptionKey_ForceKeyFrame as String] = true
        }
        let frameProperties: CFDictionary? = properties.isEmpty
            ? nil
            : properties as CFDictionary
        var infoFlags: VTEncodeInfoFlags = []
        let status = VTCompressionSessionEncodeFrame(
            session,
            imageBuffer: request.pixelBuffer,
            presentationTimeStamp: submissionPTS,
            duration: request.duration,
            frameProperties: frameProperties,
            infoFlagsOut: &infoFlags
        ) { status, callbackFlags, sampleBuffer in
            guard status == noErr else {
                completion(.failure(.callback(status)))
                return
            }
            guard !callbackFlags.contains(.frameDropped) else {
                completion(.failure(.dropped))
                return
            }
            guard let sampleBuffer,
                  CMSampleBufferIsValid(sampleBuffer),
                  CMSampleBufferDataIsReady(sampleBuffer) else {
                completion(.failure(.missingSample))
                return
            }
            let attachments = CMSampleBufferGetSampleAttachmentsArray(
                sampleBuffer,
                createIfNecessary: false
            ) as? [[String: Any]]
            let notSync = attachments?.first?[
                kCMSampleAttachmentKey_NotSync as String
            ] as? Bool
            completion(
                .success(
                    TileEncodedSample(
                        side: request.side,
                        frameSequence: request.frameSequence,
                        pts: submissionPTS,
                        captureNs: request.captureNs,
                        captureWallMs: request.captureWallMs,
                        sampleBuffer: sampleBuffer,
                        isKeyframe: notSync != true,
                        recoveryGeneration: request.recoveryGeneration,
                        requestedKeyframe: request.forceKeyframe
                    )
                )
            )
        }
        if status != noErr {
            completion(.failure(.submit(status)))
        }
    }

    func updateBitrate(_ bitrate: Int) -> Bool {
        let bounded = max(1_000_000, bitrate)
        let bitrateStatus = VTSessionSetProperty(
            session,
            key: kVTCompressionPropertyKey_AverageBitRate,
            value: bounded as CFNumber
        )
        let hardLimitBytes = max(1, Int(Double(bounded) / 8.0 * 1.25))
        let rateStatus = VTSessionSetProperty(
            session,
            key: kVTCompressionPropertyKey_DataRateLimits,
            value: [hardLimitBytes, 1] as CFArray
        )
        return bitrateStatus == noErr && rateStatus == noErr
    }

    func completeFrames() -> OSStatus {
        VTCompressionSessionCompleteFrames(
            session,
            untilPresentationTimeStamp: .invalid
        )
    }

    func invalidate() {
        lock.lock()
        guard !invalidated else {
            lock.unlock()
            return
        }
        invalidated = true
        lock.unlock()
        VTCompressionSessionCompleteFrames(session, untilPresentationTimeStamp: .invalid)
        VTCompressionSessionInvalidate(session)
    }

    deinit {
        invalidate()
    }

    private static func require(_ status: OSStatus, _ property: String) throws {
        guard status == noErr else {
            throw TileEncoderError.property(property, status)
        }
    }
}
