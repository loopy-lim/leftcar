import Foundation
import Darwin

// Only OS capture sessions/registry are replaced. The compiled ownership,
// negotiation, handoff and initial capture decision functions are production.
enum SyntheticBackend { case screenCaptureKit, other }
final class SyntheticStream {
    var configurations: [Bool] = []
    func updateConfiguration(_ audio: Bool, completionHandler: (() -> Void)?) { configurations.append(audio) }
}
final class CaptureSession {
    let sessionHandle: UInt32
    var authenticatedOwner: String?
    let backend = SyntheticBackend.screenCaptureKit
    var targetAddr = sockaddr_in()
    let targetLabel = "synthetic"
    let stateLock = NSLock()
    let audioQueueKey = DispatchSpecificKey<Void>()
    let audioQueue = DispatchQueue(label: "synthetic.audio")
    var opusAudioEncoder: Int? = 1
    var running = true
    var stopRequested = false
    var stream: SyntheticStream? = SyntheticStream()
    init(_ handle: UInt32, _ key: UInt32) { sessionHandle = handle; targetAddr.sin_addr.s_addr = key; audioQueue.setSpecific(key: audioQueueKey, value: ()) }
    func captureEmbedsCursor() -> Bool { false }
    func streamConfiguration(showsCursor: Bool) -> Bool {
        shouldCaptureSystemAudio(owner: isSystemAudioOwner(), viewerKey: viewerAddressKey)
    }
}
private var sessions: [UInt32: CaptureSession] = [:]
func withRegistry<T>(_ body: ([UInt32: CaptureSession]) -> T) -> T { body(sessions) }
@main struct AudioOwnershipSyntheticTests {
    static func main() {
        let deviceA = CaptureSession(41, 900), deviceB = CaptureSession(42, 900), anotherEndpointA = CaptureSession(43, 901)
        deviceA.authenticatedOwner = "device-A:credential-1"
        deviceB.authenticatedOwner = "device-B:credential-2"
        anotherEndpointA.authenticatedOwner = deviceA.authenticatedOwner
        sessions = [41: deviceA, 42: deviceB, 43: anotherEndpointA]
        precondition(deviceA.isSystemAudioOwner())
        precondition(deviceB.isSystemAudioOwner(), "different authenticated devices at one IP each own audio")
        precondition(!anotherEndpointA.isSystemAudioOwner(), "one device across transports shares audio")
        sessions = [:]
        let first = CaptureSession(1, 77), sibling = CaptureSession(2, 77)
        sessions = [1: first, 2: sibling]
        precondition(first.streamConfiguration(showsCursor: false))
        precondition(!sibling.streamConfiguration(showsCursor: false))
        first.handleSystemAudioCommand(Data("SNDA1O".utf8))
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"))
        sibling.handleSystemAudioCommand(Data("SNDON".utf8))
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"), "legacy refresh must not erase negotiated mode")
        first.handleSystemAudioCommand(Data("SNDA1P".utf8))
        precondition(!systemAudioOpusPreferred(forKey: "legacy-ip:77"))
        first.handleSystemAudioCommand(Data("SNDA1O".utf8))
        // A lost owner downgrade leaves the current mode unchanged until
        // the next actual owner refresh; competing siblings cannot undo it.
        sibling.handleSystemAudioCommand(Data("SNDA1O".utf8))
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"))
        first.handleSystemAudioCommand(Data("SNDA1P".utf8))
        for _ in 0..<8 {
            sibling.handleSystemAudioCommand(Data("SNDA1O".utf8))
            first.handleSystemAudioCommand(Data("SNDON".utf8))
            precondition(!systemAudioOpusPreferred(forKey: "legacy-ip:77"), "nonowner refresh must not undo owner PCM fallback")
        }
        first.handleSystemAudioCommand(Data("SNDA1O".utf8))
        let nonOwner = CaptureSession(9, 77)
        sessions[9] = nonOwner
        sessions.removeValue(forKey: 9)
        transferSystemAudioOwnership(afterRemoving: nonOwner)
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"), "closing nonowner must preserve live owner's negotiation")
        nonOwner.handleSystemAudioCommand(Data("SNDA1P".utf8))
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"), "retired session commands cannot alter current owner")
        sessions.removeValue(forKey: 1)
        transferSystemAudioOwnership(afterRemoving: first)
        precondition(sibling.isSystemAudioOwner())
        first.handleSystemAudioCommand(Data("SNDA1P".utf8))
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"), "retired owner cannot change successor preference")
        sibling.handleSystemAudioCommand(Data("SNDA1P".utf8))
        precondition(!systemAudioOpusPreferred(forKey: "legacy-ip:77"))
        sibling.handleSystemAudioCommand(Data("SNDA1O".utf8))

        precondition(sibling.stream?.configurations.last == true)
        precondition(systemAudioOpusPreferred(forKey: "legacy-ip:77"), "live sibling retains negotiated preference")
        sessions.removeValue(forKey: 2)
        transferSystemAudioOwnership(afterRemoving: sibling)
        let legacy = CaptureSession(3, 77)
        sessions[3] = legacy
        legacy.handleSystemAudioCommand(Data("SNDON".utf8))
        precondition(!systemAudioOpusPreferred(forKey: "legacy-ip:77"), "fresh legacy lifetime starts PCM")
        setenv("LEFTCAR_BENCHMARK_SYSTEM_AUDIO", "off", 1)
        precondition(!legacy.streamConfiguration(showsCursor: false), "initial configuration is audio-off")
        legacy.refreshSystemAudioCapture()
        precondition(legacy.stream?.configurations.last == false, "owner refresh is still audio-off")
        unsetenv("LEFTCAR_BENCHMARK_SYSTEM_AUDIO")
        let entered = DispatchSemaphore(value: 0), unblock = DispatchSemaphore(value: 0), retired = DispatchSemaphore(value: 0)
        let socketLock = NSLock()
        var socketClosed = false
        var writesAfterClose = 0
        // Controlled send I/O on the actual retirement queue: no real socket.
        legacy.audioQueue.async {
            entered.signal(); unblock.wait()
            socketLock.lock()
            if socketClosed { writesAfterClose += 1 }
            socketLock.unlock()
        }
        precondition(entered.wait(timeout: .now() + 1) == .success)
        DispatchQueue.global().async {
            legacy.retireAudioEncoder()
            socketLock.lock(); socketClosed = true; socketLock.unlock()
            retired.signal()
        }
        precondition(retired.wait(timeout: .now() + 0.03) == .timedOut, "retirement must wait for actual in-flight queue work")
        unblock.signal()
        precondition(retired.wait(timeout: .now() + 1) == .success)
        legacy.audioQueue.sync {
            legacy.opusAudioEncoder = 1
            legacy.retireAudioEncoder()
            precondition(legacy.opusAudioEncoder == nil)
        }
        precondition(writesAfterClose == 0, "retirement must finish queued sends before socket closure")
        print("AudioOwnershipSyntheticTests passed")
    }
}
