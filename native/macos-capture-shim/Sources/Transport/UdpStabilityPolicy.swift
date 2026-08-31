import Foundation

enum UdpStabilityProfile: String, Equatable {
    case legacy
    case auto
    case responsive
    case balanced
    case stable
    case custom
}

struct AppliedUdpStability: Equatable {
    let profile: UdpStabilityProfile
    let burstDatagrams: Int
    let fecParityShards: Int
    let adaptivePacing: Bool

    static let legacy = AppliedUdpStability(
        profile: .legacy,
        burstDatagrams: 8,
        fecParityShards: 2,
        adaptivePacing: false
    )

    static func validated(
        profileName: String?,
        burstDatagrams: Int,
        fecParityShards: Int,
        adaptivePacing: Bool
    ) -> AppliedUdpStability? {
        guard let profileName,
              let profile = UdpStabilityProfile(rawValue: profileName),
              profile != .legacy,
              [2, 4, 8, 16].contains(burstDatagrams),
              [2, 4].contains(fecParityShards) else {
            return nil
        }
        if adaptivePacing && profile != .auto && profile != .custom {
            return nil
        }
        let applied = AppliedUdpStability(
            profile: profile,
            burstDatagrams: burstDatagrams,
            fecParityShards: fecParityShards,
            adaptivePacing: adaptivePacing
        )
        let canonical: Bool
        switch profile {
        case .auto:
            canonical = applied == .init(
                profile: .auto,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: true
            )
        case .responsive:
            canonical = applied == .init(
                profile: .responsive,
                burstDatagrams: 8,
                fecParityShards: 2,
                adaptivePacing: false
            )
        case .balanced:
            canonical = applied == .init(
                profile: .balanced,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: false
            )
        case .stable:
            canonical = applied == .init(
                profile: .stable,
                burstDatagrams: 2,
                fecParityShards: 4,
                adaptivePacing: false
            )
        case .custom:
            canonical = true
        case .legacy:
            canonical = false
        }
        return canonical ? applied : nil
    }
}

struct UdpBurstObservation: Equatable {
    let nowNs: UInt64
    let incompleteAccessUnits: UInt32
    let oneFrameGapEvents: UInt32
    let multiFrameGapEvents: UInt32
    let recoveryBoundarySent: Bool
}

struct UdpBurstDecision: Equatable {
    enum Reason: String, Equatable {
        case fixed
        case initial
        case receiverLoss
        case recoveryBoundary
        case recoveryHold
        case lossHold
        case cleanWindow
    }

    let burstDatagrams: Int
    let fecParityShards: Int
    let reason: Reason
}

struct UdpBurstPolicyState {
    private static let cleanWindowNs: UInt64 = 30_000_000_000
    private static let parityCleanWindowNs: UInt64 = 5_000_000_000
    private static let recoveryHoldNs: UInt64 = 2_000_000_000

    let applied: AppliedUdpStability
    private(set) var currentBurstDatagrams: Int
    private(set) var currentFecParityShards: Int
    private var previousIncompleteAccessUnits: UInt32 = 0
    private var previousOneFrameGapEvents: UInt32 = 0
    private var previousMultiFrameGapEvents: UInt32 = 0
    private var cleanWindowStartedNs: UInt64?
    private var recoveryHoldUntilNs: UInt64 = 0

    init(applied: AppliedUdpStability) {
        self.applied = applied
        self.currentBurstDatagrams = applied.burstDatagrams
        self.currentFecParityShards = applied.fecParityShards
    }

    mutating func observe(_ observation: UdpBurstObservation) -> UdpBurstDecision {
        guard applied.adaptivePacing else {
            currentBurstDatagrams = applied.burstDatagrams
            currentFecParityShards = applied.fecParityShards
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .fixed
            )
        }

        let receiverLoss = observation.incompleteAccessUnits > previousIncompleteAccessUnits
            || observation.oneFrameGapEvents > previousOneFrameGapEvents
            || observation.multiFrameGapEvents > previousMultiFrameGapEvents
        previousIncompleteAccessUnits = observation.incompleteAccessUnits
        previousOneFrameGapEvents = observation.oneFrameGapEvents
        previousMultiFrameGapEvents = observation.multiFrameGapEvents

