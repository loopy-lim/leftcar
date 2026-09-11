import Foundation
import CoreGraphics

@main
struct CursorStreamTests {
    static func main() {
        let contentRect = CGRect(x: 0, y: 0, width: 1920, height: 1200)

        // Wire format: LCD1 | seq u32 BE | x u16 BE | y u16 BE | vis u8 | shape u8
        // (the packet is sealed with the session media key at the socket
        // boundary, so the inline token the old format carried is gone).
        var coordinator = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        coordinator.setEnabled(true)
        coordinator.note(position: CGPoint(x: 960, y: 600), visible: true)
        // First packet after enable is immediate even without a tick.
        guard let packet = coordinator.packetDue(nowUs: 1) else {
            fatalError("expected immediate first cursor packet")
        }
        let expectedX = UInt16((960 / 1920.0 * 65535.0).rounded())
        let expectedY = UInt16((600 / 1200.0 * 65535.0).rounded())
        precondition(packet.count == 14,
                     "packet must be exactly the fixed 14 bytes")
        precondition(packet.prefix(4) == Data("LCD1".utf8), "magic must be LCD1")
        precondition(packet[8] == UInt8(expectedX >> 8) && packet[9] == UInt8(truncatingIfNeeded: expectedX),
                     "x must be the bounds-normalized u16 big-endian value")
        precondition(packet[10] == UInt8(expectedY >> 8) && packet[11] == UInt8(truncatingIfNeeded: expectedY),
                     "y must be the bounds-normalized u16 big-endian value")
        precondition(packet[12] == 1, "visibility must be 1 for an on-screen cursor")
        precondition(packet[13] == 0, "shape is reserved and always 0")

        // Coalesced duplicate state produces no packet.
        precondition(coordinator.packetDue(nowUs: 1_000) == nil,
                     "unchanged state must not emit a packet")
        precondition(coordinator.packetDue(nowUs: 1_000_000) == nil,
                     "unchanged state must not emit a packet even after a long idle")

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
        precondition(hidden?[12] == 0, "off-screen cursor must report visibility 0")

        // Normalization clamps coordinates outside the capture content rect.
        coordinator.note(position: CGPoint(x: 4_000, y: -20), visible: true)
        let clamped = coordinator.packetDue(nowUs: 26_000)
        precondition(clamped != nil, "out-of-bounds movement is still a state change")
        precondition(UInt16(clamped![8]) << 8 | UInt16(clamped![9]) == 65_535,
                     "x beyond the content rect clamps to 65535")
        precondition(UInt16(clamped![10]) << 8 | UInt16(clamped![11]) == 0,
                     "y before the content rect clamps to 0")

        // Non-finite coordinates collapse to 0 instead of trapping the
        // UInt16 conversion — the host-side mirror of the viewer's
        // normalized_axis is_finite guard.
        coordinator.note(position: CGPoint(x: CGFloat.nan, y: CGFloat.nan), visible: true)
        let nanPacket = coordinator.packetDue(nowUs: 35_000)
        precondition(nanPacket != nil, "NaN movement is still a state change")
        precondition(UInt16(nanPacket![8]) << 8 | UInt16(nanPacket![9]) == 0,
                     "NaN x must collapse to 0, not trap")
        precondition(UInt16(nanPacket![10]) << 8 | UInt16(nanPacket![11]) == 0,
                     "NaN y must collapse to 0, not trap")

        // Disable stops packets; re-enable starts a fresh stream.
        coordinator.setEnabled(false)
        precondition(coordinator.packetDue(nowUs: 36_000) == nil,
                     "disabled stream must not emit")
        coordinator.setEnabled(true)
        precondition(coordinator.packetDue(nowUs: 36_001) != nil, "re-enable sends an immediate sample")

        // Enabling a stream that never observed the cursor stays silent
        // instead of inventing a (0, 0, hidden) sample; the first real
        // observation flushes immediately.
        var freshStream = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        freshStream.setEnabled(true)
        precondition(freshStream.packetDue(nowUs: 1) == nil,
                     "enable without any observation must not fabricate a sample")
        freshStream.note(position: CGPoint(x: 100, y: 100), visible: true)
        precondition(freshStream.packetDue(nowUs: 1) != nil,
                     "the first observation flushes on the never-sent sentinel")

        // Observations while disabled are still recorded, so a later enable
        // flushes the current truth — never the stale pre-disable position.
        var toggled = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        toggled.setEnabled(true)
        toggled.note(position: CGPoint(x: 100, y: 100), visible: true)
        precondition(toggled.packetDue(nowUs: 1) != nil, "priming send for the toggle scenario")
        toggled.setEnabled(false)
        toggled.note(position: CGPoint(x: 1_000, y: 900), visible: false)
        toggled.setEnabled(false)
        toggled.setEnabled(true)
        let truth = toggled.packetDue(nowUs: 2)
        precondition(truth != nil, "re-enable must flush the recorded current state")
        let expectedTruthX = UInt16((1_000 / 1920.0 * 65535.0).rounded())
        let expectedTruthY = UInt16((900 / 1200.0 * 65535.0).rounded())
        precondition(UInt16(truth![8]) << 8 | UInt16(truth![9]) == expectedTruthX,
                     "re-enable must flush the position observed while disabled, not the stale one")
        precondition(UInt16(truth![10]) << 8 | UInt16(truth![11]) == expectedTruthY,
                     "re-enable must flush the position observed while disabled, not the stale one")
        precondition(truth![12] == 0, "re-enable flush must carry the current visibility")

        // Re-enabling an already-enabled stream is a no-op: it must not reset
        // the pacing deadline and allow an early duplicate send.
        var paced = CursorStreamCoordinator(fps: 60, bounds: contentRect)
        paced.setEnabled(true)
        paced.note(position: CGPoint(x: 10, y: 10), visible: true)
        precondition(paced.packetDue(nowUs: 10_000) != nil, "priming send for the pacing scenario")
        paced.setEnabled(true)
        paced.note(position: CGPoint(x: 500, y: 500), visible: true)
        precondition(paced.packetDue(nowUs: 10_001) == nil,
                     "a redundant enable must not reset the polling interval")


        // MediaSealer interop self-test: the wire layout must match
        // crates/secure-channel exactly — counter u64 BE ‖ tag 16B ‖ ct with
        // nonce 00{4} ‖ counter u64 BE — and the directional key derivation
        // must match secure_channel::media_keys byte for byte.
        let key = Data((0..<32).map { UInt8($0) })
        let derived = MediaKeyDerivation.directionalKeys(mediaKey: key)!
        precondition(derived.c2s.map { String(format: "%02x", $0) }.joined()
                        == "5608c4ec91f01a93afdd876da3419cafd5fc6862faaa6c15a008b16dab6ac72f",
                     "c2s derivation must match the Rust vector")
        precondition(derived.s2c.map { String(format: "%02x", $0) }.joined()
                        == "2c100b32a507ab3af07ec3d39a61df7072d1d97e22d0e43da19d44fabedaf182",
                     "s2c derivation must match the Rust vector")

        // The session pair models the host: seal() uses the derived s2c key
        // and open() the derived c2s key. A frame sealed under the raw
        // session key is rejected in both windows, and the session's own s2c
        // output is not openable by its c2s window.
        let sessionCrypto = MediaSessionCrypto(mediaKey: key)!
        let plaintext = Data("LCH1challenge-plaintext".utf8)
        guard let sealed = sessionCrypto.seal(plaintext) else {
            fatalError("seal of a small challenge must succeed")
        }
        precondition(sealed.count == plaintext.count + 24,
                     "sealed frame adds exactly counter + tag bytes")
        precondition(sealed.prefix(4) != Data("LCH1".utf8),
                     "wire bytes must not leak the plaintext prefix")
        var viewerRx = MediaSealer(mediaKey: derived.s2c)!
        precondition(viewerRx.open(sealed)! == plaintext,
                     "the viewer s2c sealer must open the session's frame")
        precondition(viewerRx.open(sealed) == nil,
                     "replaying the same frame must be rejected")
        precondition(sessionCrypto.seal(plaintext)! != sealed,
                     "a fresh counter must produce a different frame")
        var viewerTx = MediaSealer(mediaKey: derived.c2s)!
        guard let echoed = viewerTx.seal(plaintext) else {
            fatalError("viewer-side seal must succeed")
        }
        precondition(sessionCrypto.open(echoed)! == plaintext,
                     "the session must open the viewer's c2s echo")
        precondition(sessionCrypto.open(echoed) == nil,
                     "replaying the echo must be rejected")
        precondition(sessionCrypto.open(sealed) == nil,
                     "the session's own s2c frame is not openable by its c2s window")
        var rawKeySealer = MediaSealer(mediaKey: key)!
        precondition(sessionCrypto.open(rawKeySealer.seal(plaintext)!) == nil,
                     "a raw-key frame must not open under derived keys")
        precondition(viewerRx.open(rawKeySealer.seal(plaintext)!) == nil,
                     "a raw-key frame must not open under a derived window")
        var tampered = sealed
        tampered[8] ^= 0xff
        precondition(viewerRx.open(tampered) == nil,
                     "a forged frame must fail authentication")


        // Degenerate capture rects normalize every axis to 0.
        var degenerate = CursorStreamCoordinator(fps: 60, bounds: CGRect(x: 5, y: 7, width: 0, height: 0))
        degenerate.setEnabled(true)
        degenerate.note(position: CGPoint(x: 100, y: 100), visible: true)
        let collapsed = degenerate.packetDue(nowUs: 1)
        precondition(collapsed != nil, "degenerate bounds still deliver state changes")
        precondition(UInt16(collapsed![8]) << 8 | UInt16(collapsed![9]) == 0,
                     "a zero-extent x axis normalizes to 0")
        precondition(UInt16(collapsed![10]) << 8 | UInt16(collapsed![11]) == 0,
                     "a zero-extent y axis normalizes to 0")

        // Polling rate mirrors the LCI1 pointer policy, including the floor
        // and the clamp edges.
        precondition(cursorPollingHz(fps: 0) == 30, "zero fps must fall back to the 30Hz floor")
        precondition(cursorPollingHz(fps: 15) == 30, "fps below the floor clamps to 30Hz")
        precondition(cursorPollingHz(fps: 60) == 120, "2x fps for ordinary streams")
        precondition(cursorPollingHz(fps: 90) == 180, "2x fps for ordinary streams")
        precondition(cursorPollingHz(fps: 119) == 238, "2x fps just under the clamp edge")
        precondition(cursorPollingHz(fps: 120) == 240, "the clamp edge starts at 120fps")
        precondition(cursorPollingHz(fps: 200) == 240, "fps above the ceiling clamps to 240Hz")
        precondition(cursorPollingHz(fps: 240) == 240, "fps above the ceiling clamps to 240Hz")

        print("cursor stream tests passed")
    }
}
