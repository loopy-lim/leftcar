import Foundation

@main
struct SystemAudioOwnershipTests {
    static func main() {
        let headset: UInt32 = 0x249_a8c0 // 192.168.0.249
        let tablet: UInt32 = 0x213_a8c0 // 192.168.0.19

        // A lone session owns its viewer's audio plane.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 7, screenCaptureKit: true, viewerKey: headset),
                ],
                viewerKey: headset
            ) == 7
        )

        // Two displays shared to one device: the earliest-started session is
        // the single owner, so the viewer hears the audio exactly once.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 7, screenCaptureKit: true, viewerKey: headset),
                    SystemAudioCandidate(handle: 12, screenCaptureKit: true, viewerKey: headset),
                ],
                viewerKey: headset
            ) == 7
        )

        // Different devices keep independent audio planes.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 7, screenCaptureKit: true, viewerKey: headset),
                    SystemAudioCandidate(handle: 12, screenCaptureKit: true, viewerKey: tablet),
                ],
                viewerKey: tablet
            ) == 12
        )

        // The legacy CGDisplayStream backend never captures audio and must
        // not win ownership even with the lowest handle.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 3, screenCaptureKit: false, viewerKey: headset),
                    SystemAudioCandidate(handle: 9, screenCaptureKit: true, viewerKey: headset),
                ],
                viewerKey: headset
            ) == 9
        )

        // Closing the owner hands the plane to the surviving session.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 12, screenCaptureKit: true, viewerKey: headset),
                ],
                viewerKey: headset
            ) == 12
        )

        // A viewer with no ScreenCaptureKit session left owns nothing.
        precondition(
            systemAudioOwnerHandle(
                candidates: [
                    SystemAudioCandidate(handle: 3, screenCaptureKit: false, viewerKey: headset),
                ],
                viewerKey: headset
            ) == nil
        )
        precondition(
            systemAudioOwnerHandle(candidates: [], viewerKey: headset) == nil
        )

        // SNDON is the default state: an unmuted viewer's owner captures.
        precondition(shouldCaptureSystemAudio(owner: true, viewerKey: headset))

        // SNDOFF gates the plane even while the session still owns it.
        setSystemAudioDeliveryEnabled(false, viewerKey: headset)
        precondition(!systemAudioDeliveryEnabled(forKey: headset))
        precondition(!shouldCaptureSystemAudio(owner: true, viewerKey: headset))
        // The gate is per viewer: another device keeps its audio.
        precondition(systemAudioDeliveryEnabled(forKey: tablet))
        precondition(shouldCaptureSystemAudio(owner: true, viewerKey: tablet))

        // Repeating SNDOFF stays muted (idempotent), and SNDON restores.
        setSystemAudioDeliveryEnabled(false, viewerKey: headset)
        precondition(!systemAudioDeliveryEnabled(forKey: headset))
        setSystemAudioDeliveryEnabled(true, viewerKey: headset)
        precondition(systemAudioDeliveryEnabled(forKey: headset))
        precondition(shouldCaptureSystemAudio(owner: true, viewerKey: headset))
        // Repeating SNDON stays enabled.
        setSystemAudioDeliveryEnabled(true, viewerKey: headset)
        precondition(systemAudioDeliveryEnabled(forKey: headset))

        print("SystemAudioOwnershipTests passed")
    }
}
