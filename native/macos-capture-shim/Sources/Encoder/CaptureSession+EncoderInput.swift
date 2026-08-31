import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Security
import Darwin
import OSLog

extension CaptureSession {
     func encoderInputBuffer(
        for source: CVPixelBuffer
    ) -> (buffer: CVPixelBuffer?, status: OSStatus) {
        stateLock.lock()
        let mode = EncoderMode(rawValue: encoderMode)
        let aveRetryStagingEnabled = aveInputStagingEnabled
        let experiment = appliedEncoderExperimentValue
        stateLock.unlock()
        guard let mode else {
            return (source, noErr)
        }
        let inputPolicy = encoderInputSurfacePolicy(
            experiment: experiment,
            mode: mode,
            width: outWidth,
            height: outHeight,
            captureBackend: backend.rawValue,
            aveRetryStagingEnabled: aveRetryStagingEnabled
        )
        guard inputPolicy != .direct else { return (source, noErr) }

        if experiment == .encoderPool {
            guard let compressionSession = session,
                  let pool = VTCompressionSessionGetPixelBufferPool(compressionSession) else {
                return (nil, kVTInvalidSessionErr)
            }
            encoderInputPool = pool
        } else if encoderInputPool == nil {
            let poolAttributes: [String: Any] = [
                kCVPixelBufferPoolMinimumBufferCountKey as String:
                    configuredEncodeInFlightLimit + 1,
            ]
            let pixelAttributes: [String: Any] = [
                kCVPixelBufferPixelFormatTypeKey as String: encoderSourcePixelFormat(),
                kCVPixelBufferWidthKey as String: Int(outWidth),
                kCVPixelBufferHeightKey as String: Int(outHeight),
                kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
                kCVPixelBufferMetalCompatibilityKey as String: true,
            ]
            var pool: CVPixelBufferPool?
            let poolStatus = CVPixelBufferPoolCreate(
                kCFAllocatorDefault,
                poolAttributes as CFDictionary,
                pixelAttributes as CFDictionary,
                &pool
            )
            guard poolStatus == kCVReturnSuccess, let pool else {
                return (nil, poolStatus)
            }
            encoderInputPool = pool
        }

        if inputPolicy == .pixelTransfer, pixelTransferSession == nil {
            var transferSession: VTPixelTransferSession?
            let transferCreateStatus = VTPixelTransferSessionCreate(
                allocator: kCFAllocatorDefault,
                pixelTransferSessionOut: &transferSession
            )
            guard transferCreateStatus == noErr, let transferSession else {
                return (nil, transferCreateStatus)
            }
            _ = VTSessionSetProperty(
                transferSession,
                key: kVTPixelTransferPropertyKey_RealTime,
                value: true as CFBoolean
            )
            pixelTransferSession = transferSession
        }

        guard let encoderInputPool else {
            return (nil, kVTInvalidSessionErr)
        }
        var destination: CVPixelBuffer?
        let allocationStatus = CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            encoderInputPool,
            &destination
        )
        guard allocationStatus == kCVReturnSuccess, let destination else {
            return (nil, allocationStatus)
        }
        let transferStatus: OSStatus
        switch inputPolicy {
        case .direct:
            transferStatus = noErr
        case .pixelTransfer:
            guard let pixelTransferSession else {
                return (nil, kVTInvalidSessionErr)
            }
            transferStatus = VTPixelTransferSessionTransferImage(
                pixelTransferSession,
                from: source,
                to: destination
            )
        case .cpuCopy:
            transferStatus = copyPlanarPixelBufferPlanes(
                from: source,
                to: destination
            )
        }
        guard transferStatus == noErr else {
            return (nil, transferStatus)
        }
        if inputPolicy == .pixelTransfer {
            CVBufferPropagateAttachments(source, destination)
        }
        return (destination, noErr)
    }
}

