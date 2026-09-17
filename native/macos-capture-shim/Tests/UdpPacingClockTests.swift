import Foundation
import Darwin

@main
struct UdpPacingClockTests {
    static func main() {
        let session = CaptureSession(
            targetAddr: sockaddr_in(), targetPort: 0, targetLabel: "pacing-clock-test",
            width: 2560, height: 1440, fps: 60, backend: .screenCaptureKit,
            mediaKey: Data(repeating: 7, count: 32)
        )
        session.currentAverageBitrate = 16_000_000
        // 4,000 bytes at 16 Mbps occupy exactly 2 ms. A late wakeup should
        // consume that interval, never shift the next deadline by its lateness.
        let deadline = DispatchTime.now().uptimeNanoseconds + 5_000_000
        session.nextUdpSendNs = deadline
        session.paceUdpDatagram(bytes: 4000)
        guard session.nextUdpSendNs == deadline + 2_000_000 else {
            print("FAIL: wakeup lateness extended the next burst deadline by \(Int64(session.nextUdpSendNs) - Int64(deadline + 2_000_000)) ns")
            exit(1)
        }

        // Idle time must not accumulate credit for a long catch-up burst.
        session.nextUdpSendNs = 1
        let before = DispatchTime.now().uptimeNanoseconds
        session.paceUdpDatagram(bytes: 4000)
        let after = DispatchTime.now().uptimeNanoseconds
        guard session.nextUdpSendNs >= before + 2_000_000,
              session.nextUdpSendNs <= after + 2_000_000 else {
            print("FAIL: stale deadline accumulated unlimited burst credit")
            exit(1)
        }
        print("UdpPacingClockTests: deadline drift and bounded idle recovery PASS")
    }
}
