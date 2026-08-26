import Foundation

@main
struct EncodePolicyTests {
    static func main() {
        precondition(encodeInFlightLimit(width: 3_840, height: 2_160) == 4)
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
    }
}
