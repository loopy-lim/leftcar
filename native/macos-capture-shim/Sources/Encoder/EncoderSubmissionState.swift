import Foundation

enum EncoderCallbackDisposition: Equatable {
    case valid
    case dropped
    case failed
}

enum RecoveryDroppedFrameAction: Equatable {
    case none
    case retryAfterCooldown
}

enum PacketizationAdmissionDecision: Equatable {
    case admit
    case dropAndBeginRecovery
    case dropDuringRecovery
}

enum RecoveryKeyframeRequestDecision: Equatable {
    case requestNow
    case suppressForDelayedRetry
    case suppressForCooldown
}

enum EncodeSlotCompletionSource: Equatable {
    case callback
    case submitFailureReturn
    case watchdog
}

/// One token belongs to one VideoToolbox submission. The output callback may
/// run before `VTCompressionSessionEncodeFrame` returns an error, so both paths
/// may attempt completion but only the first claimant owns the encode slot.
final class EncodeSlotCompletionToken {
     let lock = NSLock()
     var source: EncodeSlotCompletionSource?

    var completedBy: EncodeSlotCompletionSource? {
        lock.lock()
        defer { lock.unlock() }
        return source
    }

    var completionCount: Int {
        completedBy == nil ? 0 : 1
    }

    @discardableResult
    func claim(_ completionSource: EncodeSlotCompletionSource) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard source == nil else { return false }
        source = completionSource
        return true
    }
}

/// Per-submission timing avoids a PTS dictionary lookup between timestamping
/// and `VTCompressionSessionEncodeFrame`, and remains valid if the callback is
/// invoked synchronously from that call.
final class EncoderSubmissionTimingContext {
     let lock = NSLock()
     var recordedInputPreparationUs: UInt64 = 0
     var submitCallStartNs: UInt64?

    var inputPreparationUs: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return recordedInputPreparationUs
    }

    func recordInputPreparation(startNs: UInt64, endNs: UInt64) {
        lock.lock()
        recordedInputPreparationUs = endNs >= startNs
            ? (endNs - startNs) / 1_000
            : 0
        lock.unlock()
    }

    func beginSubmitCall(at startNs: UInt64) {
        lock.lock()
        submitCallStartNs = startNs
        lock.unlock()
    }

    func submitCallDurationUs(at endNs: UInt64) -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        guard let startNs = submitCallStartNs, endNs >= startNs else { return 0 }
        return (endNs - startNs) / 1_000
    }

    func callbackLatencyUs(at callbackNs: UInt64) -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        guard let startNs = submitCallStartNs, callbackNs >= startNs else { return 0 }
        return (callbackNs - startNs) / 1_000
    }
}

enum NetworkRecoveryFrameAdmission: Equatable {
    case admit
    case dropDelta
}

struct NetworkRecoveryBoundaryState: Equatable {
    private(set) var awaitingKeyframe = false

    mutating func establishAwaitingKeyframe() {
        awaitingKeyframe = true
    }

    mutating func clearAfterSuccessfulKeyframe() {
        awaitingKeyframe = false
    }

    mutating func setAwaitingKeyframe(_ awaiting: Bool) {
        awaitingKeyframe = awaiting
    }

    func admission(isKeyframe: Bool) -> NetworkRecoveryFrameAdmission {
        awaitingKeyframe && !isKeyframe ? .dropDelta : .admit
    }
}

struct RecoveryDropRetryState: Equatable {
    private(set) var pendingGeneration: UInt64?

    var hasPendingRetry: Bool { pendingGeneration != nil }

    mutating func register(
        generation: UInt64,
        currentGeneration: UInt64
    ) -> Bool {
        guard generation == currentGeneration, pendingGeneration == nil else {
            return false
        }
        pendingGeneration = generation
        return true
    }

    mutating func establishBoundaryIfOwned(
        generation: UInt64,
        currentGeneration: UInt64,
        boundary: inout NetworkRecoveryBoundaryState
    ) -> Bool {
        guard pendingGeneration == generation else { return false }
        guard generation == currentGeneration else {
            pendingGeneration = nil
            return false
        }
        boundary.establishAwaitingKeyframe()
        return true
    }

    mutating func consume(generation: UInt64) -> Bool {
        guard pendingGeneration == generation else { return false }
        pendingGeneration = nil
        return true
    }

    mutating func clear() {
        pendingGeneration = nil
    }
}

struct PacketizationAdmissionState: Equatable {
    let limit: Int
    private(set) var inFlight = 0

    init(limit: Int) {
        self.limit = max(1, limit)
    }

    mutating func admit(
        recoveryBoundaryPending: Bool
    ) -> PacketizationAdmissionDecision {
        let decision = packetizationAdmissionDecision(
            inFlight: inFlight,
            limit: limit,
            recoveryBoundaryPending: recoveryBoundaryPending
        )
        if decision == .admit {
            inFlight += 1
        }
        return decision
    }

    mutating func finish() {
        inFlight = max(0, inFlight - 1)
    }
}
