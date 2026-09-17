import Foundation

struct UdpPacingObservation {
    let requestedUs: UInt64
    let waitedUs: UInt64
    let overshootUs: UInt64
    static let zero = UdpPacingObservation(requestedUs: 0, waitedUs: 0, overshootUs: 0)
}

/// Opt-in single-stream transport diagnostics. Contains timings and counts,
/// never frame contents, addresses, input events, or session credentials.
struct FrameSendTrace: Encodable {
    let schema = 1
    let traceSession: String
    let auID: UInt16
    let captureWallMs: UInt64
    let encodeWallMs: UInt64
    let sendStartNs: UInt64
    let sendEndNs: UInt64
    let queuedUs: UInt64?
    let preparationUs: UInt64
    let pacingRequestedUs: UInt64
    let pacingWaitUs: UInt64
    let pacingOvershootUs: UInt64
    // Includes retransmit-ring bookkeeping and AEAD sealing around sendto.
    let sendCallUs: UInt64
    let bytes: Int
    let datagrams: Int
    let expectedDatagrams: Int
    let keyframe: Bool
    let recovery: Bool
    let succeeded: Bool

    static func wallMs(_ data: Data, offset: Int) -> UInt64 {
        data[offset..<(offset + 8)].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
    }
}
