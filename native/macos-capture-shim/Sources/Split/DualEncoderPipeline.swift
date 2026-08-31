import Foundation
import CoreMedia

func splitEncoderInFlightLimit(fps: UInt32) -> Int {
    fps >= 60 ? 5 : 3
}

func splitPairRetentionLimit(maximumInFlightPairs: Int) -> Int {
    max(1, maximumInFlightPairs)
}

func splitPairCallbackExpiryBudgetNs(
    fps: UInt32,
    maximumInFlightPairs: Int
) -> UInt64 {
    let framesPerSecond = UInt64(max(1, fps))
    let frameBudgetNs = max(
        1,
        (1_000_000_000 + framesPerSecond - 1) / framesPerSecond
    )
    return frameBudgetNs * UInt64(max(1, maximumInFlightPairs))
}

enum DualEncoderPipelineEvent {
    case configured(
        leftEncoderID: String,
        rightEncoderID: String,
        leftHardware: Bool,
        rightHardware: Bool
    )
    case prepared(microseconds: UInt64)
    case encodedPair(
        lease: SplitFlowLease?,
        left: TileEncodedSample,
        right: TileEncodedSample
    )
    case injectedRightDrop(lease: SplitFlowLease?, left: TileEncodedSample)
    case dropped(
        reason: String,
        releasedPairs: Int,
        releasedLeases: [SplitFlowLease]
    )
}

final class DualEncoderPipeline {
    private let queue: DispatchQueue
    private let splitter: MetalNv12Splitter
    private let leftEncoder: VideoToolboxTileEncoder
    private let rightEncoder: VideoToolboxTileEncoder?
    private let strategy: SplitEncoderStrategy
    private let pairCallbackExpiryBudgetNs: UInt64
    private let maximumInFlightPairs: Int
    private let event: (DualEncoderPipelineEvent) -> Void

    private var barrier: EncodedPairAssembler<TileEncodedSample>
    private var pairLifecycle = SplitPairLifecycleState()
    private var faultInjection = SplitFaultInjection()
    private var stopped = false

    init(
        queue: DispatchQueue,
        fps: UInt32,
        aggregateBitrate: Int,
        maximumInFlightPairs: Int? = nil,
        strategy: SplitEncoderStrategy = .parse(),
        event: @escaping (DualEncoderPipelineEvent) -> Void
    ) throws {
        let maximumInFlightPairs = max(
            1,
            maximumInFlightPairs ?? splitEncoderInFlightLimit(fps: fps)
        )
        let pairCallbackExpiryBudgetNs = splitPairCallbackExpiryBudgetNs(
            fps: fps,
            maximumInFlightPairs: maximumInFlightPairs
        )
        self.queue = queue
        self.pairCallbackExpiryBudgetNs = pairCallbackExpiryBudgetNs
        self.maximumInFlightPairs = maximumInFlightPairs
        self.strategy = strategy
        self.event = event
        self.barrier = EncodedPairAssembler(
            frameBudgetNs: pairCallbackExpiryBudgetNs,
            maximumRetainedSequences: splitPairRetentionLimit(
                maximumInFlightPairs: maximumInFlightPairs
            )
        )
        self.splitter = try MetalNv12Splitter(
            pairedInFlightLimit: maximumInFlightPairs
        )
        let perTileBitrate = max(1_000_000, aggregateBitrate / 2)
        self.leftEncoder = try VideoToolboxTileEncoder(
            side: .left,
            fps: fps,
            bitrate: perTileBitrate,
            backend: strategy.tileBackend
        )
        if strategy.usesIndependentRightEncoder {
            do {
                self.rightEncoder = try VideoToolboxTileEncoder(
                    side: .right,
                    fps: fps,
                    bitrate: perTileBitrate,
                    backend: strategy.tileBackend
                )
            } catch {
                leftEncoder.invalidate()
                throw error
            }
        } else {
            self.rightEncoder = nil
        }
        event(
            .configured(
                leftEncoderID: leftEncoder.encoderID,
                rightEncoderID: rightEncoder?.encoderID ?? "mirror-left-diagnostic",
                leftHardware: leftEncoder.hardwareAccelerated,
                rightHardware: rightEncoder?.hardwareAccelerated
                    ?? leftEncoder.hardwareAccelerated
            )
        )
    }

