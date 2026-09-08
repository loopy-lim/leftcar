import Foundation
import ScreenCaptureKit

/// Candidate for per-viewer system-audio ownership, projected out of the
/// live session registry so the arbitration decision stays a pure function
/// (and testable without constructing capture sessions).
struct SystemAudioCandidate {
    let handle: UInt32
    let screenCaptureKit: Bool
    let viewerKey: UInt32
}

/// Exactly one system-audio stream per viewer device: the lowest-handle live
/// ScreenCaptureKit session aimed at that viewer owns the audio plane. Two
/// display sessions opened on the same device would otherwise each capture
/// the same system audio and the viewer would hear everything twice.
func systemAudioOwnerHandle(
    candidates: [SystemAudioCandidate],
    viewerKey: UInt32
) -> UInt32? {
    candidates
        .filter { $0.screenCaptureKit && $0.viewerKey == viewerKey }
        .map(\.handle)
        .min()
}

private func projectedCandidates(_ registry: [UInt32: CaptureSession]) -> [SystemAudioCandidate] {
    registry.values.map {
        SystemAudioCandidate(
            handle: $0.sessionHandle,
            screenCaptureKit: $0.backend == .screenCaptureKit,
            viewerKey: $0.viewerAddressKey
        )
    }
}

extension CaptureSession {
    /// Viewer identity for shared-resource arbitration. Sessions streaming
    /// different displays to the same device share one audio plane; sessions
    /// to different devices each keep their own.
    var viewerAddressKey: UInt32 {
        targetAddr.sin_addr.s_addr
    }

    func isSystemAudioOwner() -> Bool {
        guard backend == .screenCaptureKit else { return false }
        let key = viewerAddressKey
        return withRegistry { reg in
            systemAudioOwnerHandle(
                candidates: projectedCandidates(reg),
                viewerKey: key
            ) == sessionHandle
        }
    }

    /// Re-apply the canonical stream configuration after ownership changed,
    /// so a new owner begins capturing system audio on its live stream (and
    /// a demoted session stops) without a session restart. The configuration
    /// builder recomputes `capturesAudio` from the current registry state.
    func refreshSystemAudioCapture() {
        guard backend == .screenCaptureKit else { return }
        stateLock.lock()
        let live = running && !stopRequested && stream != nil
        stateLock.unlock()
        guard live else { return }
        NSLog(
            "Leftcar system audio owner %@ capturesAudio=%d",
            targetLabel,
            isSystemAudioOwner() ? 1 : 0
        )
        stream?.updateConfiguration(
            streamConfiguration(showsCursor: captureEmbedsCursor()),
            completionHandler: nil
        )
    }
}

/// Hand the audio plane to the next session for the same viewer after a
/// stop removed a session from the registry. Ownership actually moved only
/// when the removed session held it: the minimum handle of the surviving
/// set is then strictly larger (handles are monotonic and never reused).
func transferSystemAudioOwnership(afterRemoving removed: CaptureSession) {
    guard removed.backend == .screenCaptureKit else { return }
    let key = removed.viewerAddressKey
    let successor = withRegistry { reg -> CaptureSession? in
        guard let handle = systemAudioOwnerHandle(
            candidates: projectedCandidates(reg),
            viewerKey: key
        ), handle > removed.sessionHandle else { return nil }
        return reg[handle]
    }
    guard let successor else { return }
    NSLog(
        "Leftcar system audio ownership transfer %@ -> %@",
        removed.targetLabel,
        successor.targetLabel
    )
    successor.refreshSystemAudioCapture()
}
