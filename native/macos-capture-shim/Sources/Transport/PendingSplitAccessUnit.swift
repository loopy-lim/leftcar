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
    let queuedNs: UInt64
    let dropRightForTest: Bool

    init(
        sequence: UInt64,
        generation: UInt64,
        lease: SplitFlowLease,
        left: SplitEncodedPayload,
        right: SplitEncodedPayload,
        isKeyframe: Bool,
        isRecoveryKeyframe: Bool,
        queuedNs: UInt64,
        dropRightForTest: Bool = false
    ) {
        self.sequence = sequence
        self.generation = generation
        self.lease = lease
        self.left = left
        self.right = right
        self.isKeyframe = isKeyframe
        self.isRecoveryKeyframe = isRecoveryKeyframe
        self.queuedNs = queuedNs
        self.dropRightForTest = dropRightForTest
    }

    var bytes: Int {
        left.annexB.count + right.annexB.count
    }
}
