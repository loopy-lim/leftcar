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

/// Viewer keys whose audio plane is muted by an SNDOFF command. State is
/// keyed by viewer (not session) because every session aimed at one device
/// shares a single audio plane. The viewer re-asserts its request once per
/// second, so a datagram lost after a toggle or a host restart heals without
/// an ACK plane, and the set needs no expiry: a stale key is harmless while
/// no live session carries it.
private var systemAudioMutedKeys: Set<UInt32> = []
private let systemAudioMuteLock = NSLock()

/// Whether the viewer behind `key` still wants the system-audio plane.
func systemAudioDeliveryEnabled(forKey key: UInt32) -> Bool {
    systemAudioMuteLock.lock()
    defer { systemAudioMuteLock.unlock() }
    return !systemAudioMutedKeys.contains(key)
}

/// Viewer opt-in (SNDON) or opt-out (SNDOFF) of the system-audio plane,
/// token-authenticated like IDR/BYE before dispatch. Every live session for
/// that viewer re-applies its stream configuration: only the owner's
/// `capturesAudio` matters, and updateConfiguration applies it live without
/// a session restart.
func setSystemAudioDeliveryEnabled(_ enabled: Bool, viewerKey: UInt32) {
    systemAudioMuteLock.lock()
    let changed = enabled
        ? systemAudioMutedKeys.remove(viewerKey) != nil
        : systemAudioMutedKeys.insert(viewerKey).inserted
    systemAudioMuteLock.unlock()
    guard changed else { return }
    NSLog("Leftcar system audio muted=%d viewerKey=%u", enabled ? 0 : 1, viewerKey)
    let sessions = withRegistry { registry in
        registry.values.filter { $0.viewerAddressKey == viewerKey }
    }
    sessions.forEach { $0.refreshSystemAudioCapture() }
}

/// True only while a session may capture: it must hold the viewer's audio
/// plane and the viewer must not have muted it.
func shouldCaptureSystemAudio(owner: Bool, viewerKey: UInt32) -> Bool {
    owner && systemAudioDeliveryEnabled(forKey: viewerKey)
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
            shouldCaptureSystemAudio(
                owner: isSystemAudioOwner(),
                viewerKey: viewerAddressKey
            ) ? 1 : 0
        )
        stream?.updateConfiguration(
            streamConfiguration(showsCursor: captureEmbedsCursor()),
            completionHandler: nil
        )
    }

    /// SNDOFF mutes the audio plane; SNDON restores it. Any session aimed at
    /// the viewer may carry the command — the toggle is per viewer, so the
    /// session that owns the plane obeys even when a sibling display's
    /// session received the datagram.
    func handleSystemAudioCommand(_ command: Data) {
        guard backend == .screenCaptureKit else { return }
        if command == Data("SNDON".utf8) {
            setSystemAudioDeliveryEnabled(true, viewerKey: viewerAddressKey)
        } else if command == Data("SNDOFF".utf8) {
            setSystemAudioDeliveryEnabled(false, viewerKey: viewerAddressKey)
        }
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
