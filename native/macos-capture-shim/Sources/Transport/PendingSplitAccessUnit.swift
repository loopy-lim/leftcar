import Foundation

struct SplitEncodedPayload {
    let config: Data?
    let annexB: Data
    let captureWallMs: UInt64
    let encodeWallMs: UInt64
}

struct PendingSplitAccessUnit {
    let sequence: UInt64
    let generation: UInt64
    let lease: SplitFlowLease
    let left: SplitEncodedPayload
    let right: SplitEncodedPayload
    let isKeyframe: Bool
    let isRecoveryKeyframe: Bool
    var queuedNs: UInt64 = DispatchTime.now().uptimeNanoseconds
    var dropRightForTest: Bool = false

    var bytes: Int {
        left.annexB.count + right.annexB.count
    }
}
