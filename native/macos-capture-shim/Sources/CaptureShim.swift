// CaptureShim: real macOS screen capture -> H.264 -> TCP, as a C-ABI dylib.
//
// Path (docs/02 §4, H16-H18; rebuild design 2026-08-18):
//   SCShareableContent (selected display) -> SCStream (BGRA frames)
//   -> VTCompressionSession (H.264 baseline, realtime, no B-frames)
//   -> TCP framed stream ([u32 BE len][payload]; CFG csd + AU packets).
//
// v2: handle table — multiple concurrent sessions (multi-display /
// multi-viewer), parameterized fps/bitrate, JSON stats, auto-stop on
// viewer disconnect.

import Foundation
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import Darwin

// MARK: - C ABI surface (v2, handle-based)

private let registryLock = NSLock()
private var registry: [UInt32: CaptureSession] = [:]
private var nextHandle: UInt32 = 1

// SCShareableContent enumeration is relatively expensive and concurrent
// callers can amplify a temporary WindowServer delay. Keep one in-flight
// request, serve the last successful catalog immediately, and refresh stale
// data in the background. Display topology changes are rare compared with
// catalog/status polling, so stale-while-refresh is the safer UI contract.
private final class DisplayCatalogCache {
    private let condition = NSCondition()
    private var displays: [SCDisplay] = []
    private var refreshedAt: Date?
    private var refreshInFlight = false
    private var refreshGeneration: UInt64 = 0
    private var lastRefreshError: String?

    func snapshot() -> (displays: [SCDisplay], age: TimeInterval)? {
        condition.lock()
        defer { condition.unlock() }
        guard !displays.isEmpty, let refreshedAt else { return nil }
        return (displays, Date().timeIntervalSince(refreshedAt))
    }

    func display(at index: Int) -> SCDisplay? {
        condition.lock()
        defer { condition.unlock() }
        guard displays.indices.contains(index) else { return nil }
        return displays[index]
    }

    func refreshIfStale(maxAge: TimeInterval) {
        let shouldRefresh: Bool
        condition.lock()
        shouldRefresh = refreshedAt.map { Date().timeIntervalSince($0) > maxAge } ?? true
        condition.unlock()
        if shouldRefresh {
            beginRefreshIfNeeded()
        }
    }

    func firstSnapshot(timeout: TimeInterval) -> (displays: [SCDisplay]?, error: String?) {
        beginRefreshIfNeeded()

        condition.lock()
        let deadline = Date().addingTimeInterval(timeout)
        while displays.isEmpty && refreshInFlight {
            if !condition.wait(until: deadline) {
                break
            }
        }
        let snapshot = displays.isEmpty ? nil : displays
        let error = lastRefreshError
        let stillRefreshing = refreshInFlight
        if displays.isEmpty && stillRefreshing {
            // ScreenCaptureKit occasionally never invokes an enumeration
            // callback after a host reinstall/relaunch. Invalidate that
            // generation so the next refresh can issue a fresh request instead
            // of timing out forever behind a permanently in-flight flag.
            refreshGeneration &+= 1
            refreshInFlight = false
            lastRefreshError = "SCShareableContent timed out"
            condition.broadcast()
        }
        condition.unlock()

        if snapshot != nil {
            return (snapshot, nil)
        }
        if stillRefreshing {
            return (nil, "SCShareableContent timed out")
        }
        return (nil, error ?? "ScreenCaptureKit returned no displays")
    }

    private func beginRefreshIfNeeded() {
        condition.lock()
        guard !refreshInFlight else {
            condition.unlock()
            return
        }
        refreshInFlight = true
        refreshGeneration &+= 1
        let generation = refreshGeneration
        condition.unlock()

        requestShareableContent { [weak self] content, error in
            guard let self else { return }
            self.condition.lock()
            guard self.refreshGeneration == generation else {
                self.condition.unlock()
                return
            }
            if let content, error == nil, !content.displays.isEmpty {
                self.displays = normalizedDisplays(content.displays)
                self.refreshedAt = Date()
                self.lastRefreshError = nil
            } else {
                self.lastRefreshError = error.map {
                    "SCShareableContent failed: \($0.localizedDescription)"
                } ?? "ScreenCaptureKit returned no displays"
            }
            self.refreshInFlight = false
            self.condition.broadcast()
            self.condition.unlock()
        }

        // A ScreenCaptureKit request issued while the host is still bringing
        // up AppKit can occasionally never invoke its completion handler.
        // Release that generation without waiting for a foreground
        // startStream call to hit its much longer timeout. A later catalog or
        // stream request can then issue a fresh enumeration on the live app.
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 5) { [weak self] in
            guard let self else { return }
            self.condition.lock()
            guard self.refreshGeneration == generation, self.refreshInFlight else {
                self.condition.unlock()
                return
            }
            self.refreshGeneration &+= 1
            self.refreshInFlight = false
            self.lastRefreshError = "SCShareableContent timed out"
            self.condition.broadcast()
            self.condition.unlock()
        }
    }
}

private let displayCatalogCache = DisplayCatalogCache()

private func normalizedDisplays(_ displays: [SCDisplay]) -> [SCDisplay] {
    let mainDisplayID = CGMainDisplayID()
    return displays.sorted { lhs, rhs in
        if lhs.displayID == mainDisplayID { return true }
        if rhs.displayID == mainDisplayID { return false }
        return lhs.displayID < rhs.displayID
    }
}

