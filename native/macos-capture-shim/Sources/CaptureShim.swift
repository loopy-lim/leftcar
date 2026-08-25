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

private enum MediaTransportKind: String {
    case udp
    case tcp
    case adbTcp

    var usesTCP: Bool {
        self == .tcp || self == .adbTcp
    }

    static func parse(_ value: String?) -> MediaTransportKind? {
        guard let value else { return .udp }
        switch value.lowercased() {
        case "udp": return .udp
        case "tcp", "wifitcp", "wifi-tcp": return .tcp
        case "adbtcp", "adb-tcp", "usb": return .adbTcp
        default: return nil
        }
    }
}

private func nativePixelSize(for displayID: CGDirectDisplayID) -> (width: Int, height: Int) {
    if let mode = CGDisplayCopyDisplayMode(displayID),
       mode.pixelWidth > 0,
       mode.pixelHeight > 0 {
        return (mode.pixelWidth, mode.pixelHeight)
    }
    return (CGDisplayPixelsWide(displayID), CGDisplayPixelsHigh(displayID))
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
        let pixelSize = nativePixelSize(for: displayID)
        return [
            "index": index,
            "name": "Display \(index)",
            "width": pixelSize.width,
            "height": pixelSize.height,
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
        backend: backend,
        mediaTransport: .udp
    )
}

/// v4 adds an explicit media transport. TCP transports feed the existing
/// bounded UDP decoder path through the viewer's local bridge; `tcp` is the
/// reliable Wi-Fi path and `adbTcp` is the USB fallback.
@_cdecl("leftcar_capture_start_v4")
public func leftcarCaptureStartV4(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backendName: UnsafePointer<CChar>?,
    transportName: UnsafePointer<CChar>?
) -> UInt32 {
    let rawBackend = backendName.flatMap { String(validatingUTF8: $0) }
    guard let backend = CaptureBackendKind.parse(rawBackend) else {
        setLastError("unknown capture backend: \(rawBackend ?? "null")")
        return 0
    }
    let rawTransport = transportName.flatMap { String(validatingUTF8: $0) }
    guard let mediaTransport = MediaTransportKind.parse(rawTransport) else {
        setLastError("unknown media transport: \(rawTransport ?? "null")")
        return 0
    }
    return startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: backend,
        mediaTransport: mediaTransport
    )
}

