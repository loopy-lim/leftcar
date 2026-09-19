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
     func beginCapture() -> Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        guard !stopRequested, sock >= 0 else { return false }
        running = true
        lifecycleState = "starting_capture"
        return true
    }

     func captureDidStart() {
        stateLock.lock()
        if running, !stopRequested {
            lifecycleState = firstCaptureNs == nil ? "waiting_first_frame" : "running"
        }
        stateLock.unlock()
        armFirstFrameWatchdog()
        armCaptureHealthCheck()
        // Start the viewer watchdog even before the first LCF1 packet. A
        // viewer can disappear immediately after the UDP reachability proof;
        // waiting for feedback to arm this timer would leak that session.
        armReceiverHealthCheck()
    }

    func startPerformanceLogging() {
        stateLock.lock()
        let shouldStart = running && !stopRequested
        let ticker = performanceLogTicker
        stateLock.unlock()
        guard shouldStart, let ticker, ticker.start() else { return }

        stateLock.lock()
        let shouldCancel = stopRequested || performanceLogTicker !== ticker
        stateLock.unlock()
        if shouldCancel {
            ticker.stop()
        }
    }

     func stopPerformanceLogging() {
        stateLock.lock()
        let ticker = performanceLogTicker
        performanceLogTicker = nil
        stateLock.unlock()
        ticker?.stop()
    }

     func armFirstFrameWatchdog() {
        DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 5) { [weak self] in
            guard let self else { return }
            self.stateLock.lock()
            let timedOut = self.running && !self.stopRequested && self.firstSendNs == nil
            let captured = self.firstCaptureNs != nil
            let encoded = self.firstEncodeNs != nil
            self.stateLock.unlock()
            if timedOut {
                let stage = !captured ? "capture" : (!encoded ? "encoder" : "network")
                self.markStopped("\(stage) produced no video frame within 5s")
            }
        }
    }

    @discardableResult
    func setupScreenCaptureKit(filter: SCContentFilter) -> Bool {
        guard sourceAuthorization?.begin() ?? true else { setLastError("source authorization revoked"); return false }
        defer { sourceAuthorization?.end() }
        inputLock.lock()
        if #available(macOS 14.0, *) {
            inputBounds = filter.contentRect
        }
        inputLock.unlock()
        stateLock.lock()
        restartFilter = filter
        restartDisplayID = nil
        stateLock.unlock()
        guard beginCapture() else {
            setLastError("media socket closed before capture start")
            return false
        }
        let completion = DispatchSemaphore(value: 0)
        let completionLock = NSLock()
        var setupFailure: String?
        var startError: Error?
        var stopAborted = false

        // Construct and start AppKit-adjacent ScreenCaptureKit objects on the
        // main queue. Do not wait there: the Rust control worker owns the
        // semaphore wait, leaving the main run loop free to receive replayd's
        // completion callback.
        DispatchQueue.main.async { [self] in
            // beginCapture checked stopRequested before this hop was queued. A
            // BYE that lands in between must not let the hop create a fresh
            // stream — that would extend a new sleep assertion past the
            // stop-driven teardown until the control plane reaps it.
            stateLock.lock()
            let aborted = stopRequested
            stateLock.unlock()
            if aborted {
                completionLock.lock()
                stopAborted = true
                completionLock.unlock()
                completion.signal()
                return
            }
            // The cursor plane is negotiated after creation, so the stream is
            // born with the embedded cursor. The one shared builder keeps the
            // creation-time and update-time configuration in lockstep —
            // updateConfiguration replaces the whole configuration.
            let config = streamConfiguration(showsCursor: captureEmbedsCursor())

            let handler = CaptureOutputHandler(session: self)
            let candidate = SCStream(
                filter: filter,
                configuration: config,
                delegate: handler
            )

            do {
                try candidate.addStreamOutput(
                    handler,
                    type: .screen,
                    sampleHandlerQueue: queue
                )
                if #available(macOS 13.3, *) {
                    // System audio joins the same stream; failures degrade to
                    // a silent plane rather than blocking video capture.
                    do {
                        try candidate.addStreamOutput(
                            handler,
                            type: .audio,
                            sampleHandlerQueue: queue
                        )
                        NSLog("Leftcar audio output registered capturesAudio=%d", config.capturesAudio ? 1 : 0)
                    } catch {
                        NSLog("Leftcar audio output registration failed: %@", "\(error)")
                    }
                }
                streamHandler = handler
                stream = candidate
                candidate.startCapture { error in
                    completionLock.lock()
                    startError = error
                    completionLock.unlock()
                    completion.signal()
                }
            } catch {
                completionLock.lock()
                setupFailure = "addStreamOutput: \(error.localizedDescription)"
                completionLock.unlock()
                completion.signal()
            }
        }

        // replayd can take more than eight seconds to complete a cold start,
        // especially after a display/profile transition. Keep this lifecycle
        // guard outside the frame pipeline so it prevents a false teardown
        // without adding any steady-state buffering or latency.
        guard completion.wait(timeout: .now() + 15) == .success else {
            let reason = "SCStream startCapture timed out after 15s"
            setLastError(reason)
            markStopped(reason)
            return false
        }
        completionLock.lock()
        let failure = setupFailure
        let error = startError
        let abortedByStop = stopAborted
        completionLock.unlock()
        if abortedByStop {
            // The stop path already owns the teardown; do not mark an
            // intentional stop as an error (same convention as the
            // CGDisplayStream stopped handler) and do not report success.
            return false
        }
        if let failure {
            setLastError(failure)
            markStopped(failure)
            return false
        }
        if let error {
            let reason = "startCapture failed: \(error.localizedDescription) — \(CaptureSession.tccDeniedHint)"
            setLastError(reason)
            markStopped(reason)
            return false
        }
        captureDidStart()
        return true
    }

    @discardableResult
    func setupCGDisplayStream(displayID: CGDirectDisplayID) -> Bool {
        guard sourceAuthorization?.begin() ?? true else { setLastError("source authorization revoked"); return false }
        defer { sourceAuthorization?.end() }
        inputLock.lock()
        inputBounds = CGDisplayBounds(displayID)
        inputLock.unlock()
        stateLock.lock()
        restartDisplayID = displayID
        restartFilter = nil
        stateLock.unlock()
        guard beginCapture() else {
            setLastError("media socket closed before capture start")
            return false
        }
        guard let api = LegacyCGDisplayStreamAPI() else {
            let reason = "CGDisplayStream symbols are unavailable on this macOS version"
            setLastError(reason)
            markStopped(reason)
            return false
        }
        let handler: CGFrameHandler = { [weak self] status, _, surface, _ in
            guard let self else { return }
            if status == .stopped {
                self.stateLock.lock()
                let intentional = self.stopRequested
                self.stateLock.unlock()
                if !intentional {
                    self.markStopped("CGDisplayStream stopped")
                }
                return
            }
            guard status == .frameComplete, let surface else { return }
            var unmanagedPixelBuffer: Unmanaged<CVPixelBuffer>?
            let result = CVPixelBufferCreateWithIOSurface(
                kCFAllocatorDefault,
                surface,
                nil,
                &unmanagedPixelBuffer
            )
            guard result == kCVReturnSuccess,
                  let pixelBuffer = unmanagedPixelBuffer?.takeRetainedValue() else {
                return
            }
            self.handlePixelBuffer(
                pixelBuffer,
                pts: CMClockGetTime(CMClockGetHostTimeClock()),
                duration: CMTime(value: 1, timescale: CMTimeScale(self.fps))
            )
        }
        // The current SDK header documents a false default even though older
        // online documentation described the cursor as visible by default.
        // Resolve the obsoleted key dynamically alongside CGDisplayStream and
        // opt in explicitly so the Host cursor remains part of the video.
        // The legacy stream's cursor property is fixed at creation; the
        // cursor-plane opt-in refuses this backend rather than silently
        // ignoring LCDOFF-driven visibility changes.
        let properties: NSDictionary = [
            api.showCursorKey: captureEmbedsCursor() ? kCFBooleanTrue! : kCFBooleanFalse!,
            // CGDisplayStream interprets this as a maximum update rate. It
            // cannot manufacture frames when the display is idle, but an
            // explicit 60fps ceiling keeps the legacy path aligned with the
            // ScreenCaptureKit configuration and avoids an OS-selected rate.
            api.minimumFrameTimeKey: NSNumber(
                value: captureMinimumFrameTimeSeconds(fps: fps)
            ),
            api.queueDepthKey: NSNumber(
                value: captureQueueDepth(experiment: requestedEncoderExperiment)
            )
        ]
        guard let cgStream = api.create(
            displayID,
            Int(outWidth),
            Int(outHeight),
            Int32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
            properties,
            queue,
            handler
        )?.takeRetainedValue() else {
            let reason = "CGDisplayStream creation failed"
            setLastError(reason)
            markStopped(reason)
            return false
        }
        self.cgStreamAPI = api
        self.cgStream = cgStream
        let status = api.start(cgStream)
        guard status == .success else {
            let reason = "CGDisplayStreamStart failed: \(status.rawValue)"
            setLastError(reason)
            markStopped(reason)
            return false
        }
        captureDidStart()
        return true
    }

    // MARK: - Capture-stall watchdog

    /// Arm the capture-stall check. Called from `captureDidStart` so every
    /// start — including a watchdog-driven restart — re-enters monitoring.
     func armCaptureHealthCheck() {
        stateLock.lock()
        guard running, !stopRequested, !captureHealthCheckScheduled else {
            stateLock.unlock()
            return
        }
        captureHealthCheckScheduled = true
        stateLock.unlock()
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.captureHealthCheckTick()
        }
    }

     private func captureHealthCheckTick() {
        stateLock.lock()
        captureHealthCheckScheduled = false
        let runningNow = running
        let stopping = stopRequested
        let last = lastCaptureCallbackNs ?? 0
        let restarts = captureWatchdogRestarts
        stateLock.unlock()
        let action = captureStallAction(
            lastCallbackNs: last,
            nowNs: DispatchTime.now().uptimeNanoseconds,
            running: runningNow,
            stopRequested: stopping,
            restarts: restarts,
            maxRestarts: captureStallMaxRestarts,
            stallThresholdNs: captureStallThresholdNs
        )
        switch action {
        case .idle:
            // Stopping or never started: the stop path owns teardown.
            return
        case .monitor:
            break
        case .restart:
            stateLock.lock()
            captureWatchdogRestarts &+= 1
            lastCaptureCallbackNs = DispatchTime.now().uptimeNanoseconds
            stateLock.unlock()
            restartCaptureStream()
        case .exhausted:
            let reason = "capture stalled; \(captureStallMaxRestarts) restart attempts exhausted"
            NSLog("Leftcar %@", reason)
            markStopped(reason)
            return
        }
        armCaptureHealthCheck()
    }

    /// Stop the live capture stream and rebuild it from the parameters the
    /// start path retained. VideoToolbox, the network queue, and the control
    /// plane are untouched — only the frame source bounces, so the viewer
    /// sees at most a momentary pause instead of a silent permanent freeze.
     func restartCaptureStream() {
        stateLock.lock()
        let filter = restartFilter
        let displayID = restartDisplayID
        let oldStream = stream
        let oldCG = cgStream
        let api = cgStreamAPI
        stream = nil
        streamHandler = nil
        cgStream = nil
        cgStreamAPI = nil
        stateLock.unlock()
        NSLog("Leftcar capture watchdog: no capture callbacks for %.1fs; restarting capture stream", Double(captureStallThresholdNs) / 1_000_000_000)
        // 재시작 후 첫 프레임을 IDR로 싣는다 — 공백 동안 뷰어가 참조 프레임을
        // 잃었을 수 있고, 뷰어가 손실을 알아채 회복을 요청하기 전에 디코드
        // 연속성을 회복하는 쪽이 싸다. 재시작이 실패하면 세션이 곧 종료되므로
        // 남는 플래그는 무해하다.
        stateLock.lock()
        forceKeyframe = true
        stateLock.unlock()
        if let oldStream {
            oldStream.stopCapture(completionHandler: nil)
        }
        if let oldCG, let api {
            _ = api.stop(oldCG)
        }
        if let filter {
            _ = setupScreenCaptureKit(filter: filter)
        } else if let displayID {
            _ = setupCGDisplayStream(displayID: displayID)
        } else {
            NSLog("Leftcar capture watchdog: no retained capture parameters; cannot restart")
            markStopped("capture stalled; no restart parameters retained")
        }
    }
}

