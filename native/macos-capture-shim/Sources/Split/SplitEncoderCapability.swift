import Foundation
import CoreMedia
import CoreVideo
import Darwin

private let aveH264EncoderID = "com.apple.videotoolbox.videoencoder.ave.avc"

func verifiedDualAveEncoderPair(
    leftEncoderID: String,
    leftHardware: Bool,
    rightEncoderID: String,
    rightHardware: Bool
) -> Bool {
    leftHardware
        && rightHardware
        && leftEncoderID == aveH264EncoderID
        && rightEncoderID == aveH264EncoderID
}

func verifiedDualAveConcurrentProbe(
    pairVerified: Bool,
    leftEncoded: Bool,
    rightEncoded: Bool
) -> Bool {
    pairVerified && leftEncoded && rightEncoded
}

private final class DualAveProbeCompletionState {
    private let lock = NSLock()
    private var successfulSides: Set<TileSide> = []

    func record(_ side: TileSide, result: Result<TileEncodedSample, TileEncoderError>) {
        guard case .success = result else { return }
        lock.lock()
        successfulSides.insert(side)
        lock.unlock()
    }

    func succeeded(_ side: TileSide) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return successfulSides.contains(side)
    }
}

private let dualAveCapabilityLock = NSLock()
private var cachedDualAveCapability: Bool?

private func makeDualAveProbePixelBuffer() -> CVPixelBuffer? {
    var pixelBuffer: CVPixelBuffer?
    let status = CVPixelBufferCreate(
        kCFAllocatorDefault,
        1_920,
        2_160,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        [
            kCVPixelBufferMetalCompatibilityKey as String: true,
            kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
        ] as CFDictionary,
        &pixelBuffer
    )
    guard status == kCVReturnSuccess, let pixelBuffer else { return nil }
    guard CVPixelBufferLockBaseAddress(pixelBuffer, []) == kCVReturnSuccess else {
        return nil
    }
    defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }
    for plane in 0..<CVPixelBufferGetPlaneCount(pixelBuffer) {
        guard let base = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, plane) else {
            return nil
        }
        let byteCount = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, plane)
            * CVPixelBufferGetHeightOfPlane(pixelBuffer, plane)
        memset(base, plane == 0 ? 16 : 128, byteCount)
    }
    return pixelBuffer
}

private func probeDualAveConcurrentAllocation(
    left: VideoToolboxTileEncoder,
    right: VideoToolboxTileEncoder
) -> Bool {
    guard let leftBuffer = makeDualAveProbePixelBuffer(),
          let rightBuffer = makeDualAveProbePixelBuffer() else {
        return false
    }
    let group = DispatchGroup()
    let completionState = DualAveProbeCompletionState()
    let duration = CMTime(value: 1, timescale: 60)
    for (encoder, side, pixelBuffer) in [
        (left, TileSide.left, leftBuffer),
        (right, TileSide.right, rightBuffer),
    ] {
        group.enter()
        encoder.submit(
            TileEncodeRequest(
                side: side,
                frameSequence: 0,
                pts: .zero,
                duration: duration,
                captureNs: 0,
                captureWallMs: 0,
                recoveryGeneration: 0,
                forceKeyframe: true,
                pixelBuffer: pixelBuffer
            )
        ) { result in
            completionState.record(side, result: result)
            group.leave()
        }
    }
    let leftComplete = left.completeFrames() == noErr
    let rightComplete = right.completeFrames() == noErr
    let callbacksCompleted = group.wait(timeout: .now() + 2) == .success
    return leftComplete
        && rightComplete
        && callbacksCompleted
        && completionState.succeeded(.left)
        && completionState.succeeded(.right)
}

func dualAveTileEncoderPairAvailable() -> Bool {
    dualAveCapabilityLock.lock()
    defer { dualAveCapabilityLock.unlock() }
    if let cachedDualAveCapability { return cachedDualAveCapability }

    var left: VideoToolboxTileEncoder?
    var right: VideoToolboxTileEncoder?
    defer {
        left?.invalidate()
        right?.invalidate()
    }

    do {
        left = try VideoToolboxTileEncoder(
            side: .left,
            bitrate: 30_000_000,
            backend: .ave
        )
        right = try VideoToolboxTileEncoder(
            side: .right,
            bitrate: 30_000_000,
            backend: .ave
        )
    } catch {
        NSLog("Leftcar dual AVE capability probe failed: %@", String(describing: error))
        cachedDualAveCapability = false
        return false
    }

    guard let left, let right else {
        cachedDualAveCapability = false
        return false
    }
    let pairVerified = verifiedDualAveEncoderPair(
        leftEncoderID: left.encoderID,
        leftHardware: left.hardwareAccelerated,
        rightEncoderID: right.encoderID,
        rightHardware: right.hardwareAccelerated
    )
    let concurrentProbeSucceeded = pairVerified
        && probeDualAveConcurrentAllocation(left: left, right: right)
    let available = verifiedDualAveConcurrentProbe(
        pairVerified: pairVerified,
        leftEncoded: concurrentProbeSucceeded,
        rightEncoded: concurrentProbeSucceeded
    )
    cachedDualAveCapability = available
    return available
}
