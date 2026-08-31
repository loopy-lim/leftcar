import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import Darwin

final class CaptureSession {
     let queue = DispatchQueue(label: "leftcar.capture", qos: .userInteractive)
    // Capture callbacks only publish the newest sample here. Encoding runs on
    // its own serial queue, so a slow VideoToolbox callback cannot make
    // ScreenCaptureKit wait behind an older frame.
     let encodeQueue = DispatchQueue(label: "leftcar.encode", qos: .userInteractive)
    // 4K access units can require hundreds of MTU-sized fragments and FEC
    // shards. Prepare those packets away from the VideoToolbox callback so
    // packetization cannot delay the next encoded output callback.
     let packetizationQueue = DispatchQueue(label: "leftcar.packetization", qos: .userInteractive)
     let encodeQueueKey = DispatchSpecificKey<Void>()
     let captureLock = NSLock()
     var pendingCapture: PendingCaptureFrame?
     var pendingSplitCaptures: [PendingCaptureFrame] = []
     var encodeScheduled = false
    // VideoToolbox accepts frames asynchronously. A latest-frame slot alone
    // does not prevent its internal queue from growing, so keep a small
    // resolution-aware number of hardware encode submissions in flight and
    // retain only the newest frame while those slots are occupied.
     var configuredEncodeInFlightLimit: Int {
        encoderLatencyPolicy(width: outWidth, height: outHeight).maxEncodeInFlight
    }
    // AVE allocates its resources on the first real frame because the optional
    // prepare call malfunctions on the target Mac. Keep startup serial until
    // one output proves that the encoder is usable, then restore the normal
    // resolution-aware parallelism.
     var maxEncodeInFlight = 1
     var encodeInFlight = 0
    // Once a recovery request is consumed by VideoToolbox, do not submit
    // another delta until that IDR has reached the viewer. Otherwise the
    // encoder advances its reference chain while the IDR burst is on the
    // wire; dropping any of those deltas makes the first post-IDR frame
    // undecodable and immediately starts another recovery loop.
     var recoveryEncodeInFlight = false
     var recoveryEncodeGateStartedNs: UInt64 = 0
     var sock: Int32 = -1
     let tcpWriteLock = NSLock()
     var tcpControlBuffer = Data()
     var viewerControlToken = Data()
     let inputQueue = DispatchQueue(label: "leftcar.input", qos: .userInteractive)
     let inputLock = NSLock()
     var inputReadSource: DispatchSourceRead?
     var inputEnabled = false
     var inputBounds: CGRect?
     var lastReliableInputSequence: UInt32 = 0
     var lastPointerInputSequence: UInt32 = 0
     var pressedKeys = Set<CGKeyCode>()
     var pressedButtons = Set<CGMouseButton>()
     var lastPointerPosition = CGPoint.zero
     var horizontalScrollRemainder: Int32 = 0
     var verticalScrollRemainder: Int32 = 0
     var stream: SCStream?
     var streamHandler: CaptureOutputHandler?
     var cgStream: CGDisplayStream?
     var cgStreamAPI: LegacyCGDisplayStreamAPI?
     var session: VTCompressionSession?
     var splitPipeline: DualEncoderPipeline?
     let targetAddr: sockaddr_in
     let targetPort: UInt16
     let targetLabel: String
     let outWidth: UInt32
     let outHeight: UInt32
     let fps: UInt32
     let backend: CaptureBackendKind
     let mediaTransport: MediaTransportKind
     let contentMode: StreamContentMode
     let appliedUdpStability: AppliedUdpStability
     var udpBurstPolicyState: UdpBurstPolicyState
     var activeUdpBurstDatagrams: Int
     var activeUdpFecParityShards: Int
     var activeUdpBurstReason = UdpBurstDecision.Reason.initial.rawValue
     var performanceLogTicker: PerformanceLogTicker?
     var codecKind: VideoCodecKind = .h264
     var encoderID = "not_ready"
     var encoderHardwareAccelerated: Bool?
     var encoderPreset = "not_applied"
     var encoderProfile = "not_applied"
     var encoderMode = "not_ready"
     let requestedEncoderExperiment: EncoderExperiment
     var appliedEncoderExperimentValue: EncoderExperiment
     var encoderAppliedProperties: [String] = []
     var encoderSuppressedProperties: [String] = []
     var encoderUnsupportedProperties: [String] = []
     var encoderRejectedProperties: [String] = []
     var encoderFallbackReason: String?
    // Protected by stateLock. AVE gets one retry through a pool-backed NV12
    // surface when it rejects the IOSurface delivered directly by capture.
     var aveInputStagingEnabled = false
    // Accessed only on encodeQueue. A mode that fails before producing its
    // first output after its staging retry is excluded for the rest of this
    // capture session so setup cannot spin forever.
     var unavailableEncoderModes = Set<EncoderMode>()
    // Accessed only on encodeQueue. VideoToolbox owns submitted destination
    // buffers until their encode callbacks complete, so the pool may safely
    // issue another surface for the next in-flight frame.
     var encoderInputPool: CVPixelBufferPool?
     var pixelTransferSession: VTPixelTransferSession?
     var csdSent = false

