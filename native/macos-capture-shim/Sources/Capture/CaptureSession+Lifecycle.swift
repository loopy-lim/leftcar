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

func viewerConnectionAlive(feedback: UInt64, heartbeat: UInt64, now: UInt64) -> Bool {
    let latest = max(feedback, heartbeat)
    return latest > 0 && now >= latest && now - latest <= 15_000_000_000
}

extension CaptureSession {
    func stop() {
        stateLock.lock()
        stopRequested = true
        recoveryDropRetryState.clear()
        stateLock.unlock()
        retireAudioEncoder()
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

    /// The viewer sends authenticated LCF1 feedback, or LCK1 heartbeats while
    /// its Surface is hidden. When both go silent, the viewer is gone
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
                && !viewerConnectionAlive(feedback: self.receiverFeedbackNs,
                    heartbeat: self.receiverHeartbeatNs, now: DispatchTime.now().uptimeNanoseconds)
            let shouldRearm = self.running && !self.stopRequested && !shouldStop
            self.stateLock.unlock()
            if shouldStop {
                let reason = "viewer connection lost (feedback timeout)"
                print("health check terminating \(self.targetLabel): \(reason)")
                self.notifyViewerTermination(code: 1, reason: reason)
            } else if shouldRearm {
                // Keep one watchdog alive after healthy feedback. Without
                // re-arming here, a viewer disappearing just after this check
                // would never schedule another timeout because no new control
                // datagram exists to call armReceiverHealthCheck().
                self.armReceiverHealthCheck()
            }
        }
    }

    /// Lifecycle output has only this fixed terminal packet. Revoking a source
    /// still fences every ordinary media/input operation; it must not suppress
    /// the notification that tells the already-authenticated viewer to retire.
    func notifyViewerTermination(code: UInt8, reason: String) {
        if (1...3).contains(code) { sendTerminalNotice(code: code) }
        markStopped(reason)
    }

    private func sendTerminalNotice(code: UInt8) {
        let deadline = DispatchTime.now().uptimeNanoseconds + 100_000_000
        stateLock.lock()
        // Own this descriptor while stop() closes the session's descriptor.
        // A later session reusing its integer fd cannot receive this notice.
        let fd = sock >= 0 ? dup(sock) : -1
        stateLock.unlock()
        guard fd >= 0 else { return }
        defer { close(fd) }
        var notice = Data("LCT1".utf8)
        notice.append(code)
        guard let sealed = mediaCrypto.seal(notice) else { return }
        if mediaTransport.usesTCP {
            var noSigPipe: Int32 = 1
            _ = setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe,
                socklen_t(MemoryLayout<Int32>.size))
            // Existing admitted media can finish its frame, but teardown never
            // waits indefinitely for a stalled writer. Do not alter O_NONBLOCK
            // on a duplicated descriptor (it is shared with the original).
            while !tcpWriteLock.try() {
                guard DispatchTime.now().uptimeNanoseconds < deadline else { return }
                usleep(1_000)
            }
            defer { tcpWriteLock.unlock() }
            var length = UInt32(sealed.count).bigEndian
            var framed = Data()
            withUnsafeBytes(of: &length) { framed.append(contentsOf: $0) }
            framed.append(sealed)
            framed.withUnsafeBytes { raw in
                var offset = 0
                while offset < raw.count && terminalWritable(fd: fd, until: deadline) {
                    let sent = Darwin.send(fd, raw.baseAddress!.advanced(by: offset),
                        raw.count - offset, MSG_DONTWAIT)
                    if sent > 0 { offset += sent }
                    else if errno != EINTR && errno != EAGAIN && errno != EWOULDBLOCK { break }
                }
            }
            return
        }
        // Two independently sealed attempts keep the existing UDP loss budget.
        for attempt in 0..<2 {
            if attempt > 0 { usleep(20_000) }
            guard terminalWritable(fd: fd, until: deadline),
                  let packet = attempt == 0 ? sealed : mediaCrypto.seal(notice) else { return }
            var address = targetAddr // Immutable, admitted session endpoint only.
            packet.withUnsafeBytes { raw in
                withUnsafePointer(to: &address) { pointer in
                    pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                        _ = sendto(fd, raw.baseAddress, raw.count, MSG_DONTWAIT, $0,
                            socklen_t(MemoryLayout<sockaddr_in>.size))
                    }
                }
            }
        }
    }

    private func terminalWritable(fd: Int32, until deadline: UInt64) -> Bool {
        let now = DispatchTime.now().uptimeNanoseconds
        guard now < deadline else { return false }
        var descriptor = pollfd(fd: fd, events: Int16(POLLOUT), revents: 0)
        let remainingMs = Int32(max(1, (deadline - now) / 1_000_000))
        return poll(&descriptor, 1, remainingMs) > 0 && descriptor.revents & Int16(POLLOUT) != 0
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

        retireAudioEncoder()
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
