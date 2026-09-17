import Foundation
import Darwin

@main
struct UdpPacketSizeTests {
    static func main() {
        for (address, mtu, plaintextLimit) in [
            ("192.168.0.19", 1_500, 1_400),
            ("100.63.255.255", 1_500, 1_400),
            ("100.64.0.0", 1_280, 1_200),
            ("100.77.109.50", 1_280, 1_200),
            ("100.127.255.255", 1_280, 1_200),
            ("100.128.0.0", 1_500, 1_400),
        ] {
            var target = sockaddr_in()
            precondition(inet_pton(AF_INET, address, &target.sin_addr) == 1)
            let key = Data(repeating: 7, count: 32)
            let session = CaptureSession(
                targetAddr: target, targetPort: 5_001, targetLabel: "packet-size-test",
                width: 2_560, height: 1_440, fps: 60, backend: .cgDisplayStream,
                mediaTransport: .udp, requestedEncoderExperiment: .auto,
                mediaKey: key
            )
            for payloadSize in [1, 1_367, 1_368, 10_936, 128 * 1_024] {
                for keyframe in [false, true] {
                    var header = Data([0x47, 7, 0, 0x4c, 0x32])
                    header.append(Data(repeating: 0, count: 16))
                    let payload = Data((0..<payloadSize).map { UInt8($0 % 251) })
                    let prepared = session.prepareUdpAccessUnit(header + payload, isKeyframe: keyframe)!
                    var sealer = MediaSealer(mediaKey: key, firstCounter: 1)!
                    var opener = MediaSealer(mediaKey: key, firstCounter: 1)!
                    var fragments: [Int: Data] = [:]
                    for datagram in prepared.datagrams {
                        let sealed = sealer.seal(datagram)!
                        precondition(sealed.count + 28 <= mtu,
                            "IPv4/UDP/AEAD packet exceeds path MTU for \(address): \(sealed.count + 28) > \(mtu)")
                        precondition(datagram.count <= plaintextLimit)
                        precondition(opener.open(sealed) == datagram)
                        if datagram[0] == 0x47 {
                            let index = Int(datagram[1]) * 256 + Int(datagram[2])
                            fragments[index] = Data(datagram.dropFirst(33))
                        }
                    }
                    let reassembled = (0..<prepared.fragmentCount).reduce(into: Data()) {
                        $0.append(fragments[$1]!)
                    }
                    precondition(reassembled == payload)
                    precondition(prepared.datagrams.contains { $0.count == plaintextLimit }
                        || payloadSize < plaintextLimit - 33)
                    if prepared.fragmentCount > 1 { precondition(prepared.parityCount > 0) }
                }
            }
        }
        print("UdpPacketSizeTests: 60 encrypted packetization cases, route boundaries, FEC and payload round trips PASS")
    }
}
