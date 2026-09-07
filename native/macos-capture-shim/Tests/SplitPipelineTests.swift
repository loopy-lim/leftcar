import Foundation
import VideoToolbox
import CoreMedia
import CoreVideo
import Dispatch

@main
struct SplitPipelineTests {
    static func main() {
        precondition(!splitDiagnosticEnabled(environment: [:]))
        precondition(splitDiagnosticEnabled(environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "1"]))
        precondition(!splitDiagnosticEnabled(environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "true"]))
        precondition(
            splitPipelineStartupDecision(
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                environment: [:]
            ) == .failure("splitVertical diagnostics are disabled")
        )
        precondition(
            splitPipelineStartupDecision(
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: false,
                environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "1"]
            ) == .failure("splitVertical requires 3840x2160 at 60fps over direct UDP")
        )
        precondition(
            splitPipelineStartupDecision(
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                environment: ["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC": "1"]
            ) == .success(applied: .splitVertical)
        )

        let geometry = SplitGeometry.vertical4K
        precondition(geometry.fullWidth == 3_840)
        precondition(geometry.fullHeight == 2_160)
        precondition(geometry.left.lumaX == 0)
        precondition(geometry.left.chromaX == 0)
        precondition(geometry.left.width == 1_920)
        precondition(geometry.left.height == 2_160)
        precondition(geometry.right.lumaX == 1_920)
        precondition(geometry.right.chromaX == 960)
        precondition(geometry.right.width == 1_920)
        precondition(geometry.right.height == 2_160)

        precondition(
            PairAdmissionState(leftAvailable: true, rightAvailable: true).decision == .admitPair
        )
        precondition(
            PairAdmissionState(leftAvailable: true, rightAvailable: false).decision == .dropPair
        )
        precondition(
            PairAdmissionState(leftAvailable: false, rightAvailable: true).decision == .dropPair
        )

        var barrier = EncodedPairAssembler<String>(frameBudgetNs: 16_666_667)
        precondition(
            barrier.insert(
                .init(side: .left, sequence: 9, value: "L", valid: true),
                nowNs: 100
            ) == .wait
        )
        precondition(
            barrier.insert(
                .init(side: .right, sequence: 9, value: "R", valid: true),
                nowNs: 200
            ) == .emit(left: "L", right: "R")
        )
        precondition(barrier.retainedValueCount == 0)
        precondition(barrier.retainedSequenceCount == 0)

        _ = barrier.insert(
            .init(side: .left, sequence: 10, value: "L10", valid: true),
            nowNs: 1_000
        )
        _ = barrier.insert(
            .init(side: .left, sequence: 11, value: "L11", valid: true),
            nowNs: 2_000
        )
        precondition(barrier.retainedSequenceCount == 2)
        precondition(barrier.retainedValueCount == 2)
        precondition(
            barrier.expire(nowNs: 16_667_667) == .drop(requestPairedKeyframe: true)
        )
        precondition(barrier.retainedSequenceCount == 0)

        var capacityBarrier = EncodedPairAssembler<String>(frameBudgetNs: 16_666_667)
        _ = capacityBarrier.insert(
            .init(side: .right, sequence: 20, value: "R20", valid: true),
            nowNs: 20
        )
        _ = capacityBarrier.insert(
            .init(side: .right, sequence: 21, value: "R21", valid: true),
            nowNs: 21
        )
        precondition(
            capacityBarrier.insert(
                .init(side: .right, sequence: 22, value: "R22", valid: true),
                nowNs: 22
            ) == .drop(requestPairedKeyframe: true)
        )
        precondition(capacityBarrier.retainedSequenceCount == 0)

        var invalidBarrier = EncodedPairAssembler<String>(frameBudgetNs: 16_666_667)
        _ = invalidBarrier.insert(
            .init(side: .left, sequence: 30, value: "L30", valid: true),
            nowNs: 30
        )
        precondition(
            invalidBarrier.insert(
                .init(side: .right, sequence: 30, value: "invalid", valid: false),
                nowNs: 31
            ) == .drop(requestPairedKeyframe: true)
        )
        precondition(invalidBarrier.retainedSequenceCount == 0)

        let identity = SplitFrameIdentity(sequence: 42, ptsUs: 700_000)
        precondition(identity.sequence == 42)
        precondition(identity.ptsUs == 700_000)

        precondition(EncoderExperiment.parse("splitVertical") == .splitVertical)
        precondition(splitEncoderInFlightLimit(fps: 60) == 5)
        precondition(splitTileEntropyMode(prioritizeSpeed: true) == .cavlc)
        precondition(splitTileEntropyMode(prioritizeSpeed: false) == .cabac)
        precondition(SplitTileEncoderBackend.rtvc.usesLowLatencyRateControl)
        precondition(SplitTileEncoderBackend.rtvc.requestedEncoderID == nil)
        precondition(SplitTileEncoderBackend.rtvc.preparesEagerly)
        precondition(!SplitTileEncoderBackend.ave.usesLowLatencyRateControl)
        precondition(
            SplitTileEncoderBackend.ave.requestedEncoderID
                == "com.apple.videotoolbox.videoencoder.ave.avc"
        )
        precondition(!SplitTileEncoderBackend.ave.preparesEagerly)
        precondition(splitEncoderInFlightLimit(fps: 30) == 3)
        precondition(splitPairRetentionLimit(maximumInFlightPairs: 5) == 5)
        let callbackExpiryBudgetNs = splitPairCallbackExpiryBudgetNs(
            fps: 60,
            maximumInFlightPairs: 5
        )
        precondition(callbackExpiryBudgetNs == 83_333_335)
        var callbackBudgetBarrier = EncodedPairAssembler<String>(
            frameBudgetNs: callbackExpiryBudgetNs,
            maximumRetainedSequences: 5
        )
        _ = callbackBudgetBarrier.insert(
            .init(side: .left, sequence: 40, value: "L40", valid: true),
            nowNs: 0
        )
        precondition(
            callbackBudgetBarrier.expire(nowNs: 16_666_667) == .wait
        )
        precondition(
            callbackBudgetBarrier.expire(nowNs: 83_333_334) == .wait
        )
        precondition(
            callbackBudgetBarrier.expire(nowNs: 83_333_335)
                == .drop(requestPairedKeyframe: true)
        )

        var splitFlow = SplitFlowControlState(capacity: 5)
        let startupLease = splitFlow.admit()!
        precondition(startupLease.id == 0)
        precondition(startupLease.generation == 0)
        precondition(startupLease.isRecoveryBoundary)
        precondition(splitFlow.accepts(startupLease))
        precondition(splitFlow.activeCount == 1)
        precondition(splitFlow.admit() == nil)
        precondition(splitFlow.complete(startupLease))
        precondition(!splitFlow.complete(startupLease))

        let deltaLeases = (0..<5).map { _ in splitFlow.admit()! }
        precondition(deltaLeases.allSatisfy { !$0.isRecoveryBoundary })
        precondition(splitFlow.activeCount == 5)
        precondition(splitFlow.admit() == nil)

        let flowRecovery = splitFlow.beginRecovery()
        precondition(flowRecovery.generation == 1)
        precondition(flowRecovery.releasedLeaseCount == 5)
        precondition(splitFlow.activeCount == 0)
        precondition(deltaLeases.allSatisfy { !splitFlow.accepts($0) })
        let recoveryLease = splitFlow.admit()!
        precondition(recoveryLease.generation == 1)
        precondition(recoveryLease.isRecoveryBoundary)
        precondition(splitFlow.admit() == nil)
        precondition(splitFlow.complete(recoveryLease))
        let resumedLease = splitFlow.admit()!
        precondition(!resumedLease.isRecoveryBoundary)
        precondition(splitFlow.cancelAll() == 1)
        precondition(splitFlow.activeCount == 0)

        var leasedLifecycle = SplitPairLifecycleState()
        let lifecycleLease = SplitFlowLease(
            id: 100,
            generation: 0,
            isRecoveryBoundary: true
        )
        let leasedAdmission = leasedLifecycle.admit(
            lease: lifecycleLease,
            maximumInFlightPairs: 5
        )!
        precondition(leasedAdmission.lease == lifecycleLease)
        precondition(
            leasedLifecycle.completePair(sequence: leasedAdmission.sequence)
                == lifecycleLease
        )
        precondition(
            leasedLifecycle.completePair(sequence: leasedAdmission.sequence) == nil
        )
        let pendingLeaseA = SplitFlowLease(
            id: 101,
            generation: 0,
            isRecoveryBoundary: false
        )
        let pendingLeaseB = SplitFlowLease(
            id: 102,
            generation: 0,
            isRecoveryBoundary: false
        )
        _ = leasedLifecycle.admit(lease: pendingLeaseA, maximumInFlightPairs: 5)
        _ = leasedLifecycle.admit(lease: pendingLeaseB, maximumInFlightPairs: 5)
        precondition(
            Set(leasedLifecycle.beginPairedRecovery().releasedLeases)
                == Set([pendingLeaseA, pendingLeaseB])
        )

        // External IDR requests must preserve already admitted encoder work.
        var pairLifecycle = SplitPairLifecycleState()
        let firstPairAdmission = pairLifecycle.admit(maximumInFlightPairs: 5)!
        precondition(
            firstPairAdmission
                == SplitPairAdmission(sequence: 0, generation: 0, requestKeyframe: true)
        )
        let secondPairAdmission = pairLifecycle.admit(maximumInFlightPairs: 5)!
        precondition(
            secondPairAdmission
                == SplitPairAdmission(sequence: 1, generation: 0, requestKeyframe: false)
        )
        pairLifecycle.requestPairedKeyframe()
        precondition(pairLifecycle.inFlightPairs == 2)
        precondition(pairLifecycle.recoveryGeneration == 0)
        _ = pairLifecycle.completePair(sequence: firstPairAdmission.sequence)
        precondition(
            pairLifecycle.admit(maximumInFlightPairs: 5)
                == SplitPairAdmission(
                    sequence: 2,
                    generation: 0,
                    requestKeyframe: true
                )
        )

        // Internal encoder failures still invalidate the active generation.
        precondition(pairLifecycle.beginPairedRecovery().releasedPairCount == 2)
        precondition(pairLifecycle.inFlightPairs == 0)
        precondition(pairLifecycle.recoveryGeneration == 1)
        precondition(
            pairLifecycle.admit(maximumInFlightPairs: 5)
                == SplitPairAdmission(
                    sequence: 3,
                    generation: 1,
                    requestKeyframe: true
                )
        )
        precondition(splitCaptureQueueLimit(fps: 60) == 2)
        precondition(splitCaptureQueueLimit(fps: 30) == 1)

        // Recovery-boundary seeding: a recovery that begins while capture is
        // idle (empty pending queue) must submit the newest retained frame
        // immediately instead of waiting for the next ScreenCaptureKit
        // callback, which is unbounded on a static screen.
        precondition(
            splitRecoverySeedDecision(pendingCaptureCount: 2, hasCarrier: true)
                == .queueAlreadyPending
        )
        precondition(
            splitRecoverySeedDecision(pendingCaptureCount: 1, hasCarrier: false)
                == .queueAlreadyPending
        )
        precondition(
            splitRecoverySeedDecision(pendingCaptureCount: 0, hasCarrier: true)
                == .seedCarrier
        )
        precondition(
            splitRecoverySeedDecision(pendingCaptureCount: 0, hasCarrier: false)
                == .noCarrierAvailable
        )

        // The recovery boundary lease must remain the sole admission until the
        // paired IDR is sent: no capture is admitted behind it.
        do {
            var gate = SplitFlowControlState(capacity: 5)
            _ = gate.beginRecovery()
            let boundary = gate.admit()!
            precondition(boundary.isRecoveryBoundary)
            precondition(gate.admit() == nil)
            precondition(gate.admit() == nil)
            precondition(gate.complete(boundary))
            precondition(!gate.recoveryBoundaryPending)
            let resumed = gate.admit()!
            precondition(!resumed.isRecoveryBoundary)
        }
        precondition(SplitEncoderStrategy.parse(environment: [:]) == .dualAve)
        precondition(
            SplitEncoderStrategy.parse(
                environment: ["LEFTCAR_SPLIT_ENCODER_DIAGNOSTIC_MODE": "mirrorLeft"]
            ) == .mirrorLeft
        )
        precondition(
            SplitEncoderStrategy.parse(
                environment: ["LEFTCAR_SPLIT_ENCODER_DIAGNOSTIC_MODE": "dualAve"]
            ) == .dualAve
        )
        precondition(
            SplitEncoderStrategy.parse(
                environment: ["LEFTCAR_SPLIT_ENCODER_DIAGNOSTIC_MODE": "dualRtvc"]
            ) == .dualRtvc
        )
        precondition(
            SplitEncoderStrategy.parse(
                environment: ["LEFTCAR_SPLIT_ENCODER_DIAGNOSTIC_MODE": "unexpected"]
            ) == .dualAve
        )
        precondition(SplitEncoderStrategy.dualAve.tileBackend == .ave)
        precondition(SplitEncoderStrategy.dualAve.encoderMode == "splitVertical")
        precondition(shouldRunAdaptiveBitrate(encodedFrames: 60, fps: 60))
        precondition(!SplitEncoderStrategy.dualAve.isDiagnostic)
        precondition(SplitEncoderStrategy.dualRtvc.tileBackend == .rtvc)
        precondition(SplitEncoderStrategy.dualRtvc.isDiagnostic)
        precondition(SplitEncoderStrategy.mirrorLeft.tileBackend == .rtvc)
        precondition(!SplitEncoderStrategy.mirrorLeft.usesIndependentRightEncoder)
        precondition(
            verifiedDualAveEncoderPair(
                leftEncoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                leftHardware: true,
                rightEncoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                rightHardware: true
            )
        )
        precondition(
            !verifiedDualAveEncoderPair(
                leftEncoderID: "com.apple.videotoolbox.videoencoder.h264.rtvc",
                leftHardware: true,
                rightEncoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
                rightHardware: true
            )
        )
        precondition(
            verifiedDualAveConcurrentProbe(
                pairVerified: true,
                leftEncoded: true,
                rightEncoded: true
            )
        )
        precondition(
            !verifiedDualAveConcurrentProbe(
                pairVerified: true,
                leftEncoded: true,
                rightEncoded: false
            )
        )
        let interleaved = interleaveSplitTransmissions(
            left: [Data([1]), Data([2]), Data([3])],
            right: [Data([11]), Data([12])]
        )
        precondition(interleaved.map(\.side) == [.left, .left, .right, .left, .right])
        precondition(interleaved.map { $0.datagram.first! } == [1, 2, 11, 3, 12])
        let rightHeavy = interleaveSplitTransmissions(
            left: [Data([1]), Data([2])],
            right: [Data([11]), Data([12]), Data([13]), Data([14])]
        )
        precondition(
            rightHeavy.map(\.side) == [.right, .right, .right, .left, .right, .left]
        )
        precondition(
            rightHeavy.map { $0.datagram.first! } == [11, 12, 13, 1, 14, 2]
        )
        let shortAuDatagram = Data([0x47, 0x01])
        precondition(
            addSingletonFecTailUdpRedundancy(
                primary: [shortAuDatagram],
                assembled: [shortAuDatagram]
            ) == [shortAuDatagram, shortAuDatagram]
        )
        let multiFragmentDatagrams = [Data([0x47, 0x01]), Data([0x47, 0x02])]
        precondition(
            addSingletonFecTailUdpRedundancy(
                primary: multiFragmentDatagrams,
                assembled: multiFragmentDatagrams
            ) == multiFragmentDatagrams
        )
        let singletonTail = (0..<9).map { Data([0x47, UInt8($0)]) }
        precondition(
            addSingletonFecTailUdpRedundancy(
                primary: singletonTail,
                assembled: singletonTail
            ) == singletonTail + [singletonTail.last!]
        )
        let protectedTail = (0..<10).map { Data([0x47, UInt8($0)]) }
        precondition(
            addSingletonFecTailUdpRedundancy(
                primary: protectedTail,
                assembled: protectedTail
            ) == protectedTail
        )
        precondition(
            splitPacingBurstLimit(
                configured: 4,
                leftFragmentCount: 1,
                rightFragmentCount: 1
            ) == 2
        )
        precondition(
            udpPacingBurstRanges(
                datagramCount: 4,
                maxDatagrams: splitPacingBurstLimit(
                    configured: 4,
                    leftFragmentCount: 1,
                    rightFragmentCount: 1
                )
            ) == [0..<2, 2..<4]
        )
        precondition(
            splitPacingBurstLimit(
                configured: 4,
                leftFragmentCount: 9,
                rightFragmentCount: 8
            ) == 2
        )
        precondition(
            splitPacingBurstLimit(
                configured: 4,
                leftFragmentCount: 8,
                rightFragmentCount: 8
            ) == 4
        )
        precondition(
            splitPacingBurstLimit(
                configured: 4,
                leftFragmentCount: 1,
                rightFragmentCount: nil
            ) == 1
        )

        var splitWireSequence = SplitWireSequence(next: UInt16.max)
        precondition(splitWireSequence.allocate() == UInt16.max)
        precondition(splitWireSequence.allocate() == 0)
        let splitPayload = SplitEncodedPayload(
            config: nil,
            annexB: Data([0, 0, 0, 1, 0x65]),
            captureWallMs: 10,
            encodeWallMs: 20
        )
        let splitFrame = splitWireFrame(payload: splitPayload, auID: 42)
        precondition(splitFrame.count == 26)
        precondition(splitFrame[0] == 0x47)
        precondition(splitFrame[1] == 42)
        precondition(splitFrame[2] == 0)
        precondition(splitFrame[3...4] == Data([0x4C, 0x32]))
        precondition(splitFrame[5...12] == Data([0, 0, 0, 0, 0, 0, 0, 10]))
        precondition(splitFrame[13...20] == Data([0, 0, 0, 0, 0, 0, 0, 20]))
        precondition(splitFrame.suffix(5) == splitPayload.annexB)
        var splitFeedback = Data(repeating: 0, count: 52)
        var keyframeGapRecoveries = UInt32(7).bigEndian
        var deltaGapRecoveries = UInt32(8).bigEndian
        withUnsafeBytes(of: &keyframeGapRecoveries) {
            splitFeedback.append(contentsOf: $0)
        }
        withUnsafeBytes(of: &deltaGapRecoveries) {
            splitFeedback.append(contentsOf: $0)
        }
        precondition(
            splitGapRecoveryCounts(splitFeedback)!
                == (keyframe: 7, delta: 8)
        )
        precondition(splitGapRecoveryCounts(Data(repeating: 0, count: 52)) == nil)

        var unsentWireSequence = SplitWireSequence(next: 42)
        let unsentLease = SplitFlowLease(
            id: 200,
            generation: 1,
            isRecoveryBoundary: false
        )
        _ = PendingSplitAccessUnit(
            sequence: 12,
            generation: 1,
            lease: unsentLease,
            left: splitPayload,
            right: splitPayload,
            isKeyframe: false,
            isRecoveryKeyframe: false,
            queuedNs: 1
        )
        precondition(unsentWireSequence.allocate() == 42)
        precondition(
            splitTileHardwareEncoderVerified(
                queryStatus: kVTPropertyNotSupportedErr,
                queriedHardware: nil
            )
        )
        precondition(
            !splitTileHardwareEncoderVerified(
                queryStatus: noErr,
                queriedHardware: false
            )
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .splitVertical,
                width: 3_840,
                height: 2_160,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                splitDiagnosticEnabled: true
            ) == .success(applied: .splitVertical)
        )
        precondition(
            encoderExperimentStartupDecision(
                requested: .splitVertical,
                width: 2_560,
                height: 1_440,
                fps: 60,
                mediaTransport: "udp",
                hasEncoderPixelBufferPool: true,
                splitDiagnosticEnabled: true
            ) == .failure(
                "splitVertical requires 3840x2160 at 60fps over direct UDP"
            )
        )

        var disabledFault = SplitFaultInjection(environment: [:])
        precondition(!disabledFault.shouldDropRightAu())
        precondition(disabledFault.injectedDrops == 0)

        var countdownFault = SplitFaultInjection(
            environment: ["LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER": "2"]
        )
        precondition(!countdownFault.shouldDropRightAu())
        precondition(countdownFault.shouldDropRightAu())
        precondition(!countdownFault.shouldDropRightAu())
        precondition(countdownFault.injectedDrops == 1)

        var invalidFault = SplitFaultInjection(
            environment: ["LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER": "0"]
        )
        precondition(!invalidFault.shouldDropRightAu())
        precondition(invalidFault.injectedDrops == 0)

        // Regression: strict monotonic encode submission PTS.
        // Apple's VTCompressionSessionEncodeFrame requires every presentation
        // timestamp in a session to be strictly greater than the previous
        // one. The recovery carrier is a replayed frame whose source PTS was
        // already encoded by the same live sessions, so the submission clock
        // must absorb duplicates/regressions for the replay AND for the
        // following real captures.
        do {
            let ptsA = CMTime(value: 1_000, timescale: 1_000)
            let ptsB = CMTime(value: 2_000, timescale: 1_000)
            // Fresh encoder session: the first submission keeps its source PTS.
            let first = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: ptsA,
                lastSubmittedPTS: nil
            )
            precondition(CMTimeCompare(first, ptsA) == 0)
            // Normal advance: a source PTS strictly above last passes through.
            let advanced = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: ptsB,
                lastSubmittedPTS: first
            )
            precondition(CMTimeCompare(advanced, ptsB) == 0)
            // Repeated same carrier: the replayed PTS duplicates the last
            // submission, so the clock steps exactly one tick past it.
            let replayed = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: ptsB,
                lastSubmittedPTS: advanced
            )
            precondition(CMTimeCompare(replayed, advanced) > 0)
            precondition(replayed.value == advanced.value + 1)
            precondition(replayed.timescale == advanced.timescale)
            // Another replay keeps stepping strictly upward.
            let replayedAgain = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: ptsB,
                lastSubmittedPTS: replayed
            )
            precondition(CMTimeCompare(replayedAgain, replayed) > 0)
            // Next real frame whose source PTS is equal to the replayed clock
            // (screen stayed idle across the recovery) still submits strictly
            // above the last submission.
            let equalRealFrame = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: replayedAgain,
                lastSubmittedPTS: replayedAgain
            )
            precondition(CMTimeCompare(equalRealFrame, replayedAgain) > 0)
            // And one whose source PTS is lower than the replayed clock is
            // lifted past it instead of violating the session contract.
            let lowerRealFrame = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: ptsA,
                lastSubmittedPTS: equalRealFrame
            )
            precondition(CMTimeCompare(lowerRealFrame, equalRealFrame) > 0)
        }

        // Session reset: a recreated pipeline owns fresh VTCompressionSessions
        // with an empty submission clock, so a source PTS below the previous
        // session's last submission is accepted unchanged.
        do {
            let previousSessionLastPTS = CMTime(value: 99_999, timescale: 1_000)
            _ = previousSessionLastPTS
            let freshSessionSubmission = nextStrictlyMonotonicSubmissionPTS(
                sourcePTS: CMTime(value: 10, timescale: 1_000),
                lastSubmittedPTS: nil
            )
            precondition(
                CMTimeCompare(freshSessionSubmission, CMTime(value: 10, timescale: 1_000)) == 0
            )
        }

        // Live VideoToolbox probe (skipped when no hardware tile encoder is
        // available): replayed carrier PTS and a following lower/equal source
        // PTS must all encode successfully with strictly increasing submission
        // PTS, and a fresh encoder session must restart the clock.
        do {
            if let encoder = try? VideoToolboxTileEncoder(
                side: .left,
                width: 320,
                height: 240,
                fps: 60,
                bitrate: 1_000_000,
                backend: .rtvc
            ) {
                func submitProbe(_ pts: CMTime, encoder: VideoToolboxTileEncoder) -> TileEncodedSample {
                guard let pixelBuffer = makeProbePixelBuffer(width: 320, height: 240) else {
                    preconditionFailure("split probe pixel buffer")
                }
                let semaphore = DispatchSemaphore(value: 0)
                var outcome: Result<TileEncodedSample, TileEncoderError>?
                encoder.submit(
                    TileEncodeRequest(
                        side: .left,
                        frameSequence: 0,
                        pts: pts,
                        duration: .invalid,
                        captureNs: 1_234,
                        captureWallMs: 5_678,
                        recoveryGeneration: 0,
                        forceKeyframe: false,
                        pixelBuffer: pixelBuffer
                    )
                ) { result in
                    outcome = result
                    semaphore.signal()
                }
                _ = semaphore.wait(timeout: .now() + 10)
                guard let outcome, case let .success(sample) = outcome else {
                    preconditionFailure(
                        "split probe submit failed at pts \(pts.value): \(String(describing: outcome))"
                    )
                }
                // Honest age: the sample keeps the original capture timestamps.
                precondition(sample.captureNs == 1_234)
                precondition(sample.captureWallMs == 5_678)
                return sample
            }
            let carrierPTS = CMTime(value: 10_000, timescale: 1_000)
            let firstSample = submitProbe(carrierPTS, encoder: encoder)
            precondition(firstSample.pts.value == 10_000)
            // Replayed carrier: same PTS must still encode, submitted one tick up.
            let replaySample = submitProbe(carrierPTS, encoder: encoder)
            precondition(replaySample.pts.value == 10_001)
            // Lower/equal real frame after the replay still encodes.
            let lowerSample = submitProbe(CMTime(value: 10_000, timescale: 1_000), encoder: encoder)
            precondition(lowerSample.pts.value == 10_002)
            let olderSample = submitProbe(CMTime(value: 9_000, timescale: 1_000), encoder: encoder)
            precondition(olderSample.pts.value == 10_003)
                encoder.invalidate()
                // Session reset: a new encoder session accepts the same low PTS.
                if let freshEncoder = try? VideoToolboxTileEncoder(
                    side: .left,
                    width: 320,
                    height: 240,
                    fps: 60,
                    bitrate: 1_000_000,
                    backend: .rtvc
                ) {
                    let freshSample = submitProbe(CMTime(value: 9_000, timescale: 1_000), encoder: freshEncoder)
                    precondition(freshSample.pts.value == 9_000)
                    freshEncoder.invalidate()
                }
            } else {
                print("split probe: hardware tile encoder unavailable; skipping live PTS probe")
            }
        }

        // Production carrier seeding wiring (not just the seed decision):
        // beginRecovery() followed by seedSplitRecoveryCarrierLocked() must
        // queue the newest retained carrier for the boundary drain with its
        // original capture timestamps, and stop()'s clearPendingCapturesLocked()
        // must clear the carrier so a stopped session cannot reseed it.
        do {
            let session = CaptureSession(
                targetAddr: sockaddr_in(),
                targetPort: 0,
                targetLabel: "split-seed-test",
                width: 3_840,
                height: 2_160,
                fps: 60,
                backend: .screenCaptureKit,
                mediaTransport: .udp,
                requestedEncoderExperiment: .splitVertical
            )
            guard let carrierBuffer = makeProbePixelBuffer(width: 4, height: 4) else {
                preconditionFailure("carrier pixel buffer")
            }
            let carrier = PendingCaptureFrame(
                pixelBuffer: carrierBuffer,
                pts: CMTime(value: 42_000, timescale: 1_000),
                duration: .invalid,
                callbackNs: 777,
                captureWallMs: 123_456
            )
            session.captureLock.lock()
            // Recovery with no carrier and no pending captures: seeding is a
            // no-op and the boundary lease still gates the flow.
            _ = session.splitFlowState.beginRecovery()
            session.seedSplitRecoveryCarrierLocked()
            precondition(session.pendingSplitCaptures.isEmpty)
            let boundary = session.splitFlowState.admit()!
            precondition(boundary.isRecoveryBoundary)
            precondition(session.splitFlowState.admit() == nil)
            precondition(session.splitFlowState.complete(boundary))

            // Production wiring: handlePixelBuffer keeps the newest frame as
            // the recovery carrier while the capture queue drains normally.
            _ = session.enqueuePendingCaptureLocked(carrier)
            session.splitRecoveryCarrier = carrier
            precondition(session.pendingSplitCaptures.count == 1)

            // Recovery while a capture is already pending: no duplicate seed.
            _ = session.splitFlowState.beginRecovery()
            session.seedSplitRecoveryCarrierLocked()
            precondition(session.pendingSplitCaptures.count == 1)
            _ = session.dequeuePendingCaptureLocked()

            // Recovery on an idle screen: the seeded boundary is exactly the
            // carrier, with its original honest age and identity.
            session.seedSplitRecoveryCarrierLocked()
            precondition(session.pendingSplitCaptures.count == 1)
            let seeded = session.dequeuePendingCaptureLocked()!
            precondition(CMTimeCompare(seeded.pts, carrier.pts) == 0)
            precondition(seeded.callbackNs == carrier.callbackNs)
            precondition(seeded.captureWallMs == carrier.captureWallMs)

            // Repeated idle recoveries reseed the same carrier each time;
            // this is the replay loop the encoder submission clock absorbs.
            session.seedSplitRecoveryCarrierLocked()
            let reseeded = session.dequeuePendingCaptureLocked()!
            precondition(CMTimeCompare(reseeded.pts, carrier.pts) == 0)
            precondition(reseeded.callbackNs == carrier.callbackNs)

            // Lifecycle clear wiring: stop() clears the carrier together with
            // the pending queue.
            session.clearPendingCapturesLocked()
            precondition(session.pendingSplitCaptures.isEmpty)
            precondition(session.splitRecoveryCarrier == nil)
            session.seedSplitRecoveryCarrierLocked()
            precondition(session.pendingSplitCaptures.isEmpty)
            session.captureLock.unlock()
        }

        // Regression: the wire cannot distinguish a
        // genuinely new loss episode from a delayed duplicate of the PLI that
        // produced the boundary just sent, so Host-side time-window gating is
        // forbidden. A genuine PLI arriving immediately after a boundary send
        // must trigger a fresh paired-IDR recovery, while the preexisting
        // pending-boundary gate still coalesces requests for a boundary that
        // has not completed yet.
        do {
            fputs("split genuine-PLI: block start\n", stderr)
            let session = CaptureSession(
                targetAddr: sockaddr_in(),
                targetPort: 0,
                targetLabel: "split-genuine-pli-test",
                width: 3_840,
                height: 2_160,
                fps: 60,
                backend: .screenCaptureKit,
                mediaTransport: .udp,
                requestedEncoderExperiment: .splitVertical
            )
            fputs("split genuine-PLI: session built\n", stderr)
            // Complete the startup pair so the session is in steady state.
            session.captureLock.lock()
            let startupBoundary = session.splitFlowState.admit()!
            precondition(startupBoundary.isRecoveryBoundary)
            precondition(session.splitFlowState.complete(startupBoundary))
            session.captureLock.unlock()

            // First PLI of an episode: the full recovery runs exactly once.
            session.beginSplitTransportRecovery(
                reason: "viewer or transport requested IDR"
            )
            session.stateLock.lock()
            let rebuildsAfterFirstPLI = session.recoveryKeyframes
            session.stateLock.unlock()
            precondition(rebuildsAfterFirstPLI == 1)
            fputs("split genuine-PLI: first PLI rebuilt once ok\n", stderr)

            // Recovery #1's boundary completes at send time: simulate the
            // finished send so recoveryBoundaryPending is false.
            session.captureLock.lock()
            let sentBoundary = session.splitFlowState.admit()!
            precondition(sentBoundary.isRecoveryBoundary)
            precondition(session.splitFlowState.complete(sentBoundary))
            session.captureLock.unlock()
            session.stateLock.lock()
            session.lastRecoverySendNs = DispatchTime.now().uptimeNanoseconds
            session.stateLock.unlock()

            // A genuine PLI arriving immediately after that send must start a
            // fresh recovery: no time-window suppression may delay it.
            session.beginSplitTransportRecovery(
                reason: "viewer or transport requested IDR"
            )
            session.stateLock.lock()
            let rebuildsAfterImmediatePLI = session.recoveryKeyframes
            session.stateLock.unlock()
            precondition(
                rebuildsAfterImmediatePLI == 2,
                "genuine PLI immediately after a boundary send was "
                    + "suppressed (recoveryKeyframes="
                    + "\(rebuildsAfterImmediatePLI))"
            )
            fputs("split genuine-PLI: immediate PLI rebuilt fresh ok\n", stderr)

            // Recovery #2's boundary is still pending (never sent here): the
            // preexisting pending-boundary gate coalesces the next request.
            session.beginSplitTransportRecovery(
                reason: "viewer or transport requested IDR"
            )
            session.stateLock.lock()
            let rebuildsAfterPending = session.recoveryKeyframes
            let suppressedByPending = session.recoveryRequestsSuppressed
            session.stateLock.unlock()
            precondition(rebuildsAfterPending == 2)
            precondition(suppressedByPending == 1)
            fputs("split genuine-PLI: pending boundary coalesced ok\n", stderr)

            // Internal boundary-invalidating failures always rebuild.
            session.beginSplitTransportRecovery(
                reason: "split pair send failed",
                invalidatePendingBoundary: true
            )
            session.stateLock.lock()
            let rebuildsAfterInvalidate = session.recoveryKeyframes
            session.stateLock.unlock()
            precondition(rebuildsAfterInvalidate == 3)
            fputs("split genuine-PLI: invalidate rebuilds ok\n", stderr)
        }
    }
}

private func makeProbePixelBuffer(width: Int, height: Int) -> CVPixelBuffer? {
    var buffer: CVPixelBuffer?
    guard CVPixelBufferCreate(
        kCFAllocatorDefault,
        width,
        height,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        [kCVPixelBufferMetalCompatibilityKey: true] as CFDictionary,
        &buffer
    ) == kCVReturnSuccess, let buffer else {
        return nil
    }
    CVPixelBufferLockBaseAddress(buffer, [])
    for plane in 0..<2 {
        if let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane) {
            let rowBytes = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane)
            let planeHeight = CVPixelBufferGetHeightOfPlane(buffer, plane)
            memset(base, plane == 0 ? 0x40 : 0x80, rowBytes * planeHeight)
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer, [])
    return buffer
}
