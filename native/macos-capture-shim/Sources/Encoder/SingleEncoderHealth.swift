import Foundation

enum SingleEncoderHealthDecision: Equatable {
    case healthy
    case restart(generation: UInt64)
    case terminate
}

struct SingleEncoderHealthState {
    static let stallBudgetNs: UInt64 = 250_000_000
    static let restartCooldownNs: UInt64 = 1_000_000_000
    static let restartWindowNs: UInt64 = 10_000_000_000
    static let maximumRestarts = 2

    private var lastObservedCaptureCallbackNs: UInt64?
    private var latestValidOutputNs: UInt64?
    private var lastObservedValidOutputNs: UInt64?
    private var recoveredGeneration: UInt64?
    private var restartRecords: [(timestampNs: UInt64, generation: UInt64)] = []

    mutating func evaluate(
        nowNs: UInt64,
        captureCallbackNs: UInt64?,
        lastValidOutputNs: UInt64?,
        encodeInFlight: Int,
        oldestSubmissionNs: UInt64?,
        generation: UInt64
    ) -> SingleEncoderHealthDecision {
        pruneRestartRecords(at: nowNs)

        guard let captureCallbackNs,
              lastObservedCaptureCallbackNs.map({ captureCallbackNs > $0 }) ?? true else {
            return .healthy
        }
        lastObservedCaptureCallbackNs = captureCallbackNs

        guard encodeInFlight > 0,
              let oldestSubmissionNs,
              nowNs >= oldestSubmissionNs,
              nowNs - oldestSubmissionNs >= Self.stallBudgetNs else {
            return .healthy
        }

        let validOutputNs = maxOptional(latestValidOutputNs, lastValidOutputNs)
        let validOutputAdvanced = validOutputNs != lastObservedValidOutputNs
        lastObservedValidOutputNs = validOutputNs
        if validOutputAdvanced,
           let validOutputNs,
           validOutputNs > oldestSubmissionNs {
            return .healthy
        }
        guard recoveredGeneration != generation else {
            return .healthy
        }
        if let lastRestartNs = restartRecords.last?.timestampNs,
           nowNs >= lastRestartNs,
           nowNs - lastRestartNs < Self.restartCooldownNs {
            return .healthy
        }

        recoveredGeneration = generation
        if restartRecords.count >= Self.maximumRestarts {
            return .terminate
        }
        return .restart(generation: generation)
    }

    mutating func recordRestart(at nowNs: UInt64, generation: UInt64) {
        pruneRestartRecords(at: nowNs)
        recoveredGeneration = generation
        restartRecords.append((timestampNs: nowNs, generation: generation))
    }

    mutating func recordValidOutput(at nowNs: UInt64) {
        latestValidOutputNs = maxOptional(latestValidOutputNs, nowNs)
    }

    private mutating func pruneRestartRecords(at nowNs: UInt64) {
        restartRecords.removeAll { record in
            nowNs >= record.timestampNs
                && nowNs - record.timestampNs > Self.restartWindowNs
        }
    }

    private func maxOptional(_ left: UInt64?, _ right: UInt64?) -> UInt64? {
        switch (left, right) {
        case let (.some(left), .some(right)):
            return max(left, right)
        case let (.some(left), .none):
            return left
        case let (.none, .some(right)):
            return right
        case (.none, .none):
            return nil
        }
    }
}

struct SingleEncodeSubmission {
    let id: UInt64
    let generation: UInt64
    let pts: Int64
    let submittedNs: UInt64
    let token: EncodeSlotCompletionToken
}

struct SingleEncoderCallbackRetirement {
    let submission: SingleEncodeSubmission?
    let generationWasCurrent: Bool
}

struct SingleEncodeSubmissionLedger {
    private var submissionsByID: [UInt64: SingleEncodeSubmission] = [:]

    mutating func register(_ submission: SingleEncodeSubmission) {
        submissionsByID[submission.id] = submission
    }

    mutating func retire(id: UInt64) -> SingleEncodeSubmission? {
        submissionsByID.removeValue(forKey: id)
    }

    mutating func retireCallbackAndRecordHealthProgress(
        id: UInt64,
        callbackGeneration: UInt64,
        currentGeneration: UInt64,
        callbackNs: UInt64,
        isValidOutput: Bool,
        healthState: inout SingleEncoderHealthState,
        lastValidOutputNs: inout UInt64?
    ) -> SingleEncoderCallbackRetirement {
        let submission = submissionsByID.removeValue(forKey: id)
        let generationWasCurrent = callbackGeneration == currentGeneration
        if let submission,
           submission.generation == callbackGeneration,
           generationWasCurrent,
           isValidOutput {
            lastValidOutputNs = max(lastValidOutputNs ?? 0, callbackNs)
            healthState.recordValidOutput(at: callbackNs)
        }
        return .init(
            submission: submission,
            generationWasCurrent: generationWasCurrent
        )
    }

    mutating func reclaim(generation: UInt64) -> [SingleEncodeSubmission] {
        let reclaimed = submissionsByID.values
            .filter { $0.generation == generation }
            .sorted { $0.id < $1.id }
        for submission in reclaimed {
            submissionsByID.removeValue(forKey: submission.id)
        }
        return reclaimed
    }

    func oldestSubmissionNs(generation: UInt64) -> UInt64? {
        submissionsByID.values
            .filter { $0.generation == generation }
            .map(\.submittedNs)
            .min()
    }
}
