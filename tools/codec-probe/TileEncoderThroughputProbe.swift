import CoreMedia
import CoreVideo
import Darwin
import Foundation

private final class ProbeCounters: @unchecked Sendable {
    private let lock = NSLock()
    private var accepted = [TileSide: Int]()
    private var valid = [TileSide: Int]()
    private var dropped = [TileSide: Int]()
    private var failed = [TileSide: Int]()
    private var callbackLatencyUs = [TileSide: [UInt64]]()
    private var validCallbackNs = [TileSide: [UInt64]]()

    func recordSubmission(side: TileSide) {
        lock.lock()
        accepted[side, default: 0] += 1
        lock.unlock()
    }

    func record(
        side: TileSide,
        submittedNs: UInt64,
        result: Result<TileEncodedSample, TileEncoderError>
    ) {
        let nowNs = DispatchTime.now().uptimeNanoseconds
        lock.lock()
        callbackLatencyUs[side, default: []].append((nowNs - submittedNs) / 1_000)
        switch result {
        case .success:
            valid[side, default: 0] += 1
            validCallbackNs[side, default: []].append(nowNs)
        case .failure(.dropped):
            dropped[side, default: 0] += 1
        case .failure:
            failed[side, default: 0] += 1
        }
        lock.unlock()
    }

    func snapshot(
        side: TileSide,
        startedNs: UInt64,
        submissionEndedNs: UInt64,
        elapsedSeconds: Double
    ) -> [String: Any] {
        lock.lock()
        defer { lock.unlock() }
        let samples = callbackLatencyUs[side, default: []].sorted()
        func percentileIndex(_ percentile: Double) -> Int {
            samples.isEmpty
                ? 0
                : min(
                    samples.count - 1,
                    Int((Double(samples.count) * percentile).rounded(.up)) - 1
                )
        }
        let validCount = valid[side, default: 0]
        let submissionSeconds = Double(submissionEndedNs - startedNs) / 1_000_000_000
        let callbacksDuringSubmission = validCallbackNs[side, default: []].filter {
            $0 <= submissionEndedNs
        }.count
        let callbackP95Us: UInt64 = samples.isEmpty ? 0 : samples[percentileIndex(0.95)]
        let callbackP99Us: UInt64 = samples.isEmpty ? 0 : samples[percentileIndex(0.99)]
        let callbackMaxUs: UInt64 = samples.last ?? 0
        return [
            "accepted": accepted[side, default: 0],
            "valid": validCount,
            "dropped": dropped[side, default: 0],
            "failed": failed[side, default: 0],
            "validFps": Double(validCount) / max(0.001, elapsedSeconds),
            "validDuringSubmission": callbacksDuringSubmission,
            "validFpsDuringSubmission": Double(callbacksDuringSubmission)
                / max(0.001, submissionSeconds),
            "callbackP95Us": callbackP95Us,
            "callbackP99Us": callbackP99Us,
            "callbackMaxUs": callbackMaxUs,
        ]
    }
}

private final class PairCompletion: @unchecked Sendable {
    private let lock = NSLock()
    private var remaining: Int
    private let release: () -> Void

    init(remaining: Int, release: @escaping () -> Void) {
        self.remaining = remaining
        self.release = release
    }

    func complete() {
        lock.lock()
        remaining -= 1
        let shouldRelease = remaining == 0
        lock.unlock()
        if shouldRelease { release() }
    }
}

private enum ProbePattern: String {
    case moving
    case noise
}

private func makeFrame(seed: UInt8, pattern: ProbePattern) -> CVPixelBuffer {
    let attributes: [String: Any] = [
        kCVPixelBufferPixelFormatTypeKey as String:
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        kCVPixelBufferWidthKey as String: 1_920,
        kCVPixelBufferHeightKey as String: 2_160,
        kCVPixelBufferMetalCompatibilityKey as String: true,
        kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
    ]
    var pixelBuffer: CVPixelBuffer?
    precondition(
        CVPixelBufferCreate(
            kCFAllocatorDefault,
            1_920,
            2_160,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            attributes as CFDictionary,
            &pixelBuffer
        ) == kCVReturnSuccess
    )
    let buffer = pixelBuffer!
    CVPixelBufferLockBaseAddress(buffer, [])
    defer { CVPixelBufferUnlockBaseAddress(buffer, []) }

    var random = UInt32(seed) &+ 0x9E37_79B9
    func nextByte(in range: ClosedRange<UInt8>) -> UInt8 {
        random ^= random << 13
        random ^= random >> 17
        random ^= random << 5
        let span = UInt32(range.upperBound - range.lowerBound) + 1
        return range.lowerBound &+ UInt8(random % span)
    }

    let luma = CVPixelBufferGetBaseAddressOfPlane(buffer, 0)!
    let lumaStride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 0)
    for row in 0..<2_160 {
        let bytes = luma.advanced(by: row * lumaStride).assumingMemoryBound(to: UInt8.self)
        for column in 0..<1_920 {
            switch pattern {
            case .noise:
                bytes[column] = nextByte(in: 16...235)
            case .moving:
                let phase = Int(seed) * 7
                let checker = (((column + phase) / 64) + ((row + phase) / 64)) & 1
                let gradient = (column + row + phase) % 96
                bytes[column] = UInt8((checker == 0 ? 40 : 136) + gradient)
            }
        }
    }

    let chroma = CVPixelBufferGetBaseAddressOfPlane(buffer, 1)!
    let chromaStride = CVPixelBufferGetBytesPerRowOfPlane(buffer, 1)
    for row in 0..<1_080 {
        let bytes = chroma.advanced(by: row * chromaStride).assumingMemoryBound(to: UInt8.self)
        for column in 0..<1_920 {
            switch pattern {
            case .noise:
                bytes[column] = nextByte(in: 16...240)
            case .moving:
                bytes[column] = column.isMultiple(of: 2)
                    ? 96 &+ (seed % 48)
                    : 160 &- (seed % 48)
            }
        }
    }
    return buffer
}

