struct SplitFlowLease: Hashable {
    let id: UInt64
    let generation: UInt64
    let isRecoveryBoundary: Bool
}

struct SplitFlowRecoveryTransition: Equatable {
    let generation: UInt64
    let releasedLeaseCount: Int
}

struct SplitFlowControlState {
    let capacity: Int

    private(set) var activeCount = 0
    private(set) var recoveryBoundaryPending = true

    private var nextID: UInt64 = 0
    private var generation: UInt64 = 0
    private var boundaryLeaseID: UInt64?
    private var activeLeases = Set<SplitFlowLease>()

    init(capacity: Int) {
        self.capacity = max(1, capacity)
    }

    mutating func admit() -> SplitFlowLease? {
        guard activeCount < capacity else { return nil }
        if recoveryBoundaryPending, boundaryLeaseID != nil {
            return nil
        }
        let lease = SplitFlowLease(
            id: nextID,
            generation: generation,
            isRecoveryBoundary: recoveryBoundaryPending
        )
        nextID &+= 1
        activeLeases.insert(lease)
        activeCount = activeLeases.count
        if lease.isRecoveryBoundary {
            boundaryLeaseID = lease.id
        }
        return lease
    }

    func accepts(_ lease: SplitFlowLease) -> Bool {
        lease.generation == generation && activeLeases.contains(lease)
    }

    @discardableResult
    mutating func complete(_ lease: SplitFlowLease) -> Bool {
        guard activeLeases.remove(lease) != nil else { return false }
        activeCount = activeLeases.count
        if lease.generation == generation,
           lease.isRecoveryBoundary,
           boundaryLeaseID == lease.id {
            recoveryBoundaryPending = false
            boundaryLeaseID = nil
        }
        return true
    }

    mutating func beginRecovery() -> SplitFlowRecoveryTransition {
        let releasedLeaseCount = activeLeases.count
        activeLeases.removeAll(keepingCapacity: true)
        activeCount = 0
        generation &+= 1
        recoveryBoundaryPending = true
        boundaryLeaseID = nil
        return SplitFlowRecoveryTransition(
            generation: generation,
            releasedLeaseCount: releasedLeaseCount
        )
    }

    mutating func cancelAll() -> Int {
        let releasedLeaseCount = activeLeases.count
        activeLeases.removeAll(keepingCapacity: true)
        activeCount = 0
        recoveryBoundaryPending = true
        boundaryLeaseID = nil
        return releasedLeaseCount
    }
}