    // The encoder callback must never wait behind network transmission. Keep at
    // most the newest encoded AU plus the latest config packet; an older AU
    // that has not reached the socket is intentionally dropped.
     let networkQueue = DispatchQueue(label: "leftcar.network", qos: .userInteractive)
     let networkLock = NSLock()
     var pendingConfig: Data?
     var pendingTileConfigs: [TileSide: Data] = [:]
    // Both transports favor the newest screen state over preserving stale
    // encoded frames. UDP gets a short recovery cushion because a keyframe is
    // sent at a higher burst rate than ordinary deltas; eight slots cover that
    // bounded burst without turning the network queue into a playback buffer.
     var maxPendingNetworkFrames: Int {
        if contentMode == .video && mediaTransport == .udp {
            // A 60fps video stream must not accumulate eight stale frames
            // while a large AU is being paced. The next recovery IDR is the
            // only safe dependency boundary after overflow.
            return 3
        }
        return mediaTransport == .udp ? 8 : 4
    }
     var pendingFrames: [PendingEncodedFrame] = []
     var pendingSplitAccessUnits: [PendingSplitAccessUnit] = []
     var networkRecoveryBoundary = NetworkRecoveryBoundaryState()
     var networkKeyframeInFlight = false
     var networkDrainScheduled = false
    // UDP datagrams are individually loss-tolerant, but a burst of many
    // fragments can overflow the Wi-Fi/AP receive queue as a group. Keep one
    // global pacing deadline across access units so a normal frame does not
    // arrive as a back-to-back burst behind the previous frame.
     var nextUdpSendNs: UInt64 = 0

