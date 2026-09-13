import Foundation

@main
struct SplitPolicyTests {
    static func expect(_ condition: @autoclosure () -> Bool, _ message: String = "assertion failed") {
        if !condition() { fputs("FAIL: \(message)\n", stderr); exit(1) }
    }
    static func main() {
        if CommandLine.arguments.contains("packetization") {
            testPacketizationFailureAfterExpiry()
            return
        }
        testRecoveryTransitionSeedsBeforeScheduledDrain()
        testOverlappingRecoveryCleanup()
        testSameEpisodeCleanupPreservesBoundary()
        testPacketizationFailureAfterExpiry()
        var confirmationFlow = SplitFlowControlState(capacity: 5)
        var confirmationPairs = SplitPairLifecycleState()
        let confirmationLease = confirmationFlow.admit()!
        let confirmation = confirmationPairs.admit(lease: confirmationLease, maximumInFlightPairs: 5)!
        guard case .recover = confirmationPairs.completeEncodedPair(
            sequence: confirmation.sequence,
            leftRequested: true, rightRequested: true,
            leftKeyframe: true, rightKeyframe: false
        ) else { fatalError("unconfirmed paired IDR must not emit") }
        let confirmationRecovery = confirmationPairs.beginPairedRecovery()
        expect(confirmationRecovery.releasedPairCount == 1, "unconfirmed IDR must retain its slot for recovery retirement")
        expect(confirmationRecovery.releasedLeases == [confirmationLease])
        expect(confirmationFlow.admit() == nil)
        print("PASS: unconfirmed paired IDR cannot emit or lose resource accounting")
        for (leftRequested, rightRequested, leftIDR, rightIDR, accepted) in [
            (true, true, false, false, false), // mirrored failed IDR
            (true, true, true, true, true),
            (false, false, true, false, false), // unexplained asymmetry
            (true, false, true, false, true), // requested per-tile recovery
            (false, true, false, true, true),
            (false, false, false, false, true)
        ] {
            var lifecycle = SplitPairLifecycleState()
            let admission = lifecycle.admit(maximumInFlightPairs: 1)!
            let result = lifecycle.completeEncodedPair(
                sequence: admission.sequence,
                leftRequested: leftRequested, rightRequested: rightRequested,
                leftKeyframe: leftIDR, rightKeyframe: rightIDR
            )
            switch result {
            case .emit: expect(accepted && lifecycle.inFlightPairs == 0)
            case .recover: expect(!accepted && lifecycle.beginPairedRecovery().releasedPairCount == 1)
            }
        }
        print("PASS: paired, mirrored and per-tile keyframe contracts preserve retirement")

        var barrier = EncodedPairAssembler<Data>(frameBudgetNs: 8, maximumRetainedSequences: 5)
        var flow = SplitFlowControlState(capacity: 5)
        var pairs = SplitPairLifecycleState()
        let bootLease = flow.admit()!
        let boot = pairs.admit(lease: bootLease, maximumInFlightPairs: 5)!
        _ = pairs.completePair(sequence: boot.sequence)
        _ = flow.complete(bootLease)
        let lostLease = flow.admit()!
        let lost = pairs.admit(lease: lostLease, maximumInFlightPairs: 5)!
        let dependentLease = flow.admit()!
        let dependent = pairs.admit(lease: dependentLease, maximumInFlightPairs: 5)!
        expect(!dependent.requestKeyframe)
        expect(barrier.insert(.init(side: .left, sequence: lost.sequence, value: Data([30]), valid: true), nowNs: 0) == .wait)
        expect(barrier.insert(.init(side: .right, sequence: dependent.sequence, value: Data([31]), valid: true), nowNs: 5) == .wait)
        expect(barrier.takeExpiredSequences(nowNs: 8) == [lost.sequence])
        let recovery = pairs.expirePairs(sequences: [lost.sequence], generation: lost.generation, barrier: &barrier)!
        expect(barrier.retainedValueCount == 0, "expiry must release dependent retained buffers")
        expect(recovery.releasedPairCount == 2, "expiry must retire all admitted dependent encode slots")
        expect(Set(recovery.releasedLeases) == Set([lostLease, dependentLease]))
        expect(dependent.generation != pairs.recoveryGeneration, "late dependent callback must be fenced")
        expect(pairs.completePair(sequence: dependent.sequence) == nil, "late callback must not release a slot twice")
        expect(pairs.expirePairs(sequences: [lost.sequence], generation: lost.generation, barrier: &barrier) == nil)
        print("PASS: soft expiry retires dependent generation and late callbacks once")

        // This is the capture adapter's actual generation transition and carrier
        // queue mutation, exercised with synthetic payloads and no capture APIs.
        expect(flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true))
        expect(!flow.accepts(lostLease) && !flow.accepts(dependentLease), "old packetization and send leases must be fenced")
        expect(!flow.complete(lostLease) && !flow.complete(dependentLease))
        var pending = [Data]()
        let carrier = Data([7, 8, 9])
        expect(seedSplitRecoveryCarrier(pending: &pending, carrier: carrier))
        expect(pending == [Data([7, 8, 9])], "idle recovery must have an immediate retained carrier")
        expect(!seedSplitRecoveryCarrier(pending: &pending, carrier: Data([1])))
        expect(pending == [Data([7, 8, 9])], "repeat recovery must preserve pending capture")
        let boundaryLease = flow.admit()!
        let boundary = pairs.admit(lease: boundaryLease, maximumInFlightPairs: 5)!
        expect(boundary.requestKeyframeLeft && boundary.requestKeyframeRight)
        expect(flow.admit() == nil, "dependent delta must wait for the paired IDR")
        expect(!flow.beginRecoveryIfNeeded(invalidatePendingBoundary: false))
        expect(!flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true, failedLease: dependentLease), "late old send failure must not restart recovery")
        expect(flow.accepts(boundaryLease), "repeat recovery must not cancel its own boundary")
        expect(flow.admit() == nil)
        guard case let .emit(confirmedLease) = pairs.completeEncodedPair(
            sequence: boundary.sequence,
            leftRequested: true, rightRequested: true,
            leftKeyframe: true, rightKeyframe: true
        ) else { fatalError("confirmed paired IDR must emit") }
        expect(confirmedLease == boundaryLease)
        expect(flow.complete(boundaryLease))
        expect(!flow.complete(boundaryLease), "send completion releases once")
        let resumed = flow.admit()!
        expect(!resumed.isRecoveryBoundary)
        print("PASS: flow fences old sends, coalesces recovery and seeds idle carrier")

        // Expiry of the boundary itself is allowed to retry; duplicate/old timers
        // are rejected by the lifecycle generation before they reach the adapter.
        expect(flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true))
        let retryLease = flow.admit()!
        let retry = pairs.admit(lease: retryLease, maximumInFlightPairs: 5)!
        let failedBoundary = pairs.expirePairs(sequences: [retry.sequence], generation: retry.generation, barrier: &barrier)!
        expect(failedBoundary.releasedPairCount == 1)
        expect(pairs.expirePairs(sequences: [retry.sequence], generation: retry.generation, barrier: &barrier) == nil)
        expect(flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true))
        expect(!flow.complete(retryLease))
        expect(flow.admit()!.isRecoveryBoundary)
        pending.removeAll()
        expect(!seedSplitRecoveryCarrier(pending: &pending, carrier: Optional<Data>.none))
        expect(pending.isEmpty)
        print("PASS: failed IDR retries once and missing carrier does not invent a capture")

        print("PASS: production expiry policy releases synthetic retained half-pairs")
    }
    static func testRecoveryTransitionSeedsBeforeScheduledDrain() {
        var flow = SplitFlowControlState(capacity: 3)
        let boot = flow.admit()!
        expect(flow.complete(boot))
        var captures = [Data]()
        expect(beginSplitRecovery(
            flow: &flow, invalidatePendingBoundary: true,
            pendingCaptures: &captures, carrier: Data([1, 2, 3])
        ) != nil)
        // An already scheduled drain runs before network cleanup. Its actual
        // admission/dequeue inputs must contain a frame, so it cannot take the
        // empty-capture branch that completes an unencoded boundary lease.
        let boundary = flow.admit()!
        expect(!captures.isEmpty, "scheduled drain: recovery transition publishes a retained frame atomically")
        expect(captures.removeFirst() == Data([1, 2, 3]))
        var pairs = SplitPairLifecycleState()
        let admission = pairs.admit(lease: boundary, maximumInFlightPairs: 3)!
        expect(admission.requestKeyframeLeft && admission.requestKeyframeRight)
        expect(flow.recoveryBoundaryPending && flow.admit() == nil, "scheduled drain: unconfirmed IDR must keep dependent admission closed")
        print("PASS: transition seeds retained capture before an already scheduled idle drain")
    }

    static func testOverlappingRecoveryCleanup() {
        var flow = SplitFlowControlState(capacity: 3)
        let original = flow.admit()!
        expect(flow.complete(original), "overlap: establish a running stream")
        let old = flow.admit()!
        // A begins and pauses before acquiring network ownership.
        var captures = [Data]()
        let episodeA = beginSplitRecovery(
            flow: &flow, invalidatePendingBoundary: true, failedLease: old,
            pendingCaptures: &captures, carrier: Data([9])
        )!
        expect(captures == [Data([9])], "overlap: initial transition must seed before an idle encode drain")
        // B begins, cleans up and enqueues its paired IDR before A resumes.
        let episodeB = beginSplitRecovery(
            flow: &flow, invalidatePendingBoundary: true,
            pendingCaptures: &captures, carrier: Data([10])
        )!
        var queued = [old]
        expect(finishSplitRecoveryCleanup(generation: episodeB, flow: flow, pending: &queued, lease: { $0 }) == 1)
        let boundaryB = flow.admit()!
        expect(captures.removeFirst() == Data([9]), "overlap: keep the already pending carrier")
        queued.append(boundaryB)
        let cleanupA = finishSplitRecoveryCleanup(generation: episodeA, flow: flow, pending: &queued, lease: { $0 })
        expect(cleanupA == nil && queued == [boundaryB], "overlap: delayed A cleanup must not delete B's queued paired IDR")
        expect(flow.accepts(boundaryB) && flow.admit() == nil, "overlap: B keeps exactly one active boundary")
        expect(flow.complete(queued.removeFirst()), "overlap: surviving B boundary completes")
        expect(!flow.complete(boundaryB), "overlap: duplicate completion releases nothing")
        expect(flow.admit() != nil, "overlap: stream resumes without a stranded boundary")
        print("PASS: A transition, B transition/enqueue, A cleanup preserves B")
    }

    static func testSameEpisodeCleanupPreservesBoundary() {
        var flow = SplitFlowControlState(capacity: 3)
        let old = flow.admit()!
        expect(flow.complete(old))
        let stale = flow.admit()!
        expect(flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true))
        let episode = flow.recoveryGeneration
        let boundary = flow.admit()!
        var queued = [stale, boundary]
        expect(finishSplitRecoveryCleanup(generation: episode, flow: flow, pending: &queued, lease: { $0 }) == 1)
        expect(queued == [boundary], "same episode: cleanup must retain an already enqueued current IDR")
        expect(flow.complete(boundary))
        expect(finishSplitRecoveryCleanup(generation: episode, flow: flow, pending: &queued, lease: { $0 }) == nil, "completed episode: delayed cleanup must not reopen recovery")
        print("PASS: current queued and already sent boundaries survive delayed cleanup")
    }

    static func testPacketizationFailureAfterExpiry() {
        var flow = SplitFlowControlState(capacity: 3)
        let boot = flow.admit()!
        expect(flow.complete(boot))
        let old = flow.admit()!
        expect(flow.accepts(old), "packetization: old pair accepted before conversion")
        var fresh: SplitFlowLease?
        var recoveryStarts = 0
        let payload: (Data, Data)? = prepareSplitPairPayloads(
            lease: old,
            left: {
                // Expiry occurs after initial acceptance, during buffer conversion.
                expect(flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true))
                fresh = flow.admit()!
                return nil
            },
            right: { Data([2]) },
            onFailure: { failedLease in
                if flow.beginRecoveryIfNeeded(invalidatePendingBoundary: true, failedLease: failedLease) {
                    recoveryStarts += 1
                }
            }
        )
        expect(payload == nil, "packetization: conversion failure produces no payload")
        expect(recoveryStarts == 0 && flow.accepts(fresh!), "packetization: stale conversion failure must not cancel the new boundary")
        expect(flow.complete(fresh!), "packetization: new boundary can still complete")
        print("PASS: expiry during packetization cannot restart the newer recovery")
    }

}