    @discardableResult
    func submit(captured: PendingCaptureFrame) -> PairAdmissionDecision {
        submit(captured: captured, lease: nil)
    }

    @discardableResult
    func submit(
        captured: PendingCaptureFrame,
        lease: SplitFlowLease
    ) -> PairAdmissionDecision {
        submit(captured: captured, lease: Optional(lease))
    }

    private func submit(
        captured: PendingCaptureFrame,
        lease: SplitFlowLease?
    ) -> PairAdmissionDecision {
        guard !stopped,
              let admission = lease.map({
                  pairLifecycle.admit(
                      lease: $0,
                      maximumInFlightPairs: maximumInFlightPairs
                  )
              }) ?? pairLifecycle.admit(
                  maximumInFlightPairs: maximumInFlightPairs
              ) else {
            event(
                .dropped(
                    reason: "pair admission saturated",
                    releasedPairs: 0,
                    releasedLeases: []
                )
            )
            return .dropPair
        }
        let frameSequence = admission.sequence
        let generation = admission.generation
        let requestKeyframe = admission.requestKeyframe

        splitter.prepare(source: captured.pixelBuffer) { [weak self] result in
            guard let self else { return }
            self.queue.async {
                guard !self.stopped,
                      generation == self.pairLifecycle.recoveryGeneration else { return }
                switch result {
                case let .failure(error):
                    self.beginPairedRecovery(
                        reason: "Metal NV12 split failed: \(error)"
                    )
                case let .success(pair):
                    self.event(.prepared(microseconds: pair.preparationUs))
                    self.submitPreparedPair(
                        pair,
                        captured: captured,
                        frameSequence: frameSequence,
                        generation: generation,
                        requestKeyframe: requestKeyframe
                    )
                }
            }
        }
        return .admitPair
    }

    func requestPairedKeyframe(reason _: String) {
        guard !stopped else { return }
        pairLifecycle.requestPairedKeyframe()
    }

    func updateAggregateBitrate(_ bitrate: Int) -> Bool {
        let perTileBitrate = max(1_000_000, bitrate / 2)
        let left = leftEncoder.updateBitrate(perTileBitrate)
        let right = rightEncoder?.updateBitrate(perTileBitrate) ?? true
        return left && right
    }

    func invalidate() {
        guard !stopped else { return }
        stopped = true
        barrier.reset()
        _ = pairLifecycle.cancelAll()
        leftEncoder.invalidate()
        rightEncoder?.invalidate()
    }

    private func submitPreparedPair(
        _ pair: MetalNv12Splitter.PreparedPair,
        captured: PendingCaptureFrame,
        frameSequence: UInt64,
        generation: UInt64,
        requestKeyframe: Bool
    ) {
        let leftRequest = TileEncodeRequest(
            side: .left,
            frameSequence: frameSequence,
            pts: captured.pts,
            duration: captured.duration,
            captureNs: captured.callbackNs,
            captureWallMs: captured.captureWallMs,
            recoveryGeneration: generation,
            forceKeyframe: requestKeyframe,
            pixelBuffer: pair.left
        )
        if strategy == .mirrorLeft {
            leftEncoder.submit(leftRequest) { [weak self] result in
                self?.publishMirrored(
                    result,
                    sequence: frameSequence,
                    generation: generation
                )
            }
            return
        }
        guard let rightEncoder else {
            beginPairedRecovery(reason: "right tile encoder unavailable")
            return
        }
        let rightRequest = TileEncodeRequest(
            side: .right,
            frameSequence: frameSequence,
            pts: captured.pts,
            duration: captured.duration,
            captureNs: captured.callbackNs,
            captureWallMs: captured.captureWallMs,
            recoveryGeneration: generation,
            forceKeyframe: requestKeyframe,
            pixelBuffer: pair.right
        )
        leftEncoder.submit(leftRequest) { [weak self] result in
            self?.publish(result, side: .left, sequence: frameSequence, generation: generation)
        }
        rightEncoder.submit(rightRequest) { [weak self] result in
            self?.publish(result, side: .right, sequence: frameSequence, generation: generation)
        }
    }

