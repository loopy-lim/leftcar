struct SplitPairAdmission: Equatable {
    let sequence: UInt64
    let generation: UInt64
    /// Paired keyframe request: recovery boundary or paired IDR. When true,
    /// both tiles are forced to keyframe (`requestKeyframeLeft/Right` are
    /// then also true).
    let requestKeyframe: Bool
    /// Effective per-side force-keyframe flags (paired || per-tile). The
    /// legacy `requestKeyframe`-only initializer treats a paired request as
    /// both-sided, so the released test constructions stay valid.
    let requestKeyframeLeft: Bool
    let requestKeyframeRight: Bool
    let lease: SplitFlowLease?

    init(
        sequence: UInt64,
        generation: UInt64,
        requestKeyframe: Bool,
        requestKeyframeLeft: Bool? = nil,
        requestKeyframeRight: Bool? = nil,
        lease: SplitFlowLease? = nil
    ) {
        self.sequence = sequence
        self.generation = generation
        self.requestKeyframe = requestKeyframe
        self.requestKeyframeLeft = requestKeyframeLeft ?? requestKeyframe
        self.requestKeyframeRight = requestKeyframeRight ?? requestKeyframe
        self.lease = lease
    }
}

struct SplitPairRecovery {
    let releasedPairCount: Int
    let releasedLeases: [SplitFlowLease]
}

struct SplitPairLifecycleState {
    private var nextFrameSequence: UInt64 = 0
    private var forcePairedKeyframe = true
    // Per-tile keyframe requests (viewer per-tile IDR path, R2). They ride
    // the NEXT admission like the paired flag but without a flow-generation
    // bump: only the requested tile's encoder is forced, and the peer keeps
    // streaming deltas with zero interruption.
    private var forceLeftKeyframe = false
    private var forceRightKeyframe = false
    private var inFlightSequences = Set<UInt64>()
    private var leasesBySequence: [UInt64: SplitFlowLease] = [:]

    private(set) var recoveryGeneration: UInt64 = 0
    private(set) var inFlightPairs = 0

    mutating func admit(maximumInFlightPairs: Int) -> SplitPairAdmission? {
        admit(lease: nil, maximumInFlightPairs: maximumInFlightPairs)
    }

    mutating func admit(
        lease: SplitFlowLease,
        maximumInFlightPairs: Int
    ) -> SplitPairAdmission? {
        admit(lease: Optional(lease), maximumInFlightPairs: maximumInFlightPairs)
    }

    private mutating func admit(
        lease: SplitFlowLease?,
        maximumInFlightPairs: Int
    ) -> SplitPairAdmission? {
        guard inFlightPairs < max(1, maximumInFlightPairs) else { return nil }
        let pairedRequest = forcePairedKeyframe || lease?.isRecoveryBoundary == true
        let admission = SplitPairAdmission(
            sequence: nextFrameSequence,
            generation: recoveryGeneration,
            requestKeyframe: pairedRequest,
            requestKeyframeLeft: pairedRequest || forceLeftKeyframe,
            requestKeyframeRight: pairedRequest || forceRightKeyframe,
            lease: lease
        )
        inFlightSequences.insert(nextFrameSequence)
        if let lease {
            leasesBySequence[nextFrameSequence] = lease
        }
        nextFrameSequence &+= 1
        forcePairedKeyframe = false
        forceLeftKeyframe = false
        forceRightKeyframe = false
        inFlightPairs = inFlightSequences.count
        return admission
    }

    mutating func requestPairedKeyframe() {
        forcePairedKeyframe = true
    }

    /// Request a keyframe on ONE tile only (per-tile gap recovery). The
    /// request is consumed by the next admission; repeated requests before
    /// that are idempotent. No recovery generation is bumped, so the peer
    /// tile's delta chain and every other in-flight pair stay valid.
    mutating func requestTileKeyframe(_ side: TileSide) {
        switch side {
        case .left:
            forceLeftKeyframe = true
        case .right:
            forceRightKeyframe = true
        }
    }

    mutating func completePair(sequence: UInt64) -> SplitFlowLease? {
        guard inFlightSequences.remove(sequence) != nil else { return nil }
        inFlightPairs = inFlightSequences.count
        return leasesBySequence.removeValue(forKey: sequence)
    }

    mutating func beginPairedRecovery() -> SplitPairRecovery {
        let recovery = SplitPairRecovery(
            releasedPairCount: inFlightSequences.count,
            releasedLeases: Array(leasesBySequence.values)
        )
        recoveryGeneration &+= 1
        forcePairedKeyframe = true
        inFlightSequences.removeAll(keepingCapacity: true)
        leasesBySequence.removeAll(keepingCapacity: true)
        inFlightPairs = 0
        return recovery
    }

    mutating func cancelAll() -> SplitPairRecovery {
        let recovery = SplitPairRecovery(
            releasedPairCount: inFlightSequences.count,
            releasedLeases: Array(leasesBySequence.values)
        )
        inFlightSequences.removeAll(keepingCapacity: true)
        leasesBySequence.removeAll(keepingCapacity: true)
        inFlightPairs = 0
        return recovery
    }
}
