// Pure recovery policy shared by the capture adapter and focused tests.
extension SplitPairLifecycleState {
    /// An unsent reference invalidates every admitted descendant. Advance once;
    /// timers and callbacks from this generation then become harmless no-ops.
    mutating func expirePairs<Value>(
        sequences: [UInt64], generation: UInt64,
        barrier: inout EncodedPairAssembler<Value>
    ) -> SplitPairRecovery? {
        guard generation == recoveryGeneration, !sequences.isEmpty else { return nil }
        barrier.reset()
        return beginPairedRecovery()
    }
}

extension SplitFlowControlState {
    /// Repeated requests preserve the current paired-IDR boundary. A confirmed
    /// loss of that boundary itself must explicitly invalidate it for a retry.
    mutating func beginRecoveryIfNeeded(
        invalidatePendingBoundary: Bool,
        failedLease: SplitFlowLease? = nil
    ) -> Bool {
        if let failedLease, !accepts(failedLease) { return false }
        guard invalidatePendingBoundary || !recoveryBoundaryPending else { return false }
        _ = beginRecovery()
        return true
    }
}

enum SplitRecoverySeedDecision: Equatable {
    case queueAlreadyPending
    case seedCarrier
    case noCarrierAvailable
}

func splitRecoverySeedDecision(pendingCaptureCount: Int, hasCarrier: Bool) -> SplitRecoverySeedDecision {
    if pendingCaptureCount > 0 { return .queueAlreadyPending }
    return hasCarrier ? .seedCarrier : .noCarrierAvailable
}

/// Generic only over the retained payload: the actual pending capture queue
/// and synthetic test buffers both consume this same mutation.
@discardableResult
func seedSplitRecoveryCarrier<Frame>(pending: inout [Frame], carrier: Frame?) -> Bool {
    guard splitRecoverySeedDecision(pendingCaptureCount: pending.count, hasCarrier: carrier != nil) == .seedCarrier,
          let carrier else { return false }
    pending.append(carrier)
    return true
}

enum SplitPairCompletion {
    case emit(SplitFlowLease?)
    case recover
}

extension SplitPairLifecycleState {
    mutating func completeEncodedPair(
        sequence: UInt64,
        leftRequested: Bool, rightRequested: Bool,
        leftKeyframe: Bool, rightKeyframe: Bool
    ) -> SplitPairCompletion {
        guard !(leftRequested && !leftKeyframe),
              !(rightRequested && !rightKeyframe),
              leftRequested || rightRequested || leftKeyframe == rightKeyframe else {
            return .recover
        }
        return .emit(completePair(sequence: sequence))
    }
}

/// Capture-owned first phase. Seed with the transition so an already scheduled
/// idle drain cannot admit and immediately complete an empty recovery lease.
func beginSplitRecovery<Frame>(
    flow: inout SplitFlowControlState,
    invalidatePendingBoundary: Bool,
    failedLease: SplitFlowLease? = nil,
    pendingCaptures: inout [Frame],
    carrier: Frame?
) -> UInt64? {
    guard flow.beginRecoveryIfNeeded(
        invalidatePendingBoundary: invalidatePendingBoundary,
        failedLease: failedLease
    ) else { return nil }
    seedSplitRecoveryCarrier(pending: &pendingCaptures, carrier: carrier)
    return flow.recoveryGeneration
}

/// Called with both network and capture ownership held by the adapter.
func finishSplitRecoveryCleanup<Value>(
    generation: UInt64,
    flow: SplitFlowControlState,
    pending: inout [Value],
    lease: (Value) -> SplitFlowLease
) -> Int? {
    guard generation == flow.recoveryGeneration, flow.recoveryBoundaryPending else { return nil }
    let previousCount = pending.count
    // An already scheduled encoder can enqueue this episode's IDR before
    // cleanup gets the network lock. Only retired leases belong to cleanup.
    pending.removeAll { !flow.accepts(lease($0)) }
    return previousCount - pending.count
}

/// The conversion may outlive the initial acceptance check on another queue.
func prepareSplitPairPayloads<Value>(
    lease: SplitFlowLease,
    left: () -> Value?,
    right: () -> Value?,
    onFailure: (SplitFlowLease) -> Void
) -> (Value, Value)? {
    guard let leftPayload = left(), let rightPayload = right() else {
        onFailure(lease)
        return nil
    }
    return (leftPayload, rightPayload)
}