     let stateLock = NSLock()
     var running = false
     var stopRequested = false
     var lifecycleState = "connecting"
     let createdNs = DispatchTime.now().uptimeNanoseconds
     var firstCaptureNs: UInt64?
     var firstEncodeNs: UInt64?
     var firstSendNs: UInt64?
     var framesEncoded: Int64 = 0
     var framesDropped: Int64 = 0
     var networkQueueDropped: Int64 = 0
     var sentDatagrams: Int64 = 0
     var sentParityDatagrams: Int64 = 0
     var captureCallbacks: Int64 = 0
     var encodeOutputCallbacks: Int64 = 0
     var encoderFrameDrops: Int64 = 0
    // Reset for every VTCompressionSession. Startup fallback decisions must
    // not use outputs produced by an older encoder attempt.
     var encoderSessionGeneration: UInt64 = 0
     var encoderSessionOutputCallbacks: Int64 = 0
     var encodeSubmitFailures: Int64 = 0
     var lastAuBytes: UInt64 = 0
     var lastAuFragments: UInt32 = 0
     var lastAuParity: UInt32 = 0
     var lastAuDatagrams: UInt32 = 0
     var lastAuExpectedDatagrams: UInt32 = 0
     var lastAuSendUs: UInt64 = 0
     var lastAuIsKeyframe = false
     var maxAuBytes: UInt64 = 0
     var maxAuFragments: UInt32 = 0
    // Frames discarded while waiting for the next independently decodable
    // IDR are expected recovery behavior, not evidence that the sender is
    // congested. Keep them in user-facing drop telemetry, but exclude them
    // from bitrate adaptation so recovery cannot ratchet the bitrate down.
     var recoveryFramesDropped: Int64 = 0
     var udpSendFailures: Int64 = 0
     var udpSendRetries: Int64 = 0
     var recoveryKeyframes: Int64 = 0
     var recoveryRequestsSuppressed: Int64 = 0
     var captureQueueDropped: Int64 = 0
     var bytesSent: Int64 = 0
     var lastCaptureToEncodeUs: UInt64 = 0
     var maxCaptureToEncodeUs: UInt64 = 0
     var lastCaptureQueueWaitUs: UInt64 = 0
     var maxCaptureQueueWaitUs: UInt64 = 0
     var lastInputPreparationUs: UInt64 = 0
     var maxInputPreparationUs: UInt64 = 0
     var lastEncodeSubmitCallUs: UInt64 = 0
     var maxEncodeSubmitCallUs: UInt64 = 0
     var lastEncoderCallbackUs: UInt64 = 0
     var maxEncoderCallbackUs: UInt64 = 0
     var lastEncodeOutputUs: UInt64 = 0
     var maxEncodeOutputUs: UInt64 = 0
     var packetizationAdmissionState: PacketizationAdmissionState
     var packetizationAdmissionDrops: Int64 = 0
     var lastPacketizationUs: UInt64 = 0
     var maxPacketizationUs: UInt64 = 0
     var lastPacketizationQueueWaitUs: UInt64 = 0
     var maxPacketizationQueueWaitUs: UInt64 = 0
     var lastSendBlockUs: UInt64 = 0
     var maxSendBlockUs: UInt64 = 0
     var lastSendPaceUs: UInt64 = 0
     var maxSendPaceUs: UInt64 = 0
     var lastCaptureCallbackNs: UInt64?
     var lastEncodeOutputCallbackNs: UInt64?
     var captureIntervalSamplesUs: [UInt64] = []
     var encodeOutputIntervalSamplesUs: [UInt64] = []
     var captureToEncodeSamplesUs: [UInt64] = []
     var captureQueueWaitSamplesUs: [UInt64] = []
     var inputPreparationSamplesUs: [UInt64] = []
     var encodeSubmitCallSamplesUs: [UInt64] = []
     var encoderCallbackSamplesUs: [UInt64] = []
     var encodeOutputSamplesUs: [UInt64] = []
     var packetizationSamplesUs: [UInt64] = []
     var packetizationQueueWaitSamplesUs: [UInt64] = []
     var sendBlockSamplesUs: [UInt64] = []
     var sendPaceSamplesUs: [UInt64] = []
     var stoppedReason = ""

    // Capture callback timestamps keyed by the real sample PTS. VideoToolbox
    // may call its output callback asynchronously, so this lets stats expose
    // capture -> encoded-output latency without putting a wait in the hot path.
     var captureNsByPts: [Int64: UInt64] = [:]
     var captureWallMsByPts: [Int64: UInt64] = [:]
    // VideoToolbox output callbacks can arrive after the next input frame has
    // already been submitted. Allocate the wire AU id at submission time and
    // recover it by PTS in the callback; reading framesEncoded in the callback
    // can assign the same id to two asynchronously completed frames.
     var encodeAuIdByPts: [Int64: UInt16] = [:]
     var nextAuId: UInt16 = 0

