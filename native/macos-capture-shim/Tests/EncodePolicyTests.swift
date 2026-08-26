import Foundation

@main
struct EncodePolicyTests {
    static func main() {
        precondition(encodeInFlightLimit(width: 3_840, height: 2_160) == 5)
        precondition(encodeInFlightLimit(width: 2_560, height: 1_440) == 5)
        precondition(encodeInFlightLimit(width: 1_920, height: 1_080) == 2)
        precondition(shouldOffloadEncodedSample(width: 3_840, height: 2_160))
        precondition(!shouldOffloadEncodedSample(width: 1_920, height: 1_080))
        precondition(
            preferredVideoCodec(width: 3_840, height: 2_160, contentMode: "video") == .hevc
        )
        precondition(
            preferredVideoCodec(width: 1_920, height: 1_080, contentMode: "video") == .h264
        )
        precondition(
            preferredVideoCodec(width: 3_840, height: 2_160, contentMode: "interactive") == .h264
        )
        precondition(codecParameterSetCount(.h264) == 2)
        precondition(codecParameterSetCount(.hevc) == 3)
        precondition(fecParityCount(dataCount: 8, reduced: true) == 2)
        precondition(fecParityCount(dataCount: 3, reduced: true) == 1)
        precondition(
            shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: true,
                keyframeInFlight: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: true,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldStartNetworkRecovery(
                awaitingKeyframe: false,
                keyframeInFlight: false,
                keyframeQueued: true
            )
        )
        precondition(
            shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: false,
                keyframeQueued: false
            )
        )
        precondition(
            !shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: false,
                keyframeQueued: true
            )
        )
        precondition(
            !shouldRecoverAfterNetworkOverflow(
                incomingIsKeyframe: true,
                keyframeQueued: false
            )
        )
        precondition(
            recoveryEncodeGateExpired(
                startedNs: 1_000,
                nowNs: 751_001_000,
                timeoutNs: 750_000_000
            )
        )
        precondition(
            !recoveryEncodeGateExpired(
                startedNs: 1_000,
                nowNs: 750_000_999,
                timeoutNs: 750_000_000
            )
        )
        precondition(frameBudgetUs(fps: 60) == 16_667)
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 7_700_000,
                bytes: 100 * 1024,
                fps: 60
            ) >= 40_000_000
        )
        precondition(
            recoveryPacingBitrate(
                targetBitrate: 80_000_000,
                bytes: 100 * 1024,
                fps: 60
            ) == 80_000_000
        )
        let queuedFrames = [
            PendingEncodedFrame(
                data: Data(repeating: 0, count: 120),
                isKeyframe: false,
                isRecoveryKeyframe: false,
                queuedNs: 1_000
            ),
            PendingEncodedFrame(
                data: Data(repeating: 0, count: 80),
                isKeyframe: true,
                isRecoveryKeyframe: true,
                queuedNs: 2_000
            ),
        ]
        let snapshot = networkQueueSnapshot(frames: queuedFrames, nowNs: 2_500)
        precondition(snapshot.count == 2)
        precondition(snapshot.bytes == 200)
        precondition(snapshot.oldestAgeUs == 1)
    }
}
