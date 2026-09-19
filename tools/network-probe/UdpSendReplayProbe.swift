// Host-only transport probe. Synthetic frame bytes replay recorded sizes;
// no capture, encoder, tablet, decoder, or display is exercised.
import Foundation
import Darwin

private struct SendSample: Decodable {
    let bytes: Int
    let bitrate: Int
    let burst: Int
    let parity: Int
}

private final class ReceiveState: @unchecked Sendable {
    let lock = NSLock()
    var stop = false
    // Written by receiver, read only after its completion semaphore.
    var received = 0
}

private final class SendState: @unchecked Sendable {
    // Written on networkQueue, read only after its completion semaphore.
    var durations = [UInt64]()
    var lateness = [UInt64]()
    var succeeded = 0
}

@main struct UdpSendReplayProbe {
    static func distribution(_ values: [UInt64]) -> [String: UInt64] {
        let ordered = values.sorted()
        func p(_ fraction: Double) -> UInt64 {
            ordered[max(0, Int(ceil(Double(ordered.count) * fraction)) - 1)]
        }
        return ["p50": p(0.5), "p95": p(0.95), "max": ordered.last!]
    }

    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            fatalError("usage: udp-send-probe fixture.json")
        }
        let samples = try JSONDecoder().decode([SendSample].self,
            from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        precondition(!samples.isEmpty && samples.count <= 600)
        precondition(samples.allSatisfy {
            (22...1_000_000).contains($0.bytes) && (1...120_000_000).contains($0.bitrate)
                && [2, 4, 8, 16].contains($0.burst) && (1...4).contains($0.parity)
        })
        let receiver = socket(AF_INET, SOCK_DGRAM, 0)
        let sender = socket(AF_INET, SOCK_DGRAM, 0)
        precondition(receiver >= 0 && sender >= 0)
        defer { close(receiver); close(sender) }
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        precondition(inet_pton(AF_INET, "127.0.0.1", &address.sin_addr) == 1)
        precondition(withUnsafePointer(to: &address) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(receiver, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        } == 0)
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        precondition(withUnsafeMutablePointer(to: &address) { p in
            p.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(receiver, $0, &length) }
        } == 0)
        var bufferSize: Int32 = 1_048_576
        _ = setsockopt(receiver, SOL_SOCKET, SO_RCVBUF, &bufferSize, socklen_t(MemoryLayout<Int32>.size))
        let receiverDone = DispatchSemaphore(value: 0)
        let receiveState = ReceiveState()
        DispatchQueue(label: "leftcar.probe.receive", qos: .userInteractive).async {
            var buffer = [UInt8](repeating: 0, count: 2048)
            while true {
                var descriptor = pollfd(fd: receiver, events: Int16(POLLIN), revents: 0)
                if poll(&descriptor, 1, 50) > 0 {
                    if recv(receiver, &buffer, buffer.count, 0) > 0 { receiveState.received += 1 }
                } else {
                    receiveState.lock.lock()
                    let finished = receiveState.stop
                    receiveState.lock.unlock()
                    if finished { break }
                }
            }
            receiverDone.signal()
        }
        let session = CaptureSession(targetAddr: address, targetPort: UInt16(bigEndian: address.sin_port),
            targetLabel: "host-loopback-replay", width: 2560, height: 1440, fps: 60,
            backend: .screenCaptureKit,
            udpStability: AppliedUdpStability(profile: .auto, burstDatagrams: 4,
                fecParityShards: 2, adaptivePacing: false),
            mediaKey: Data(repeating: 0x19, count: 32))
        session.sock = sender
        defer { session.sock = -1 }
        let sendDone = DispatchSemaphore(value: 0)
        let sendState = SendState()
        session.networkQueue.async {
            let start = DispatchTime.now().uptimeNanoseconds
            for (index, sample) in samples.enumerated() {
                var frame = Data(repeating: 0x37, count: sample.bytes)
                frame[0] = 0x47; frame[1] = UInt8(index & 255)
                frame[2] = UInt8((index >> 8) & 255); frame[3] = 0x4c; frame[4] = 0x32
                session.currentAverageBitrate = sample.bitrate
                session.activeUdpBurstDatagrams = sample.burst
                session.activeUdpFecParityShards = sample.parity
                // Fixed 60fps arrivals isolate send capacity from capture jitter.
                let due = start + UInt64(index) * 1_000_000_000 / 60
                session.paceNetwork(until: due)
                let began = DispatchTime.now().uptimeNanoseconds
                sendState.lateness.append(began > due ? (began - due) / 1000 : 0)
                if session.writePacket(frame, isFrame: true) { sendState.succeeded += 1 }
                sendState.durations.append((DispatchTime.now().uptimeNanoseconds - began) / 1000)
            }
            sendDone.signal()
        }
        sendDone.wait()
        receiveState.lock.lock(); receiveState.stop = true; receiveState.lock.unlock()
        receiverDone.wait()
        let durations = sendState.durations
        let meanUs = Double(durations.reduce(0, +)) / Double(durations.count)
        let report: [String: Any] = [
            "schema": 1, "frames": samples.count, "succeeded": sendState.succeeded,
            "sentDatagrams": session.sentDatagrams, "receivedDatagrams": receiveState.received,
            "meanSendUs": meanUs, "sendUs": distribution(durations),
            "arrivalLatenessUs": distribution(sendState.lateness),
            "capacityWithin60Fps": meanUs < 1_000_000.0 / 60,
            "boundary": "Production FEC, pacing, retransmit bookkeeping, AEAD and UDP sendto on IPv4 loopback (1400-byte plaintext). Recorded delta sizes, fixed 60fps arrivals. No real compressed content, network path, tablet decode or display; not video acceptance."
        ]
        print(String(decoding: try JSONSerialization.data(withJSONObject: report,
            options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
    }
}
