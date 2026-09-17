import Foundation
import Darwin

@main struct UdpFramePacingBudgetTests {
    static func main() {
        var cases = 0
        for address in ["127.0.0.1", "100.77.109.50"] {
            var target = sockaddr_in()
            precondition(inet_pton(AF_INET, address, &target.sin_addr) == 1)
            let session = CaptureSession(targetAddr: target, targetPort: 0,
                targetLabel: "frame-budget-test", width: 2560, height: 1440, fps: 60,
                backend: .screenCaptureKit,
                udpStability: AppliedUdpStability(profile: .auto, burstDatagrams: 4,
                    fecParityShards: 2, adaptivePacing: false),
                mediaKey: Data(repeating: 7, count: 32))
            let fragmentBytes = udpMediaPlaintextLimit(
                ipv4HostOrder: UInt32(bigEndian: target.sin_addr.s_addr)) - 33
            // Include singleton tails (8n+1 fragments), a partial FEC group,
            // and high-motion sizes seen in the device trace.
            for size in [16_000, fragmentBytes * 8 + 1, 59_455, 110_227] {
                for parity in [2, 3, 4] {
                    var frame = Data(repeating: 0x37, count: size + 21)
                    frame[0] = 0x47; frame[1] = 1; frame[2] = 0
                    frame[3] = 0x4c; frame[4] = 0x32
                    let prepared = session.prepareUdpAccessUnit(frame,
                        isKeyframe: false, parityOverride: parity)!
                    precondition(prepared.parityCount >= parity * (prepared.fragmentCount / 8))
                    let mtu = address.hasPrefix("100.") ? 1280 : 1500
                    precondition(prepared.datagrams.allSatisfy {
                        $0.count + UdpFramePacingBudget.datagramOverheadBytes <= mtu
                    }, "Frame budget changed the datagram MTU")
                    let plaintext = prepared.datagrams.reduce(0) { $0 + $1.count }
                    let budget = UdpFramePacingBudget(targetBitrate: 14_000_000,
                        payloadBytes: size, plaintextBytes: plaintext,
                        datagramCount: prepared.datagrams.count, fps: 60)!
                    for burst in [2, 4, 8] {
                        let ranges = udpPacingBurstRanges(datagramCount: prepared.datagrams.count,
                            maxDatagrams: burst)
                        let reservedUs = ranges.reduce(UInt64(0)) { total, range in
                            let bytes = range.reduce(0) {
                                $0 + prepared.datagrams[$1].count + UdpFramePacingBudget.datagramOverheadBytes
                            }
                            return total + udpDatagramIntervalUs(bytes: bytes,
                                isKeyframe: false, accessUnitBytes: frame.count,
                                targetBitrate: 14_000_000, fps: 60,
                                dataFragmentCount: prepared.fragmentCount, selectedParity: parity,
                                framePacingBitrate: budget.bitrate)
                        }
                        // Account for per-burst integer rounding and the 100us
                        // minimum on the final short burst, not an entire frame.
                        let roundingSlack = UInt64(ranges.count) + 100
                        precondition(reservedUs <= frameBudgetUs(fps: 60) * 4 / 5 + roundingSlack,
                            "FEC/header/tail overhead exhausted the frame budget: \(reservedUs)us")
                        cases += 1
                    }
                }
            }
        }
        precondition(UdpFramePacingBudget(targetBitrate: 0, payloadBytes: 1,
            plaintextBytes: 1, datagramCount: 1, fps: 60) == nil)
        precondition(UdpFramePacingBudget(targetBitrate: 1, payloadBytes: 1,
            plaintextBytes: 1, datagramCount: 1, fps: 0) == nil)
        let oversized = UdpFramePacingBudget(targetBitrate: 10_000_000,
            payloadBytes: 2_000_000, plaintextBytes: 3_000_000, datagramCount: 3000, fps: 60)!
        precondition(oversized.bitrate == 120_000_000, "Frame budget removed the acceleration cap")
        let recovery = udpDatagramIntervalUs(bytes: 4800, isKeyframe: true,
            accessUnitBytes: 300_000, targetBitrate: 14_000_000, fps: 60)
        precondition(recovery == udpDatagramIntervalUs(bytes: 4800, isKeyframe: true,
            accessUnitBytes: 300_000, targetBitrate: 14_000_000, fps: 60,
            framePacingBitrate: 120_000_000), "Delta budget accelerated recovery keyframes")
        print("UdpFramePacingBudgetTests: \(cases) packetization/budget cases, cap and recovery isolation PASS")
    }
}
