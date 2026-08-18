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

// MARK: - C ABI surface (v2, handle-based)

private let registryLock = NSLock()
private var registry: [UInt32: CaptureSession] = [:]
private var nextHandle: UInt32 = 1

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

@_cdecl("leftcar_capture_list_displays")
public func leftcarCaptureListDisplays() -> UnsafeMutablePointer<CChar> {
    let sem = DispatchSemaphore(value: 0)
    var json = "[]"
    SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) { content, error in
        defer { sem.signal() }
        guard error == nil, let content else {
            json = "[]"
            return
        }
        let arr: [[String: Any]] = content.displays.enumerated().map { idx, d in
            [
                "index": idx,
                "name": "Display \(idx)",
                "width": Int(d.width),
                "height": Int(d.height),
            ]
        }
        if let data = try? JSONSerialization.data(withJSONObject: arr),
           let s = String(data: data, encoding: .utf8) {
            json = s
        }
        _ = error // silence unused when nil
    }
    sem.wait()
    return UnsafeMutablePointer<CChar>(strdup(json))
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
        width: width,
        height: height,
        fps: fps
    )

    let sem = DispatchSemaphore(value: 0)
    var ok = false
    SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) { content, error in
        defer { sem.signal() }
        guard let content, error == nil else {
            setLastError("SCShareableContent failed: \(error?.localizedDescription ?? "nil")")
            return
        }
        let displays = content.displays
        guard Int(displayIndex) < displays.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displays.count) displays)")
            return
        }
        session.setupStream(display: displays[Int(displayIndex)])
        ok = session.isRunning
    }
    sem.wait()
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

final class CaptureSession {
    private let queue = DispatchQueue(label: "leftcar.capture", qos: .userInteractive)
    private var sock: Int32 = -1
    private var stream: SCStream?
    private var streamHandler: CaptureOutputHandler?
    private var session: VTCompressionSession?
    private let targetAddr: sockaddr_in
    private let targetPort: UInt16
    private let outWidth: UInt32
    private let outHeight: UInt32
    private let fps: UInt32
    private var csdSent = false

    private let stateLock = NSLock()
    private var running = false
    private var framesEncoded: Int64 = 0
    private var bytesSent: Int64 = 0
    private var stoppedReason = ""

    // 1s-window rate counters for stats
    private var rateWindowStart = Date()
    private var rateWindowFrames: Int64 = 0
    private var rateWindowBytes: Int64 = 0
    private var lastFps: UInt32 = 0
    private var lastKbps: UInt32 = 0

