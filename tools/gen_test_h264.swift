// Real H.264 test-pattern generator via VideoToolbox (H18 path preview).
// Usage: swift gen_test_h264.swift <output.264> [seconds]
// Encodes a moving color pattern as baseline H.264 Annex-B at 320x240.

import Foundation
import VideoToolbox
import CoreVideo
import CoreMedia

let args = CommandLine.arguments
guard args.count >= 2 else { FileHandle.standardError.write("usage: gen <out.264> [secs] [count]\n".data(using: .utf8)!); exit(2) }
let outPath = args[1]
let seconds = args.count > 2 ? Double(args[2])! : 2.0
let layerCount = args.count > 3 ? Int(args[3])! : 1

var session: VTCompressionSession?
var status = VTCompressionSessionCreate(
    allocator: nil,
    width: 320, height: 240,
    codecType: kCMVideoCodecType_H264,
    encoderSpecification: nil,
    imageBufferAttributes: nil,
    compressedDataAllocator: nil,
    outputCallback: nil, refcon: nil,
    compressionSessionOut: &session)
guard status == noErr, let s = session else {
    FileHandle.standardError.write("VTCompressionSessionCreate failed: \(status)\n".data(using: .utf8)!); exit(1)
}

VTSessionSetProperty(s, key: kVTCompressionPropertyKey_RealTime, value: true as CFBoolean)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_AutoLevel as CFString)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AllowFrameReordering, value: false as CFBoolean)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_AverageBitRate, value: 2_000_000 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: 30 as CFNumber)
VTSessionSetProperty(s, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)
// prepare optional

var out = Data()
var frameID: Int64 = 0
let lock = NSLock()

var wroteConfig = false
func emit(_ sample: CMSampleBuffer) {
    // parameter sets live in the format description, not the bitstream:
    // prepend them once as Annex-B (csd-0/csd-1 equivalents)
    if !wroteConfig, let fd = sample.formatDescription {
        var idx: Int = 0
        while true {
            var ptr: UnsafePointer<UInt8>? = nil
            var size: Int = 0
            let st = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fd, parameterSetIndex: idx, parameterSetPointerOut: &ptr, parameterSetSizeOut: &size, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil)
            if st != noErr { break }
            if let ptr = ptr {
                out.append(contentsOf: [0,0,0,1])
                out.append(contentsOf: UnsafeBufferPointer(start: ptr, count: size))
            }
            idx += 1
        }
        if idx > 0 { wroteConfig = true }
    }
    guard let bb = CMSampleBufferGetDataBuffer(sample) else { return }
    var lengthAtOffset: Int = 0
    var totalLength: Int = 0
    var dataPointer: UnsafeMutablePointer<Int8>? = nil
    CMBlockBufferGetDataPointer(bb, atOffset: 0, lengthAtOffsetOut: &lengthAtOffset, totalLengthOut: &totalLength, dataPointerOut: &dataPointer)
    guard let ptr = dataPointer else { return }
    var offset = 0
    let bytes = UnsafeRawBufferPointer(start: ptr, count: totalLength)
    while offset < totalLength {
        var length: Int = 0
        for j in 0..<4 { length = (length << 8) | Int(bytes[offset + j]) }
        out.append(contentsOf: [0,0,0,1])
        out.append(contentsOf: bytes[(offset+4)..<(offset+4+length)])
        offset += 4 + length
    }
}

final class Box { var done = false }
let box = Box()
let queue = DispatchQueue(label: "gen")

// encode frames
let frames = Int(seconds * 30)
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
                let t = UInt32((f * 3 + row + col) % 256)
                line[col] = 0xFF000000 | (t << 16) | (UInt32((col * 255) / 320) << 8) | UInt32((row * 255) / h)
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
    let time = CMTime(value: CMTimeValue(f), timescale: 30)
    let dur = CMTime(value: 1, timescale: 30)
    let sem = DispatchSemaphore(value: 0)
    var flags: VTEncodeInfoFlags = []
    VTCompressionSessionEncodeFrame(s, imageBuffer: pixelBuffer, presentationTimeStamp: time, duration: dur, frameProperties: nil, infoFlagsOut: &flags) { (_: OSStatus, _: VTEncodeInfoFlags, sample: CMSampleBuffer?) in
        if let sample = sample, CMSampleBufferDataIsReady(sample) {
            lock.lock(); emit(sample); lock.unlock()
        }
        sem.signal()
    }
    sem.wait()
    _ = frameID
    frameID += 1
}
VTCompressionSessionCompleteFrames(s, untilPresentationTimeStamp: CMTime(value: CMTimeValue(frames), timescale: 30))
queue.sync {}

try! out.write(to: URL(fileURLWithPath: outPath))
print("WROTE \(out.count) bytes, \(frames) frames, layerCount=\(layerCount)")
