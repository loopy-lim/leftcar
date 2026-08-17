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
// optional geometry: W H FPS (S1 = 1920 1080 60)
let W = args.count > 4 ? Int(args[4])! : 320
let H = args.count > 5 ? Int(args[5])! : 240
let FPS = args.count > 6 ? Int(args[6])! : 30

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
    allocator: nil, width: Int32(W), height: Int32(H),
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
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: (FPS * 2) as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: FPS as CFNumber)

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
        if idx > 0 {
            // send CFG repeatedly for the first ~2s so late-joining receivers get it
            for _ in 0..<30 { send(cfg); Thread.sleep(forTimeInterval: 0.05) }
            wroteConfig = true
            print("CONFIG sent x30 (\(cfg.count)B)")
        }
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
    // fragment large AUs across datagrams: [F][fragIdx:u8][fragCnt:u8][auId:u16][payload]
    let MTU = 1400
    if pkt.count <= MTU {
        var p2 = pkt
        p2.insert(contentsOf: [0x46, 0, 1, UInt8(sentFrames & 0xFF)], at: 0)
        send(p2)
        sentFrames += 1
        sentBytes += pkt.count
    } else {
        let payload = pkt
        let fragCnt = UInt8((payload.count + MTU - 1) / MTU)
        let auId = UInt16(sentFrames & 0xFFFF)
        var idx = 0
        var frag = 0
        while idx < payload.count {
            let end = min(idx + MTU, payload.count)
            var p2 = Data([0x46, UInt8(frag), fragCnt, UInt8(auId & 0xFF), UInt8(auId >> 8)])
            p2.append(contentsOf: payload[idx..<end])
            send(p2)
            idx = end
            frag += 1
        }
        sentFrames += 1
        sentBytes += payload.count
    }
}

let frames = Int(seconds * Double(FPS))
print("STREAMING \(frames) frames @\(FPS)fps \(W)x\(H) to \(host):\(port) ...")

// pool of reusable pixel buffers (VT retains them during encode)
var pool: [CVPixelBuffer] = []
for _ in 0..<4 {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(nil, W, H, kCVPixelFormatType_32BGRA, nil, &pb)
    if let pb = pb { pool.append(pb) }
}
var poolIdx = 0
let poolLock = NSLock()

func fill(_ pixelBuffer: CVPixelBuffer, _ f: Int) {
    CVPixelBufferLockBaseAddress(pixelBuffer, [])
    if let base = CVPixelBufferGetBaseAddress(pixelBuffer) {
        let stride = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let rows = CVPixelBufferGetHeight(pixelBuffer)
        let p = base.assumingMemoryBound(to: UInt32.self)
        for row in 0..<rows {
            let line = p.advanced(by: row * stride / 4)
            let tRow = UInt32((f * 8 + row) % 256)
            let gRow = UInt32((row * 255) / rows)
            var col = 0
            while col < W - 3 {
                line[col] = 0xFF000000 | (tRow << 16) | (UInt32((col * 255) / W) << 8) | gRow
                line[col + 1] = 0xFF000000 | (tRow << 16) | (UInt32(((col + 1) * 255) / W) << 8) | gRow
                line[col + 2] = 0xFF000000 | (tRow << 16) | (UInt32(((col + 2) * 255) / W) << 8) | gRow
                line[col + 3] = 0xFF000000 | (tRow << 16) | (UInt32(((col + 3) * 255) / W) << 8) | gRow
                col += 4
            }
            while col < W {
                line[col] = 0xFF000000 | (tRow << 16) | (UInt32((col * 255) / W) << 8) | gRow
                col += 1
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
}

for f in 0..<frames {
    poolLock.lock()
    let pixelBuffer = pool[poolIdx % pool.count]
    poolIdx += 1
    poolLock.unlock()
    fill(pixelBuffer, f)
    var flags: VTEncodeInfoFlags = []
    let time = CMTime(value: CMTimeValue(f), timescale: CMTimeScale(FPS))
    VTCompressionSessionEncodeFrame(s, imageBuffer: pixelBuffer, presentationTimeStamp: time, duration: CMTime(value: 1, timescale: CMTimeScale(FPS)), frameProperties: nil, infoFlagsOut: &flags) { _, _, sample in
        if let sample = sample, CMSampleBufferDataIsReady(sample) {
            packetize(sample)
        }
        sem.signal()
    }
    // async pipeline: only wait when too many frames are in flight
    let inflight = f - sentFrames
    if inflight >= 3 { sem.wait() }
    let target = t0.addingTimeInterval(Double(f + 1) / Double(FPS))
    let now = Date()
    if now < target { Thread.sleep(forTimeInterval: target.timeIntervalSince(now)) }
}
// drain remaining
while sentFrames < frames {
    sem.wait()
}
let elapsed = Date().timeIntervalSince(t0)
print("DONE frames=\(sentFrames) bytes=\(sentBytes) elapsed=\(String(format: "%.1f", elapsed))s rate=\(String(format: "%.1f", Double(sentFrames) / elapsed))fps")