    var isRunning: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return running
    }

    init(targetAddr: sockaddr_in, targetPort: UInt16, width: UInt32, height: UInt32, fps: UInt32) {
        self.targetAddr = targetAddr
        self.targetPort = targetPort
        self.outWidth = width
        self.outHeight = height
        self.fps = max(1, fps)
    }

    // MARK: Setup

    func connectSocket() -> Bool {
        sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else {
            markStopped("socket() failed")
            return false
        }
        var noDelay: Int32 = 1
        setsockopt(sock, IPPROTO_TCP, TCP_NODELAY, &noDelay, socklen_t(MemoryLayout<Int32>.size))
        var addr = targetAddr
        var result: Int32 = -1
        withUnsafePointer(to: &addr) { ap in
            ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                result = connect(sock, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard result == 0 else {
            markStopped("TCP connect to viewer failed (errno \(errno)) — is the stream window open?")
            return false
        }
        running = true
        return true
    }

    static let tccDeniedHint = "screen-recording permission required (System Settings > Privacy & Security > Screen Recording)"

    func setupStream(display: SCDisplay) {
        let config = SCStreamConfiguration()
        config.width = Int(outWidth)
        config.height = Int(outHeight)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = true
        config.queueDepth = 4

        let filter = SCContentFilter(display: display, excludingWindows: [])
        let s = SCStream(filter: filter, configuration: config, delegate: nil)
        let handler = CaptureOutputHandler(session: self)

        do {
            try s.addStreamOutput(handler, type: .screen, sampleHandlerQueue: queue)
            streamHandler = handler
            s.startCapture { [weak self] error in
                if let error = error {
                    self?.markStopped("startCapture failed: \(error.localizedDescription) — \(CaptureSession.tccDeniedHint)")
                }
            }
            stream = s
        } catch {
            markStopped("addStreamOutput: \(error.localizedDescription)")
        }
    }

    func stop() {
        if let s = stream {
            s.stopCapture(completionHandler: nil)
            stream = nil
            streamHandler = nil
        }
        if let s = session {
            VTCompressionSessionInvalidate(s)
            session = nil
        }
        stateLock.lock()
        if sock >= 0 {
            close(sock)
            sock = -1
        }
        running = false
        stateLock.unlock()
    }

    func markStopped(_ reason: String) {
        stoppedReason = reason
        stop()
    }

    // MARK: Frame Processing & VideoToolbox Encoding

    func handleFrame(_ sample: CMSampleBuffer) {
        stateLock.lock()
        let stillRunning = running
        stateLock.unlock()
        guard stillRunning else { return }

        guard let pb = CMSampleBufferGetImageBuffer(sample) else { return }
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else { return }

        let pts = CMTime(value: CMTimeValue(framesEncoded), timescale: CMTimeScale(fps))
        let duration = CMTime(value: 1, timescale: CMTimeScale(fps))
        var flags: VTEncodeInfoFlags = []

        let status = VTCompressionSessionEncodeFrame(
            s,
            imageBuffer: pb,
            presentationTimeStamp: pts,
            duration: duration,
            frameProperties: nil,
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
        }
    }

    private func setupEncoder(for imageBuffer: CVImageBuffer) {
        let w = Int32(CVPixelBufferGetWidth(imageBuffer))
        let h = Int32(CVPixelBufferGetHeight(imageBuffer))

        // bitrate budget: ~0.07 bits/pixel/frame, clamped to a sane LAN range
        let idealBits = Double(w) * Double(h) * Double(fps) * 0.07
        let avgBitrate = min(max(idealBits, 4_000_000), 24_000_000)

        var s: VTCompressionSession?
        let status = VTCompressionSessionCreate(
            allocator: nil,
            width: w,
            height: h,
            codecType: kCMVideoCodecType_H264,
            encoderSpecification: nil,
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
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: Int(avgBitrate) as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_DataRateLimits, value: [Int(avgBitrate * 1.5), 1] as CFArray)
        // short GOP: a lost keyframe stalls the stream until the next IDR —
        // fps/2 frames bounds worst-case freeze to ~0.5s
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: max(1, fps / 2) as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: Int32(fps) as CFNumber)

        VTCompressionSessionPrepareToEncodeFrames(s)
        session = s
    }

    // MARK: Packetization & TCP Transmission

    private func handleEncoded(_ sample: CMSampleBuffer) {
        // Send parameter sets (csd: SPS/PPS) periodically so a viewer that
        // joins late (or restarted its decoder) can configure before the
        // next keyframe.
        let notSync = CMGetAttachment(sample, key: "NotSync" as CFString, attachmentModeOut: nil)
        let isKeyframe = notSync == nil
        if !csdSent || isKeyframe, let fd = sample.formatDescription {
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
                // never block the encoder callback: a short burst without
                // sleeps still covers a late joiner, and the periodic resend
                // (every keyframe) guarantees recovery.
                for _ in 0..<3 {
                    sendPacket(cfg)
                }
                csdSent = true
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
        stateLock.lock()
        let auId = UInt16(framesEncoded & 0xFFFF)
        stateLock.unlock()
        var p2 = Data([0x46, 0, 1, UInt8(auId & 0xFF), UInt8(auId >> 8)])
        p2.append(pkt)
        sendPacket(p2)
    }

    /// Send one framed packet: [u32 BE length][payload] over the TCP stream.
    /// On send failure the session auto-stops (viewer window closed).
    private func sendPacket(_ data: Data) {
        stateLock.lock()
        let fd = sock
        stateLock.unlock()
        guard fd >= 0 else { return }
        var framed = Data()
        var lenBE = UInt32(data.count).bigEndian
        withUnsafeBytes(of: &lenBE) { framed.append(contentsOf: $0) }
        framed.append(data)
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
            markStopped("viewer disconnected (send failed)")
            return
        }
        stateLock.lock()
        bytesSent &+= Int64(data.count)
        rateWindowBytes &+= Int64(data.count)
        stateLock.unlock()
    }

    // MARK: Stats

    func statsJSON() -> String {
        stateLock.lock()
        defer { stateLock.unlock() }

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
        let obj: [String: Any] = [
            "frames": framesEncoded,
            "bytes": bytesSent,
            "state": state,
            "fps": lastFps,
            "kbps": lastKbps,
            "error": stoppedReason,
        ]
        if let data = try? JSONSerialization.data(withJSONObject: obj),
           let s = String(data: data, encoding: .utf8) {
            return s
        }
        return "{\"state\":\"\(state)\"}"
    }
}
