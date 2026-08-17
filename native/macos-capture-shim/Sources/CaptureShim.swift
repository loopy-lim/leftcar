// CaptureShim: real macOS screen capture -> H.264 -> UDP, as a C-ABI dylib.
//
// Path (docs/02 §4, H16-H18):
//   SCShareableContent (main display) -> SCStream (BGRA frames)
//   -> VTCompressionSession (H.264 baseline, realtime, no B-frames)
//   -> UDP datagrams (CFG csd packet + fragmented AU packets) to the viewer.

import Foundation
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo

// MARK: - C ABI surface

@_cdecl("leftcar_capture_start")
public func leftcarCaptureStart(port: UInt16) -> Int32 {
    Shim.shared.start(port: port)
}

@_cdecl("leftcar_capture_stop")
public func leftcarCaptureStop() -> Int32 {
    Shim.shared.stop()
}

@_cdecl("leftcar_capture_frames")
public func leftcarCaptureFrames() -> Int64 {
    Int64(Shim.shared.framesEncoded)
}

@_cdecl("leftcar_capture_bytes")
public func leftcarCaptureBytes() -> Int64 {
    Int64(Shim.shared.bytesSent)
}

@_cdecl("leftcar_capture_last_error")
public func leftcarCaptureLastError() -> UnsafePointer<CChar> {
    Shim.shared.lastErrorUTF8
}

@_cdecl("leftcar_capture_set_target")
public func leftcarCaptureSetTarget(ip: UnsafePointer<CChar>, port: UInt16) -> Int32 {
    var addr = sockaddr_in()
    addr.sin_family = sa_family_t(AF_INET)
    addr.sin_port = port.bigEndian
    _ = inet_pton(AF_INET, ip, &addr.sin_addr)
    Shim.shared.targetAddr = addr
    return 0
}

// MARK: - Stream Handler

final class CaptureOutputHandler: NSObject, SCStreamOutput, SCStreamDelegate {
    weak var shim: Shim?

    init(shim: Shim) {
        self.shim = shim
        super.init()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen else { return }
        shim?.handleFrame(sampleBuffer)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        _ = shim?.fail("Stream stopped: \(error.localizedDescription)")
    }
}

// MARK: - Shim Implementation

final class Shim {
    static let shared = Shim()

    private let queue = DispatchQueue(label: "leftcar.capture", qos: .userInteractive)
    private var sock: Int32 = -1
    private var stream: SCStream?
    private var streamHandler: CaptureOutputHandler?
    private var session: VTCompressionSession?
    private var targetPort: UInt16 = 0
    private var csdSent = false

    var framesEncoded: Int64 = 0
    var bytesSent: Int64 = 0
    private var lastError: String = ""

    lazy var lastErrorUTF8: UnsafePointer<CChar> = UnsafePointer(strdup(""))
    var targetAddr = sockaddr_in()

    func fail(_ message: String) -> Int32 {
        lastError = message
        free(UnsafeMutablePointer(mutating: lastErrorUTF8))
        lastErrorUTF8 = UnsafePointer(strdup(message))
        return 1
    }

    // MARK: Start & Stop

    func start(port: UInt16) -> Int32 {
        _ = stop()
        targetPort = port
        csdSent = false
        framesEncoded = 0
        bytesSent = 0
        sock = socket(AF_INET, SOCK_DGRAM, 0)

        let sem = DispatchSemaphore(value: 0)
        var setupSuccess = false

        SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) { [weak self] content, error in
            guard let self = self else { sem.signal(); return }
            guard let content = content, error == nil, let display = content.displays.first else {
                _ = self.fail("SCShareableContent failed: \(error?.localizedDescription ?? "no display")")
                sem.signal()
                return
            }
            self.setupStream(display: display)
            setupSuccess = (self.stream != nil)
            sem.signal()
        }
        sem.wait()

