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
        inputLock.lock()
        if #available(macOS 14.0, *) {
            inputBounds = filter.contentRect
        }
        inputLock.unlock()
        guard beginCapture() else {
            setLastError("media socket closed before capture start")
            return false
        }
        let completion = DispatchSemaphore(value: 0)
        let completionLock = NSLock()
        var setupFailure: String?
        var startError: Error?

        // Construct and start AppKit-adjacent ScreenCaptureKit objects on the
        // main queue. Do not wait there: the Rust control worker owns the
        // semaphore wait, leaving the main run loop free to receive replayd's
        // completion callback.
        DispatchQueue.main.async { [self] in
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
        completionLock.unlock()
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
        inputLock.lock()
        inputBounds = CGDisplayBounds(displayID)
        inputLock.unlock()
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
}

