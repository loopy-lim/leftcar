import Foundation
import Darwin

@main
struct InputStatusHeartbeatTests {
    static func main() throws {
        let receiver = socket(AF_INET, SOCK_DGRAM, 0)
        let sender = socket(AF_INET, SOCK_DGRAM, 0)
        defer { close(receiver) }
        var target = sockaddr_in()
        target.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        target.sin_family = sa_family_t(AF_INET)
        target.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &target) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(receiver, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard bound == 0 else { throw NSError(domain: "UDP fixture bind", code: Int(errno)) }
        var size = socklen_t(MemoryLayout<sockaddr_in>.size)
        withUnsafeMutablePointer(to: &target) {
            _ = $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(receiver, $0, &size) }
        }
        let key = Data(repeating: 98, count: 32)
        let session = CaptureSession(targetAddr: target, targetPort: UInt16(bigEndian: target.sin_port),
            targetLabel: "status-heartbeat-fixture", width: 1, height: 1, fps: 1,
            backend: .screenCaptureKit, mediaKey: key)
        session.sock = sender
        defer { session.stop() }
        var opener = MediaSealer(mediaKey: MediaKeyDerivation.directionalKeys(mediaKey: key)!.s2c)!
        for enabled in [false, true, false] {
            session.inputEnabled = enabled
            // This dispatcher is shared by authenticated UDP/TCP receive paths.
            // No key event is sent to repair the Viewer's cached input lock.
            _ = session.dispatchViewerControlCommand(Data("LCK1".utf8), fd: sender, destination: target)
            var descriptor = pollfd(fd: receiver, events: Int16(POLLIN), revents: 0)
            guard poll(&descriptor, 1, 100) > 0 else {
                throw NSError(domain: "heartbeat must refresh current input status", code: 1)
            }
            var packet = [UInt8](repeating: 0, count: 64)
            let count = recv(receiver, &packet, packet.count, 0)
            guard count == 29, opener.open(Data(packet.prefix(count))) == Data([0x4c, 0x43, 0x53, 0x31, enabled ? 1 : 0]) else {
                throw NSError(domain: "heartbeat status must be authenticated current LCS1", code: 2)
            }
            guard session.inputEnabled == enabled else {
                throw NSError(domain: "heartbeat cannot change Host input approval", code: 3)
            }
        }
        print("Authenticated heartbeat refreshes current Mac input status without a blocked key")
    }
}