private func displayCatalogJSON(_ displays: [SCDisplay]) -> String {
    let arr: [[String: Any]] = displays.enumerated().map { idx, d in
        [
            "index": idx,
            "name": "Display \(idx)",
            "width": Int(d.width),
            "height": Int(d.height),
        ]
    }
    guard let data = try? JSONSerialization.data(withJSONObject: arr),
          let json = String(data: data, encoding: .utf8) else {
        return "[]"
    }
    return json
}

private func coreGraphicsCatalogJSON() -> String? {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else {
        return nil
    }
    var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(count))
    var filled = count
    guard CGGetActiveDisplayList(count, &displayIDs, &filled) == .success else {
        return nil
    }
    let mainDisplayID = CGMainDisplayID()
    let sortedIDs = displayIDs.prefix(Int(filled)).sorted { lhs, rhs in
        if lhs == mainDisplayID { return true }
        if rhs == mainDisplayID { return false }
        return lhs < rhs
    }
    let entries: [[String: Any]] = sortedIDs.enumerated().map { index, displayID in
        [
            "index": index,
            "name": "Display \(index)",
            "width": CGDisplayPixelsWide(displayID),
            "height": CGDisplayPixelsHigh(displayID),
        ]
    }
    guard let data = try? JSONSerialization.data(withJSONObject: entries),
          let json = String(data: data, encoding: .utf8) else {
        return nil
    }
    return json
}

private func withRegistry<T>(_ body: (inout [UInt32: CaptureSession]) -> T) -> T {
    registryLock.lock()
    defer { registryLock.unlock() }
    return body(&registry)
}

private var lastErrorUTF8: UnsafeMutablePointer<CChar> = UnsafeMutablePointer<CChar>(strdup(""))

private func setLastError(_ message: String) {
    free(lastErrorUTF8)
    lastErrorUTF8 = UnsafeMutablePointer<CChar>(strdup(message))
}

private func hasScreenCaptureAccess() -> Bool {
    // Do not call CGRequestScreenCaptureAccess from the control request. On a
    // release bundle with a new TCC identity macOS may wait for user input
    // while this call is running, which leaves getCatalog stuck on
    // "loading". Permission must be granted explicitly in System Settings;
    // catalog/start then fail immediately with a useful error until it is.
    // CGPreflightScreenCaptureAccess is a non-UI query; keeping it on the
    // control caller avoids synchronously waiting for Tauri's AppKit thread.
    CGPreflightScreenCaptureAccess()
}

// Use ScreenCaptureKit's structured-concurrency API on the main actor, as in
// Apple's current sample. The control plane waits on its own background
// thread, so AppKit stays free to service TCC and WindowServer callbacks.
private func requestShareableContent(
    completion: @escaping (SCShareableContent?, Error?) -> Void
) {
    Task { @MainActor in
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(
                true,
                onScreenWindowsOnly: true
            )
            completion(content, nil)
        } catch {
            completion(nil, error)
        }
    }
}

@_cdecl("leftcar_capture_list_displays")
public func leftcarCaptureListDisplays() -> UnsafeMutablePointer<CChar> {
    guard hasScreenCaptureAccess() else {
        setLastError("screen-recording permission is not granted to Leftcar Host")
        return UnsafeMutablePointer<CChar>(strdup("[]"))
    }
    if let snapshot = displayCatalogCache.snapshot() {
        displayCatalogCache.refreshIfStale(maxAge: 30)
        setLastError("")
        return UnsafeMutablePointer<CChar>(strdup(displayCatalogJSON(snapshot.displays)))
    }
    // The source catalog only needs stable display metadata. CoreGraphics can
    // provide that synchronously while ScreenCaptureKit warms in the
    // background; both paths use the same main-display-first ordering.
    displayCatalogCache.refreshIfStale(maxAge: 0)
    if let fallback = coreGraphicsCatalogJSON() {
        setLastError("")
        return UnsafeMutablePointer<CChar>(strdup(fallback))
    }
    let result = displayCatalogCache.firstSnapshot(timeout: 13)
    guard let displays = result.displays else {
        setLastError(result.error ?? "SCShareableContent failed")
        return UnsafeMutablePointer<CChar>(strdup("[]"))
    }
    setLastError("")
    return UnsafeMutablePointer<CChar>(strdup(displayCatalogJSON(displays)))
}

@_cdecl("leftcar_capture_start_v2")
public func leftcarCaptureStartV2(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32
) -> UInt32 {
    guard hasScreenCaptureAccess() else {
        setLastError("screen-recording permission is not granted to Leftcar Host")
        return 0
    }
    guard let ipStr = ip.loadedCString() else {
        setLastError("null ip")
        return 0
    }
    var addr = sockaddr_in()
    addr.sin_family = sa_family_t(AF_INET)
    addr.sin_port = port.bigEndian
    guard inet_pton(AF_INET, ipStr, &addr.sin_addr) == 1 else {
        setLastError("invalid viewer ip: \(ipStr)")
        return 0
    }

    let session = CaptureSession(
        targetAddr: addr,
        targetPort: port,
        targetLabel: "\(ipStr):\(port)",
        width: width,
        height: height,
        fps: fps
    )

    var ok = false
    var selectedDisplay = displayCatalogCache.display(at: Int(displayIndex))
    if selectedDisplay == nil {
        let result = displayCatalogCache.firstSnapshot(timeout: 13)
        guard let displays = result.displays else {
            setLastError(result.error ?? "SCShareableContent failed")
            return 0
        }
        guard Int(displayIndex) < displays.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displays.count) displays)")
            return 0
        }
        selectedDisplay = displays[Int(displayIndex)]
    }
    if let selectedDisplay {
        // Keep SCStream setup off the AppKit/ScreenCaptureKit callback queue;
        // the query itself is filtered to on-screen content, while stream
        // output and encoder startup belong to the capture queue.
        ok = session.setupStream(display: selectedDisplay)
    }
    guard ok else { return 0 }

    // TCP connect happens on the session queue before first frame send
    let connected = session.connectSocket()
    guard connected else { return 0 }

    let handle = withRegistry { reg in
        let h = nextHandle
        nextHandle += 1
        reg[h] = session
        return h
    }
    return handle
}