    private func publishMirrored(
        _ result: Result<TileEncodedSample, TileEncoderError>,
        sequence: UInt64,
        generation: UInt64
    ) {
        queue.async { [weak self] in
            guard let self,
                  !self.stopped,
                  generation == self.pairLifecycle.recoveryGeneration else { return }
            switch result {
            case let .failure(error):
                self.beginPairedRecovery(
                    reason: "mirror-left encoder callback failed: \(error)"
                )
            case let .success(left):
                let lease = self.pairLifecycle.completePair(sequence: sequence)
                guard !left.requestedKeyframe || left.isKeyframe else {
                    self.beginPairedRecovery(reason: "mirror-left keyframe mismatch")
                    return
                }
                if self.faultInjection.shouldDropRightAu() {
                    self.event(.injectedRightDrop(lease: lease, left: left))
                    return
                }
                let right = TileEncodedSample(
                    side: .right,
                    frameSequence: left.frameSequence,
                    pts: left.pts,
                    captureNs: left.captureNs,
                    captureWallMs: left.captureWallMs,
                    sampleBuffer: left.sampleBuffer,
                    isKeyframe: left.isKeyframe,
                    recoveryGeneration: left.recoveryGeneration,
                    requestedKeyframe: left.requestedKeyframe
                )
                self.event(.encodedPair(lease: lease, left: left, right: right))
            }
        }
    }

    private func publish(
        _ result: Result<TileEncodedSample, TileEncoderError>,
        side: TileSide,
        sequence: UInt64,
        generation: UInt64
    ) {
        queue.async { [weak self] in
            guard let self,
                  !self.stopped,
                  generation == self.pairLifecycle.recoveryGeneration else { return }
            switch result {
            case let .failure(error):
                self.beginPairedRecovery(
                    reason: "\(side) encoder callback failed: \(error)"
                )
            case let .success(sample):
                let decision = self.barrier.insert(
                    EncodedTile(
                        side: side,
                        sequence: sequence,
                        value: sample,
                        valid: true
                    ),
                    nowNs: DispatchTime.now().uptimeNanoseconds
                )
                switch decision {
                case .wait:
                    self.schedulePairExpiry(generation: generation)
                case let .emit(left, right):
                    let lease = self.pairLifecycle.completePair(sequence: sequence)
                    guard left.isKeyframe == right.isKeyframe,
                          !left.requestedKeyframe || left.isKeyframe,
                          !right.requestedKeyframe || right.isKeyframe else {
                        self.beginPairedRecovery(reason: "paired keyframe mismatch")
                        return
                    }
                    if self.faultInjection.shouldDropRightAu() {
                        self.event(.injectedRightDrop(lease: lease, left: left))
                    } else {
                        self.event(.encodedPair(lease: lease, left: left, right: right))
                    }
                case let .drop(requestPairedKeyframe):
                    if requestPairedKeyframe {
                        self.beginPairedRecovery(reason: "encoded pair barrier overflow")
                    }
                }
            }
        }
    }

    /// Start the bounded callback-skew budget when the first encoder output arrives,
    /// not when capture was submitted. Metal and VideoToolbox latency is
    /// shared work and must not consume the left/right rendezvous budget.
    private func schedulePairExpiry(generation: UInt64) {
        queue.asyncAfter(
            deadline: .now() + .nanoseconds(
                Int(min(pairCallbackExpiryBudgetNs, UInt64(Int.max)))
            )
        ) { [weak self] in
            guard let self,
                  !self.stopped,
                  generation == self.pairLifecycle.recoveryGeneration else { return }
            if case .drop(requestPairedKeyframe: true) = self.barrier.expire(
                nowNs: DispatchTime.now().uptimeNanoseconds
            ) {
                self.beginPairedRecovery(reason: "encoded pair callback timeout")
            }
        }
    }

    private func beginPairedRecovery(reason: String) {
        let recovery = pairLifecycle.beginPairedRecovery()
        barrier.reset()
        event(
            .dropped(
                reason: reason,
                releasedPairs: recovery.releasedPairCount,
                releasedLeases: recovery.releasedLeases
            )
        )
    }
}
