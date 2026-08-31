import Foundation

struct SplitWireSequence {
    private var next: UInt16

    init(next: UInt16 = 0) {
        self.next = next
    }

    mutating func allocate() -> UInt16 {
        let value = next
        next &+= 1
        return value
    }
}

func splitWireFrame(payload: SplitEncodedPayload, auID: UInt16) -> Data {
    var frame = Data([0x47, UInt8(auID & 0xFF), UInt8(auID >> 8), 0x4C, 0x32])
    var captureWallMs = payload.captureWallMs.bigEndian
    var encodeWallMs = payload.encodeWallMs.bigEndian
    withUnsafeBytes(of: &captureWallMs) { frame.append(contentsOf: $0) }
    withUnsafeBytes(of: &encodeWallMs) { frame.append(contentsOf: $0) }
    frame.append(payload.annexB)
    return frame
}