@_cdecl("leftcar_capture_stop_v2")
public func leftcarCaptureStopV2(handle: UInt32) -> Int32 {
    let session = withRegistry { reg in
        reg.removeValue(forKey: handle)
    }
    guard let session else {
        setLastError("no such handle \(handle)")
        return 1
    }
    session.stop()
    return 0
}

@_cdecl("leftcar_capture_stats_v2")
public func leftcarCaptureStatsV2(handle: UInt32) -> UnsafeMutablePointer<CChar> {
    let session = withRegistry { reg in reg[handle] }
    guard let session else {
        return UnsafeMutablePointer<CChar>(strdup("{\"state\":\"unknown\"}"))
    }
    let json = session.statsJSON()
    return UnsafeMutablePointer<CChar>(strdup(json))
}

@_cdecl("leftcar_capture_free_string")
public func leftcarCaptureFreeString(s: UnsafeMutablePointer<CChar>) {
    free(s)
}

@_cdecl("leftcar_capture_last_error_v2")
public func leftcarCaptureLastErrorV2() -> UnsafePointer<CChar> {
    UnsafePointer(lastErrorUTF8)
}

extension UnsafePointer where Pointee == CChar {
    /// Read a C string safely (nil-check + UTF-8 decode).
    func loadedCString() -> String? {
        String(validatingUTF8: self)
    }
}

// MARK: - Stream Handler

final class CaptureOutputHandler: NSObject, SCStreamOutput, SCStreamDelegate {
    weak var session: CaptureSession?

    init(session: CaptureSession) {
        self.session = session
        super.init()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen else { return }
        session?.handleFrame(sampleBuffer)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        session?.markStopped("Stream stopped: \(error.localizedDescription)")
    }
}

// MARK: - Session (one per active stream)

private struct PendingCaptureFrame {
    let sample: CMSampleBuffer
    let callbackNs: UInt64
}

final class CaptureSession {
    private let queue = DispatchQueue(label: "leftcar.capture", qos: .userInteractive)
    // Capture callbacks only publish the newest sample here. Encoding runs on
    // its own serial queue, so a slow VideoToolbox callback cannot make
    // ScreenCaptureKit wait behind an older frame.
    private let encodeQueue = DispatchQueue(label: "leftcar.encode", qos: .userInteractive)
    private let encodeQueueKey = DispatchSpecificKey<Void>()
    private let captureLock = NSLock()
    private var pendingCapture: PendingCaptureFrame?
    private var encodeScheduled = false
    private var sock: Int32 = -1
    private var stream: SCStream?
    private var streamHandler: CaptureOutputHandler?
    private var session: VTCompressionSession?
    private let targetAddr: sockaddr_in
    private let targetPort: UInt16
    private let targetLabel: String
    private let outWidth: UInt32
    private let outHeight: UInt32
    private let fps: UInt32
    private var csdSent = false

    // The encoder callback must never wait behind a slow TCP reader. Keep at
    // most the newest encoded AU plus the latest config packet; an older AU
    // that has not reached the socket is intentionally dropped.
    private let networkQueue = DispatchQueue(label: "leftcar.network", qos: .userInteractive)
    private let networkLock = NSLock()
    private var pendingConfig: Data?
    private var pendingFrame: Data?
    private var viewerControlBuffer = Data()
    private var networkDrainScheduled = false
    private var reconnectScheduled = false

    private let stateLock = NSLock()
    private var running = false
    private var stopRequested = false
    private var framesEncoded: Int64 = 0
    private var framesDropped: Int64 = 0
    private var captureQueueDropped: Int64 = 0
    private var bytesSent: Int64 = 0
    private var lastCaptureToEncodeUs: UInt64 = 0
    private var maxCaptureToEncodeUs: UInt64 = 0
    private var lastCaptureQueueWaitUs: UInt64 = 0
    private var maxCaptureQueueWaitUs: UInt64 = 0
    private var lastEncodeOutputUs: UInt64 = 0
    private var maxEncodeOutputUs: UInt64 = 0
    private var lastSendBlockUs: UInt64 = 0
    private var maxSendBlockUs: UInt64 = 0
    private var stoppedReason = ""

    // Capture callback timestamps keyed by the real sample PTS. VideoToolbox
    // may call its output callback asynchronously, so this lets stats expose
    // capture -> encoded-output latency without putting a wait in the hot path.
    private var captureNsByPts: [Int64: UInt64] = [:]
    private var encodeSubmitNsByPts: [Int64: UInt64] = [:]
    // VideoToolbox output callbacks can arrive after the next input frame has
    // already been submitted. Allocate the wire AU id at submission time and
    // recover it by PTS in the callback; reading framesEncoded in the callback
    // can assign the same id to two asynchronously completed frames.
    private var encodeAuIdByPts: [Int64: UInt16] = [:]
    private var nextAuId: UInt16 = 0

