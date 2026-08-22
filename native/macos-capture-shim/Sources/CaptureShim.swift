// CaptureShim: real macOS screen capture -> H.264 -> low-latency UDP, as a C-ABI dylib.
//
// Path (docs/02 §4, H16-H18; rebuild design 2026-08-18):
//   SCShareableContent (selected display) -> SCStream (420v IOSurface frames)
//   -> VTCompressionSession (H.264 Main, realtime, no B-frames)
//   -> MTU-bounded UDP datagrams (CFG + fragmented H.264 access units).
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
import IOSurface
import Security
import Darwin

// MARK: - C ABI surface (v2, handle-based)

private let registryLock = NSLock()
private var registry: [UInt32: CaptureSession] = [:]
private var nextHandle: UInt32 = 1

private enum CaptureBackendKind: String {
    case screenCaptureKit
    case cgDisplayStream

    static func parse(_ value: String?) -> CaptureBackendKind? {
        guard let value else { return .screenCaptureKit }
        switch value.lowercased() {
        case "sck", "screencapturekit": return .screenCaptureKit
        case "cg", "cgdisplaystream": return .cgDisplayStream
        default: return nil
        }
    }
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

private func activeDisplayIDs() -> [CGDirectDisplayID] {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else {
        return []
    }
    var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(count))
    var filled = count
    guard CGGetActiveDisplayList(count, &displayIDs, &filled) == .success else {
        return []
    }
    let mainDisplayID = CGMainDisplayID()
    return displayIDs.prefix(Int(filled)).sorted { lhs, rhs in
        if lhs == mainDisplayID { return true }
        if rhs == mainDisplayID { return false }
        return lhs < rhs
    }
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

private let persistentContentCaptureEntitlement =
    "com.apple.developer.persistent-content-capture" as CFString

/// Apple's persistent-content-capture entitlement is restricted to approved
/// remote-desktop/VNC apps. Read the entitlement from the running task instead
/// of trusting a build flag: an unsigned development binary must never claim
/// that it can bypass the system picker.
private func hasPersistentContentCaptureEntitlement() -> Bool {
    guard let task = SecTaskCreateFromSelf(nil),
          let value = SecTaskCopyValueForEntitlement(
              task,
              persistentContentCaptureEntitlement,
              nil
          ) else {
        return false
    }
    return CFGetTypeID(value) == CFBooleanGetTypeID()
        && CFBooleanGetValue((value as! CFBoolean))
}

private final class PersistentDisplayFilterRequest: @unchecked Sendable {
    private let lock = NSLock()
    private let completed = DispatchSemaphore(value: 0)
    private var selectedFilter: SCContentFilter?
    private var failure: String?

    func finish(filter: SCContentFilter? = nil, error: String? = nil) {
        lock.lock()
        selectedFilter = filter
        failure = error
        lock.unlock()
        completed.signal()
    }

    func wait(timeout: TimeInterval) -> (filter: SCContentFilter?, error: String?) {
        guard completed.wait(timeout: .now() + timeout) == .success else {
            return (nil, "persistent display lookup timed out after \(Int(timeout))s")
        }
        lock.lock()
        defer { lock.unlock() }
        return (selectedFilter, failure)
    }
}

private func requestPersistentDisplayFilter(
    displayID: CGDirectDisplayID,
    timeout: TimeInterval
) -> (filter: SCContentFilter?, error: String?) {
    let request = PersistentDisplayFilterRequest()

    Task {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(
                false,
                onScreenWindowsOnly: false
            )
            if let display = content.displays.first(where: { $0.displayID == displayID }) {
                request.finish(
                    filter: SCContentFilter(display: display, excludingWindows: [])
                )
            } else {
                request.finish(error: "persistent display lookup returned no matching display")
            }
        } catch {
            request.finish(error: "persistent display lookup failed: \(error.localizedDescription)")
        }
    }
    return request.wait(timeout: timeout)
}

@_cdecl("leftcar_capture_has_persistent_access_v1")
public func leftcarCaptureHasPersistentAccessV1() -> Int32 {
    hasPersistentContentCaptureEntitlement() ? 1 : 0
}

@_cdecl("leftcar_capture_list_displays")
public func leftcarCaptureListDisplays() -> UnsafeMutablePointer<CChar> {
    guard hasScreenCaptureAccess() else {
        setLastError("screen-recording permission is not granted to Leftcar Host")
        return UnsafeMutablePointer<CChar>(strdup("[]"))
    }
    // Source cards only need stable display metadata. Using CoreGraphics here
    // keeps catalog refresh read-only; ScreenCaptureKit consent is requested
    // exactly once, when the viewer starts a stream.
    if let catalog = coreGraphicsCatalogJSON() {
        setLastError("")
        return UnsafeMutablePointer<CChar>(strdup(catalog))
    }
    setLastError("CoreGraphics returned no active displays")
    return UnsafeMutablePointer<CChar>(strdup("[]"))
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
    startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: .screenCaptureKit
    )
}

@_cdecl("leftcar_capture_start_v3")
public func leftcarCaptureStartV3(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backendName: UnsafePointer<CChar>?
) -> UInt32 {
    let rawBackend = backendName.flatMap { String(validatingUTF8: $0) }
    guard let backend = CaptureBackendKind.parse(rawBackend) else {
        setLastError("unknown capture backend: \(rawBackend ?? "null")")
        return 0
    }
    return startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: backend
    )
}

private func startCaptureSession(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backend: CaptureBackendKind
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
        fps: fps,
        backend: backend
    )

    // Establish the media socket first. Capture callbacks can then be accepted
    // immediately without losing the initial CFG/IDR while the viewer listener
    // is still racing to bind its port.
    let connected = session.connectSocket()
    guard connected else { return 0 }

    let started: Bool
    switch backend {
    case .screenCaptureKit:
        guard hasPersistentContentCaptureEntitlement() else {
            setLastError(
                "persistent ScreenCaptureKit access is not approved; use the automatic cgDisplayStream backend"
            )
            session.stop()
            return 0
        }
        let displayIDs = activeDisplayIDs()
        guard Int(displayIndex) < displayIDs.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displayIDs.count) displays)")
            session.stop()
            return 0
        }
        // Approved VNC-style builds reconnect directly to the requested
        // display after Screen Recording permission has been granted. Builds
        // without approval never show a picker; the Host advertises the
        // automatic CGDisplayStream backend instead.
        let selection = requestPersistentDisplayFilter(
            displayID: displayIDs[Int(displayIndex)],
            timeout: 15
        )
        guard let filter = selection.filter else {
            setLastError(selection.error ?? "screen capture returned no display")
            session.stop()
            return 0
        }
        started = session.setupScreenCaptureKit(filter: filter)
    case .cgDisplayStream:
        let displayIDs = activeDisplayIDs()
        guard Int(displayIndex) < displayIDs.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displayIDs.count) displays)")
            session.stop()
            return 0
        }
        started = session.setupCGDisplayStream(displayID: displayIDs[Int(displayIndex)])
    }
    guard started else {
        session.stop()
        return 0
    }

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