        if observation.recoveryBoundarySent {
            let hold = observation.nowNs.addingReportingOverflow(
                Self.recoveryHoldNs
            )
            recoveryHoldUntilNs = hold.overflow ? .max : hold.partialValue
            cleanWindowStartedNs = observation.nowNs
            if applied.profile == .auto {
                // The direct LAN can carry a little more recovery data, but
                // parity four plus burst two made 4K IDRs take 70-120ms and
                // triggered a self-sustaining recovery loop. Keep the
                // throughput-friendly burst and add one temporary shard.
                currentFecParityShards = 3
            }
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .recoveryBoundary
            )
        }

        if receiverLoss {
            cleanWindowStartedNs = observation.nowNs
            if applied.profile == .auto {
                currentFecParityShards = 3
            }
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .receiverLoss
            )
        }

        if cleanWindowStartedNs == nil {
            cleanWindowStartedNs = observation.nowNs
        }
        if observation.nowNs < recoveryHoldUntilNs {
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .recoveryHold
            )
        }
        var returnedToBaselineParity = false
        if currentFecParityShards != applied.fecParityShards,
           let cleanWindowStartedNs,
           observation.nowNs.saturatingSubtract(cleanWindowStartedNs)
            >= Self.parityCleanWindowNs {
            currentFecParityShards = applied.fecParityShards
            returnedToBaselineParity = true
        }
        if currentBurstDatagrams == 2,
           let cleanWindowStartedNs,
           observation.nowNs.saturatingSubtract(cleanWindowStartedNs)
            >= Self.cleanWindowNs {
            currentBurstDatagrams = min(4, applied.burstDatagrams)
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .cleanWindow
            )
        }
        if returnedToBaselineParity {
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .cleanWindow
            )
        }
        if currentBurstDatagrams == 2 {
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .lossHold
            )
        }
        return .init(
            burstDatagrams: currentBurstDatagrams,
            fecParityShards: currentFecParityShards,
            reason: .initial
        )
    }
}

private extension UInt64 {
    func saturatingSubtract(_ other: UInt64) -> UInt64 {
        self >= other ? self - other : 0
    }
}

extension CaptureSession {
    func currentUdpBurstLimit() -> Int {
        stateLock.lock()
        defer { stateLock.unlock() }
        return motionAdjustedUdpBurstDatagrams(
            base: activeUdpBurstDatagrams,
            mode: adaptiveMotionState.mode(
                at: DispatchTime.now().uptimeNanoseconds
            )
        )
    }

    func currentUdpParityCount() -> Int {
        stateLock.lock()
        defer { stateLock.unlock() }
        return activeUdpFecParityShards
    }

    func selectedUdpParityCount(
        dataCount: Int,
        reducedLegacyParity: Bool,
        recovery: Bool,
        parityOverride: Int? = nil
    ) -> Int {
        if appliedUdpStability.profile == .legacy {
            return fecParityCount(
                dataCount: dataCount,
                reduced: reducedLegacyParity,
                recovery: recovery
            )
        }
        return fecParityCount(
            dataCount: dataCount,
            selectedParity: parityOverride ?? currentUdpParityCount(),
            recovery: recovery
        )
    }

    /// A fresh IDR generation must not inherit pacing debt from the damaged
    /// dependency chain. Automatic sessions temporarily add one parity shard
    /// without shrinking the normal LAN microburst.
    func observeUdpRecoveryBoundary() {
        let now = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        let decision = udpBurstPolicyState.observe(.init(
            nowNs: now,
            incompleteAccessUnits: receiverIncompleteAUs,
            oneFrameGapEvents: receiverOneFrameGapEvents,
            multiFrameGapEvents: receiverMultiFrameGapEvents,
            recoveryBoundarySent: true
        ))
        activeUdpBurstDatagrams = decision.burstDatagrams
        activeUdpFecParityShards = decision.fecParityShards
        activeUdpBurstReason = decision.reason.rawValue
        stateLock.unlock()
        nextUdpSendNs = now
    }
}