    // 1s-window rate counters for stats
     var rateWindowStart = Date()
     var rateWindowCaptureCallbacks: Int64 = 0
     var rateWindowFrames: Int64 = 0
     var rateWindowEncodeOutputCallbacks: Int64 = 0
     var rateWindowEncoderFrameDrops: Int64 = 0
     var rateWindowBytes: Int64 = 0
     var lastCaptureFps: UInt32 = 0
     var lastFps: UInt32 = 0
     var lastEncodeOutputFps: UInt32 = 0
     var lastEncoderFrameDropFps: UInt32 = 0
     var lastKbps: UInt32 = 0
     var lastPerfLogNs: UInt64 = 0
     var forceKeyframe = false
     var recoveryKeyframePending = false
     var lastKeyframeRequestNs: UInt64 = 0
     var recoveryDropRetryState = RecoveryDropRetryState()
     var currentAverageBitrate = 0
     var currentQualityHint: Float?
     var manualQualityHint: Float?
     var adaptiveQpController = AdaptiveQpController()
     var adaptiveQpWindowStartNs: UInt64 = 0
     var adaptiveQpWindowSubmittedFrames: Int64 = 0
     var adaptiveQpWindowEncoderDrops: Int64 = 0
     var adaptiveQpWindowValidOutputFrames: Int64 = 0
     var adaptiveQpWindowSubmitSamplesUs: [UInt64] = []
     var adaptiveQpWindowCallbackSamplesUs: [UInt64] = []
     var baseFrameQpChanges: Int64 = 0
     var qualityAdaptationChecks: Int64 = 0
     var qualityAdaptationChanges: Int64 = 0
     var qualityAdaptationRejections: Int64 = 0
     var qualityAdaptationLastStatus = "not_checked"
     var qualityAdaptationLastEncodeP95Us: UInt64 = 0
     var qualityAdaptationLastQueueP95Us: UInt64 = 0
     var qualityAdaptationLastReceiverLoss: UInt64 = 0
     var lastAdaptedDropped: Int64 = 0
     var receiverFrameGaps: UInt32 = 0
     var receiverInputDrops: UInt32 = 0
     var receiverIncompleteAUs: UInt32 = 0
     var receiverStaleFrames: UInt32 = 0
     var receiverStaleInputDrops: UInt32? = nil
     var receiverOutputBurstDiscards: UInt32 = 0
     var receiverRttMs: UInt16 = .max
     var receiverWireMs: UInt16 = .max
     var receiverFeedbackNs: UInt64 = 0
     var receiverRenderedFps: UInt32?
     var lastAdaptedReceiverLoss: UInt64 = 0
     var stableBitrateWindows = 0
    // WindowServer dirty regions provide pre-encode spatial evidence while
    // encoded AU shape remains the fallback for capture backends without that
    // metadata. Both signals drive one reconnect-free interactive/video state.
     var recentAuBytesEwma: Double = 0
     var adaptiveMotionState = AdaptiveMotionState()
     var lastDirtyChangedPixelRatio: Double = 0
     var lastDirtyRectCount: UInt32 = 0
     var adaptiveMotionTransitions: Int64 = 0
     var adaptiveMotionEvidence = "none"
    // An IDR is an intentional intra-frame burst. Do not interpret its
    // bounded send cost or the receiver's recovery boundary as persistent
    // congestion and ratchet the stream bitrate downward.
     var lastRecoverySendNs: UInt64 = 0
    // Transient Wi-Fi/power-save blips must not ratchet the bitrate to the
    // floor. Drop only when congestion persists across two windows; recover
    // with progressively larger steps so a floor exit takes seconds, not a
    // minute.
     var consecutiveCongestedWindows = 0
     var consecutiveRaiseSteps = 0
     var healthCheckScheduled = false
    // Single-session VideoToolbox submissions stay owned by this ledger until
    // callback, synchronous submit failure, or watchdog reclaim wins the slot.
    // These fields and the watchdog counters are protected by stateLock.
     var singleEncoderHealthState = SingleEncoderHealthState()
     var singleEncodeSubmissionLedger = SingleEncodeSubmissionLedger()
     var nextSingleEncodeSubmissionID: UInt64 = 0
     var singleEncoderHealthCheckScheduled = false
     var lastValidSingleEncoderOutputNs: UInt64?
     var encoderWatchdogRestarts: Int64 = 0
     var encoderWatchdogTerminations: Int64 = 0
     var encoderLateCallbacks: Int64 = 0

