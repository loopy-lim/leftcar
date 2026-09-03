import Foundation
import CoreGraphics

@main
struct CursorStreamTests {
    static func main() {
        let token = Data("session-token".utf8)
        let contentRect = CGRect(x: 0, y: 0, width: 1920, height: 1200)

        // Wire format: LCD1 | seq u32 BE | x u16 BE | y u16 BE | vis u8 | shape u8 | token
        var coordinator = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        coordinator.setToken(token)
        coordinator.setEnabled(true)
        coordinator.note(position: CGPoint(x: 960, y: 600), visible: true)
        // First packet after enable is immediate even without a tick.
        guard let packet = coordinator.packetDue(nowUs: 1) else {
            fatalError("expected immediate first cursor packet")
        }
        let expectedX = UInt16((960 / 1920.0 * 65535.0).rounded())
        let expectedY = UInt16((600 / 1200.0 * 65535.0).rounded())
        precondition(packet.count == 14 + token.count)
        precondition(packet.prefix(4) == Data("LCD1".utf8))
        precondition(packet[8] == UInt8(expectedX >> 8) && packet[9] == UInt8(truncatingIfNeeded: expectedX),
                     "x must be the bounds-normalized u16 big-endian value")
        precondition(packet[10] == UInt8(expectedY >> 8) && packet[11] == UInt8(truncatingIfNeeded: expectedY),
                     "y must be the bounds-normalized u16 big-endian value")
        precondition(packet[12] == 1, "visibility must be 1 for an on-screen cursor")
        precondition(packet[13] == 0, "shape is reserved and always 0")
        precondition(packet.suffix(token.count) == token)

        // Coalesced duplicate state produces no packet.
        precondition(coordinator.packetDue(nowUs: 1_000) == nil)
        precondition(coordinator.packetDue(nowUs: 1_000_000) == nil)

        // Movement emits at most one packet per polling tick (2x fps = 120Hz,
        // one interval = 1_000_000 / 120 = 8_333us).
        coordinator.note(position: CGPoint(x: 970, y: 600), visible: true)
        precondition(coordinator.packetDue(nowUs: 1_001) == nil, "must respect the 2xFPS interval")
        let second = coordinator.packetDue(nowUs: 9_000)
        precondition(second != nil, "due packet after one 120Hz interval")
        let firstSequence = UInt32(packet[4]) << 24 | UInt32(packet[5]) << 16
            | UInt32(packet[6]) << 8 | UInt32(packet[7])
        let secondSequence = UInt32(second![4]) << 24 | UInt32(second![5]) << 16
            | UInt32(second![6]) << 8 | UInt32(second![7])
        precondition(secondSequence == firstSequence &+ 1, "sequence must increment")

        // Off-screen cursor reports invisibility. The previous send stamped
        // nowUs=9_000, so the next tick opens at 9_000 + 8_333 = 17_333.
        coordinator.note(position: CGPoint(x: -50, y: 300), visible: false)
        let hidden = coordinator.packetDue(nowUs: 17_400)
        precondition(hidden?[12] == 0)

        // Normalization clamps coordinates outside the capture content rect.
        coordinator.note(position: CGPoint(x: 4_000, y: -20), visible: true)
        let clamped = coordinator.packetDue(nowUs: 26_000)
        precondition(clamped != nil, "out-of-bounds movement is still a state change")
        precondition(UInt16(clamped![8]) << 8 | UInt16(clamped![9]) == 65_535,
                     "x beyond the content rect clamps to 65535")
        precondition(UInt16(clamped![10]) << 8 | UInt16(clamped![11]) == 0,
                     "y before the content rect clamps to 0")

        // Disable stops packets; re-enable starts a fresh stream.
        coordinator.setEnabled(false)
        precondition(coordinator.packetDue(nowUs: 27_000) == nil)
        coordinator.setEnabled(true)
        precondition(coordinator.packetDue(nowUs: 27_001) != nil, "re-enable sends an immediate sample")

        // Samples are never emitted before an authenticated token is known.
        var unauthenticated = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        unauthenticated.setEnabled(true)
        unauthenticated.note(position: CGPoint(x: 10, y: 10), visible: true)
        precondition(unauthenticated.packetDue(nowUs: 1) == nil,
                     "no cursor samples without a session token")
        unauthenticated.setToken(token)
        precondition(unauthenticated.packetDue(nowUs: 1) != nil,
                     "token arrival flushes the pending sample")

        // Polling rate mirrors the LCI1 pointer policy.
        precondition(cursorPollingHz(fps: 0) == 30)
        precondition(cursorPollingHz(fps: 60) == 120)
        precondition(cursorPollingHz(fps: 90) == 180)
        precondition(cursorPollingHz(fps: 200) == 240)
        precondition(cursorPollingHz(fps: 240) == 240)

        print("cursor stream tests passed")
    }
}
