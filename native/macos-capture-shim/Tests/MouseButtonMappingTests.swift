import Foundation
import Darwin
import CoreGraphics

@main struct MouseButtonMappingTests {
    static func main() {
        var target = sockaddr_in()
        precondition(inet_pton(AF_INET, "192.168.0.249", &target.sin_addr) == 1)
        let session = CaptureSession(targetAddr: target, targetPort: 0,
            targetLabel: "mouse-button-test", width: 2560, height: 1440, fps: 60,
            backend: .screenCaptureKit,
            udpStability: AppliedUdpStability(profile: .auto, burstDatagrams: 4,
                fecParityShards: 2, adaptivePacing: false),
            mediaKey: Data(repeating: 7, count: 32))
        precondition(session.mouseButton(mask: 1) == .left)
        precondition(session.mouseButton(mask: 2) == .right)
        precondition(session.mouseButton(mask: 4) == .center)
        // Side buttons: Android BUTTON_BACK(8)/FORWARD(16) land on macOS
        // physical buttons 3/4 so browsers act on back/forward navigation.
        precondition(session.mouseButton(mask: 8) == CGMouseButton(rawValue: 3))
        precondition(session.mouseButton(mask: 16) == CGMouseButton(rawValue: 4))
        precondition(session.mouseButton(mask: 32) == nil)
        precondition(session.mouseButton(mask: 3) == nil)
        print("MouseButtonMappingTests: left/right/center preserved, back(8)->3, forward(16)->4 PASS")
    }
}
