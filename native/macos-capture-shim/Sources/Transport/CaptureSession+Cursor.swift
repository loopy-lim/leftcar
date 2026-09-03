import Foundation
import CoreGraphics
import CoreMedia
import CoreVideo
import ScreenCaptureKit
import Darwin

extension CaptureSession {
    /// Viewer opt-in (LCDON) or opt-out (LCDOFF). Both arrive as plaintext
    /// token-authenticated frames on the control channel — the same class as
    /// IDR/BYE — so authentication already happened in the dispatching parse
    /// loop. UDP keeps the requester's ephemeral control port as the LCD1
    /// destination; TCP responses reuse the control framing.
     func handleCursorStreamCommand(
        _ command: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        let enable = command == Data("LCDON".utf8)
        guard enable || command == Data("LCDOFF".utf8) else { return }
        // splitVertical tiles stream half-frame video, and the legacy
        // CGDisplayStream backend cannot change its cursor property after
        // creation. Both refuse the opt-in; the viewer observes no LCD1
        // traffic and falls back to the embedded cursor per the design's
        // error-handling table.
        guard requestedEncoderExperiment != .splitVertical,
              backend == .screenCaptureKit else { return }
        cursorLock.lock()
        if mediaTransport.usesTCP {
            cursorStreamDestination = nil
        } else {
            guard let destination else {
                cursorLock.unlock()
                return
            }
            cursorStreamDestination = destination
        }
        cursorStreamFD = fd
        cursorLock.unlock()
        setCursorStreamEnabled(enable)
    }

     func setCursorStreamEnabled(_ enable: Bool) {
        // Read the content rect before taking cursorLock: nothing else takes
        // inputLock while holding cursorLock, and keeping this path free of
        // nested locks removes any lock-order reasoning entirely.
        let bounds = capturedContentRect()
        cursorLock.lock()
        let changed = cursorStreamEnabled != enable
        cursorStreamEnabled = enable
        if enable {
            var coordinator = cursorCoordinator ?? CursorStreamCoordinator(
                fps: fps,
                bounds: bounds
            )
            // The coordinator embeds the session token inside every encoded
            // packet (encodeCursorPacket appends it) and stays silent until a
            // token is installed, so enabling must carry the current token.
            coordinator.setToken(viewerControlToken)
            coordinator.setEnabled(true)
            cursorCoordinator = coordinator
        } else {
            cursorCoordinator?.setEnabled(false)
        }
        cursorLock.unlock()
        guard changed else { return }
        if enable {
            startCursorPollingTimer()
            guard installCursorEventTap() else {
                // A tap that cannot be created (untrusted process) means no
                // observation will ever flow. Fail closed: keep the cursor
                // embedded in the video instead of hiding it behind a stream
                // that stays silent.
                print("cursor event tap unavailable; LCD1 stream stays off for \(targetLabel)")
                setCursorStreamEnabled(false)
                return
            }
        } else {
            removeCursorEventTap()
            stopCursorPollingTimer()
        }
        applyCaptureCursorVisibility()
    }

    /// True while the embedded cursor belongs in the video — the default —
    /// and false exactly while LCD1 samples are flowing.
     func captureEmbedsCursor() -> Bool {
        cursorLock.lock()
        let embeds = !cursorStreamEnabled
        cursorLock.unlock()
        return embeds
    }

