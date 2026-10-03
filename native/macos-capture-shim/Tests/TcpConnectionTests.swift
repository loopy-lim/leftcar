import Foundation
import Darwin

@main
struct TcpConnectionTests {
    static func main() throws {
        var failures: [String] = []
        for (name, run) in [("incomplete frame", incompleteFrameRetiresConnection),
                            ("handshake deadline", handshakeHasOneDeadline),
                            ("fragmented handshake", fragmentedHandshakeWithinDeadlineSucceeds)] {
            do { try run() } catch { failures.append("\(name): \(error)") }
        }
        try expect(failures.isEmpty, failures.joined(separator: "\n"))
        print("TCP partial writes retire the connection; handshake deadline covers the whole frame")
    }

    static func expect(_ condition: Bool, _ message: String) throws {
        if !condition { throw NSError(domain: message, code: 1) }
    }

    static func fixture() throws -> (CaptureSession, Int32) {
        var pair: [Int32] = [-1, -1]
        try expect(socketpair(AF_UNIX, SOCK_STREAM, 0, &pair) == 0, "socketpair")
        var noSignal: Int32 = 1
        _ = setsockopt(pair[0], SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
        _ = setsockopt(pair[1], SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout<Int32>.size))
        let session = CaptureSession(targetAddr: sockaddr_in(), targetPort: 0,
            targetLabel: "tcp-connection-fixture", width: 1, height: 1, fps: 1,
            backend: .screenCaptureKit, mediaTransport: .usb,
            mediaKey: Data(repeating: 42, count: 32))
        session.sock = pair[0]
        return (session, pair[1])
    }

    static func incompleteFrameRetiresConnection() throws {
        let (session, receiver) = try fixture()
        defer { close(receiver); session.stop() }
        var buffer: Int32 = 1024
        _ = setsockopt(session.sock, SOL_SOCKET, SO_SNDBUF, &buffer, socklen_t(MemoryLayout<Int32>.size))
        let flags = fcntl(session.sock, F_GETFL, 0)
        _ = fcntl(session.sock, F_SETFL, flags | O_NONBLOCK)
        let payload = Data(repeating: 77, count: MediaWireFormat.maxDatagram)
        try expect(session.mediaCrypto.seal(payload) != nil, "fixture payload must pass the production seal boundary")
        try expect(session.sendTCPFrame(payload, fd: session.sock) == -1, "backpressure must fail incomplete frame")
        session.stateLock.lock()
        let stopped = session.stopRequested && session.sock == -1 && session.lifecycleState == "error"
        session.stateLock.unlock()
        try expect(stopped, "an incomplete TCP frame must retire its transport before another frame can be sent")
        try expect(session.sendTCPFrame(Data("next frame".utf8), fd: session.sock) == -1,
            "subsequent frames cannot follow a truncated prefix/payload")
    }

    static func handshakeHasOneDeadline() throws {
        let (session, sender) = try fixture()
        defer { close(sender); session.stop() }
        let writer = DispatchGroup()
        writer.enter()
        DispatchQueue.global().async {
            defer { writer.leave() }
            // Each byte arrives within the old per-read timeout, but the
            // complete frame does not arrive within one handshake deadline.
            for byte: UInt8 in [0, 0, 0, 1, 88] {
                usleep(35_000)
                var byte = byte
                _ = Darwin.send(sender, &byte, 1, 0)
            }
        }
        let start = DispatchTime.now().uptimeNanoseconds
        let response = session.receiveTCPFrame(fd: session.sock, timeoutMs: 80)
        let elapsed = DispatchTime.now().uptimeNanoseconds - start
        writer.wait()
        try expect(response == nil, "slow trickle must not extend the whole-frame handshake deadline")
        try expect(elapsed < 150_000_000, "handshake must stop near its total deadline")
    }

    static func fragmentedHandshakeWithinDeadlineSucceeds() throws {
        let (session, sender) = try fixture()
        defer { close(sender); session.stop() }
        let writer = DispatchGroup()
        writer.enter()
        DispatchQueue.global().async {
            defer { writer.leave() }
            for byte: UInt8 in [0, 0, 0, 2, 88, 89] {
                usleep(1_000)
                var byte = byte
                _ = Darwin.send(sender, &byte, 1, 0)
            }
        }
        let response = session.receiveTCPFrame(fd: session.sock, timeoutMs: 500)
        writer.wait()
        try expect(response == Data([88, 89]), "valid fragmented frame must still be accepted")
    }
}
