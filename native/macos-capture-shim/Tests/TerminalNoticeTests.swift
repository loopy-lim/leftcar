import Foundation
import Darwin

private func beginFixtureLease(_ context: UnsafeMutableRawPointer?) -> Int32 {
    context!.assumingMemoryBound(to: Int32.self).pointee
}
private func endFixtureLease(_ context: UnsafeMutableRawPointer?) {}
private func releaseFixtureLease(_ context: UnsafeMutableRawPointer?) {
    context!.assumingMemoryBound(to: Int32.self).deallocate()
}

@main
struct TerminalNoticeTests {
    static let key = Data(repeating: 91, count: 32)

    static func main() throws {
        try deniedSourceStillReceivesTerminal(tcp: false)
        try deniedSourceStillReceivesTerminal(tcp: true)
        try terminalDoesNotWaitForBusyWriter()
        try unknownReasonCannotEmitLifecyclePayload()
        print("Terminal notices remain authenticated after source fencing (UDP + TCP)")
    }

    static func terminalDoesNotWaitForBusyWriter() throws {
        var pair: [Int32] = [-1, -1]
        try expect(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0, "busy socketpair")
        defer { close(pair[1]) }
        let session = CaptureSession(targetAddr: sockaddr_in(), targetPort: 0,
            targetLabel: "busy-terminal-fixture", width: 1, height: 1, fps: 1,
            backend: .screenCaptureKit, mediaTransport: .usb, mediaKey: key)
        session.sock = pair[0]
        session.tcpWriteLock.lock()
        defer { session.tcpWriteLock.unlock() }
        let started = DispatchTime.now().uptimeNanoseconds
        session.notifyViewerTermination(code: 3, reason: "fixture shutdown")
        try expect(DispatchTime.now().uptimeNanoseconds - started < 300_000_000,
            "busy media writer cannot delay terminal retirement indefinitely")
        try expect(session.sock == -1, "bounded notification still retires the original socket")
    }

    static func unknownReasonCannotEmitLifecyclePayload() throws {
        var pair: [Int32] = [-1, -1]
        try expect(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0, "unknown reason socketpair")
        defer { close(pair[1]) }
        let session = CaptureSession(targetAddr: sockaddr_in(), targetPort: 0,
            targetLabel: "unknown-terminal-fixture", width: 1, height: 1, fps: 1,
            backend: .screenCaptureKit, mediaTransport: .usb, mediaKey: key)
        session.sock = pair[0]
        session.notifyViewerTermination(code: 99, reason: "fixture invalid reason")
        var buffer = [UInt8](repeating: 0, count: 64)
        try expect(recv(pair[1], &buffer, buffer.count, 0) == 0,
            "lifecycle bypass is restricted to known terminal reason codes")
    }

    static func expect(_ condition: Bool, _ message: String) throws {
        if !condition { throw NSError(domain: message, code: 1) }
    }

    static func deniedSourceStillReceivesTerminal(tcp: Bool) throws {
        let lease = UnsafeMutablePointer<Int32>.allocate(capacity: 1)
        lease.initialize(to: 1)
        let authorization = SourceAuthorization(context: UnsafeMutableRawPointer(lease),
            begin: beginFixtureLease, end: endFixtureLease, release: releaseFixtureLease)
        var target = sockaddr_in()
        target.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        target.sin_family = sa_family_t(AF_INET)
        target.sin_addr.s_addr = inet_addr("127.0.0.1")
        let receiver: Int32
        let sender: Int32
        if tcp {
            var pair: [Int32] = [-1, -1]
            try expect(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0, "socketpair")
            sender = pair[0]; receiver = pair[1]
        } else {
            receiver = socket(AF_INET, SOCK_DGRAM, 0)
            sender = socket(AF_INET, SOCK_DGRAM, 0)
            let result = withUnsafePointer(to: &target) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    bind(receiver, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                }
            }
            try expect(result == 0, "UDP bind")
            var size = socklen_t(MemoryLayout<sockaddr_in>.size)
            withUnsafeMutablePointer(to: &target) {
                _ = $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(receiver, $0, &size) }
            }
        }
        defer { close(receiver) }
        let session = CaptureSession(targetAddr: target, authorization: authorization,
            targetPort: UInt16(bigEndian: target.sin_port), targetLabel: "terminal-fixture",
            width: 1, height: 1, fps: 1, backend: .screenCaptureKit,
            mediaTransport: tcp ? .usb : .udp, mediaKey: key)
        session.sock = sender
        lease.pointee = 0 // Same source lease is fenced before Host lifecycle stop.
        let denied = tcp ? session.sendTCPFrame(Data("private media".utf8), fd: sender)
            : session.sendToViewer(Data("private media".utf8), fd: sender)
        try expect(denied == -1, "source fencing must continue to deny media")
        session.notifyViewerTermination(code: 2, reason: "fixture operator stop")
        var readiness = pollfd(fd: receiver, events: Int16(POLLIN), revents: 0)
        try expect(poll(&readiness, 1, 150) > 0, "fenced source must still receive terminal notice")
        var bytes = [UInt8](repeating: 0, count: 128)
        let count = recv(receiver, &bytes, bytes.count, 0)
        let packet: Data
        if tcp {
            try expect(count >= 4, "TCP terminal prefix")
            let length = bytes.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
            try expect(count == length + 4, "one complete terminal TCP frame")
            packet = Data(bytes[4..<count])
        } else {
            try expect(count == 29, "terminal has five plaintext bytes plus AEAD overhead")
            packet = Data(bytes.prefix(count))
        }
        var opener = MediaSealer(mediaKey: MediaKeyDerivation.directionalKeys(mediaKey: key)!.s2c)!
        try expect(opener.open(packet) == Data([0x4c, 0x43, 0x54, 0x31, 2]),
            "terminal notice must decrypt under existing directional session key")
        try expect(!authorization.begin(), "terminal must not restore source authority")
        try expect(session.sock == -1, "terminal retirement must still close the session socket")
    }
}
