// Real-time Mac -> tablet H.264 UDP streamer (E6 sender side).
// Usage: swift stream_sender.swift <tablet_ip> <port> [seconds]
// Encodes a live moving pattern with VideoToolbox (baseline, realtime) and
// sends: CONFIG packet (csd), then one UDP datagram per access unit.

import Foundation
import VideoToolbox
import CoreVideo
import CoreMedia

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write("usage: stream_sender.swift <ip> <port> [secs]\n".data(using: .utf8)!)
    exit(2)
}
let host = args[1]
let port = UInt16(args[2]) ?? 5000
let seconds = args.count > 3 ? Double(args[3])! : 30.0

let sock = socket(AF_INET, SOCK_DGRAM, 0)
var dest = sockaddr_in()
dest.sin_family = sa_family_t(AF_INET)
dest.sin_port = port.bigEndian
inet_pton(AF_INET, host, &dest.sin_addr)

func send(_ data: Data) {
    data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
        var addr = dest
        withUnsafePointer(to: &addr) { ap in
            ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                _ = sendto(sock, raw.baseAddress, raw.count, 0, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
    }
}

var session: VTCompressionSession?
guard VTCompressionSessionCreate(
    allocator: nil, width: 320, height: 240,
    codecType: kCMVideoCodecType_H264,
    encoderSpecification: nil, imageBufferAttributes: nil,
    compressedDataAllocator: nil,
    outputCallback: nil, refcon: nil,
    compressionSessionOut: &session) == noErr, let s = session else {
    FileHandle.standardError.write("session create failed\n".data(using: .utf8)!); exit(1)
}
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_RealTime, value: true as CFBoolean)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_AutoLevel as CFString)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AllowFrameReordering, value: false as CFBoolean)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: 2_000_000 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_DataRateLimits, value: [3_000_000, 1] as CFArray)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: 30 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)

var sentFrames = 0
var sentBytes = 0
var wroteConfig = false
let t0 = Date()
let sem = DispatchSemaphore(value: 0)

func packetize(_ sample: CMSampleBuffer) {
    // prepend parameter sets (csd) once as a CONFIG datagram
    if !wroteConfig, let fd = sample.formatDescription {
        var cfg = Data([0x43, 0x46, 0x47]) // "CFG"
        var idx = 0
        while true {
            var ptr: UnsafePointer<UInt8>? = nil; var size = 0
            if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fd, parameterSetIndex: idx, parameterSetPointerOut: &ptr, parameterSetSizeOut: &size, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil) != noErr { break }
            if let ptr = ptr {
                var lenBE = UInt32(size + 4).bigEndian
                withUnsafeBytes(of: &lenBE) { cfg.append(contentsOf: $0) }
                cfg.append(contentsOf: [0,0,0,1])
                cfg.append(contentsOf: UnsafeBufferPointer(start: ptr, count: size))
            }
            idx += 1
        }
        if idx > 0 { send(cfg); wroteConfig = true; print("CONFIG sent (\(cfg.count)B)") }
    }
    guard let bb = CMSampleBufferGetDataBuffer(sample) else { return }
    var lengthAtOffset = 0, totalLength = 0
    var dataPointer: UnsafeMutablePointer<Int8>? = nil
    CMBlockBufferGetDataPointer(bb, atOffset: 0, lengthAtOffsetOut: &lengthAtOffset, totalLengthOut: &totalLength, dataPointerOut: &dataPointer)
    guard let ptr = dataPointer else { return }
    let bytes = UnsafeRawBufferPointer(start: ptr, count: totalLength)
    // one datagram per access unit, AU prefixed with AU marker + pts
    var pkt = Data([0x41, 0x55]) // "AU"
    let pts = CMSampleBufferGetPresentationTimeStamp(sample).value
    var ptsBE = UInt64(pts).bigEndian
    withUnsafeBytes(of: &ptsBE) { pkt.append(contentsOf: $0) }
    var offset = 0
    while offset < totalLength {
        var length = 0
        for j in 0..<4 { length = (length << 8) | Int(bytes[offset + j]) }
        pkt.append(contentsOf: [0,0,0,1])
        pkt.append(contentsOf: bytes[(offset+4)..<(offset+4+length)])
        offset += 4 + length
    }
    if pkt.count < 65000 {
        send(pkt)
        sentFrames += 1
        sentBytes += pkt.count
    }
}

let frames = Int(seconds * 30)
print("STREAMING \(frames) frames @30fps to \(host):\(port) ...")
for f in 0..<frames {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(nil, 320, 240, kCVPixelFormatType_32BGRA, nil, &pb)
    guard let pixelBuffer = pb else { continue }
    CVPixelBufferLockBaseAddress(pixelBuffer, [])
    if let base = CVPixelBufferGetBaseAddress(pixelBuffer) {
        let stride = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let h = CVPixelBufferGetHeight(pixelBuffer)
        for row in 0..<h {
            let line = base.assumingMemoryBound(to: UInt32.self).advanced(by: row * stride / 4)
            for col in 0..<320 {
                let t = UInt32((f * 8 + row + col) % 256)
                line[col] = 0xFF000000 | (t << 16) | (UInt32((col * 255) / 320) << 8) | UInt32((row * 255) / h)
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
    var flags: VTEncodeInfoFlags = []
    let time = CMTime(value: CMTimeValue(f), timescale: 30)
    VTCompressionSessionEncodeFrame(s, imageBuffer: pixelBuffer, presentationTimeStamp: time, duration: CMTime(value: 1, timescale: 30), frameProperties: nil, infoFlagsOut: &flags) { _, _, sample in
        if let sample = sample, CMSampleBufferDataIsReady(sample) {
            packetize(sample)
        }
        sem.signal()
    }
    sem.wait()
    // pace to ~30fps
    let target = t0.addingTimeInterval(Double(f + 1) / 30.0)
    let now = Date()
    if now < target { Thread.sleep(forTimeInterval: target.timeIntervalSince(now)) }
}
let elapsed = Date().timeIntervalSince(t0)
print("DONE frames=\(sentFrames) bytes=\(sentBytes) elapsed=\(String(format: "%.1f", elapsed))s rate=\(String(format: "%.1f", Double(sentFrames) / elapsed))fps")