private func startCaptureSession(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backend: CaptureBackendKind,
    mediaTransport: MediaTransportKind = .udp
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
        backend: backend,
        mediaTransport: mediaTransport
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

/// Stop with a viewer-visible reason. `reasonCode` uses the LCT1 wire codes:
/// 2 = host operator forced the stop, 3 = ordinary stop/shutdown. The viewer
/// closes its window on receipt instead of waiting for a media timeout.
@_cdecl("leftcar_capture_stop_v3")
public func leftcarCaptureStopV3(handle: UInt32, reasonCode: Int32) -> Int32 {
    let session = withRegistry { reg in
        reg.removeValue(forKey: handle)
    }
    guard let session else {
        setLastError("no such handle \(handle)")
        return 1
    }
    if reasonCode > 0 {
        session.notifyViewerTermination(
            code: UInt8(clamping: reasonCode),
            reason: reasonCode == 2 ? "host operator stopped the stream" : "stream stopped"
        )
        return 0
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
    let captureWallMs: UInt64
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
    private var encodeScheduled = false
    // VideoToolbox accepts frames asynchronously. A latest-frame slot alone
    // does not prevent its internal queue from growing, so keep at most two
    // hardware encode submissions in flight and retain only the newest frame
    // while both slots are occupied.
    private let maxEncodeInFlight = 2
    private var encodeInFlight = 0
    private var sock: Int32 = -1
    private let tcpWriteLock = NSLock()
    private var tcpControlBuffer = Data()
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
    private let mediaTransport: MediaTransportKind
    private var csdSent = false

    // The encoder callback must never wait behind network transmission. Keep at
    // most the newest encoded AU plus the latest config packet; an older AU
    // that has not reached the socket is intentionally dropped.
    private let networkQueue = DispatchQueue(label: "leftcar.network", qos: .userInteractive)
    private let networkLock = NSLock()
    private var pendingConfig: Data?
    // Both transports favor the newest screen state over preserving stale
    // encoded frames. TCP's ordered stream must not be allowed to become a
    // latency queue while a recovery AU is in flight; four slots are enough
    // to absorb normal encoder/network jitter without hiding congestion.
    private var maxPendingNetworkFrames: Int {
        mediaTransport.usesTCP ? 4 : 1
    }
    private var pendingFrames: [PendingEncodedFrame] = []
    private var networkAwaitingKeyframe = false
    private var networkKeyframeInFlight = false
    private var networkDrainScheduled = false

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
    private var networkQueueDropped: Int64 = 0
    // Frames discarded while waiting for the next independently decodable
    // IDR are expected recovery behavior, not evidence that the sender is
    // congested. Keep them in user-facing drop telemetry, but exclude them
    // from bitrate adaptation so recovery cannot ratchet the bitrate down.
    private var recoveryFramesDropped: Int64 = 0
    private var udpSendFailures: Int64 = 0
    private var udpSendRetries: Int64 = 0
    private var recoveryKeyframes: Int64 = 0
    private var recoveryRequestsSuppressed: Int64 = 0
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
    private var lastSendPaceUs: UInt64 = 0
    private var maxSendPaceUs: UInt64 = 0
    private var lastCaptureCallbackNs: UInt64?
    private var captureIntervalSamplesUs: [UInt64] = []
    private var captureToEncodeSamplesUs: [UInt64] = []
    private var captureQueueWaitSamplesUs: [UInt64] = []
    private var encodeOutputSamplesUs: [UInt64] = []
    private var sendBlockSamplesUs: [UInt64] = []
    private var sendPaceSamplesUs: [UInt64] = []
    private var stoppedReason = ""

    // Capture callback timestamps keyed by the real sample PTS. VideoToolbox
    // may call its output callback asynchronously, so this lets stats expose
    // capture -> encoded-output latency without putting a wait in the hot path.
    private var captureNsByPts: [Int64: UInt64] = [:]
    private var captureWallMsByPts: [Int64: UInt64] = [:]
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
    private var recoveryKeyframePending = false
    private var currentAverageBitrate = 0
    private var lastAdaptedDropped: Int64 = 0
    private var receiverFrameGaps: UInt32 = 0
    private var receiverInputDrops: UInt32 = 0
    private var receiverIncompleteAUs: UInt32 = 0
    private var receiverStaleFrames: UInt32 = 0
    private var receiverRttMs: UInt16 = .max
    private var receiverWireMs: UInt16 = .max
    private var receiverFeedbackNs: UInt64 = 0
    private var lastAdaptedReceiverLoss: UInt64 = 0
    private var stableBitrateWindows = 0
    // An IDR is an intentional intra-frame burst. Do not interpret its
    // bounded send cost or the receiver's recovery boundary as persistent
    // congestion and ratchet the stream bitrate downward.
    private var lastRecoverySendNs: UInt64 = 0
    // Transient Wi-Fi/power-save blips must not ratchet the bitrate to the
    // floor. Drop only when congestion persists across two windows; recover
    // with progressively larger steps so a floor exit takes seconds, not a
    // minute.
    private var consecutiveCongestedWindows = 0
    private var consecutiveRaiseSteps = 0
    private var healthCheckScheduled = false

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
        backend: CaptureBackendKind,
        mediaTransport: MediaTransportKind = .udp
    ) {
        self.targetAddr = targetAddr
        self.targetPort = targetPort
        self.targetLabel = targetLabel
        self.outWidth = width
        self.outHeight = height
        self.fps = min(max(1, fps), 90)
        self.backend = backend
        self.mediaTransport = mediaTransport
        encodeQueue.setSpecific(key: encodeQueueKey, value: ())
    }

    // MARK: Setup

    private func sendToViewer(_ data: Data, fd: Int32) -> Int {
        var addr = targetAddr
        return send(data, fd: fd, to: &addr)
    }

    private func sendTCPBytes(_ data: Data, fd: Int32) -> Int {
        data.withUnsafeBytes { raw in
            guard let baseAddress = raw.baseAddress else { return 0 }
            var offset = 0
            while offset < raw.count {
                let sent = Darwin.send(
                    fd,
                    baseAddress.advanced(by: offset),
                    raw.count - offset,
                    0
                )
                guard sent > 0 else { return offset }
                offset += sent
            }
            return offset
        }
    }

    /// TCP carries the existing CFG/G/control payloads as independent frames.
    /// The Android USB bridge converts them back to loopback UDP datagrams, so
    /// the decoder and its recovery telemetry stay identical on both paths.
    private func sendTCPFrame(_ data: Data, fd: Int32) -> Int {
        guard data.count <= 2 * 1024 * 1024 else { return -1 }
        var length = UInt32(data.count).bigEndian
        var framed = Data(capacity: data.count + 4)
        withUnsafeBytes(of: &length) { framed.append(contentsOf: $0) }
        framed.append(data)
        tcpWriteLock.lock()
        defer { tcpWriteLock.unlock() }
        return sendTCPBytes(framed, fd: fd) == framed.count ? data.count : -1
    }

    private func send(_ data: Data, fd: Int32, to addr: inout sockaddr_in) -> Int {
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

    /// A non-blocking UDP socket can transiently report EAGAIN/ENOBUFS during
    /// a Wi-Fi scheduling pause. This is an intentional loss-tolerant media
    /// path: never wait for the kernel to drain because the wait would hold
    /// every newer frame behind an already-lost datagram. The next IDR
    /// re-establishes the H.264 dependency chain.
    private func sendMediaDatagram(_ data: Data, fd: Int32) -> Int {
        if mediaTransport.usesTCP {
            let sent = sendTCPFrame(data, fd: fd)
            if sent == data.count { return sent }
            stateLock.lock()
            udpSendFailures &+= 1
            stateLock.unlock()
            return sent
        }
        let sent = sendToViewer(data, fd: fd)
        if sent == data.count { return sent }
        stateLock.lock()
        udpSendFailures &+= 1
        stateLock.unlock()
        return sent
    }

    private func paceNetwork(until deadlineNs: UInt64) {
        let now = DispatchTime.now().uptimeNanoseconds
        guard deadlineNs > now else { return }
        let remainingUs = (deadlineNs - now) / 1_000
        if remainingUs > 0 {
            usleep(useconds_t(min(remainingUs, UInt64(useconds_t.max))))
        }
    }

    private func receiveTCPFrame(fd: Int32, timeoutMs: Int32) -> Data? {
        func readExactly(_ length: Int) -> Data? {
            var result = Data(count: length)
            var offset = 0
            while offset < length {
                var descriptor = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
                guard poll(&descriptor, 1, timeoutMs) > 0 else { return nil }
                let received = result.withUnsafeMutableBytes { raw in
                    Darwin.recv(
                        fd,
                        raw.baseAddress!.advanced(by: offset),
                        length - offset,
                        MSG_DONTWAIT
                    )
                }
                guard received > 0 else { return nil }
                offset += received
            }
            return result
        }

        guard let header = readExactly(4) else { return nil }
        let headerBytes = Array(header)
        guard headerBytes.count == 4 else { return nil }
        let length = (UInt32(headerBytes[0]) << 24)
            | (UInt32(headerBytes[1]) << 16)
            | (UInt32(headerBytes[2]) << 8)
            | UInt32(headerBytes[3])
        guard length > 0, length <= 2 * 1024 * 1024 else { return nil }
        return readExactly(Int(length))
    }

    private func connectTCPSocket(stopOnFailure: Bool) -> Bool {
        sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else {
            let reason = "TCP socket() failed"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        var noDelay: Int32 = 1
        _ = setsockopt(sock, IPPROTO_TCP, TCP_NODELAY, &noDelay, socklen_t(MemoryLayout<Int32>.size))
        var receiveTimeout = timeval(tv_sec: 2, tv_usec: 0)
        _ = setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &receiveTimeout, socklen_t(MemoryLayout<timeval>.size))
        var sendTimeout = timeval(tv_sec: 0, tv_usec: 100_000)
        _ = setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &sendTimeout, socklen_t(MemoryLayout<timeval>.size))

        var address = targetAddr
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                Darwin.connect(
                    sock,
                    socketAddress,
                    socklen_t(MemoryLayout<sockaddr_in>.size)
                ) == 0
            }
        }
        guard connected else {
            let reason = "TCP media connection failed for \(targetLabel): errno=\(errno)"
            close(sock)
            sock = -1
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }

        let token = Data(UUID().uuidString.utf8)
        var challenge = Data("LCH1".utf8)
        challenge.append(token)
        guard sendTCPFrame(challenge, fd: sock) == challenge.count,
              let response = receiveTCPFrame(fd: sock, timeoutMs: 2_000),
              response == challenge else {
            let reason = "TCP media handshake failed for \(targetLabel)"
            close(sock)
            sock = -1
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }

        let originalFlags = fcntl(sock, F_GETFL, 0)
        if originalFlags >= 0 {
            _ = fcntl(sock, F_SETFL, originalFlags | O_NONBLOCK)
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

    func connectSocket(stopOnFailure: Bool = true) -> Bool {
        stopInputReceiver()
        stateLock.lock()
        let shouldStop = stopRequested
        stateLock.unlock()
        if shouldStop { return false }

        if mediaTransport.usesTCP {
            return connectTCPSocket(stopOnFailure: stopOnFailure)
        }

        sock = socket(AF_INET, SOCK_DGRAM, 0)
        guard sock >= 0 else {
            let reason = "UDP socket() failed"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        // Keep only a short kernel burst behind the app's latest-frame queue.
        // A multi-megabyte buffer can preserve stale video through Wi-Fi
        // scheduling pauses even though userspace keeps only one frame.
        var sendBuffer: Int32 = 512 * 1024
        setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &sendBuffer, socklen_t(MemoryLayout<Int32>.size))
        // AF41 is a best-effort Wi-Fi/WMM hint for interactive video.
        var videoTos: Int32 = 0x88
        _ = setsockopt(sock, IPPROTO_IP, IP_TOS, &videoTos, socklen_t(MemoryLayout<Int32>.size))
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
        var descriptor = pollfd(fd: sock, events: Int16(POLLIN), revents: 0)
        for attempt in 0..<60 {
            if attempt % 4 == 0 {
                _ = sendToViewer(challenge, fd: sock)
            }
            descriptor.revents = 0
            guard poll(&descriptor, 1, 50) > 0 else { continue }
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
        // Start the viewer watchdog even before the first LCF1 packet. A
        // viewer can disappear immediately after the UDP reachability proof;
        // waiting for feedback to arm this timer would leak that session.
        armReceiverHealthCheck()
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
            // Keep one extra framework buffer so a short encoder scheduling
            // pause does not make ScreenCaptureKit drop a source frame. The
            // application still coalesces pending work to the newest frame,
            // so this is a small capture cushion rather than a playback queue.
            config.queueDepth = 3
            config.backgroundColor = CGColor.black
            if #available(macOS 14.0, *) {
                config.shouldBeOpaque = true
            }

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

    /// The viewer sends authenticated LCF1 feedback once per second. When that
    /// stream goes silent while the session is running, the viewer is gone
    /// (network drop, app kill, device sleep) and the media socket will never
    /// report an error because it is unconnected UDP. Re-check after a grace
    /// period and terminate the session so capture/encode work stops instead
    /// of streaming into the void forever.
    private func armReceiverHealthCheck() {
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
        let captureWallMs = UInt64(Date().timeIntervalSince1970 * 1_000.0)
        stateLock.lock()
        let stillRunning = running
        if stillRunning, firstCaptureNs == nil {
            firstCaptureNs = callbackNs
            lifecycleState = "encoding_first_frame"
            NSLog(
                "Leftcar first capture frame %@: %dx%d",
                targetLabel,
                CVPixelBufferGetWidth(pixelBuffer),
                CVPixelBufferGetHeight(pixelBuffer)
            )
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
            callbackNs: callbackNs,
            captureWallMs: captureWallMs
        )
        captureLock.lock()
        let replaced = pendingCapture != nil
        pendingCapture = frame
        let shouldSchedule = !encodeScheduled && encodeInFlight < maxEncodeInFlight
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
            guard encodeInFlight < maxEncodeInFlight, let next = pendingCapture else {
                encodeScheduled = false
                captureLock.unlock()
                return
            }
            pendingCapture = nil
            encodeInFlight += 1
            captureLock.unlock()
            encodeFrame(next)
        }
    }

    private func completeEncodeSlot() {
        captureLock.lock()
        encodeInFlight = max(0, encodeInFlight - 1)
        let shouldSchedule = pendingCapture != nil && !encodeScheduled
        if shouldSchedule {
            encodeScheduled = true
        }
        captureLock.unlock()
        if shouldSchedule {
            encodeQueue.async { [weak self] in
                self?.drainEncodeQueue()
            }
        }
    }

    private func encodeFrame(_ captured: PendingCaptureFrame) {
        stateLock.lock()
        let stillRunning = running
        let submittedFrames = framesEncoded
        stateLock.unlock()
        guard stillRunning else {
            completeEncodeSlot()
            return
        }

        let pb = captured.pixelBuffer
        let encodeStartNs = DispatchTime.now().uptimeNanoseconds
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else {
            completeEncodeSlot()
            return
        }

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
        captureWallMsByPts[pts.value] = captured.captureWallMs
        encodeSubmitNsByPts[pts.value] = encodeStartNs
        encodeAuIdByPts[pts.value] = auId
        let queueWaitUs = (encodeStartNs &- captured.callbackNs) / 1_000
        lastCaptureQueueWaitUs = queueWaitUs
        maxCaptureQueueWaitUs = max(maxCaptureQueueWaitUs, queueWaitUs)
        appendRollingSample(queueWaitUs, to: &captureQueueWaitSamplesUs)
        if captureNsByPts.count > 256 {
            captureNsByPts.removeValue(forKey: captureNsByPts.keys.first!)
        }
        if captureWallMsByPts.count > 256 {
            captureWallMsByPts.removeValue(forKey: captureWallMsByPts.keys.first!)
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
            defer { self?.completeEncodeSlot() }
            guard status == noErr, let encodedSample = encodedSample else {
                NSLog(
                    "Leftcar H.264 output failed %@: status=%d",
                    self?.targetLabel ?? "unknown",
                    status
                )
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
            NSLog("Leftcar H.264 submit failed %@: status=%d", targetLabel, status)
            stateLock.lock()
            captureNsByPts.removeValue(forKey: pts.value)
            captureWallMsByPts.removeValue(forKey: pts.value)
            encodeSubmitNsByPts.removeValue(forKey: pts.value)
            encodeAuIdByPts.removeValue(forKey: pts.value)
            stateLock.unlock()
            requestRecoveryKeyframe()
            completeEncodeSlot()
        }
    }

    private var lastKeyframeRequestNs: UInt64 = 0

    private func requestRecoveryKeyframe() {
        stateLock.lock()
        let now = DispatchTime.now().uptimeNanoseconds
        // Keep one recovery request outstanding. If its keyframe never reaches
        // the viewer, retry after 750ms instead of creating a 5Hz IDR storm.
        let cooldownElapsed = now &- lastKeyframeRequestNs >= 750_000_000
        if (!recoveryKeyframePending || cooldownElapsed) && cooldownElapsed {
            lastKeyframeRequestNs = now
            recoveryKeyframePending = true
            forceKeyframe = true
            csdSent = false
            recoveryKeyframes &+= 1
        } else {
            recoveryRequestsSuppressed &+= 1
        }
        stateLock.unlock()
    }

    private func recoveryKeyframeDidSend() {
        stateLock.lock()
        recoveryKeyframePending = false
        stateLock.unlock()
    }

    private func discardTrackedFrame(pts: Int64) {
        stateLock.lock()
        captureNsByPts.removeValue(forKey: pts)
        captureWallMsByPts.removeValue(forKey: pts)
        encodeSubmitNsByPts.removeValue(forKey: pts)
        encodeAuIdByPts.removeValue(forKey: pts)
        stateLock.unlock()
    }

    private func adaptBitrateIfNeeded() {
        guard let session else { return }
        stateLock.lock()
        let congestionDrops = max(0, framesDropped - recoveryFramesDropped) + captureQueueDropped
        let newDrops = congestionDrops - lastAdaptedDropped
        lastAdaptedDropped = congestionDrops
        let nowNs = DispatchTime.now().uptimeNanoseconds
        let feedbackFresh = receiverFeedbackNs > 0
            && nowNs &- receiverFeedbackNs <= 3_000_000_000
        let recoveryBurstGrace = lastRecoverySendNs != 0
            && nowNs &- lastRecoverySendNs < 1_000_000_000
        let receiverLoss = UInt64(receiverFrameGaps)
            + UInt64(receiverInputDrops)
            + UInt64(receiverIncompleteAUs)
            + UInt64(receiverStaleFrames)
        let newReceiverLoss: UInt64
        if feedbackFresh {
            newReceiverLoss = receiverLoss >= lastAdaptedReceiverLoss
                ? receiverLoss - lastAdaptedReceiverLoss
                : receiverLoss
            lastAdaptedReceiverLoss = receiverLoss
        } else {
            newReceiverLoss = 0
        }
        let receiverLatencyHigh = feedbackFresh
            && !recoveryBurstGrace
            && ((receiverRttMs != .max && receiverRttMs >= 50)
                || (receiverWireMs != .max && receiverWireMs >= 40))
        let congested = newDrops > 0
            || (!recoveryBurstGrace && lastSendBlockUs > 8_000)
            || (!recoveryBurstGrace && newReceiverLoss > 0)
            || receiverLatencyHigh
        let current = currentAverageBitrate
        if congested {
            stableBitrateWindows = 0
            consecutiveCongestedWindows += 1
            consecutiveRaiseSteps = 0
        } else {
            stableBitrateWindows += 1
            if stableBitrateWindows >= 8 {
                consecutiveCongestedWindows = 0
            }
        }
        // Only treat sustained congestion (two consecutive windows) as real.
        // A single lost datagram or one RTT spike is normal Wi-Fi behavior
        // and must not cut the bitrate.
        let congestionConfirmed = consecutiveCongestedWindows >= 2
        let canRaise = !congested && stableBitrateWindows >= 8
        if canRaise {
            stableBitrateWindows = 0
            consecutiveRaiseSteps += 1
        }
        stateLock.unlock()

        guard current > 0 else { return }
        let activeCount = max(1, withRegistry { $0.count })
        let streamFactor = activeCount > 1 ? (1.0 / Double(activeCount) * 1.3) : 1.0
        let pixelsPerSecond = Double(outWidth) * Double(outHeight) * Double(fps) * streamFactor
        // A hard 8Mbps single-stream floor prevented the controller from
        // escaping congestion even while receiver loss kept rising. Text is
        // still readable at the 4-6Mbps recovery band; quality climbs again
        // only after eight stable windows.
        let minFloor = activeCount > 1 ? 3_000_000 : 4_000_000
        let maxFloor = activeCount > 1 ? 10_000_000 : 14_000_000
        let minCeiling = activeCount > 1 ? 8_000_000 : 24_000_000
        let maxCeiling = activeCount > 1 ? 28_000_000 : 60_000_000
        let floorBitrate = Int(min(max(pixelsPerSecond * 0.035, Double(minFloor)), Double(maxFloor)))
        let ceilingBitrate = Int(min(max(pixelsPerSecond * 0.14, Double(minCeiling)), Double(maxCeiling)))
        let target: Int
        if congestionConfirmed {
            target = max(floorBitrate, Int(Double(current) * 0.80))
        } else if canRaise {
            // Accelerating recovery: 4% → 8% → 16% per stable window so a
            // ratchet-down to the floor recovers in a few seconds while an
            // early overshoot is still corrected by the next cut.
            let raiseFactor = min(0.04 * pow(2.0, Double(min(consecutiveRaiseSteps - 1, 5))), 0.30)
            target = min(ceilingBitrate, Int(Double(current) * (1.0 + raiseFactor)))
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

        let activeCount = max(1, withRegistry { $0.count })
        let streamFactor = activeCount > 1 ? (1.0 / Double(activeCount) * 1.3) : 1.0
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.07 * streamFactor
        let minRate = activeCount > 1 ? 4_000_000 : 6_000_000
        let maxRate = activeCount > 1 ? 24_000_000 : 60_000_000
        let avgBitrate = min(max(idealBits, Double(minRate)), Double(maxRate))

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
        // UDP needs a short recovery boundary because a lost fragment can
        // invalidate every following delta. TCP is reliable, so avoid a
        // periodic IDR burst during normal playback; reconnect and explicit
        // recovery requests still force an immediate IDR through the
        // authenticated control channel.
        let nominalKeyframeInterval = mediaTransport.usesTCP
            ? max(1, fps * 60)
            : max(1, fps)
        VTSessionSetProperty(
            s,
            key: kVTCompressionPropertyKey_MaxKeyFrameInterval,
            value: nominalKeyframeInterval as CFNumber
        )
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
        NSLog(
            "Leftcar hardware H.264 encoder ready %@: %dx%d bitrate=%d",
            targetLabel,
            w,
            h,
            Int(avgBitrate)
        )
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
            NSLog("Leftcar first encoded frame %@", targetLabel)
        }
        let auId = encodeAuIdByPts.removeValue(forKey: encodedPts)
        let captureWallMs = captureWallMsByPts.removeValue(forKey: encodedPts)
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
        guard let auId, let captureWallMs else {
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
        // UDP datagrams. Its base envelope carries the AU id and stage times;
        // each emitted fragment receives its own index/count fields.
        // Logical L2 header: G, AU id (LE), "L2", capture wall ms,
        // encoded-output wall ms. The network writer adds send wall ms.
        var p2 = Data([0x47, UInt8(auId & 0xFF), UInt8(auId >> 8), 0x4C, 0x32])
        var captureWallMsBE = captureWallMs.bigEndian
        var encodeWallMsBE = UInt64(Date().timeIntervalSince1970 * 1_000.0).bigEndian
        withUnsafeBytes(of: &captureWallMsBE) { p2.append(contentsOf: $0) }
        withUnsafeBytes(of: &encodeWallMsBE) { p2.append(contentsOf: $0) }
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
                    networkQueueDropped &+= 1
                    recoveryFramesDropped &+= 1
                    stateLock.unlock()
                    requestRecoveryKeyframe()
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
                let keyframeInFlight = networkKeyframeInFlight
                pendingFrames.removeAll(keepingCapacity: true)
                stateLock.lock()
                framesDropped &+= Int64(discarded)
                networkQueueDropped &+= Int64(discarded)
                if keyframeInFlight || networkAwaitingKeyframe {
                    recoveryFramesDropped &+= Int64(discarded)
                }
                stateLock.unlock()
                if isKeyframe {
                    networkAwaitingKeyframe = false
                    pendingFrames.append(
                        PendingEncodedFrame(data: frame, isKeyframe: true)
                    )
                } else {
                    // A keyframe already being written is the recovery
                    // boundary. Dropping deltas behind that boundary is
                    // expected; asking for another IDR here would create a
                    // second burst before the first one has arrived.
                    if !keyframeInFlight {
                        networkAwaitingKeyframe = true
                        requestRecoveryKeyframe()
                    }
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
            if frame?.isKeyframe == true {
                networkKeyframeInFlight = true
            }
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
                writePacket(frame.data, isFrame: true, isKeyframe: frame.isKeyframe)
                networkLock.lock()
                networkKeyframeInFlight = false
                networkLock.unlock()
            }
        }
    }

    /// Consume nonce-authenticated viewer-to-host datagrams on a dedicated
    /// queue. Pointer motion is latest-wins; buttons and keys are accepted in
    /// sequence and acknowledged so the viewer can retry without duplicating
    /// host events.
    private func consumeViewerControl(_ fd: Int32) {
        if mediaTransport.usesTCP {
            consumeViewerTCPControl(fd)
            return
        }
        var bytes = [UInt8](repeating: 0, count: 512)
        let token = viewerControlToken
        while true {
            var source = sockaddr_in()
            var sourceLength = socklen_t(MemoryLayout<sockaddr_in>.size)
            let count = bytes.withUnsafeMutableBytes { raw in
                withUnsafeMutablePointer(to: &source) { pointer in
                    pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                        recvfrom(
                            fd,
                            raw.baseAddress,
                            raw.count,
                            MSG_DONTWAIT,
                            socketAddress,
                            &sourceLength
                        )
                    }
                }
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
                // A prepared Android listener can consume the initial status
                // before its Surface exists. IDR is the renderer handoff
                // signal, so refresh both codec state and input-lock state.
                sendInputStatus(fd: fd)
                continue
            }
            if message.count == 16,
               message.prefix(4) == Data("LCP1".utf8) {
                sendLatencyProbeResponse(message, fd: fd, destination: source)
                continue
            }
            if message.count == 24,
               message.prefix(4) == Data("LCF1".utf8) {
                stateLock.lock()
                receiverFrameGaps = readUInt32BE(message, at: 4)
                receiverInputDrops = readUInt32BE(message, at: 8)
                receiverIncompleteAUs = readUInt32BE(message, at: 12)
                receiverStaleFrames = readUInt32BE(message, at: 16)
                receiverRttMs = readUInt16BE(message, at: 20)
                receiverWireMs = readUInt16BE(message, at: 22)
                receiverFeedbackNs = DispatchTime.now().uptimeNanoseconds
                stateLock.unlock()
                armReceiverHealthCheck()
                continue
            }
            handleInputMessage(message, fd: fd, destination: source)
        }
    }

    private func consumeViewerTCPControl(_ fd: Int32) {
        var bytes = [UInt8](repeating: 0, count: 4096)
        while true {
            let count = bytes.withUnsafeMutableBytes { raw in
                Darwin.recv(fd, raw.baseAddress, raw.count, MSG_DONTWAIT)
            }
            if count == 0 {
                requestViewerStop()
                return
            }
            if count < 0 {
                if errno == EAGAIN || errno == EWOULDBLOCK { return }
                requestViewerStop()
                return
            }
            tcpControlBuffer.append(contentsOf: bytes[0..<count])
            while true {
                // Data can retain a non-zero startIndex after a partial
                // consume. Copying to Array gives this parser a stable,
                // zero-based view and removes every direct Data subscript
                // from the untrusted TCP framing path.
                let framed = Array(tcpControlBuffer)
                guard framed.count >= 4 else { break }
                let length = (UInt32(framed[0]) << 24)
                    | (UInt32(framed[1]) << 16)
                    | (UInt32(framed[2]) << 8)
                    | UInt32(framed[3])
                guard length > 0, length <= 2 * 1024 * 1024 else {
                    requestViewerStop()
                    return
                }
                let frameLength = 4 + Int(length)
                guard framed.count >= frameLength else { break }
                let payload = Data(framed[4..<frameLength])
                tcpControlBuffer.removeAll(keepingCapacity: true)
                tcpControlBuffer.append(contentsOf: framed[frameLength...])

                // Keep all untrusted TCP parsing on Array. Foundation.Data
                // slices may retain a non-zero index and can trap when a
                // later Collection operation assumes a zero-based buffer.
                let payloadBytes = Array(payload)
                let tokenBytes = Array(viewerControlToken)
                guard payloadBytes.count >= tokenBytes.count,
                      Array(payloadBytes.suffix(tokenBytes.count)) == tokenBytes else {
                    continue
                }
                let messageBytes = Array(payloadBytes.dropLast(tokenBytes.count))
                if messageBytes == Array("BYE".utf8) {
                    print("viewer close signal received for \(targetLabel)")
                    requestViewerStop()
                    return
                }
                if messageBytes == Array("IDR".utf8) {
                    networkLock.lock()
                    pendingFrames.removeAll(keepingCapacity: true)
                    networkAwaitingKeyframe = true
                    networkLock.unlock()
                    requestRecoveryKeyframe()
                    sendInputStatus(fd: fd)
                    continue
                }
                if messageBytes.count == 16,
                   Array(messageBytes.prefix(4)) == Array("LCP1".utf8) {
                    let message = Data(messageBytes)
                    sendLatencyProbeResponse(message, fd: fd, destination: nil)
                    continue
                }
                if messageBytes.count == 24,
                   Array(messageBytes.prefix(4)) == Array("LCF1".utf8) {
                    let message = Data(messageBytes)
                    stateLock.lock()
                    receiverFrameGaps = readUInt32BE(message, at: 4)
                    receiverInputDrops = readUInt32BE(message, at: 8)
                    receiverIncompleteAUs = readUInt32BE(message, at: 12)
                    receiverStaleFrames = readUInt32BE(message, at: 16)
                    receiverRttMs = readUInt16BE(message, at: 20)
                    receiverWireMs = readUInt16BE(message, at: 22)
                    receiverFeedbackNs = DispatchTime.now().uptimeNanoseconds
                    stateLock.unlock()
                    armReceiverHealthCheck()
                    continue
                }
                let message = Data(messageBytes)
                handleInputMessage(message, fd: fd, destination: nil)
            }
        }
    }

    private func readUInt16BE(_ data: Data, at offset: Int) -> UInt16 {
        guard offset >= 0, data.count >= offset + 2 else { return 0 }
        let bytes = Array(data)
        return (UInt16(bytes[offset]) << 8) | UInt16(bytes[offset + 1])
    }

    private func readUInt32BE(_ data: Data, at offset: Int) -> UInt32 {
        guard offset >= 0, data.count >= offset + 4 else { return 0 }
        let bytes = Array(data)
        return (UInt32(bytes[offset]) << 24)
            | (UInt32(bytes[offset + 1]) << 16)
            | (UInt32(bytes[offset + 2]) << 8)
            | UInt32(bytes[offset + 3])
    }

    private func readUInt64BE(_ data: Data, at offset: Int) -> UInt64 {
        guard offset >= 0, data.count >= offset + 8 else { return 0 }
        let bytes = Array(data)
        var value: UInt64 = 0
        for byte in bytes[offset..<(offset + 8)] {
            value = (value << 8) | UInt64(byte)
        }
        return value
    }

    /// NTP-style authenticated probe. Echoing the Android send time together
    /// with Host receive/send wall times lets the viewer separate LAN RTT from
    /// Host-to-device media delivery without assuming synchronized clocks.
    private func sendLatencyProbeResponse(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
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
        _ = sendControlPayload(response, fd: fd, destination: destination)
    }

    private func sendControlPayload(
        _ data: Data,
        fd: Int32,
        destination: sockaddr_in? = nil
    ) -> Int {
        if mediaTransport.usesTCP {
            return sendTCPFrame(data, fd: fd)
        }
        guard var destination else { return -1 }
        return send(data, fd: fd, to: &destination)
    }

    private func sendInputAck(
        sequence: UInt32,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        var ack = Data("LCA1".utf8)
        var sequenceBE = sequence.bigEndian
        withUnsafeBytes(of: &sequenceBE) { ack.append(contentsOf: $0) }
        inputLock.lock()
        let enabled = inputEnabled
        inputLock.unlock()
        ack.append(enabled ? 1 : 0)
        ack.append(viewerControlToken)
        _ = sendControlPayload(ack, fd: fd, destination: destination)
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
        _ = sendControlPayload(status, fd: fd)
    }

    private func handleInputMessage(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
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
            sendInputAck(sequence: sequence, fd: fd, destination: destination)
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
            sendInputAck(sequence: sequence, fd: fd, destination: destination)
            return
        }
        guard sequence == last &+ 1,
              validateAndInjectReliableInput(message, kind: kind, enabled: enabled) else {
            return
        }
        inputLock.lock()
        lastReliableInputSequence = sequence
        inputLock.unlock()
        sendInputAck(sequence: sequence, fd: fd, destination: destination)
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
    private func writePacket(
        _ data: Data,
        isFrame: Bool = false,
        isKeyframe: Bool = false
    ) {
        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else { return }

        let sendStart = DispatchTime.now().uptimeNanoseconds
        var sentBytes = 0
        var sendSyscallUs: UInt64 = 0
        var ok = true
        if isFrame {
            // Logical L2 header: marker + AU id LE + capture/encode clocks.
            guard data.count > 21, data[0] == 0x47, data[3...4] == Data([0x4C, 0x32]) else {
                requestRecoveryKeyframe()
                return
            }
            let maxPayload = 1_167 // 1,200-byte datagram - 33-byte L2 wire header
            let payloadCount = data.count - 21
            let fragmentCount = max(1, (payloadCount + maxPayload - 1) / maxPayload)
            guard fragmentCount <= Int(UInt16.max) else {
                requestRecoveryKeyframe()
                return
            }
            var sendWallMsBE = UInt64(Date().timeIntervalSince1970 * 1_000.0).bigEndian
            // Keep delta frames latest-wins, but spread a UDP recovery
            // keyframe over a small bounded window. Sending every fragment
            // back to back creates a Wi-Fi queue burst large enough to lose
            // one fragment and immediately re-enter the IDR loop. TCP is
            // ordered and reliable, so it sends the recovery AU immediately;
            // pacing that path only builds stale latency in the bridge.
            // UDP sends the recovery keyframe twice over the same paced
            // window. The keyframe is the recovery boundary, so dropping
            // deltas while it is in flight is intentional and handled by
            // enqueuePacket.
            let recoveryCopies = isKeyframe && mediaTransport == .udp ? 2 : 1
            let paceSpanUs = isKeyframe && mediaTransport == .udp
                ? min(220_000, max(120_000, UInt64(1_000_000 / max(1, fps)) * 12))
                : 0
            let transmissions = fragmentCount * recoveryCopies
            primary: for ordinal in 0..<transmissions {
                let index = ordinal % fragmentCount
                if paceSpanUs > 0 && transmissions > 1 {
                    let offsetUs = paceSpanUs * UInt64(ordinal) / UInt64(transmissions - 1)
                    paceNetwork(until: sendStart + offsetUs * 1_000)
                }
                let start = 21 + index * maxPayload
                let end = min(data.count, start + maxPayload)
                var datagram = Data(capacity: 33 + end - start)
                datagram.append(0x47)
                var indexBE = UInt16(index).bigEndian
                var countBE = UInt16(fragmentCount).bigEndian
                withUnsafeBytes(of: &indexBE) { datagram.append(contentsOf: $0) }
                withUnsafeBytes(of: &countBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: data[1...20])
                withUnsafeBytes(of: &sendWallMsBE) { datagram.append(contentsOf: $0) }
                datagram.append(contentsOf: data[start..<end])
                let syscallStart = DispatchTime.now().uptimeNanoseconds
                let sent = sendMediaDatagram(datagram, fd: fd)
                sendSyscallUs &+= (DispatchTime.now().uptimeNanoseconds &- syscallStart) / 1_000
                if sent != datagram.count {
                    ok = false
                    break primary
                }
                sentBytes += sent
            }
        } else {
            let syscallStart = DispatchTime.now().uptimeNanoseconds
            let sent = sendMediaDatagram(data, fd: fd)
            sendSyscallUs = (DispatchTime.now().uptimeNanoseconds &- syscallStart) / 1_000
            ok = sent == data.count
            if ok {
                sentBytes = sent
            }
        }
        if !ok {
            networkLock.lock()
            pendingFrames.removeAll(keepingCapacity: true)
            networkAwaitingKeyframe = true
            networkLock.unlock()
            stateLock.lock()
            framesDropped &+= isFrame ? 1 : 0
            stateLock.unlock()
            requestRecoveryKeyframe()
            return
        }
        let sendPaceUs = (DispatchTime.now().uptimeNanoseconds &- sendStart) / 1_000
        stateLock.lock()
        if isFrame, firstSendNs == nil {
            firstSendNs = DispatchTime.now().uptimeNanoseconds
            lifecycleState = "running"
            NSLog(
                "Leftcar first media frame sent %@: bytes=%d transport=%@",
                targetLabel,
                sentBytes,
                mediaTransport.rawValue
            )
        }
        bytesSent &+= Int64(sentBytes)
        rateWindowBytes &+= Int64(sentBytes)
        lastSendBlockUs = sendSyscallUs
        maxSendBlockUs = max(maxSendBlockUs, sendSyscallUs)
        appendRollingSample(sendSyscallUs, to: &sendBlockSamplesUs)
        lastSendPaceUs = sendPaceUs
        maxSendPaceUs = max(maxSendPaceUs, sendPaceUs)
        appendRollingSample(sendPaceUs, to: &sendPaceSamplesUs)
        if isFrame && isKeyframe {
            lastRecoverySendNs = DispatchTime.now().uptimeNanoseconds
        }
        stateLock.unlock()
        if isFrame && isKeyframe {
            recoveryKeyframeDidSend()
        }
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
        let sendPaceUs = lastSendPaceUs
        let maxSendPaceUs = maxSendPaceUs
        let networkDropped = framesDropped
        let networkQueueDropped = networkQueueDropped
        let recoveryFramesDropped = recoveryFramesDropped
        let udpSendFailures = udpSendFailures
        let udpSendRetries = udpSendRetries
        let recoveryKeyframes = recoveryKeyframes
        let recoveryRequestsSuppressed = recoveryRequestsSuppressed
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
        let sendPaceP95Us = percentile95(sendPaceSamplesUs)
        stateLock.unlock()

        networkLock.lock()
        let pending = pendingFrames.count
        networkLock.unlock()

        let obj: [String: Any] = [
            "frames": framesEncoded,
            "dropped": framesDropped,
            "networkDropped": networkDropped,
            "networkQueueDropped": networkQueueDropped,
            "recoveryFramesDropped": recoveryFramesDropped,
            "udpSendFailures": udpSendFailures,
            "udpSendRetries": udpSendRetries,
            "recoveryKeyframes": recoveryKeyframes,
            "recoveryRequestsSuppressed": recoveryRequestsSuppressed,
            "captureQueueDropped": captureQueueDropped,
            "bytes": bytesSent,
            "state": state,
            "fps": reportedFps,
            "kbps": reportedKbps,
            "fpsTarget": self.fps,
            "captureBackend": backend.rawValue,
            "mediaTransport": mediaTransport.rawValue,
            "firstCaptureMs": firstCaptureMs,
            "firstEncodeMs": firstEncodeMs,
            "firstSendMs": firstSendMs,
            "currentBitrate": currentBitrate,
            "captureIntervalP95Us": captureIntervalP95Us,
            "captureToEncodeP95Us": captureToEncodeP95Us,
            "captureQueueWaitP95Us": captureQueueWaitP95Us,
            "encodeOutputP95Us": encodeOutputP95Us,
            "sendBlockP95Us": sendBlockP95Us,
            "sendPaceP95Us": sendPaceP95Us,
            "captureToEncodeUs": captureToEncodeUs,
            "maxCaptureToEncodeUs": maxCaptureToEncodeUs,
            "captureQueueWaitUs": captureQueueWaitUs,
            "maxCaptureQueueWaitUs": maxCaptureQueueWaitUs,
            "encodeOutputUs": encodeOutputUs,
            "maxEncodeOutputUs": maxEncodeOutputUs,
            "sendBlockUs": sendBlockUs,
            "maxSendBlockUs": maxSendBlockUs,
            "sendPaceUs": sendPaceUs,
            "maxSendPaceUs": maxSendPaceUs,
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