        return setupSuccess ? 0 : 1
    }

    static let tccDeniedHint = "screen-recording permission required (System Settings > Privacy & Security > Screen Recording)"

    private func setupStream(display: SCDisplay) {
        let config = SCStreamConfiguration()
        config.width = 1280
        config.height = 720
        config.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = true
        config.queueDepth = 3

        let filter = SCContentFilter(display: display, excludingWindows: [])
        let s = SCStream(filter: filter, configuration: config, delegate: nil)
        let handler = CaptureOutputHandler(shim: self)

        do {
            try s.addStreamOutput(handler, type: .screen, sampleHandlerQueue: queue)
            self.streamHandler = handler
            s.startCapture { [weak self] error in
                if let error = error {
                    _ = self?.fail("startCapture failed: \(error.localizedDescription) — \(Shim.tccDeniedHint)")
                }
            }
            self.stream = s
        } catch {
            _ = fail("addStreamOutput: \(error.localizedDescription)")
        }
    }

    func stop() -> Int32 {
        if let s = stream {
            s.stopCapture(completionHandler: nil)
            stream = nil
            streamHandler = nil
        }
        if let s = session {
            VTCompressionSessionInvalidate(s)
            session = nil
        }
        if sock >= 0 {
            close(sock)
            sock = -1
        }
        return 0
    }

    // MARK: Frame Processing & VideoToolbox Encoding

    func handleFrame(_ sample: CMSampleBuffer) {
        guard let pb = CMSampleBufferGetImageBuffer(sample) else { return }
        if session == nil {
            setupEncoder(for: pb)
        }
        guard let s = session else { return }

        let pts = CMTime(value: CMTimeValue(framesEncoded), timescale: 30)
        let duration = CMTime(value: 1, timescale: 30)
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
            framesEncoded &+= 1
        }
    }

    private func setupEncoder(for imageBuffer: CVImageBuffer) {
        let w = Int32(CVPixelBufferGetWidth(imageBuffer))
        let h = Int32(CVPixelBufferGetHeight(imageBuffer))
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
            _ = fail("VTCompressionSessionCreate failed: \(status)")
            return
        }

        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_RealTime, value: true as CFBoolean)
        VTSessionSetProperty(s, key: kVTProfileLevel_H264_Baseline_AutoLevel, value: true as CFBoolean)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AllowFrameReordering, value: false as CFBoolean)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: 4_000_000 as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_DataRateLimits, value: [6_000_000, 1] as CFArray)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: 60 as CFNumber)
        VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)

        VTCompressionSessionPrepareToEncodeFrames(s)
        self.session = s
    }

    // MARK: Packetization & UDP Transmission

    private func handleEncoded(_ sample: CMSampleBuffer) {
        // Send parameter sets (csd: SPS/PPS) upon first encoded frame or keyframe
        if !csdSent, let fd = sample.formatDescription {
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
                sendUDP(cfg)
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

        let MTU = 1400
        let auId = UInt16(framesEncoded & 0xFFFF)
        if pkt.count <= MTU {
            var p2 = pkt
            p2.insert(contentsOf: [0x46, 0, 1, UInt8(auId & 0xFF), UInt8(auId >> 8)], at: 0)
            sendUDP(p2)
        } else {
            let cnt = UInt8((pkt.count + MTU - 1) / MTU)
            var idx = 0
            var frag = 0
            while idx < pkt.count {
                let end = min(idx + MTU, pkt.count)
                var p2 = Data([0x46, UInt8(frag), cnt, UInt8(auId & 0xFF), UInt8(auId >> 8)])
                p2.append(contentsOf: pkt[idx..<end])
                sendUDP(p2)
                idx = end
                frag += 1
            }
        }
    }

    private func sendUDP(_ data: Data) {
        guard sock >= 0 else { return }
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            var addr = targetAddr
            withUnsafePointer(to: &addr) { ap in
                ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                    _ = sendto(sock, raw.baseAddress, raw.count, 0, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
                }
            }
        }
        bytesSent &+= Int64(data.count)
    }
}
