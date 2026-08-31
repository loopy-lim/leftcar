struct SplitPairAdmission: Equatable {
    let sequence: UInt64
    let generation: UInt64
    let requestKeyframe: Bool
    let lease: SplitFlowLease?

    init(
        sequence: UInt64,
        generation: UInt64,
        requestKeyframe: Bool,
        lease: SplitFlowLease? = nil
    ) {
        self.sequence = sequence
        self.generation = generation
        self.requestKeyframe = requestKeyframe
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
        let admission = SplitPairAdmission(
            sequence: nextFrameSequence,
            generation: recoveryGeneration,
            requestKeyframe: forcePairedKeyframe || lease?.isRecoveryBoundary == true,
            lease: lease
        )
        inFlightSequences.insert(nextFrameSequence)
        if let lease {
            leasesBySequence[nextFrameSequence] = lease
        }
        nextFrameSequence &+= 1
        forcePairedKeyframe = false
        inFlightPairs = inFlightSequences.count
        return admission
    }

    mutating func requestPairedKeyframe() {
        forcePairedKeyframe = true
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