    /// The canonical stream configuration for this session. Stream creation
    /// and the cursor-visibility update must apply the identical complete
    /// field set: updateConfiguration replaces the whole configuration, so a
    /// showsCursor-only config would silently reset the pixel format and
    /// frame pacing to framework defaults.
     func streamConfiguration(showsCursor: Bool) -> SCStreamConfiguration {
        let config = SCStreamConfiguration()
        config.width = Int(outWidth)
        config.height = Int(outHeight)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        // Feed VideoToolbox the native bi-planar 4:2:0 surface so the
        // capture path avoids a BGRA -> YUV conversion per frame.
        config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        config.showsCursor = showsCursor
        // Split encoding retains the source IOSurface until both tile
        // submissions finish. Give that mode enough framework surfaces
        // for its bounded app queue plus encoder in-flight slots; normal
        // capture keeps the smaller low-memory cushion.
        config.queueDepth = captureQueueDepth(
            experiment: requestedEncoderExperiment
        )
        config.backgroundColor = CGColor.black
        if #available(macOS 14.0, *) {
            config.shouldBeOpaque = true
        }
        return config
    }

    /// Cursor separation only makes sense while the stream is live; capture
    /// hides the cursor exactly while LCD1 samples are flowing and restores
    /// the embedded cursor the moment the stream stops.
     func applyCaptureCursorVisibility() {
        guard let stream else { return }
        stream.updateConfiguration(
            streamConfiguration(showsCursor: captureEmbedsCursor()),
            completionHandler: nil
        )
    }

    /// A listen-only HID tap observes physical and injected mouse movement
    /// without swallowing events — listen-only taps never consume what they
    /// watch, so remote input injection in CaptureSession+Input is
    /// unaffected. macOS may still require the process to be trusted for
    /// event listening, in which case tapCreate returns nil and the caller
    /// fails closed.
     func installCursorEventTap() -> Bool {
        cursorLock.lock()
        let existing = cursorEventTap
        cursorLock.unlock()
        if existing != nil { return true }
        let mask: CGEventMask = (1 << CGEventType.mouseMoved.rawValue)
            | (1 << CGEventType.leftMouseDragged.rawValue)
            | (1 << CGEventType.rightMouseDragged.rawValue)
            | (1 << CGEventType.otherMouseDragged.rawValue)
        guard let tap = CGEvent.tapCreate(
            tap: .cghidEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: mask,
            callback: { _, type, event, userInfo -> Unmanaged<CGEvent>? in
                // Listen-only taps pass every event through regardless of the
                // return value; nil simply declines to modify or suppress it.
                CursorEventTapBridge.handle(type: type, event: event, userInfo: userInfo)
                return nil
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else { return false }
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        cursorLock.lock()
        cursorEventTapSource = source
        cursorEventTap = tap
        cursorLock.unlock()
        return true
    }

     func removeCursorEventTap() {
        cursorLock.lock()
        let tap = cursorEventTap
        let source = cursorEventTapSource
        cursorEventTap = nil
        cursorEventTapSource = nil
        cursorLock.unlock()
        guard let tap else { return }
        CGEvent.tapEnable(tap: tap, enable: false)
        if let source {
            CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes)
        }
        // The run loop source retains the port, so ARC alone would leave a
        // disabled tap installed until that reference drained; an explicit
        // invalidation keeps teardown deterministic.
        CFMachPortInvalidate(tap)
    }

    /// macOS disables an event tap when its run loop stalls or another
    /// client re-takes the event stream; the notification arrives as a
    /// callback and the documented convention is to re-enable from there.
    private func reenableCursorEventTap() {
        cursorLock.lock()
        let tap = cursorEventTap
        cursorLock.unlock()
        guard let tap else { return }
        CGEvent.tapEnable(tap: tap, enable: true)
    }

     func startCursorPollingTimer() {
        cursorLock.lock()
        let existing = cursorPollingTimer
        cursorLock.unlock()
        guard existing == nil else { return }
        let hz = cursorPollingHz(fps: fps)
        let timer = DispatchSource.makeTimerSource(queue: inputQueue)
        timer.schedule(
            deadline: .now(),
            repeating: .milliseconds(max(1, 1_000 / Int(hz)))
        )
        timer.setEventHandler { [weak self] in
            self?.sendCursorPacketIfDue()
        }
        timer.resume()
        cursorLock.lock()
        cursorPollingTimer = timer
        cursorLock.unlock()
    }

     func stopCursorPollingTimer() {
        cursorLock.lock()
        let timer = cursorPollingTimer
        cursorPollingTimer = nil
        cursorLock.unlock()
        timer?.cancel()
    }

     func sendCursorPacketIfDue() {
        cursorLock.lock()
        let packet = cursorCoordinator?.packetDue(
            nowUs: DispatchTime.now().uptimeNanoseconds / 1_000
        )
        cursorLock.unlock()
        guard let payload = packet else { return }
        cursorLock.lock()
        let fd = cursorStreamFD
        let destination = cursorStreamDestination
        cursorLock.unlock()
        // The coordinator already embeds the session token inside the
        // encoded LCD1 packet; appending it again would break the wire
        // layout the viewer's parser expects.
        _ = sendControlPayload(payload, fd: fd, destination: destination)
    }

    /// Teardown hook called from the session stop path — restores the
    /// embedded cursor even if the viewer vanished without LCDOFF.
     func teardownCursorStream() {
        setCursorStreamEnabled(false)
        cursorLock.lock()
        cursorCoordinator = nil
        cursorStreamDestination = nil
        cursorStreamFD = -1
        cursorLock.unlock()
    }

    /// Capture content rect the coordinator normalizes against. The output
    /// dimensions stand in until capture has published real bounds — LCDON
    /// can arrive between control-receiver start and first capture setup.
    private func capturedContentRect() -> CGRect {
        inputLock.lock()
        let bounds = inputBounds
        inputLock.unlock()
        return bounds ?? CGRect(
            x: 0,
            y: 0,
            width: CGFloat(outWidth),
            height: CGFloat(outHeight)
        )
    }

    /// Runs on the main run loop via the tap's run loop source.
     func handleCursorTapEvent(type: CGEventType, event: CGEvent) {
        switch type {
        case .tapDisabledByTimeout, .tapDisabledByUserInput:
            reenableCursorEventTap()
        case .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged:
            observeCursorPosition(event.location)
        default:
            break
        }
    }

    /// Visibility follows the design's meaning for the viewer: show the
    /// overlay only while the cursor is inside the captured content rect.
    /// The legacy `CGCursorIsVisible` API (deprecated since 10.9, unavailable
    /// in the current SDK) could not have answered this anyway — it reports
    /// presence on any display, while the coordinator clamps coordinates into
    /// the rect, so an off-rect cursor must arrive as visible=0 or the
    /// viewer's overlay would stick to the clamped corner.
     func observeCursorPosition(_ position: CGPoint) {
        let onScreen = capturedContentRect().contains(position)
        cursorLock.lock()
        cursorCoordinator?.note(position: position, visible: onScreen)
        cursorLock.unlock()
    }
}

/// CGEvent tap callbacks are C function pointers: they cannot capture
/// context, so the tap's `userInfo` carries an unowned session reference
/// that this trampoline recovers per event. The tap is created, stored, and
/// invalidated by that same session — and teardown also runs in `deinit` —
/// so the pointer cannot outlive its owner. Routing through the tap's own
/// context (instead of a global session slot) keeps events bound to the
/// session that installed the tap even with the v2 registry's concurrent
/// sessions.
private enum CursorEventTapBridge {
    static func handle(
        type: CGEventType,
        event: CGEvent,
        userInfo: UnsafeMutableRawPointer?
    ) {
        guard let userInfo else { return }
        let session = Unmanaged<CaptureSession>
            .fromOpaque(userInfo)
            .takeUnretainedValue()
        session.handleCursorTapEvent(type: type, event: event)
    }
}
