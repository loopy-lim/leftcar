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
    /// 실험 1 소프트 만료: 랑데부 예산이 지난 페어를 전체 리셋 없이 정리한다.
    /// 왼쪽 반쪽이 준비됐으면 `tileEmittedSolo`로 단독 전송되고(lease는 그
    /// 이벤트로 소비된다), 그 외 취소 페어의 lease는 이 이벤트로 어댑터가
    /// 흐름 용량에 돌려놓는다. 전송 복구를 걸지 않는다 — 그것이 이 경로의
    /// 존재 이유다.
    case pairExpiredSoft(
        consecutiveExpiries: Int,
        cancelledPairs: Int,
        releasedLeases: [SplitFlowLease]
    )
    case tileEmittedSolo(lease: SplitFlowLease?, sample: TileEncodedSample)
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
    /// 소프트 만료로 취소된 시퀀스 — 늦게 도착한 반쪽 콜백을 배리어 오염
    /// 없이 폐기하기 위한 목록이다. 하드 복구·무효화에서 비운다.
    private var cancelledSequences = Set<UInt64>()
    // Expiry fences the whole admitted generation. Three consecutive failures
    // retain the existing hard-recovery safety valve and reset its streak.
    private var consecutivePairExpiries = 0
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
        cancelledSequences.removeAll()
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
                guard case let .emit(lease) = self.pairLifecycle.completeEncodedPair(
                    sequence: sequence,
                    leftRequested: left.requestedKeyframe,
                    rightRequested: left.requestedKeyframe,
                    leftKeyframe: left.isKeyframe,
                    rightKeyframe: left.isKeyframe
                ) else {
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
                if self.cancelledSequences.remove(sequence) != nil {
                    // 소프트 만료로 취소된 페어의 늦은 반쪽 — lease는 이미
                    // 회수됐고 이 반쪽을 배리어에 재적체하면 오버플로→하드
                    // 밸브로 역진한다. 조용히 폐기한다.
                    return
                }
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
                    // Validate both encoder outputs before releasing this slot;
                    // a rejected IDR belongs to the recovery's retirement set.
                    guard case let .emit(lease) = self.pairLifecycle.completeEncodedPair(
                        sequence: sequence,
                        leftRequested: left.requestedKeyframe,
                        rightRequested: right.requestedKeyframe,
                        leftKeyframe: left.isKeyframe,
                        rightKeyframe: right.isKeyframe
                    ) else {
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
            let expiredEntries = self.barrier.takeExpiredPairEntries(
                nowNs: DispatchTime.now().uptimeNanoseconds
            )
            guard !expiredEntries.isEmpty else { return }
            self.handlePairCallbackExpiry(
                expiredSequences: expiredEntries,
                generation: generation
            )
        }
    }

    /// Discard the unsent reference and every admitted dependent pair without
    /// recreating encoders. A paired IDR in the new lifecycle/flow generation
    /// reestablishes the reference chain; stale callbacks release nothing twice.
    private func handlePairCallbackExpiry(
        expiredSequences: [ExpiredPairEntry<TileEncodedSample>],
        generation: UInt64
    ) {
        guard generation == pairLifecycle.recoveryGeneration else { return }
        if consecutivePairExpiries >= 2 {
            beginPairedRecovery(reason: "encoded pair callback timeout")
            return
        }
        // 실험 1 소프트 경로: 전체 리셋·페어 IDR 폭풍 없이 만료 페어 하나만
        // 정리한다. generation 불변이라 다른 in-flight 페어와 타일 델타
        // 사슬은 계속된다. 연속 만료는 여전히 하드 밸브로 에스컬레이션한다.
        var cancelledPairs = 0
        var retiredLeases = [SplitFlowLease]()
        for entry in expiredSequences {
            guard let lease = pairLifecycle.expireSinglePair(sequence: entry.sequence) else {
                continue
            }
            cancelledPairs += 1
            cancelledSequences.insert(entry.sequence)
            if entry.missingSide == .right, let ready = entry.readyValue {
                // 왼쪽 준비: 기존 left-solo 와이어 형태로 단독 전송 — 이
                // 프레임의 절반은 시청자에게 도달한다(lease는 이 이벤트로
                // 소비된다). 오른쪽은 per-tile 키프레임으로 복구된다.
                event(.tileEmittedSolo(lease: lease, sample: ready))
            } else {
                // 오른쪽 준비(v1 한계): 오른쪽 단독 와이어 경로가 없어
                // 폐기한다 — lease는 어댑터가 흐름 용량에 돌려놓는다.
                retiredLeases.append(lease)
            }
            pairLifecycle.requestTileKeyframe(entry.missingSide)
        }
        guard cancelledPairs > 0 else { return }
        consecutivePairExpiries += 1
        event(.pairExpiredSoft(
            consecutiveExpiries: consecutivePairExpiries,
            cancelledPairs: cancelledPairs,
            releasedLeases: retiredLeases
        ))
    }

    private func beginPairedRecovery(reason: String) {
        let recovery = pairLifecycle.beginPairedRecovery()
        barrier.reset()
        cancelledSequences.removeAll()
        consecutivePairExpiries = 0
        event(
            .dropped(
                reason: reason,
                releasedPairs: recovery.releasedPairCount,
                releasedLeases: recovery.releasedLeases
            )
        )
    }
}