    // 1s-window rate counters for stats
    private var rateWindowStart = Date()
    private var rateWindowFrames: Int64 = 0
    private var rateWindowBytes: Int64 = 0
    private var lastFps: UInt32 = 0
    private var lastKbps: UInt32 = 0
    private var forceKeyframe = false

    var isRunning: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return running
    }

    init(targetAddr: sockaddr_in, targetPort: UInt16, targetLabel: String, width: UInt32, height: UInt32, fps: UInt32) {
        self.targetAddr = targetAddr
        self.targetPort = targetPort
        self.targetLabel = targetLabel
        self.outWidth = width
        self.outHeight = height
        // The current capture/display profile is S1: 1080p60. Clamp callers
        // that still send the old 90fps request so encoder, capture and
        // viewer timing cannot silently disagree.
        self.fps = min(max(1, fps), 60)
        encodeQueue.setSpecific(key: encodeQueueKey, value: ())
    }

    // MARK: Setup

    func connectSocket(stopOnFailure: Bool = true) -> Bool {
        // StreamLauncher starts the XR Activity asynchronously. The control
        // request can therefore arrive before its TCP listener has bound the
        // port. Keep trying the same endpoint for a bounded window instead of
        // turning that normal startup race into a black screen.
        let deadline = Date().addingTimeInterval(10.0)
        var lastConnectError: Int32 = ETIMEDOUT

        while Date() < deadline {
            stateLock.lock()
            let shouldStop = stopRequested
            stateLock.unlock()
            if shouldStop { return false }

            sock = socket(AF_INET, SOCK_STREAM, 0)
            guard sock >= 0 else {
                let reason = "socket() failed"
                if stopOnFailure {
                    setLastError(reason)
                    markStopped(reason)
                }
                return false
            }
            var noDelay: Int32 = 1
            setsockopt(sock, IPPROTO_TCP, TCP_NODELAY, &noDelay, socklen_t(MemoryLayout<Int32>.size))
            var noSigPipe: Int32 = 1
            setsockopt(sock, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))
            // Keep the kernel queue small so TCP backpressure cannot hide a
            // large stale-video backlog. The application-level slot below
            // already keeps only the newest frame.
            var sendBuffer: Int32 = 128 * 1024
            setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &sendBuffer, socklen_t(MemoryLayout<Int32>.size))

            // A VPN/Tailscale address may be unroutable from the Mac. A
            // blocking connect would hold the control request for the OS TCP
            // timeout; bound each attempt so the listener can appear during
            // the XR Activity startup race.
            let originalFlags = fcntl(sock, F_GETFL, 0)
            if originalFlags >= 0 {
                _ = fcntl(sock, F_SETFL, originalFlags | O_NONBLOCK)
            }
            var addr = targetAddr
            var result: Int32 = -1
            var connectError: Int32 = 0
            withUnsafePointer(to: &addr) { ap in
                ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                    result = connect(sock, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
                    if result != 0 {
                        connectError = errno
                    }
                }
            }
            if result != 0 && (connectError == EINPROGRESS || connectError == EWOULDBLOCK) {
                var pfd = pollfd(fd: sock, events: Int16(POLLOUT), revents: 0)
                if poll(&pfd, 1, 500) > 0 {
                    var socketError: Int32 = 0
                    var socketErrorLength = socklen_t(MemoryLayout<Int32>.size)
                    getsockopt(sock, SOL_SOCKET, SO_ERROR, &socketError, &socketErrorLength)
                    if socketError == 0 {
                        result = 0
                    } else {
                        connectError = socketError
                    }
                } else {
                    connectError = ETIMEDOUT
                }
            }
            if originalFlags >= 0 {
                _ = fcntl(sock, F_SETFL, originalFlags)
            }
            if result == 0 {
                stateLock.lock()
                if stopRequested {
                    stateLock.unlock()
                    close(sock)
                    sock = -1
                    return false
                }
                stateLock.unlock()
                running = true
                return true
            }

            lastConnectError = connectError
            close(sock)
            sock = -1
            usleep(100_000)
        }

        let reason = "TCP connect to \(targetLabel) failed after retry (errno \(lastConnectError)) — is the stream window open?"
        if stopOnFailure {
            setLastError(reason)
            markStopped(reason)
        }
        return false
    }

    static let tccDeniedHint = "screen-recording permission required (System Settings > Privacy & Security > Screen Recording)"

    @discardableResult
    func setupStream(display: SCDisplay) -> Bool {
        let config = SCStreamConfiguration()
        config.width = Int(outWidth)
        config.height = Int(outHeight)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        // Feed VideoToolbox the native bi-planar 4:2:0 surface so the capture
        // path does not make every frame pay for a BGRA -> YUV conversion.
        // ScreenCaptureKit supports 420v directly for screen streams.
        config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        config.showsCursor = true
        // Every queued capture frame is future latency. Two gives
        // ScreenCaptureKit one spare buffer without allowing a multi-frame
        // backlog to become visible interaction delay.
        config.queueDepth = 2

        let filter = SCContentFilter(display: display, excludingWindows: [])
        let s = SCStream(filter: filter, configuration: config, delegate: nil)
        let handler = CaptureOutputHandler(session: self)

        do {
            try s.addStreamOutput(handler, type: .screen, sampleHandlerQueue: queue)
            streamHandler = handler
            s.startCapture { [weak self] error in
                if let error = error {
                    let reason = "startCapture failed: \(error.localizedDescription) — \(CaptureSession.tccDeniedHint)"
                    setLastError(reason)
                    self?.markStopped(reason)
                }
            }
            stream = s
            return true
        } catch {
            let reason = "addStreamOutput: \(error.localizedDescription)"
            setLastError(reason)
            markStopped(reason)
            return false
        }
    }

    func stop() {
        stateLock.lock()
        stopRequested = true
        stateLock.unlock()
        networkLock.lock()
        pendingConfig = nil
        pendingFrame = nil
        networkLock.unlock()
        captureLock.lock()
        pendingCapture = nil
        captureLock.unlock()
        if let s = stream {
            s.stopCapture(completionHandler: nil)
            stream = nil
            streamHandler = nil
        }
        invalidateEncoderOnEncodeQueue()
        stateLock.lock()
        if sock >= 0 {
            close(sock)
            sock = -1
        }
        running = false
        stateLock.unlock()
    }

    /// Keep the capture/encoder alive while the viewer's listener is between
    /// connections. The Android renderer intentionally keeps its port open
    /// after EOF, so retrying here is enough to recover without a second JS
    /// control request while StreamActivity is in the foreground.
    private func scheduleReconnect() {
        stateLock.lock()
        guard running, !stopRequested, !reconnectScheduled else {
            stateLock.unlock()
            return
        }
        reconnectScheduled = true
        let staleSocket = sock
        sock = -1
        csdSent = false
        forceKeyframe = true
        stateLock.unlock()

        if staleSocket >= 0 {
            close(staleSocket)
        }
        viewerControlBuffer.removeAll(keepingCapacity: true)
        print("viewer disconnected; heartbeat reconnecting to \(targetLabel)")

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            let reconnected = self.connectSocket(stopOnFailure: false)
            if reconnected {
                print("viewer heartbeat reconnected to \(self.targetLabel)")
            } else if self.isRunning {
                let reason = "viewer connection lost after reconnect timeout"
                print("\(reason): \(self.targetLabel)")
                self.markStopped(reason)
            }
            self.stateLock.lock()
            self.reconnectScheduled = false
            self.stateLock.unlock()
        }
    }

    func markStopped(_ reason: String) {
        stateLock.lock()
        stoppedReason = reason
        stateLock.unlock()
        stop()
    }

    /// Handle an intentional viewer close without synchronously stopping
    /// ScreenCaptureKit from the network drain queue. ScreenCaptureKit may
    /// wait for an in-flight capture callback during stopCapture; doing that
    /// work on the network queue could stall the control server's status path.
    private func requestViewerStop() {
        stateLock.lock()
        guard running, !stopRequested else {
            stateLock.unlock()
            return
        }
        stoppedReason = "viewer closed stream"
        stopRequested = true
        running = false
        let staleSocket = sock
        sock = -1
        stateLock.unlock()

        if staleSocket >= 0 {
            close(staleSocket)
        }
        networkLock.lock()
        pendingConfig = nil
        pendingFrame = nil
        networkLock.unlock()

        queue.async { [weak self] in
            self?.stop()
        }
    }

    // MARK: Frame Processing & VideoToolbox Encoding

    func handleFrame(_ sample: CMSampleBuffer) {
        stateLock.lock()
        let stillRunning = running
        stateLock.unlock()
        guard stillRunning else { return }

        let frame = PendingCaptureFrame(
            sample: sample,
            callbackNs: DispatchTime.now().uptimeNanoseconds
        )
        captureLock.lock()
        let replaced = pendingCapture != nil
        pendingCapture = frame
        let shouldSchedule = !encodeScheduled
        if shouldSchedule {
            encodeScheduled = true
        }
        captureLock.unlock()

        if replaced {
            stateLock.lock()
            captureQueueDropped &+= 1
            stateLock.unlock()
        }
        if shouldSchedule {
            encodeQueue.async { [weak self] in
                self?.drainEncodeQueue()
            }
        }
    }

    private func drainEncodeQueue() {
        while true {
            captureLock.lock()
            let next = pendingCapture
            pendingCapture = nil
            if next == nil {
                encodeScheduled = false
                captureLock.unlock()
                return
            }
            captureLock.unlock()
            if let next {
                encodeFrame(next)
            }
        }
    }

    private func encodeFrame(_ captured: PendingCaptureFrame) {
        stateLock.lock()
        let stillRunning = running
        let submittedFrames = framesEncoded
        stateLock.unlock()
        guard stillRunning else { return }

        guard let pb = CMSampleBufferGetImageBuffer(captured.sample) else { return }
        let encodeStartNs = DispatchTime.now().uptimeNanoseconds
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else { return }

        let inputPts = CMSampleBufferGetPresentationTimeStamp(captured.sample)

        let pts = inputPts.timescale > 0
            ? inputPts
            : CMTime(value: CMTimeValue(submittedFrames), timescale: CMTimeScale(fps))
        let inputDuration = CMSampleBufferGetDuration(captured.sample)
        let duration = inputDuration.timescale > 0
            ? inputDuration
            : CMTime(value: 1, timescale: CMTimeScale(fps))
        stateLock.lock()
        let auId = nextAuId
        nextAuId &+= 1
        captureNsByPts[pts.value] = captured.callbackNs
        encodeSubmitNsByPts[pts.value] = encodeStartNs
        encodeAuIdByPts[pts.value] = auId
        let queueWaitUs = (encodeStartNs &- captured.callbackNs) / 1_000
        lastCaptureQueueWaitUs = queueWaitUs
        maxCaptureQueueWaitUs = max(maxCaptureQueueWaitUs, queueWaitUs)
        if captureNsByPts.count > 256 {
            captureNsByPts.removeValue(forKey: captureNsByPts.keys.first!)
        }
        if encodeSubmitNsByPts.count > 256 {
            encodeSubmitNsByPts.removeValue(forKey: encodeSubmitNsByPts.keys.first!)
        }
        stateLock.unlock()
        var flags: VTEncodeInfoFlags = []
        stateLock.lock()
        let requestKeyframe = forceKeyframe
        forceKeyframe = false
        stateLock.unlock()
        let frameProperties: CFDictionary? = requestKeyframe
            ? [kVTEncodeFrameOptionKey_ForceKeyFrame as String: true] as CFDictionary
            : nil

        let status = VTCompressionSessionEncodeFrame(
            s,
            imageBuffer: pb,
            presentationTimeStamp: pts,
            duration: duration,
            frameProperties: frameProperties,
            infoFlagsOut: &flags
        ) { [weak self] status, _, encodedSample in
            guard status == noErr, let encodedSample = encodedSample else { return }
            self?.handleEncoded(encodedSample)
        }

        if status == noErr {
            stateLock.lock()
            framesEncoded &+= 1
            rateWindowFrames &+= 1
            stateLock.unlock()
        } else {
            stateLock.lock()
            captureNsByPts.removeValue(forKey: pts.value)
            encodeSubmitNsByPts.removeValue(forKey: pts.value)
            encodeAuIdByPts.removeValue(forKey: pts.value)
            stateLock.unlock()
        }
    }

    private func invalidateEncoderOnEncodeQueue() {
        let invalidate = { [weak self] in
            guard let self else { return }
            if let s = self.session {
                VTCompressionSessionInvalidate(s)
                self.session = nil
            }
        }
        if DispatchQueue.getSpecific(key: encodeQueueKey) != nil {
            invalidate()
        } else {
            encodeQueue.sync(execute: invalidate)
        }
    }

    private func setupEncoder(for imageBuffer: CVImageBuffer) {
        let w = Int32(CVPixelBufferGetWidth(imageBuffer))
        let h = Int32(CVPixelBufferGetHeight(imageBuffer))

        // Bitrate budget: favor motion quality on a local LAN while keeping a
        // bounded ceiling. This is a target, not a forced rate: static desktop
        // content will still use less data.
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.10
        let avgBitrate = min(max(idealBits, 8_000_000), 24_000_000)

        // A software fallback is much slower for an interactive remote
        // display and would otherwise be invisible behind the same API.
        // Require the platform H.264 hardware encoder so an unsupported host
        // fails clearly instead of silently adding frame latency.
        let encoderSpecification: CFDictionary = [
            kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder as String: true,
        ] as CFDictionary

        var s: VTCompressionSession?
        let status = VTCompressionSessionCreate(
            allocator: nil,
            width: w,
            height: h,
            codecType: kCMVideoCodecType_H264,
            encoderSpecification: encoderSpecification,
            imageBufferAttributes: nil,
            compressedDataAllocator: nil,
            outputCallback: nil,
            refcon: nil,
            compressionSessionOut: &s
        )
        guard status == noErr, let s = s else {
            markStopped("VTCompressionSessionCreate failed: \(status)")
            return
        }

        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_RealTime, value: true as CFBoolean)
        VTSessionSetProperty(s, key: kVTProfileLevel_H264_Baseline_AutoLevel, value: true as CFBoolean)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AllowFrameReordering, value: false as CFBoolean)
        // This is an interactive remote display, not an offline encode. Ask
        // VideoToolbox to spend its budget on encode latency and keep no
        // additional frame-delay queue in front of the callback.
        VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
            value: true as CFBoolean
        )
        VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_MaxFrameDelayCount,
            value: 0 as CFNumber
        )
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: Int(avgBitrate) as CFNumber)
        // DataRateLimits is expressed as [bytes, seconds], while
        // AverageBitRate is expressed in bits per second. Keep a small
        // 1-second headroom without allowing multi-second bursts.
        let hardLimitBytes = max(1, Int(avgBitrate / 8.0 * 1.5))
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_DataRateLimits, value: [hardLimitBytes, 1] as CFArray)
        // A 0.5s GOP caused a visible periodic keyframe burst. TCP is
        // reliable, so use a roughly 1s nominal GOP while retaining bounded
        // decoder recovery after a reconnect.
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: max(1, fps) as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: Int32(fps) as CFNumber)

        VTCompressionSessionPrepareToEncodeFrames(s)
        session = s
    }

    // MARK: Packetization & TCP Transmission

    private func handleEncoded(_ sample: CMSampleBuffer) {
        guard isRunning else { return }
        let encodeNs = DispatchTime.now().uptimeNanoseconds
        let encodedPts = CMSampleBufferGetPresentationTimeStamp(sample).value
        stateLock.lock()
        let auId = encodeAuIdByPts.removeValue(forKey: encodedPts)
        if let captureNs = captureNsByPts.removeValue(forKey: encodedPts) {
            let elapsedUs = (encodeNs &- captureNs) / 1_000
            lastCaptureToEncodeUs = elapsedUs
            maxCaptureToEncodeUs = max(maxCaptureToEncodeUs, elapsedUs)
        }
        if let submitNs = encodeSubmitNsByPts.removeValue(forKey: encodedPts) {
            let elapsedUs = (encodeNs &- submitNs) / 1_000
            lastEncodeOutputUs = elapsedUs
            maxEncodeOutputUs = max(maxEncodeOutputUs, elapsedUs)
        }
        stateLock.unlock()
        guard let auId else {
            // An output callback arriving after its bookkeeping window is
            // still safer to drop than to emit a duplicate AU id. The next
            // frame will carry a forced IDR and restore decoder continuity.
            stateLock.lock()
            forceKeyframe = true
            csdSent = false
            stateLock.unlock()
            return
        }

        // Send parameter sets (csd: SPS/PPS) periodically so a viewer that
        // joins late (or restarted its decoder) can configure before the
        // next keyframe.
        let attachments = CMSampleBufferGetSampleAttachmentsArray(
            sample,
            createIfNecessary: false
        ) as? [[String: Any]]
        let notSync = attachments?.first?[kCMSampleAttachmentKey_NotSync as String] as? Bool
        let isKeyframe = notSync != true

        stateLock.lock()
        let shouldSendConfig = !csdSent || isKeyframe
        if shouldSendConfig {
            // Reserve this config send while holding the lock so concurrent
            // VideoToolbox callbacks do not all enqueue the same CSD packet.
            csdSent = true
        }
        stateLock.unlock()

        if shouldSendConfig, let fd = sample.formatDescription {
            var cfg = Data([0x43, 0x46, 0x47]) // "CFG"
            var idx = 0
            while true {
                var ptr: UnsafePointer<UInt8>? = nil
                var size = 0
                let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    fd,
                    parameterSetIndex: idx,
                    parameterSetPointerOut: &ptr,
                    parameterSetSizeOut: &size,
                    parameterSetCountOut: nil,
                    nalUnitHeaderLengthOut: nil
                )
                if status != noErr { break }
                if let ptr = ptr {
                    var lenBE = UInt32(size + 4).bigEndian
                    withUnsafeBytes(of: &lenBE) { cfg.append(contentsOf: $0) }
                    cfg.append(contentsOf: [0, 0, 0, 1])
                    cfg.append(contentsOf: UnsafeBufferPointer(start: ptr, count: size))
                }
                idx += 1
            }
            if idx > 0 {
                enqueuePacket(config: cfg)
            } else {
                stateLock.lock()
                csdSent = false
                stateLock.unlock()
            }
        }

        guard let bb = CMSampleBufferGetDataBuffer(sample) else { return }
        var lengthAtOffset = 0
        var totalLength = 0
        var dataPointer: UnsafeMutablePointer<Int8>? = nil
        CMBlockBufferGetDataPointer(
            bb,
            atOffset: 0,
            lengthAtOffsetOut: &lengthAtOffset,
            totalLengthOut: &totalLength,
            dataPointerOut: &dataPointer
        )
        guard let ptr = dataPointer else { return }
        let bytes = UnsafeRawBufferPointer(start: ptr, count: totalLength)

        var pkt = Data([0x41, 0x55]) // "AU"
        let pts = CMSampleBufferGetPresentationTimeStamp(sample).value
        var ptsBE = UInt64(pts).bigEndian
        withUnsafeBytes(of: &ptsBE) { pkt.append(contentsOf: $0) }

        var offset = 0
        while offset < totalLength {
            var length = 0
            for j in 0..<4 {
                length = (length << 8) | Int(bytes[offset + j])
            }
            pkt.append(contentsOf: [0, 0, 0, 1])
            pkt.append(contentsOf: bytes[(offset + 4)..<(offset + 4 + length)])
            offset += 4 + length
        }

        // TCP mode: no MTU fragmentation — the length-prefix framing carries
        // the whole AU in one logical packet; the kernel segments the stream.
        // The F envelope carries the raw Annex-B AU. Do not nest the older
        // AU+PTS packet here: the Android F receiver feeds its payload
        // directly to MediaCodec.
        // F header:
        //   F, fragment-index, fragment-count, AU id (LE), "LT", host wall ms
        // The wall timestamp is only a rough wire-age marker until the
        // control-plane clock offset is measured; it is still useful for
        // correlating host and viewer timing logs during a live run.
        var p2 = Data([0x46, 0, 1, UInt8(auId & 0xFF), UInt8(auId >> 8), 0x4C, 0x54])
        var hostWallMs = UInt64(Date().timeIntervalSince1970 * 1000.0).bigEndian
        withUnsafeBytes(of: &hostWallMs) { p2.append(contentsOf: $0) }
        p2.append(pkt.dropFirst(10))
        enqueuePacket(frame: p2)
    }

    private func enqueuePacket(config: Data? = nil, frame: Data? = nil) {
        networkLock.lock()
        if let config {
            pendingConfig = config
        }
        if let frame {
            if pendingFrame != nil {
                stateLock.lock()
                framesDropped &+= 1
                // Replacing an encoded AU means the viewer may lose an H.264
                // reference frame. Ask VideoToolbox for a fresh IDR before
                // resuming delta frames; otherwise low-latency dropping can
                // turn into visible block corruption until the next GOP.
                forceKeyframe = true
                csdSent = false
                stateLock.unlock()
            }
            pendingFrame = frame
        }
        let schedule = !networkDrainScheduled
        if schedule {
            networkDrainScheduled = true
        }
        networkLock.unlock()

        if schedule {
            networkQueue.async { [weak self] in
                self?.drainNetwork()
            }
        }
    }

    private func drainNetwork() {
        while true {
            networkLock.lock()
            let config = pendingConfig
            pendingConfig = nil
            let frame = pendingFrame
            pendingFrame = nil
            if config == nil && frame == nil {
                networkDrainScheduled = false
                networkLock.unlock()
                return
            }
            networkLock.unlock()

            // Config always precedes the newest pending frame. If capture
            // outruns TCP while writePacket is blocked, the next loop sees
            // only the latest frame rather than replaying a backlog.
            if let config {
                writePacket(config)
            }
            if let frame {
                writePacket(frame)
            }
        }
    }

    /// Consume viewer-to-host control frames on the otherwise bidirectional
    /// TCP socket. The video direction remains host -> viewer; the only
    /// reverse packet currently defined is [u32 BE 3][BYE].
    private func viewerRequestedStop(_ fd: Int32) -> Bool {
        var bytes = [UInt8](repeating: 0, count: 256)
        while true {
            let count = bytes.withUnsafeMutableBytes { raw in
                recv(fd, raw.baseAddress, raw.count, MSG_DONTWAIT)
            }
            guard count > 0 else { break }
            viewerControlBuffer.append(contentsOf: bytes[0..<count])
        }

        while viewerControlBuffer.count >= 4 {
            let length = viewerControlBuffer.prefix(4).reduce(UInt32(0)) { value, byte in
                (value << 8) | UInt32(byte)
            }
            guard length <= 64 * 1024 else {
                viewerControlBuffer.removeAll(keepingCapacity: true)
                return false
            }
            let framedLength = 4 + Int(length)
            guard viewerControlBuffer.count >= framedLength else { break }
            let payload = Data(viewerControlBuffer[4..<framedLength])
            viewerControlBuffer.removeFirst(framedLength)
            if payload == Data("BYE".utf8) {
                print("viewer close signal received for \(targetLabel)")
                requestViewerStop()
                return true
            }
        }
        return false
    }

    /// Send one framed packet: [u32 BE length][payload] over the TCP stream.
    /// On send failure, keep the session alive and reconnect in the background.
    private func writePacket(_ data: Data) {
        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else { return }
        if viewerRequestedStop(fd) { return }
        var framed = Data()
        var lenBE = UInt32(data.count).bigEndian
        withUnsafeBytes(of: &lenBE) { framed.append(contentsOf: $0) }
        framed.append(data)
        let sendStart = DispatchTime.now().uptimeNanoseconds
        let ok = framed.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
            var off = 0
            while off < raw.count {
                let n = send(fd, raw.baseAddress!.advanced(by: off), raw.count - off, 0)
                if n <= 0 { return false }
                off += n
            }
            return true
        }
        if !ok {
            scheduleReconnect()
            return
        }
        let sendUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
        stateLock.lock()
        bytesSent &+= Int64(data.count)
        rateWindowBytes &+= Int64(data.count)
        lastSendBlockUs = sendUs
        maxSendBlockUs = max(maxSendBlockUs, sendUs)
        stateLock.unlock()
    }

    // MARK: Stats

    func statsJSON() -> String {
        stateLock.lock()
        // roll the 1s rate window
        let now = Date()
        let elapsed = now.timeIntervalSince(rateWindowStart)
        if elapsed >= 1.0 {
            lastFps = UInt32((Double(rateWindowFrames) / elapsed).rounded())
            lastKbps = UInt32((Double(rateWindowBytes) * 8.0 / 1000.0 / elapsed).rounded())
            rateWindowStart = now
            rateWindowFrames = 0
            rateWindowBytes = 0
        }

        let state = running ? "running" : (stoppedReason.isEmpty ? "stopped" : "error")
        let captureToEncodeUs = lastCaptureToEncodeUs
        let maxCaptureToEncodeUs = maxCaptureToEncodeUs
        let captureQueueWaitUs = lastCaptureQueueWaitUs
        let maxCaptureQueueWaitUs = maxCaptureQueueWaitUs
        let encodeOutputUs = lastEncodeOutputUs
        let maxEncodeOutputUs = maxEncodeOutputUs
        let sendBlockUs = lastSendBlockUs
        let maxSendBlockUs = maxSendBlockUs
        let networkDropped = framesDropped
        let captureQueueDropped = captureQueueDropped
        let framesDropped = networkDropped + captureQueueDropped
        let framesEncoded = framesEncoded
        let bytesSent = bytesSent
        let reportedFps = lastFps
        let reportedKbps = lastKbps
        let error = stoppedReason
        stateLock.unlock()

        networkLock.lock()
        let pending = pendingFrame != nil ? 1 : 0
        networkLock.unlock()

        let obj: [String: Any] = [
            "frames": framesEncoded,
            "dropped": framesDropped,
            "networkDropped": networkDropped,
            "captureQueueDropped": captureQueueDropped,
            "bytes": bytesSent,
            "state": state,
            "fps": reportedFps,
            "kbps": reportedKbps,
            "fpsTarget": self.fps,
            "captureToEncodeUs": captureToEncodeUs,
            "maxCaptureToEncodeUs": maxCaptureToEncodeUs,
            "captureQueueWaitUs": captureQueueWaitUs,
            "maxCaptureQueueWaitUs": maxCaptureQueueWaitUs,
            "encodeOutputUs": encodeOutputUs,
            "maxEncodeOutputUs": maxEncodeOutputUs,
            "sendBlockUs": sendBlockUs,
            "maxSendBlockUs": maxSendBlockUs,
            "pendingFrame": pending,
            "error": error,
        ]
        if let data = try? JSONSerialization.data(withJSONObject: obj),
           let s = String(data: data, encoding: .utf8) {
            return s
        }
        return "{\"state\":\"\(state)\"}"
    }
}
