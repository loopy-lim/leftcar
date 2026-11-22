import Foundation

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
        case escalated
        case standingDown
    }

    let burstDatagrams: Int
    let fecParityShards: Int
    let reason: Reason
}

/// 연결 적응 사다리(2026-11-22). 자동(적응 페이싱) 세션은 유실 에피소드가
/// 반복될수록 보호를 단계적으로 올리고, 클린 윈도우마다 단계적으로 내린다.
///
/// - tier 0: 기본선(적용된 값, 예: 4/2)
/// - tier 1: 첫 유실 에피소드 — parity +1(예: 3)
/// - tier 2: 짧은 창 안의 두 번째 에피소드 — parity 4, burst 유지
/// - tier 3: 세 번째 이상 — parity 4 + burst 2. 단 hold(마지막 유실 +
///   2s) 동안만 유지한다. 이 조합은 4K IDR 배수를 70-120ms로 늘려 자가
///   유지 복구 루프를 만든 과거 사례라 상시 유지하지 않는다. 클린 5s마다
///   한 단계씩 내려가 기본선 복귀 시 에피소드 카운터를 리셋한다.
struct UdpBurstPolicyState {
    private static let parityCleanWindowNs: UInt64 = 5_000_000_000
    private static let recoveryHoldNs: UInt64 = 2_000_000_000
    private static let episodeWindowNs: UInt64 = 15_000_000_000

    let applied: AppliedUdpStability
    private(set) var currentBurstDatagrams: Int
    private(set) var currentFecParityShards: Int
    private var previousIncompleteAccessUnits: UInt32 = 0
    private var previousOneFrameGapEvents: UInt32 = 0
    private var previousMultiFrameGapEvents: UInt32 = 0
    private var cleanWindowStartedNs: UInt64?
    private var recoveryHoldUntilNs: UInt64 = 0
    private var lossEpisodeCount: Int = 0
    private var lastLossAtNs: UInt64?
    private var currentTier: Int = 0

    init(applied: AppliedUdpStability) {
        self.applied = applied
        self.currentBurstDatagrams = applied.burstDatagrams
        self.currentFecParityShards = applied.fecParityShards
    }

    private mutating func applyTier() {
        switch currentTier {
        case 0:
            currentFecParityShards = applied.fecParityShards
            currentBurstDatagrams = applied.burstDatagrams
        case 1:
            currentFecParityShards = min(applied.fecParityShards + 1, 4)
            currentBurstDatagrams = applied.burstDatagrams
        case 2:
            currentFecParityShards = max(applied.fecParityShards, 4)
            currentBurstDatagrams = applied.burstDatagrams
        default:
            currentFecParityShards = max(applied.fecParityShards, 4)
            currentBurstDatagrams = min(2, applied.burstDatagrams)
        }
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

        if observation.recoveryBoundarySent || receiverLoss {
            let now = observation.nowNs
            // 에피소드 판정: 마지막 유실 후 5s 이상 클린이었으면 새 에피소드다.
            // 직전 에피소드가 15s 창 밖이면 누적을 초기화한다.
            if let lastLossAtNs {
                if now.saturatingSubtract(lastLossAtNs) >= Self.parityCleanWindowNs {
                    lossEpisodeCount = now.saturatingSubtract(lastLossAtNs)
                        <= Self.episodeWindowNs
                        ? lossEpisodeCount + 1
                        : 1
                }
            } else {
                lossEpisodeCount = 1
            }
            lastLossAtNs = now
            let hold = now.addingReportingOverflow(Self.recoveryHoldNs)
            recoveryHoldUntilNs = hold.overflow ? .max : hold.partialValue
            cleanWindowStartedNs = now
            currentTier = max(currentTier, min(3, lossEpisodeCount))
            applyTier()
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: currentTier >= 2
                    ? .escalated
                    : (observation.recoveryBoundarySent ? .recoveryBoundary : .receiverLoss)
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
        // hold 이후: 클린 5s마다 한 단계씩 내려간다.
        if currentTier > 0,
           let lastCleanStartNs = cleanWindowStartedNs,
           observation.nowNs.saturatingSubtract(lastCleanStartNs)
            >= Self.parityCleanWindowNs {
            currentTier -= 1
            applyTier()
            cleanWindowStartedNs = observation.nowNs
            if currentTier == 0 {
                lossEpisodeCount = 0
                lastLossAtNs = nil
            }
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .standingDown
            )
        }
        if currentTier == 0 {
            return .init(
                burstDatagrams: currentBurstDatagrams,
                fecParityShards: currentFecParityShards,
                reason: .initial
            )
        }
        return .init(
            burstDatagrams: currentBurstDatagrams,
            fecParityShards: currentFecParityShards,
            reason: .lossHold
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
