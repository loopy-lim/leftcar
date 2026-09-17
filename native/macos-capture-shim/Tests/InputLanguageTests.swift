import Foundation
import Darwin

@main
struct InputLanguageTests {
    static func main() {
        let sources = [
            InputLanguageSource(id: "com.apple.keylayout.ABC", languages: ["en"], selectable: true),
            InputLanguageSource(id: "com.apple.inputmethod.Korean.2SetKorean", languages: ["ko"], selectable: true),
            InputLanguageSource(id: "disabled", languages: ["ko"], selectable: false),
        ]
        assert(inputLanguageSource(2, current: sources[0].id, sources: sources) == sources[1].id)
        assert(inputLanguageSource(1, current: sources[1].id, sources: sources) == sources[0].id)
        assert(inputLanguageSource(2, current: sources[1].id, sources: sources) == sources[1].id)
        assert(inputLanguageSource(3, current: sources[0].id, sources: sources) == nil)
        assert(inputLanguageSource(2, current: nil, sources: [sources[2]]) == nil)
        let custom = InputLanguageSource(id: "user.korean", languages: ["ko-KR"], selectable: true)
        assert(inputLanguageSource(2, current: custom.id, sources: sources + [custom]) == custom.id)
        var pending = InputLanguageTransition()
        assert(pending.begin(sequence: 1))
        assert(!pending.begin(sequence: 1), "retries must not apply twice")
        assert(!pending.begin(sequence: 2), "wait for the first transition")
        pending.cancel()
        assert(!pending.complete(sequence: 1), "cancelled main-thread work must not advance ACKs")
        assert(pending.begin(sequence: 2))
        assert(pending.complete(sequence: 2))
        assert(!pending.complete(sequence: 2))
        testReceiverOrderingAndCancellation()
        testCapabilityReplyHasUdpDestination()
        assert(viewerConnectionAlive(feedback: 1, heartbeat: 8_000_000_000, now: 10_000_000_000))
        assert(!viewerConnectionAlive(feedback: 1, heartbeat: 8_000_000_000, now: 14_000_000_000))
        assert(!viewerConnectionAlive(feedback: 0, heartbeat: 0, now: 1))
        print("Input language source and transition tests passed")
    }

    static func testCapabilityReplyHasUdpDestination() {
        let fd = socket(AF_INET, SOCK_DGRAM, 0)
        assert(fd >= 0)
        defer { close(fd) }
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        assert(bound == 0)
        var size = socklen_t(MemoryLayout<sockaddr_in>.size)
        withUnsafeMutablePointer(to: &address) {
            _ = $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &size) }
        }
        let session = CaptureSession(targetAddr: address, targetPort: UInt16(bigEndian: address.sin_port),
            targetLabel: "language-capability-udp", width: 2560, height: 1440, fps: 60,
            backend: .screenCaptureKit, mediaKey: Data(repeating: 1, count: 32))
        _ = session.dispatchViewerControlCommand(Data("LCL?".utf8), fd: fd, destination: address)
        var buffer = [UInt8](repeating: 0, count: 128)
        var timeout = timeval(tv_sec: 0, tv_usec: 100_000)
        _ = setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        let received = recv(fd, &buffer, buffer.count, 0)
        assert(received == 29, "capability reply must reach the requesting UDP socket (received=\(received), errno=\(errno))")
        session.sendInputStatus(fd: fd)
        assert(recv(fd, &buffer, buffer.count, 0) == 29, "initial input status must reach the media listener before the first click")
    }

    static func testReceiverOrderingAndCancellation() {
        let session = CaptureSession(targetAddr: sockaddr_in(), targetPort: 5001,
            targetLabel: "input-language-test", width: 2560, height: 1440, fps: 60,
            backend: .screenCaptureKit, mediaKey: Data(repeating: 1, count: 32))
        var applied: [UInt8] = []
        session.inputEnabled = true
        session.inputLanguageSelector = { applied.append($0); return true }
        let first = Data([0x4c, 0x43, 0x49, 0x31, 0, 0, 0, 1, 7, 1, 2])
        session.handleInputMessage(first, fd: -1, destination: nil)
        session.handleInputMessage(first, fd: -1, destination: nil)
        assert(session.lastReliableInputSequence == 0, "language ACK must wait for main-thread application")
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        assert(applied == [2])
        assert(session.lastReliableInputSequence == 1)
        session.handleInputMessage(first, fd: -1, destination: nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        assert(applied == [2], "ACK retries must not reapply the input source")
        let next = Data([0x4c, 0x43, 0x49, 0x31, 0, 0, 0, 2, 7, 1, 1])
        session.handleInputMessage(next, fd: -1, destination: nil)
        // Focus release is allowed to jump over a pending language operation.
        let release = Data([0x4c, 0x43, 0x49, 0x31, 0, 0, 0, 3, 5, 1])
        session.handleInputMessage(release, fd: -1, destination: nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        assert(applied == [2], "a focus release must cancel queued source selection")
        assert(session.lastReliableInputSequence == 3)
        let locked = Data([0x4c, 0x43, 0x49, 0x31, 0, 0, 0, 4, 7, 1, 1])
        session.handleInputMessage(locked, fd: -1, destination: nil)
        assert(session.setInputEnabled(false))
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        assert(applied == [2])
        session.handleInputMessage(locked, fd: -1, destination: nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        assert(session.lastReliableInputSequence == 4, "locked input must still drain valid reliable events")
        assert(applied == [2])
    }
}