private func waitUntil(_ targetNs: UInt64) {
    while true {
        let nowNs = DispatchTime.now().uptimeNanoseconds
        guard nowNs < targetNs else { return }
        let remainingNs = targetNs - nowNs
        if remainingNs > 1_000_000 {
            Thread.sleep(forTimeInterval: Double(remainingNs - 500_000) / 1_000_000_000)
        } else {
            sched_yield()
        }
    }
}

@main
private struct TileEncoderThroughputProbe {
    static func main() throws {
        let mode = CommandLine.arguments.dropFirst().first ?? "single"
        let validModes = ["single", "dual", "singleAve", "mixed", "dualAve"]
        guard validModes.contains(mode) else {
            fputs(
                "usage: TileEncoderThroughputProbe <single|dual|singleAve|mixed|dualAve> [seconds] [moving|noise]\n",
                stderr
            )
            exit(2)
        }
        let seconds = CommandLine.arguments.count > 2
            ? max(3, Int(CommandLine.arguments[2]) ?? 12)
            : 12
        let pattern = CommandLine.arguments.count > 3
            ? ProbePattern(rawValue: CommandLine.arguments[3]) ?? .moving
            : .moving
        let sides: [TileSide] = mode == "dual" || mode == "mixed" || mode == "dualAve"
            ? [.left, .right]
            : [.left]
        let frames = (0..<8).map {
            makeFrame(seed: UInt8($0 * 29), pattern: pattern)
        }
        let encoders = try Dictionary(
            uniqueKeysWithValues: sides.map { side in
                let backend: SplitTileEncoderBackend = mode == "singleAve" || mode == "dualAve"
                    || (mode == "mixed" && side == .right)
                    ? .ave
                    : .rtvc
                return (
                    side,
                    try VideoToolboxTileEncoder(
                        side: side,
                        bitrate: 30_000_000,
                        backend: backend
                    )
                )
            }
        )
        let counters = ProbeCounters()
        let slots = DispatchSemaphore(value: 5)
        let callbacks = DispatchGroup()
        var admissionDrops = 0
        let framePeriodNs: UInt64 = 1_000_000_000 / 60
        let startedNs = DispatchTime.now().uptimeNanoseconds
        let targetFrames = seconds * 60

        for frameIndex in 0..<targetFrames {
            waitUntil(startedNs + UInt64(frameIndex) * framePeriodNs)
            guard slots.wait(timeout: .now()) == .success else {
                admissionDrops += 1
                continue
            }
            let pair = PairCompletion(remaining: sides.count) { slots.signal() }
            for side in sides {
                let submittedNs = DispatchTime.now().uptimeNanoseconds
                counters.recordSubmission(side: side)
                callbacks.enter()
                encoders[side]!.submit(
                    TileEncodeRequest(
                        side: side,
                        frameSequence: UInt64(frameIndex),
                        pts: CMTime(value: CMTimeValue(frameIndex), timescale: 60),
                        duration: CMTime(value: 1, timescale: 60),
                        captureNs: submittedNs,
                        captureWallMs: UInt64(Date().timeIntervalSince1970 * 1_000),
                        recoveryGeneration: 0,
                        forceKeyframe: frameIndex == 0,
                        pixelBuffer: frames[frameIndex % frames.count]
                    )
                ) { result in
                    counters.record(side: side, submittedNs: submittedNs, result: result)
                    callbacks.leave()
                    pair.complete()
                }
            }
        }

        let submissionEndedNs = DispatchTime.now().uptimeNanoseconds
        let completed = callbacks.wait(timeout: .now() + 10) == .success
        let finishedNs = DispatchTime.now().uptimeNanoseconds
        encoders.values.forEach { $0.invalidate() }
        let elapsedSeconds = Double(finishedNs - startedNs) / 1_000_000_000
        var report: [String: Any] = [
            "mode": mode,
            "pattern": pattern.rawValue,
            "targetFps": 60,
            "targetFrames": targetFrames,
            "elapsedSeconds": elapsedSeconds,
            "admissionDrops": admissionDrops,
            "callbacksCompleted": completed,
        ]
        for side in sides {
            var sideReport = counters.snapshot(
                side: side,
                startedNs: startedNs,
                submissionEndedNs: submissionEndedNs,
                elapsedSeconds: elapsedSeconds
            )
            sideReport["backend"] = encoders[side]!.backend.rawValue
            sideReport["encoderID"] = encoders[side]!.encoderID
            report[side == .left ? "left" : "right"] = sideReport
        }
        let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    }
}