@_cdecl("leftcar_capture_input_permission_v1")
public func leftcarCaptureInputPermissionV1() -> Int32 {
    CGPreflightPostEventAccess() ? 1 : 0
}

@_cdecl("leftcar_capture_request_input_permission_v1")
public func leftcarCaptureRequestInputPermissionV1() -> Int32 {
    CGRequestPostEventAccess() ? 1 : 0
}

@_cdecl("leftcar_capture_set_input_enabled_v1")
public func leftcarCaptureSetInputEnabledV1(handle: UInt32, enabled: Int32) -> Int32 {
    let session = withRegistry { $0[handle] }
    guard let session else {
        setLastError("input session handle not found: \(handle)")
        return -1
    }
    if !session.setInputEnabled(enabled != 0) {
        setLastError("Accessibility input permission is not granted to Leftcar Host")
        return -2
    }
    setLastError("")
    return 0
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

private typealias CGFrameHandler = @convention(block) (
    CGDisplayStreamFrameStatus,
    UInt64,
    IOSurfaceRef?,
    CGDisplayStreamUpdate?
) -> Void
private typealias CGStreamCreateFn = @convention(c) (
    CGDirectDisplayID,
    Int,
    Int,
    Int32,
    CFDictionary?,
    DispatchQueue,
    CGFrameHandler
) -> Unmanaged<CGDisplayStream>?
private typealias CGStreamStartFn = @convention(c) (CGDisplayStream) -> CGError
private typealias CGStreamStopFn = @convention(c) (CGDisplayStream) -> CGError

/// `CGDisplayStream` was obsoleted by the macOS 15 SDK. Keep it behind an
/// explicitly selected, display-only compatibility backend loaded at runtime;
/// ScreenCaptureKit remains the supported default and no unavailable API is
/// referenced directly by Swift.
private final class LegacyCGDisplayStreamAPI {
    let library: UnsafeMutableRawPointer
    let create: CGStreamCreateFn
    let start: CGStreamStartFn
    let stop: CGStreamStopFn
    let showCursorKey: NSString

    init?() {
        guard let library = dlopen(
            "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
            RTLD_NOW | RTLD_LOCAL
        ),
        let createSymbol = dlsym(library, "CGDisplayStreamCreateWithDispatchQueue"),
        let startSymbol = dlsym(library, "CGDisplayStreamStart"),
        let stopSymbol = dlsym(library, "CGDisplayStreamStop"),
        let showCursorSymbol = dlsym(library, "kCGDisplayStreamShowCursor"),
        let showCursorKey = showCursorSymbol
            .assumingMemoryBound(to: Optional<CFString>.self)
            .pointee else {
            return nil
        }
        self.library = library
        self.create = unsafeBitCast(createSymbol, to: CGStreamCreateFn.self)
        self.start = unsafeBitCast(startSymbol, to: CGStreamStartFn.self)
        self.stop = unsafeBitCast(stopSymbol, to: CGStreamStopFn.self)
        self.showCursorKey = unsafeBitCast(showCursorKey, to: NSString.self)
    }

    deinit {
        dlclose(library)
    }
}

private struct PendingCaptureFrame {
    let pixelBuffer: CVPixelBuffer
    let pts: CMTime
    let duration: CMTime
    let callbackNs: UInt64
}

private struct PendingEncodedFrame {
    let data: Data
    let isKeyframe: Bool
}

private func appendRollingSample(_ value: UInt64, to samples: inout [UInt64]) {
    samples.append(value)
    if samples.count > 300 {
        samples.removeFirst(samples.count - 300)
    }
}

private func percentile95(_ samples: [UInt64]) -> UInt64 {
    guard !samples.isEmpty else { return 0 }
    let sorted = samples.sorted()
    let index = min(sorted.count - 1, Int(ceil(Double(sorted.count) * 0.95)) - 1)
    return sorted[max(0, index)]
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
    private var latestCapture: PendingCaptureFrame?
    private var encodeScheduled = false
    private var sock: Int32 = -1
    private var viewerControlToken = Data()
    private let inputQueue = DispatchQueue(label: "leftcar.input", qos: .userInteractive)
    private let inputLock = NSLock()
    private var inputReadSource: DispatchSourceRead?
    private var inputEnabled = false
    private var inputBounds: CGRect?
    private var lastReliableInputSequence: UInt32 = 0
    private var lastPointerInputSequence: UInt32 = 0
    private var pressedKeys = Set<CGKeyCode>()
    private var pressedButtons = Set<CGMouseButton>()
    private var lastPointerPosition = CGPoint.zero
    private var horizontalScrollRemainder: Int32 = 0
    private var verticalScrollRemainder: Int32 = 0
    private var stream: SCStream?
    private var streamHandler: CaptureOutputHandler?
    private var cgStream: CGDisplayStream?
    private var cgStreamAPI: LegacyCGDisplayStreamAPI?
    private var session: VTCompressionSession?
    private let targetAddr: sockaddr_in
    private let targetPort: UInt16
    private let targetLabel: String
    private let outWidth: UInt32
    private let outHeight: UInt32
    private let fps: UInt32
    private let backend: CaptureBackendKind
    private var csdSent = false

    // The encoder callback must never wait behind network transmission. Keep at
    // most the newest encoded AU plus the latest config packet; an older AU
    // that has not reached the socket is intentionally dropped.
    private let networkQueue = DispatchQueue(label: "leftcar.network", qos: .userInteractive)
    private let networkLock = NSLock()
    private var pendingConfig: Data?
    // Remote control favors the newest screen state over preserving every
    // encoded frame. Keep only one unsent AU; if the socket cannot keep up,
    // discard the dependency chain and resume immediately from a fresh IDR.
    private let maxPendingNetworkFrames = 1
    private var pendingFrames: [PendingEncodedFrame] = []
    private var networkAwaitingKeyframe = false
    private var networkDrainScheduled = false
    private var reconnectScheduled = false

    private let stateLock = NSLock()
    private var running = false
    private var stopRequested = false
    private var lifecycleState = "connecting"
    private let createdNs = DispatchTime.now().uptimeNanoseconds
    private var firstCaptureNs: UInt64?
    private var firstEncodeNs: UInt64?
    private var firstSendNs: UInt64?
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
    private var lastCaptureCallbackNs: UInt64?
    private var captureIntervalSamplesUs: [UInt64] = []
    private var captureToEncodeSamplesUs: [UInt64] = []
    private var captureQueueWaitSamplesUs: [UInt64] = []
    private var encodeOutputSamplesUs: [UInt64] = []
    private var sendBlockSamplesUs: [UInt64] = []
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
    private var currentAverageBitrate = 0
    private var lastAdaptedDropped: Int64 = 0
    private var stableBitrateWindows = 0

    var isRunning: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return running
    }

    fileprivate init(
        targetAddr: sockaddr_in,
        targetPort: UInt16,
        targetLabel: String,
        width: UInt32,
        height: UInt32,
        fps: UInt32,
        backend: CaptureBackendKind
    ) {
        self.targetAddr = targetAddr
        self.targetPort = targetPort
        self.targetLabel = targetLabel
        self.outWidth = width
        self.outHeight = height
        self.fps = min(max(1, fps), 90)
        self.backend = backend
        encodeQueue.setSpecific(key: encodeQueueKey, value: ())
    }

    // MARK: Setup

    private func sendToViewer(_ data: Data, fd: Int32) -> Int {
        var addr = targetAddr
        return data.withUnsafeBytes { raw in
            guard let baseAddress = raw.baseAddress else { return -1 }
            return withUnsafePointer(to: &addr) { pointer in
                pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                    sendto(
                        fd,
                        baseAddress,
                        raw.count,
                        0,
                        socketAddress,
                        socklen_t(MemoryLayout<sockaddr_in>.size)
                    )
                }
            }
        }
    }

    func connectSocket(stopOnFailure: Bool = true) -> Bool {
        stopInputReceiver()
        stateLock.lock()
        let shouldStop = stopRequested
        stateLock.unlock()
        if shouldStop { return false }

        sock = socket(AF_INET, SOCK_DGRAM, 0)
        guard sock >= 0 else {
            let reason = "UDP socket() failed"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        // A generous kernel queue absorbs a short Wi-Fi scheduling pause, but
        // O_NONBLOCK ensures the interactive encoder never waits behind it.
        var sendBuffer: Int32 = 2 * 1024 * 1024
        setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &sendBuffer, socklen_t(MemoryLayout<Int32>.size))
        let originalFlags = fcntl(sock, F_GETFL, 0)
        if originalFlags >= 0 {
            _ = fcntl(sock, F_SETFL, originalFlags | O_NONBLOCK)
        }

        // A paired viewer may reach the control port through a Tailscale
        // subnet router even when both devices share Wi-Fi. Prove that the
        // physical media candidate owns its UDP port before any screen bytes
        // are captured or sent. The echoed nonce also authenticates reverse
        // IDR/BYE messages when their VPN source address differs.
        let token = Data(UUID().uuidString.utf8)
        var challenge = Data("LCH1".utf8)
        challenge.append(token)
        var challengeVerified = false
        for attempt in 0..<60 {
            if attempt % 4 == 0 {
                _ = sendToViewer(challenge, fd: sock)
            }
            var response = [UInt8](repeating: 0, count: 256)
            var source = sockaddr_in()
            var sourceLength = socklen_t(MemoryLayout<sockaddr_in>.size)
            let count = response.withUnsafeMutableBytes { raw in
                withUnsafeMutablePointer(to: &source) { pointer in
                    pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                        recvfrom(
                            sock,
                            raw.baseAddress,
                            raw.count,
                            MSG_DONTWAIT,
                            socketAddress,
                            &sourceLength
                        )
                    }
                }
            }
            if count == challenge.count,
               Data(response[0..<count]) == challenge {
                challengeVerified = true
                break
            }
            usleep(50_000)
        }
        guard challengeVerified else {
            close(sock)
            sock = -1
            let reason = "UDP reachability proof failed for \(targetLabel)"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        viewerControlToken = token
        inputLock.lock()
        lastReliableInputSequence = 0
        lastPointerInputSequence = 0
        inputLock.unlock()
        startInputReceiver(fd: sock)
        sendInputStatus(fd: sock)

        stateLock.lock()
        if stopRequested {
            stateLock.unlock()
            close(sock)
            sock = -1
            return false
        }
        lifecycleState = firstSendNs == nil ? "starting_capture" : "running"
        stateLock.unlock()
        return true
    }

    /// Remote input is opt-in per stream. Reliable packets continue to be
    /// acknowledged while disabled so a viewer cannot build an unbounded
    /// retry queue before the host grants control.
    func setInputEnabled(_ enabled: Bool) -> Bool {
        if enabled && !CGPreflightPostEventAccess() {
            return false
        }
        inputLock.lock()
        let changed = inputEnabled != enabled
        inputEnabled = enabled
        inputLock.unlock()
        if changed && !enabled {
            inputQueue.async { [weak self] in
                self?.releaseInjectedInput()
            }
        }
        if changed, sock >= 0 {
            inputQueue.async { [weak self] in
                guard let self, self.sock >= 0 else { return }
                self.sendInputStatus(fd: self.sock)
            }
        }
        return true
    }

    private func startInputReceiver(fd: Int32) {
        inputQueue.sync {
            guard inputReadSource == nil else { return }
            let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: inputQueue)
            source.setEventHandler { [weak self] in
                self?.consumeViewerControl(fd)
            }
            inputReadSource = source
            source.resume()
        }
    }

    private func stopInputReceiver() {
        inputQueue.sync {
            inputReadSource?.cancel()
            inputReadSource = nil
        }
    }

    static let tccDeniedHint = "screen-recording permission required (System Settings > Privacy & Security > Screen Recording)"

    private func beginCapture() -> Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        guard !stopRequested, sock >= 0 else { return false }
        running = true
        lifecycleState = "starting_capture"
        return true
    }

    private func captureDidStart() {
        stateLock.lock()
        if running, !stopRequested {
            lifecycleState = firstCaptureNs == nil ? "waiting_first_frame" : "running"
        }
        stateLock.unlock()
        armFirstFrameWatchdog()
    }

    private func armFirstFrameWatchdog() {
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
            let config = SCStreamConfiguration()
            config.width = Int(outWidth)
            config.height = Int(outHeight)
            config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
            // Feed VideoToolbox the native bi-planar 4:2:0 surface so the
            // capture path avoids a BGRA -> YUV conversion per frame.
            config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            config.showsCursor = true
            // The application keeps only its newest pending frame, so the
            // framework's minimum/default depth does not add a stale queue.
            config.queueDepth = 3

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
        let properties = NSDictionary(
            object: kCFBooleanTrue!,
            forKey: api.showCursorKey
        ) as CFDictionary
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

    func stop() {
        stopInputReceiver()
        inputQueue.async { [weak self] in self?.releaseInjectedInput() }
        stateLock.lock()
        stopRequested = true
        stateLock.unlock()
        networkLock.lock()
        pendingConfig = nil
        pendingFrames.removeAll(keepingCapacity: true)
        networkLock.unlock()
        captureLock.lock()
        pendingCapture = nil
        latestCapture = nil
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

        stopInputReceiver()
        inputQueue.async { [weak self] in self?.releaseInjectedInput() }
        if staleSocket >= 0 {
            close(staleSocket)
        }
        networkLock.lock()
        pendingConfig = nil
        pendingFrames.removeAll(keepingCapacity: true)
        networkAwaitingKeyframe = true
        networkLock.unlock()
        print("viewer disconnected; heartbeat reconnecting to \(targetLabel)")

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            let reconnected = self.connectSocket(stopOnFailure: false)
            if reconnected {
                print("viewer heartbeat reconnected to \(self.targetLabel)")
                self.captureLock.lock()
                let latest = self.latestCapture
                self.captureLock.unlock()
                if let latest {
                    let replay = PendingCaptureFrame(
                        pixelBuffer: latest.pixelBuffer,
                        pts: .invalid,
                        duration: CMTime(value: 1, timescale: CMTimeScale(self.fps)),
                        callbackNs: DispatchTime.now().uptimeNanoseconds
                    )
                    self.encodeQueue.async { [weak self] in
                        self?.encodeFrame(replay)
                    }
                }
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
        lifecycleState = "error"
        let staleSocket = sock
        sock = -1
        stateLock.unlock()

        if staleSocket >= 0 {
            close(staleSocket)
        }
        networkLock.lock()
        pendingConfig = nil
        pendingFrames.removeAll(keepingCapacity: true)
        networkLock.unlock()

        queue.async { [weak self] in
            self?.stop()
        }
    }

    // MARK: Frame Processing & VideoToolbox Encoding

    func handleFrame(_ sample: CMSampleBuffer) {
        guard CMSampleBufferIsValid(sample), CMSampleBufferDataIsReady(sample),
              let pixelBuffer = CMSampleBufferGetImageBuffer(sample) else {
            return
        }
        let inputPts = CMSampleBufferGetPresentationTimeStamp(sample)
        let inputDuration = CMSampleBufferGetDuration(sample)
        handlePixelBuffer(
            pixelBuffer,
            pts: inputPts,
            duration: inputDuration
        )
    }

    func handlePixelBuffer(_ pixelBuffer: CVPixelBuffer, pts: CMTime, duration: CMTime) {
        let callbackNs = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        let stillRunning = running
        if stillRunning, firstCaptureNs == nil {
            firstCaptureNs = callbackNs
            lifecycleState = "encoding_first_frame"
        }
        if let previous = lastCaptureCallbackNs, callbackNs >= previous {
            appendRollingSample((callbackNs - previous) / 1_000, to: &captureIntervalSamplesUs)
        }
        lastCaptureCallbackNs = callbackNs
        stateLock.unlock()
        guard stillRunning else { return }

        let frame = PendingCaptureFrame(
            pixelBuffer: pixelBuffer,
            pts: pts,
            duration: duration,
            callbackNs: callbackNs
        )
        captureLock.lock()
        let replaced = pendingCapture != nil
        pendingCapture = frame
        latestCapture = frame
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

        let pb = captured.pixelBuffer
        let encodeStartNs = DispatchTime.now().uptimeNanoseconds
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else { return }

        let inputPts = captured.pts

        let pts = inputPts.timescale > 0
            ? inputPts
            : CMTime(value: CMTimeValue(submittedFrames), timescale: CMTimeScale(fps))
        let inputDuration = captured.duration
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
        appendRollingSample(queueWaitUs, to: &captureQueueWaitSamplesUs)
        if captureNsByPts.count > 256 {
            captureNsByPts.removeValue(forKey: captureNsByPts.keys.first!)
        }
        if encodeSubmitNsByPts.count > 256 {
            encodeSubmitNsByPts.removeValue(forKey: encodeSubmitNsByPts.keys.first!)
        }
        if encodeAuIdByPts.count > 256 {
            encodeAuIdByPts.removeValue(forKey: encodeAuIdByPts.keys.first!)
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

        let trackedPts = pts.value
        let status = VTCompressionSessionEncodeFrame(
            s,
            imageBuffer: pb,
            presentationTimeStamp: pts,
            duration: duration,
            frameProperties: frameProperties,
            infoFlagsOut: &flags
        ) { [weak self] status, _, encodedSample in
            guard status == noErr, let encodedSample = encodedSample else {
                self?.discardTrackedFrame(pts: trackedPts)
                self?.requestRecoveryKeyframe()
                return
            }
            self?.handleEncoded(encodedSample)
        }

        if status == noErr {
            stateLock.lock()
            framesEncoded &+= 1
            rateWindowFrames &+= 1
            let shouldAdaptBitrate = framesEncoded % Int64(max(1, fps)) == 0
            stateLock.unlock()
            if shouldAdaptBitrate {
                adaptBitrateIfNeeded()
            }
        } else {
            stateLock.lock()
            captureNsByPts.removeValue(forKey: pts.value)
            encodeSubmitNsByPts.removeValue(forKey: pts.value)
            encodeAuIdByPts.removeValue(forKey: pts.value)
            stateLock.unlock()
            requestRecoveryKeyframe()
        }
    }

    private func requestRecoveryKeyframe() {
        stateLock.lock()
        forceKeyframe = true
        csdSent = false
        stateLock.unlock()
    }

    private func discardTrackedFrame(pts: Int64) {
        stateLock.lock()
        captureNsByPts.removeValue(forKey: pts)
        encodeSubmitNsByPts.removeValue(forKey: pts)
        encodeAuIdByPts.removeValue(forKey: pts)
        stateLock.unlock()
    }

    private func adaptBitrateIfNeeded() {
        guard let session else { return }
        stateLock.lock()
        let totalDropped = framesDropped + captureQueueDropped
        let newDrops = totalDropped - lastAdaptedDropped
        lastAdaptedDropped = totalDropped
        let congested = newDrops > 0 || lastSendBlockUs > 8_000
        let current = currentAverageBitrate
        if congested {
            stableBitrateWindows = 0
        } else {
            stableBitrateWindows += 1
        }
        let canRaise = stableBitrateWindows >= 5
        if canRaise {
            stableBitrateWindows = 0
        }
        stateLock.unlock()

        guard current > 0 else { return }
        let pixelsPerSecond = Double(outWidth) * Double(outHeight) * Double(fps)
        let floorBitrate = Int(min(max(pixelsPerSecond * 0.05, 8_000_000), 24_000_000))
        // Apple recommends roughly 75 Mbps for one 4K High Performance
        // screen. Keep enough Wi-Fi headroom for UDP/IP overhead while still
        // allowing a 4K60 desktop to reach that quality range on a fast LAN.
        let ceilingBitrate = Int(min(max(pixelsPerSecond * 0.16, 24_000_000), 72_000_000))
        let target: Int
        if congested {
            target = max(floorBitrate, Int(Double(current) * 0.85))
        } else if canRaise {
            target = min(ceilingBitrate, Int(Double(current) * 1.05))
        } else {
            return
        }
        guard target != current else { return }
        let status = VTSessionSetProperty(
            session,
            key: kVTCompressionPropertyKey_AverageBitRate,
            value: target as CFNumber
        )
        guard status == noErr else { return }
        let hardLimitBytes = max(1, Int(Double(target) / 8.0 * 1.25))
        _ = VTSessionSetProperty(
            session,
            key: kVTCompressionPropertyKey_DataRateLimits,
            value: [hardLimitBytes, 1] as CFArray
        )
        stateLock.lock()
        currentAverageBitrate = target
        stateLock.unlock()
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
        // 4K60 starts at roughly 60 Mbps. Static desktop regions remain much
        // smaller because AverageBitRate is a VBR target, not forced padding.
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.12
        let avgBitrate = min(max(idealBits, 12_000_000), 72_000_000)

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

        let realTimeStatus = VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_RealTime,
            value: true as CFBoolean
        )
        let mainProfileStatus = VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_ProfileLevel,
            value: kVTProfileLevel_H264_Main_AutoLevel
        )
        if mainProfileStatus != noErr {
            _ = VTSessionSetProperty(
                s,
                key: kVTCompressionPropertyKey_ProfileLevel,
                value: kVTProfileLevel_H264_Baseline_AutoLevel
            )
        }
        let noReorderStatus = VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_AllowFrameReordering,
            value: false as CFBoolean
        )
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
        let hardLimitBytes = max(1, Int(avgBitrate / 8.0 * 1.25))
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_DataRateLimits, value: [hardLimitBytes, 1] as CFArray)
        // A 0.5s GOP caused a visible periodic keyframe burst. TCP is
        // reliable, so use a roughly 1s nominal GOP while retaining bounded
        // decoder recovery after a reconnect.
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: max(1, fps) as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: Int32(fps) as CFNumber)

        let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(s)
        guard realTimeStatus == noErr, noReorderStatus == noErr, prepareStatus == noErr else {
            VTCompressionSessionInvalidate(s)
            markStopped(
                "VideoToolbox low-latency setup failed: realtime=\(realTimeStatus) noReorder=\(noReorderStatus) prepare=\(prepareStatus)"
            )
            return
        }
        stateLock.lock()
        currentAverageBitrate = Int(avgBitrate)
        stateLock.unlock()
        session = s
    }

    // MARK: Packetization & TCP Transmission

    private func handleEncoded(_ sample: CMSampleBuffer) {
        guard isRunning else { return }
        let encodeNs = DispatchTime.now().uptimeNanoseconds
        let encodedPts = CMSampleBufferGetPresentationTimeStamp(sample).value
        stateLock.lock()
        if firstEncodeNs == nil {
            firstEncodeNs = encodeNs
            lifecycleState = "waiting_first_send"
        }
        let auId = encodeAuIdByPts.removeValue(forKey: encodedPts)
        if let captureNs = captureNsByPts.removeValue(forKey: encodedPts) {
            let elapsedUs = (encodeNs &- captureNs) / 1_000
            lastCaptureToEncodeUs = elapsedUs
            maxCaptureToEncodeUs = max(maxCaptureToEncodeUs, elapsedUs)
            appendRollingSample(elapsedUs, to: &captureToEncodeSamplesUs)
        }
        if let submitNs = encodeSubmitNsByPts.removeValue(forKey: encodedPts) {
            let elapsedUs = (encodeNs &- submitNs) / 1_000
            lastEncodeOutputUs = elapsedUs
            maxEncodeOutputUs = max(maxEncodeOutputUs, elapsedUs)
            appendRollingSample(elapsedUs, to: &encodeOutputSamplesUs)
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

        // The network writer splits this logical G access unit into 1,200-byte
        // UDP datagrams. Its base envelope carries the AU id and timestamp;
        // each emitted fragment receives its own index/count fields.
        // Logical G header: G, AU id (LE), "LT", host wall ms.
        var p2 = Data([0x47, UInt8(auId & 0xFF), UInt8(auId >> 8), 0x4C, 0x54])
        var hostWallMs = UInt64(Date().timeIntervalSince1970 * 1000.0).bigEndian
        withUnsafeBytes(of: &hostWallMs) { p2.append(contentsOf: $0) }
        p2.append(pkt.dropFirst(10))
        enqueuePacket(frame: p2, isKeyframe: isKeyframe)
    }

    private func enqueuePacket(
        config: Data? = nil,
        frame: Data? = nil,
        isKeyframe: Bool = false
    ) {
        networkLock.lock()
        if let config {
            pendingConfig = config
        }
        if let frame {
            if networkAwaitingKeyframe {
                if isKeyframe {
                    pendingFrames.removeAll(keepingCapacity: true)
                    networkAwaitingKeyframe = false
                    pendingFrames.append(
                        PendingEncodedFrame(data: frame, isKeyframe: true)
                    )
                } else {
                    stateLock.lock()
                    framesDropped &+= 1
                    forceKeyframe = true
                    stateLock.unlock()
                }
            } else if pendingFrames.count < maxPendingNetworkFrames {
                pendingFrames.append(
                    PendingEncodedFrame(data: frame, isKeyframe: isKeyframe)
                )
            } else {
                // The unsent frames are a dependency chain. Once the bounded
                // queue is full, sending a newer delta while skipping any of
                // them would create visible corruption. Discard the whole
                // unsent chain and recover on the next independently decodable
                // IDR instead.
                let discarded = pendingFrames.count + (isKeyframe ? 0 : 1)
                pendingFrames.removeAll(keepingCapacity: true)
                stateLock.lock()
                framesDropped &+= Int64(discarded)
                stateLock.unlock()
                if isKeyframe {
                    networkAwaitingKeyframe = false
                    pendingFrames.append(
                        PendingEncodedFrame(data: frame, isKeyframe: true)
                    )
                } else {
                    networkAwaitingKeyframe = true
                    requestRecoveryKeyframe()
                }
            }
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
            let frame = pendingFrames.isEmpty ? nil : pendingFrames.removeFirst()
            if config == nil && frame == nil {
                networkDrainScheduled = false
                networkLock.unlock()
                return
            }
            networkLock.unlock()

            // Config always precedes the next queued frame. The queue is
            // intentionally tiny and overflow is recovered by an IDR, so it
            // absorbs scheduler jitter without accumulating stale video.
            if let config {
                writePacket(config)
            }
            if let frame {
                writePacket(frame.data, isFrame: true)
            }
        }
    }

    /// Consume nonce-authenticated viewer-to-host datagrams on a dedicated
    /// queue. Pointer motion is latest-wins; buttons and keys are accepted in
    /// sequence and acknowledged so the viewer can retry without duplicating
    /// host events.
    private func consumeViewerControl(_ fd: Int32) {
        var bytes = [UInt8](repeating: 0, count: 512)
        let token = viewerControlToken
        while true {
            let count = bytes.withUnsafeMutableBytes { raw in
                recvfrom(fd, raw.baseAddress, raw.count, MSG_DONTWAIT, nil, nil)
            }
            guard count > 0 else { break }
            let payload = Data(bytes[0..<count])
            guard payload.count >= token.count,
                  payload.suffix(token.count) == token else {
                continue
            }
            let message = Data(payload.dropLast(token.count))
            if message == Data("BYE".utf8) {
                print("viewer close signal received for \(targetLabel)")
                requestViewerStop()
                return
            }
            if message == Data("IDR".utf8) {
                networkLock.lock()
                pendingFrames.removeAll(keepingCapacity: true)
                networkAwaitingKeyframe = true
                networkLock.unlock()
                requestRecoveryKeyframe()
                continue
            }
            if message.count == 16,
               message.prefix(4) == Data("LCP1".utf8) {
                sendLatencyProbeResponse(message, fd: fd)
                continue
            }
            handleInputMessage(message, fd: fd)
        }
    }

    private func readUInt16BE(_ data: Data, at offset: Int) -> UInt16 {
        (UInt16(data[offset]) << 8) | UInt16(data[offset + 1])
    }

    private func readUInt32BE(_ data: Data, at offset: Int) -> UInt32 {
        (UInt32(data[offset]) << 24)
            | (UInt32(data[offset + 1]) << 16)
            | (UInt32(data[offset + 2]) << 8)
            | UInt32(data[offset + 3])
    }

    private func readUInt64BE(_ data: Data, at offset: Int) -> UInt64 {
        var value: UInt64 = 0
        for byte in data[offset..<(offset + 8)] {
            value = (value << 8) | UInt64(byte)
        }
        return value
    }

    /// NTP-style authenticated probe. Echoing the Android send time together
    /// with Host receive/send wall times lets the viewer separate LAN RTT from
    /// Host-to-device media delivery without assuming synchronized clocks.
    private func sendLatencyProbeResponse(_ message: Data, fd: Int32) {
        let sequence = readUInt32BE(message, at: 4)
        let viewerSendMs = readUInt64BE(message, at: 8)
        let hostReceiveMs = UInt64(Date().timeIntervalSince1970 * 1000.0)
        var response = Data("LCP2".utf8)
        var sequenceBE = sequence.bigEndian
        var viewerSendBE = viewerSendMs.bigEndian
        var hostReceiveBE = hostReceiveMs.bigEndian
        withUnsafeBytes(of: &sequenceBE) { response.append(contentsOf: $0) }
        withUnsafeBytes(of: &viewerSendBE) { response.append(contentsOf: $0) }
        withUnsafeBytes(of: &hostReceiveBE) { response.append(contentsOf: $0) }
        var hostSendBE = UInt64(Date().timeIntervalSince1970 * 1000.0).bigEndian
        withUnsafeBytes(of: &hostSendBE) { response.append(contentsOf: $0) }
        response.append(viewerControlToken)
        _ = sendToViewer(response, fd: fd)
    }

    private func sendInputAck(sequence: UInt32, fd: Int32) {
        var ack = Data("LCA1".utf8)
        var sequenceBE = sequence.bigEndian
        withUnsafeBytes(of: &sequenceBE) { ack.append(contentsOf: $0) }
        inputLock.lock()
        let enabled = inputEnabled
        inputLock.unlock()
        ack.append(enabled ? 1 : 0)
        ack.append(viewerControlToken)
        _ = sendToViewer(ack, fd: fd)
    }

    /// Authenticated state packet for the viewer's lock indicator. It is sent
    /// after the UDP proof and whenever the per-session opt-in changes. ACKs
    /// carry the same bit so a later click also repairs a lost status packet.
    private func sendInputStatus(fd: Int32) {
        inputLock.lock()
        let enabled = inputEnabled
        inputLock.unlock()
        var status = Data("LCS1".utf8)
        status.append(enabled ? 1 : 0)
        status.append(viewerControlToken)
        _ = sendToViewer(status, fd: fd)
    }

    private func handleInputMessage(_ message: Data, fd: Int32) {
        guard message.count >= 10,
              message.prefix(4) == Data("LCI1".utf8) else {
            return
        }
        let sequence = readUInt32BE(message, at: 4)
        let kind = message[8]
        let reliable = message[9] & 1 == 1

        if !reliable {
            guard kind == 1, message.count == 18 else { return }
            inputLock.lock()
            let newer = Int32(bitPattern: sequence &- lastPointerInputSequence) > 0
            if newer {
                lastPointerInputSequence = sequence
            }
            let enabled = inputEnabled
            inputLock.unlock()
            if newer && enabled {
                injectPointerMove(message)
            }
            return
        }

        inputLock.lock()
        let last = lastReliableInputSequence
        let enabled = inputEnabled
        inputLock.unlock()
        if sequence == last {
            sendInputAck(sequence: sequence, fd: fd)
            return
        }
        // Release-all is the fail-safe resynchronization packet. It may skip
        // a lost reliable transition, but an older delayed release must never
        // cancel newer input.
        if kind == 5 {
            guard message.count == 10,
                  Int32(bitPattern: sequence &- last) > 0 else {
                return
            }
            if enabled { releaseInjectedInput() }
            inputLock.lock()
            lastReliableInputSequence = sequence
            inputLock.unlock()
            sendInputAck(sequence: sequence, fd: fd)
            return
        }
        guard sequence == last &+ 1,
              validateAndInjectReliableInput(message, kind: kind, enabled: enabled) else {
            return
        }
        inputLock.lock()
        lastReliableInputSequence = sequence
        inputLock.unlock()
        sendInputAck(sequence: sequence, fd: fd)
    }

    private func validateAndInjectReliableInput(
        _ message: Data,
        kind: UInt8,
        enabled: Bool
    ) -> Bool {
        switch kind {
        case 2:
            guard message.count == 20 else { return false }
            if enabled { injectPointerButton(message) }
        case 3:
            guard message.count == 18 else { return false }
            if enabled { injectScroll(message) }
        case 4:
            guard message.count == 21 else { return false }
            if enabled { injectKey(message) }
        case 5:
            guard message.count == 10 else { return false }
            if enabled { releaseInjectedInput() }
        default:
            return false
        }
        return true
    }

    private func pointerPosition(x: UInt16, y: UInt16) -> CGPoint? {
        inputLock.lock()
        let bounds = inputBounds
        inputLock.unlock()
        guard let bounds else { return nil }
        let px = bounds.origin.x + CGFloat(x) / CGFloat(UInt16.max) * bounds.width
        let py = bounds.origin.y + CGFloat(y) / CGFloat(UInt16.max) * bounds.height
        return CGPoint(x: px, y: py)
    }

    private func injectPointerMove(_ message: Data) {
        guard let point = pointerPosition(
            x: readUInt16BE(message, at: 10),
            y: readUInt16BE(message, at: 12)
        ) else { return }
        let buttons = readUInt32BE(message, at: 14)
        let type: CGEventType
        let button: CGMouseButton
        if buttons & 1 != 0 {
            type = .leftMouseDragged
            button = .left
        } else if buttons & 2 != 0 {
            type = .rightMouseDragged
            button = .right
        } else if buttons & 4 != 0 {
            type = .otherMouseDragged
            button = .center
        } else {
            type = .mouseMoved
            button = .left
        }
        lastPointerPosition = point
        CGEvent(
            mouseEventSource: nil,
            mouseType: type,
            mouseCursorPosition: point,
            mouseButton: button
        )?.post(tap: .cghidEventTap)
    }

    private func mouseButton(mask: UInt8) -> CGMouseButton? {
        switch mask {
        case 1: return .left
        case 2: return .right
        case 4: return .center
        default: return nil
        }
    }

    private func injectPointerButton(_ message: Data) {
        guard let point = pointerPosition(
            x: readUInt16BE(message, at: 10),
            y: readUInt16BE(message, at: 12)
        ) else { return }
        guard let button = mouseButton(mask: message[14]) else { return }
        let down = message[15] != 0
        let type: CGEventType
        switch (button, down) {
        case (.left, true): type = .leftMouseDown
        case (.left, false): type = .leftMouseUp
        case (.right, true): type = .rightMouseDown
        case (.right, false): type = .rightMouseUp
        case (_, true): type = .otherMouseDown
        case (_, false): type = .otherMouseUp
        }
        lastPointerPosition = point
        if down {
            pressedButtons.insert(button)
        } else {
            pressedButtons.remove(button)
        }
        CGEvent(
            mouseEventSource: nil,
            mouseType: type,
            mouseCursorPosition: point,
            mouseButton: button
        )?.post(tap: .cghidEventTap)
    }

    private func injectScroll(_ message: Data) {
        horizontalScrollRemainder &+= Int32(bitPattern: readUInt32BE(message, at: 10))
        verticalScrollRemainder &+= Int32(bitPattern: readUInt32BE(message, at: 14))
        let horizontal = horizontalScrollRemainder / 1_000
        let vertical = verticalScrollRemainder / 1_000
        horizontalScrollRemainder %= 1_000
        verticalScrollRemainder %= 1_000
        guard horizontal != 0 || vertical != 0 else { return }
        CGEvent(
            scrollWheelEvent2Source: nil,
            units: .line,
            wheelCount: 2,
            wheel1: vertical,
            wheel2: horizontal,
            wheel3: 0
        )?.post(tap: .cghidEventTap)
    }

    private func keyboardFlags(metaState: UInt32) -> CGEventFlags {
        var flags: CGEventFlags = []
        if metaState & 0x0000_0001 != 0 { flags.insert(.maskShift) }
        if metaState & 0x0000_0002 != 0 { flags.insert(.maskAlternate) }
        if metaState & 0x0000_1000 != 0 { flags.insert(.maskControl) }
        if metaState & 0x0001_0000 != 0 { flags.insert(.maskCommand) }
        if metaState & 0x0010_0000 != 0 { flags.insert(.maskAlphaShift) }
        return flags
    }

    private func macKeyCode(android code: UInt16) -> CGKeyCode? {
        let letters: [CGKeyCode] = [
            0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46,
            45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6,
        ]
        if (29...54).contains(code) { return letters[Int(code - 29)] }
        let digits: [CGKeyCode] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25]
        if (7...16).contains(code) { return digits[Int(code - 7)] }
        let keypad: [CGKeyCode] = [82, 83, 84, 85, 86, 87, 88, 89, 91, 92]
        if (144...153).contains(code) { return keypad[Int(code - 144)] }
        let functionKeys: [CGKeyCode] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111]
        if (131...142).contains(code) { return functionKeys[Int(code - 131)] }
        return [
            19: 126, 20: 125, 21: 123, 22: 124,
            55: 43, 56: 47, 57: 58, 58: 61, 59: 56, 60: 60,
            61: 48, 62: 49, 66: 36, 67: 51, 68: 50, 69: 27,
            70: 24, 71: 33, 72: 30, 73: 42, 74: 41, 75: 39,
            76: 44, 92: 116, 93: 121, 111: 53, 112: 117,
            113: 59, 114: 62, 115: 57, 117: 55, 118: 54,
            122: 115, 123: 119, 124: 114,
            154: 75, 155: 67, 156: 78, 157: 69, 158: 65,
            160: 76, 161: 81, 204: 104,
        ][code]
    }

    private func injectKey(_ message: Data) {
        let androidCode = readUInt16BE(message, at: 10)
        guard let keyCode = macKeyCode(android: androidCode) else { return }
        let metaState = readUInt32BE(message, at: 14)
        let down = message[18] != 0
        let repeatCount = readUInt16BE(message, at: 19)
        guard let event = CGEvent(
            keyboardEventSource: nil,
            virtualKey: keyCode,
            keyDown: down
        ) else { return }
        event.flags = keyboardFlags(metaState: metaState)
        if repeatCount > 0 {
            event.setIntegerValueField(.keyboardEventAutorepeat, value: 1)
        }
        if down {
            pressedKeys.insert(keyCode)
        } else {
            pressedKeys.remove(keyCode)
        }
        event.post(tap: .cghidEventTap)
    }

    private func releaseInjectedInput() {
        for keyCode in pressedKeys {
            CGEvent(
                keyboardEventSource: nil,
                virtualKey: keyCode,
                keyDown: false
            )?.post(tap: .cghidEventTap)
        }
        pressedKeys.removeAll(keepingCapacity: true)
        for button in pressedButtons {
            let type: CGEventType = button == .left
                ? .leftMouseUp
                : (button == .right ? .rightMouseUp : .otherMouseUp)
            CGEvent(
                mouseEventSource: nil,
                mouseType: type,
                mouseCursorPosition: lastPointerPosition,
                mouseButton: button
            )?.post(tap: .cghidEventTap)
        }
        pressedButtons.removeAll(keepingCapacity: true)
        horizontalScrollRemainder = 0
        verticalScrollRemainder = 0
    }

    /// Send one config datagram or one fragmented H.264 AU. Datagram payloads
    /// stay below 1,200 bytes to avoid IP fragmentation on Wi-Fi and Tailscale.
    /// On a local queue overflow, recover from a fresh IDR instead of blocking
    /// subsequent video behind a lost packet.
    private func writePacket(_ data: Data, isFrame: Bool = false) {
        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else { return }

        let datagrams: [Data]
        if isFrame {
            // Logical G header: marker + AU id LE + LT + wall clock.
            guard data.count > 13, data[0] == 0x47, data[3...4] == Data([0x4C, 0x54]) else {
                requestRecoveryKeyframe()
                return
            }
            let payload = Data(data.dropFirst(13))
            let maxPayload = 1_183 // 1,200-byte datagram - 17-byte wire header
            let fragmentCount = max(1, (payload.count + maxPayload - 1) / maxPayload)
            guard fragmentCount <= Int(UInt16.max) else {
                requestRecoveryKeyframe()
                return
            }
            datagrams = (0..<fragmentCount).map { index in
                let start = index * maxPayload
                let end = min(payload.count, start + maxPayload)
                var datagram = Data([0x47])
                var indexBE = UInt16(index).bigEndian
                var countBE = UInt16(fragmentCount).bigEndian
                withUnsafeBytes(of: &indexBE) { datagram.append(contentsOf: $0) }
                withUnsafeBytes(of: &countBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: data[1...12])
                datagram.append(contentsOf: payload[start..<end])
                return datagram
            }
        } else {
            datagrams = [data]
        }

        let sendStart = DispatchTime.now().uptimeNanoseconds
        var sentBytes = 0
        let ok = datagrams.allSatisfy { datagram in
            let sent = sendToViewer(datagram, fd: fd)
            if sent == datagram.count {
                sentBytes += sent
                return true
            }
            return false
        }
        if !ok {
            stateLock.lock()
            framesDropped &+= isFrame ? 1 : 0
            stateLock.unlock()
            requestRecoveryKeyframe()
            return
        }
        let sendUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
        stateLock.lock()
        if isFrame, firstSendNs == nil {
            firstSendNs = DispatchTime.now().uptimeNanoseconds
            lifecycleState = "running"
        }
        bytesSent &+= Int64(sentBytes)
        rateWindowBytes &+= Int64(sentBytes)
        lastSendBlockUs = sendUs
        maxSendBlockUs = max(maxSendBlockUs, sendUs)
        appendRollingSample(sendUs, to: &sendBlockSamplesUs)
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

        let state = lifecycleState
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
        let firstCaptureMs = firstCaptureNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let firstEncodeMs = firstEncodeNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let firstSendMs = firstSendNs.map { ($0 &- createdNs) / 1_000_000 } ?? 0
        let currentBitrate = currentAverageBitrate
        let captureIntervalP95Us = percentile95(captureIntervalSamplesUs)
        let captureToEncodeP95Us = percentile95(captureToEncodeSamplesUs)
        let captureQueueWaitP95Us = percentile95(captureQueueWaitSamplesUs)
        let encodeOutputP95Us = percentile95(encodeOutputSamplesUs)
        let sendBlockP95Us = percentile95(sendBlockSamplesUs)
        stateLock.unlock()

        networkLock.lock()
        let pending = pendingFrames.count
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
            "captureBackend": backend.rawValue,
            "mediaTransport": "udp",
            "firstCaptureMs": firstCaptureMs,
            "firstEncodeMs": firstEncodeMs,
            "firstSendMs": firstSendMs,
            "currentBitrate": currentBitrate,
            "captureIntervalP95Us": captureIntervalP95Us,
            "captureToEncodeP95Us": captureToEncodeP95Us,
            "captureQueueWaitP95Us": captureQueueWaitP95Us,
            "encodeOutputP95Us": encodeOutputP95Us,
            "sendBlockP95Us": sendBlockP95Us,
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
