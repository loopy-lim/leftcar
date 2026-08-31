import Foundation
import VideoToolbox

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
    }
}