    // Split-vertical diagnostics are protected by stateLock. The two
    // VideoToolbox callbacks may complete on different threads, while the
    // public stats snapshot must remain internally consistent.
     var splitPreparationSamplesUs: [UInt64] = []
     var splitPairCallbackSamplesUs: [UInt64] = []
     var splitPairAdmissionDrops: Int64 = 0
     var splitPairDrops: Int64 = 0
     var splitPairTimeouts: Int64 = 0
     var splitLastPairDropReason = "none"
     var splitInjectedRightDrops: Int64 = 0
     var splitPreEncodeAdmissionDrops: Int64 = 0
     var splitRecoveryBoundaryDiscards: Int64 = 0
     var splitPostEncodeDeltaDrops: Int64 = 0
     var splitWirePairsAttempted: Int64 = 0
     var splitWirePairSendFailures: Int64 = 0
     var splitPairsEncoded: Int64 = 0
     var splitLeftOutputs: Int64 = 0
     var splitRightOutputs: Int64 = 0
     var splitLeftEncoderID = "not_ready"
     var splitRightEncoderID = "not_ready"
     var splitLeftHardware = false
     var splitRightHardware = false
     var splitWireSequence = SplitWireSequence()
     var splitFlowState: SplitFlowControlState
     var splitLeftReceiverLoss: UInt64 = 0
     var splitRightReceiverLoss: UInt64 = 0
     var splitLeftRenderedFps: UInt32 = 0
     var splitRightRenderedFps: UInt32 = 0
     var splitJoinedRenderedFps: UInt32 = 0
     var splitPairReadyDeltaP95Us: UInt64 = 0
     var splitPairReadyDeltaMaxUs: UInt64 = 0
     var splitPairSyncTimeouts: Int64 = 0
     var splitUnmatchedOutputDrops: Int64 = 0
     var splitKeyframeGapRecoveries: UInt32 = 0
     var splitDeltaGapRecoveries: UInt32 = 0
     var receiverMediaDatagrams: UInt64 = 0
     var receiverDataDatagrams: UInt64 = 0
     var receiverParityDatagrams: UInt64 = 0
     var receiverFecRestoredFragments: UInt64 = 0
     var receiverUnrecoverableFecGroups: UInt32 = 0
     var receiverMaxMissingDataFragments: UInt16 = 0
     var receiverOneFrameGapEvents: UInt32 = 0
     var receiverMultiFrameGapEvents: UInt32 = 0
     var receiverPairedIdrEpisodes: UInt32 = 0
     var receiverSuppressedRecoveryRequests: UInt32 = 0
     var receiverFecDecodeFailures: UInt32 = 0

    var isRunning: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return running
    }

     init(
        targetAddr: sockaddr_in,
        targetPort: UInt16,
        targetLabel: String,
        width: UInt32,
        height: UInt32,
        fps: UInt32,
        backend: CaptureBackendKind,
        mediaTransport: MediaTransportKind = .udp,
        contentMode: StreamContentMode = .interactive,
        requestedEncoderExperiment: EncoderExperiment = .auto,
        udpStability: AppliedUdpStability = .legacy
    ) {
        self.targetAddr = targetAddr
        self.targetPort = targetPort
        self.targetLabel = targetLabel
        self.outWidth = width
        self.outHeight = height
        self.fps = min(max(1, fps), 90)
        self.backend = backend
        self.mediaTransport = mediaTransport
        self.contentMode = contentMode
        self.appliedUdpStability = udpStability
        self.udpBurstPolicyState = UdpBurstPolicyState(applied: udpStability)
        self.activeUdpBurstDatagrams = udpStability.burstDatagrams
        self.activeUdpFecParityShards = udpStability.fecParityShards
        self.requestedEncoderExperiment = requestedEncoderExperiment
        self.appliedEncoderExperimentValue = appliedEncoderExperiment(requestedEncoderExperiment)
        self.splitFlowState = SplitFlowControlState(
            capacity: splitEncoderInFlightLimit(fps: min(max(1, fps), 90))
        )
        self.packetizationAdmissionState = PacketizationAdmissionState(
            limit: packetizationInFlightLimit(width: width, height: height)
        )
        encodeQueue.setSpecific(key: encodeQueueKey, value: ())
        performanceLogTicker = PerformanceLogTicker(
            interval: .seconds(1),
            queue: DispatchQueue(label: "leftcar.performance", qos: .utility)
        ) { [weak self] in
            _ = self?.statsJSON()
        }
    }

    deinit {
        performanceLogTicker?.stop()
    }

}
