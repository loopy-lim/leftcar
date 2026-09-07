import Foundation

func splitCaptureQueueLimit(fps: UInt32) -> Int {
    fps >= 60 ? 2 : 1
}

/// Decision for seeding the split recovery boundary submission.
/// Recovery must never wait on the next ScreenCaptureKit callback: when the
/// pending capture queue is empty (idle screen delivers no callbacks), the
/// newest retained frame is submitted as the paired-IDR carrier so the
/// recovery latency is bounded by encode + send, not by capture arrival.
enum SplitRecoverySeedDecision: Equatable {
    case queueAlreadyPending
    case seedCarrier
    case noCarrierAvailable
}

func splitRecoverySeedDecision(
    pendingCaptureCount: Int,
    hasCarrier: Bool
) -> SplitRecoverySeedDecision {
    if pendingCaptureCount > 0 { return .queueAlreadyPending }
    return hasCarrier ? .seedCarrier : .noCarrierAvailable
}

extension CaptureSession {
    /// Must be called with `captureLock` held. Split mode keeps one extra
    /// frame to absorb encode-queue scheduling jitter; overflow remains
    /// latest-wins so this queue cannot turn into playback latency.
    @discardableResult
    func enqueuePendingCaptureLocked(_ frame: PendingCaptureFrame) -> Bool {
        guard requestedEncoderExperiment == .splitVertical else {
            let replaced = pendingCapture != nil
            pendingCapture = frame
            return replaced
        }

        let limit = splitCaptureQueueLimit(fps: fps)
        let replaced = pendingSplitCaptures.count >= limit
        if replaced {
            pendingSplitCaptures.removeFirst()
        }
        pendingSplitCaptures.append(frame)
        return replaced
    }

    /// Must be called with `captureLock` held.
    func dequeuePendingCaptureLocked() -> PendingCaptureFrame? {
        if requestedEncoderExperiment == .splitVertical {
            return pendingSplitCaptures.isEmpty
                ? nil
                : pendingSplitCaptures.removeFirst()
        }
        defer { pendingCapture = nil }
        return pendingCapture
    }

    /// Must be called with `captureLock` held.
    func hasPendingCaptureLocked() -> Bool {
        requestedEncoderExperiment == .splitVertical
            ? !pendingSplitCaptures.isEmpty
            : pendingCapture != nil
    }

    /// Must be called with `captureLock` held.
    func clearPendingCapturesLocked() {
        pendingCapture = nil
        pendingSplitCaptures.removeAll(keepingCapacity: true)
        splitRecoveryCarrier = nil
    }

    /// Must be called with `captureLock` held, immediately after
    /// `splitFlowState.beginRecovery()`: the boundary frame this seeds is the
    /// recovery pair the drain will admit first. The carrier is replayed with
    /// its original `captureWallMs`/`callbackNs`; the tile encoders'
    /// `nextStrictlyMonotonicSubmissionPTS` clock keeps the reused source PTS
    /// legal for the live VTCompressionSession.
    func seedSplitRecoveryCarrierLocked() {
        guard splitRecoverySeedDecision(
            pendingCaptureCount: pendingSplitCaptures.count,
            hasCarrier: splitRecoveryCarrier != nil
        ) == .seedCarrier, let carrier = splitRecoveryCarrier else {
            return
        }
        pendingSplitCaptures.append(carrier)
    }
}
