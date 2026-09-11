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
    case pairExpired(
        consecutiveExpiries: Int,
        releasedPairs: Int,
        releasedLeases: [SplitFlowLease]
    )
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
    // Soft rendezvous expiry state. Expiries discard only the late frame;
    // three in a row mean one tile encoder stopped delivering, which falls
    // back to the hard reset. Tombstones reject late callbacks for sequences
    // whose slot was already retired at expiry.
    private var consecutivePairExpiries = 0
    private var pairExpiryTombstones = Set<UInt64>()
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
                        requestKeyframeLeft: admission.requestKeyframeLeft,
                        requestKeyframeRight: admission.requestKeyframeRight
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

    /// Per-tile gap recovery (R2): force a keyframe on ONE encoder only.
    /// VideoToolboxTileEncoder already takes forceKeyframe per encode
    /// request, so the peer tile keeps producing deltas with zero
    /// interruption. No recovery generation is bumped and no in-flight pair
    /// is discarded — the emitted asymmetric pair (one IDR, one delta) is a
    /// normal emit, not a recovery.
    func requestTileKeyframe(side: TileSide, reason _: String) {
        guard !stopped else { return }
        pairLifecycle.requestTileKeyframe(side)
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
        requestKeyframeLeft: Bool,
        requestKeyframeRight: Bool
    ) {
        let leftRequest = TileEncodeRequest(
            side: .left,
            frameSequence: frameSequence,
            pts: captured.pts,
            duration: captured.duration,
            captureNs: captured.callbackNs,
            captureWallMs: captured.captureWallMs,
            recoveryGeneration: generation,
            forceKeyframe: requestKeyframeLeft,
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
            forceKeyframe: requestKeyframeRight,
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
                // A tile callback for an already-expired pair arrives after
                // its slot was retired at expiry; drop it here instead of
                // leaving a half pair in the assembler that would expire
                // again and inflate the consecutive-expiry streak.
                guard !pairExpiryTombstones.contains(sequence) else { return }
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
                    self.consecutivePairExpiries = 0
                    let lease = self.pairLifecycle.completePair(sequence: sequence)
                    // Side-aware keyframe contract (per-tile recovery, R2):
                    // a tile that was REQUESTED to keyframe must deliver one;
                    // asymmetry itself is legal exactly when the request
                    // pattern explains it. A pair carrying exactly the
                    // requested side's keyframe (peer delta) emits normally
                    // through this path — no recovery, no generation bump.
                    // An asymmetry NO request explains still means the two
                    // reference chains diverged, so the hard reset stays.
                    let leftKeyframeMissing = left.requestedKeyframe && !left.isKeyframe
                    let rightKeyframeMissing = right.requestedKeyframe && !right.isKeyframe
                    let unexplainedAsymmetry =
                        !left.requestedKeyframe
                        && !right.requestedKeyframe
                        && left.isKeyframe != right.isKeyframe
                    guard !leftKeyframeMissing,
                          !rightKeyframeMissing,
                          !unexplainedAsymmetry else {
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
            let expiredSequences = self.barrier.takeExpiredSequences(
                nowNs: DispatchTime.now().uptimeNanoseconds
            )
            guard !expiredSequences.isEmpty else { return }
            self.handlePairCallbackExpiry(
                expiredSequences: expiredSequences,
                generation: generation
            )
        }
    }

    /// Rendezvous expiry is a per-frame discard, not an encoder failure:
    /// whole pairs are dropped on both tiles, so the VideoToolbox reference
    /// chains and every other in-flight pair stay valid. A pair dropped
    /// before enqueue never reaches the wire, so it burns no auID and leaves
    /// no gap for the viewer to detect — each expiry therefore asks the
    /// capture side for a paired keyframe directly, keeping later deltas
    /// from referencing the dropped pair. Consecutive expiries mean one
    /// encoder stopped delivering, so the third in a row falls back to
    /// beginPairedRecovery. That hard reset remains the path for encoder
    /// callback failures, keyframe mismatches, and barrier overflow.
    private func handlePairCallbackExpiry(
        expiredSequences: [UInt64],
        generation: UInt64
    ) {
        guard generation == pairLifecycle.recoveryGeneration else { return }
        consecutivePairExpiries += 1
        if consecutivePairExpiries >= 3 {
            beginPairedRecovery(reason: "encoded pair callback timeout")
            return
        }
        var releasedLeases = [SplitFlowLease]()
        for sequence in expiredSequences {
            pairExpiryTombstones.insert(sequence)
            if let lease = pairLifecycle.completePair(sequence: sequence) {
                releasedLeases.append(lease)
            }
        }
        event(
            .pairExpired(
                consecutiveExpiries: consecutivePairExpiries,
                releasedPairs: expiredSequences.count,
                releasedLeases: releasedLeases
            )
        )
    }

    private func beginPairedRecovery(reason: String) {
        let recovery = pairLifecycle.beginPairedRecovery()
        barrier.reset()
        consecutivePairExpiries = 0
        pairExpiryTombstones.removeAll(keepingCapacity: true)
        event(
            .dropped(
                reason: reason,
                releasedPairs: recovery.releasedPairCount,
                releasedLeases: recovery.releasedLeases
            )
        )
    }
}
