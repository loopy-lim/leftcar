import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Security
import Darwin
import OSLog

extension CaptureSession {
    func stop() {
        stateLock.lock()
        stopRequested = true
        recoveryDropRetryState.clear()
        stateLock.unlock()
        stopPerformanceLogging()
        // Kill the control receiver first: cancelling the read source (and
        // waiting out any in-flight drain) guarantees no concurrent LCDON can
        // reinstall tap or timer behind the teardown. Then restore the
        // embedded cursor — every stop path funnels through here, so a viewer
        // that vanished without LCDOFF still gets its cursor back.
        stopInputReceiver()
        teardownCursorStream()
        inputQueue.async { [weak self] in self?.releaseInjectedInput() }
        networkLock.lock()
        pendingConfig = nil
        pendingTileConfigs.removeAll(keepingCapacity: true)
        pendingFrames.removeAll(keepingCapacity: true)
        pendingSplitAccessUnits.removeAll(keepingCapacity: true)
        networkRecoveryBoundary.clearAfterSuccessfulKeyframe()
        networkKeyframeInFlight = false
        networkLock.unlock()
        captureLock.lock()
        clearPendingCapturesLocked()
        _ = splitFlowState.cancelAll()
        recoveryEncodeInFlight = false
        recoveryEncodeGateStartedNs = 0
        splitRecoveryGateStartedNs = 0
        captureLock.unlock()
        if let s = stream {
            s.stopCapture(completionHandler: nil)
            stream = nil
            streamHandler = nil
        }
        if let cgStream, let cgStreamAPI {
            _ = cgStreamAPI.stop(cgStream)
            self.cgStream = nil
            self.cgStreamAPI = nil
        }
        invalidateSplitPipeline()
        invalidateEncoderOnEncodeQueue()
        stateLock.lock()
        if sock >= 0 {
            close(sock)
            sock = -1
        }
        running = false
        if stoppedReason.isEmpty {
            lifecycleState = "stopped"
        } else {
            lifecycleState = "error"
        }
        stateLock.unlock()
    }

    /// The viewer sends authenticated LCF1 feedback once per second. When that
    /// stream goes silent while the session is running, the viewer is gone
    /// (network drop, app kill, device sleep) and the media socket will never
    /// report an error because it is unconnected UDP. Re-check after a grace
    /// period and terminate the session so capture/encode work stops instead
    /// of streaming into the void forever.
     func armReceiverHealthCheck() {
        stateLock.lock()
        guard running, !stopRequested, !healthCheckScheduled else {
            stateLock.unlock()
            return
        }
        healthCheckScheduled = true
        stateLock.unlock()

        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 6) { [weak self] in
            guard let self else { return }
            self.stateLock.lock()
            self.healthCheckScheduled = false
            let shouldStop = self.running
                && !self.stopRequested
                && (self.receiverFeedbackNs == 0
                    || (DispatchTime.now().uptimeNanoseconds &- self.receiverFeedbackNs) > 5_000_000_000)
            let shouldRearm = self.running && !self.stopRequested && !shouldStop
            self.stateLock.unlock()
            if shouldStop {
                let reason = "viewer connection lost (feedback timeout)"
                print("health check terminating \(self.targetLabel): \(reason)")
                self.notifyViewerTermination(code: 1, reason: reason)
            } else if shouldRearm {
                // Keep one watchdog alive after healthy feedback. Without
                // re-arming here, a viewer disappearing just after this check
                // would never schedule another timeout because no new LCF1
                // datagram exists to call armReceiverHealthCheck().
                self.armReceiverHealthCheck()
            }
        }
    }

    /// Best-effort authenticated termination notice so a live viewer can close
    /// its window immediately instead of waiting for its own stale-frame
    /// timeout. A dead viewer simply never receives it.
    func notifyViewerTermination(code: UInt8, reason: String) {
        stateLock.lock()
        let fd = sock
        let token = viewerControlToken
        stateLock.unlock()
        guard fd >= 0, !token.isEmpty else {
            markStopped(reason)
            return
        }
        var notice = Data("LCT1".utf8)
        notice.append(code)
        notice.append(token)
        if mediaTransport.usesTCP {
            _ = sendTCPFrame(notice, fd: fd)
            markStopped(reason)
            return
        }
        // Send twice: one datagram may be lost exactly when the network that
        // killed the feedback stream is degrading.
        _ = sendToViewer(notice, fd: fd)
        usleep(20_000)
        _ = sendToViewer(notice, fd: fd)
        markStopped(reason)
    }

    func markStopped(_ reason: String) {
        NSLog("Leftcar capture session stopped %@: %@", targetLabel, reason)
        stateLock.lock()
        stoppedReason = reason
        stateLock.unlock()
        stop()
    }

    /// Handle an intentional viewer close without synchronously stopping
    /// ScreenCaptureKit from the network drain queue. ScreenCaptureKit may
    /// wait for an in-flight capture callback during stopCapture; doing that
    /// work on the network queue could stall the control server's status path.
     func requestViewerStop() {
        stateLock.lock()
        guard running, !stopRequested else {
            stateLock.unlock()
            return
        }
        stoppedReason = "viewer closed stream"
        stopRequested = true
        running = false
        lifecycleState = "error"
        recoveryDropRetryState.clear()
        let staleSocket = sock
        sock = -1
        stateLock.unlock()

        stopPerformanceLogging()
        if staleSocket >= 0 {
            close(staleSocket)
        }
        networkLock.lock()
        pendingConfig = nil
        pendingTileConfigs.removeAll(keepingCapacity: true)
        pendingFrames.removeAll(keepingCapacity: true)
        pendingSplitAccessUnits.removeAll(keepingCapacity: true)
        networkRecoveryBoundary.clearAfterSuccessfulKeyframe()
        networkKeyframeInFlight = false
        networkLock.unlock()

        queue.async { [weak self] in
            self?.stop()
        }
    }
}
