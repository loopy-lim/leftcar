// CaptureShim: real macOS screen capture -> H.264/HEVC -> low-latency UDP, as a C-ABI dylib.
//
// Path (docs/02 §4, H16-H18; rebuild design 2026-08-18):
//   SCShareableContent (selected display) -> SCStream (420v IOSurface frames)
//   -> VTCompressionSession (H.264/HEVC, low-latency, no B-frames)
//   -> MTU-bounded UDP datagrams (CFG/CF2 + fragmented access units).
//
// v2: handle table — multiple concurrent sessions (multi-display /
// multi-viewer), parameterized fps/bitrate, JSON stats, auto-stop on
// viewer disconnect.

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
        guard type == .screen else {
            if type == .audio, let session {
                session.audioQueue.async { [weak session] in
                    session?.handleAudioSampleBuffer(sampleBuffer)
                }
            }
            return
        }
        session?.handleFrame(sampleBuffer)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        session?.markStopped("Stream stopped: \(error.localizedDescription)")
    }
}

// MARK: - Session (one per active stream)

 typealias CGFrameHandler = @convention(block) (
    CGDisplayStreamFrameStatus,
    UInt64,
    IOSurfaceRef?,
    CGDisplayStreamUpdate?
) -> Void
 typealias CGStreamCreateFn = @convention(c) (
    CGDirectDisplayID,
    Int,
    Int,
    Int32,
    CFDictionary?,
    DispatchQueue,
    CGFrameHandler
) -> Unmanaged<CGDisplayStream>?
 typealias CGStreamStartFn = @convention(c) (CGDisplayStream) -> CGError
 typealias CGStreamStopFn = @convention(c) (CGDisplayStream) -> CGError

/// `CGDisplayStream` was obsoleted by the macOS 15 SDK. Keep it behind an
/// explicitly selected, display-only compatibility backend loaded at runtime;
/// ScreenCaptureKit remains the supported default and no unavailable API is
/// referenced directly by Swift.
 final class LegacyCGDisplayStreamAPI {
    let library: UnsafeMutableRawPointer
    let create: CGStreamCreateFn
    let start: CGStreamStartFn
    let stop: CGStreamStopFn
    let showCursorKey: NSString
    let minimumFrameTimeKey: NSString
    let queueDepthKey: NSString

    init?() {
        guard let library = dlopen(
            "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
            RTLD_NOW | RTLD_LOCAL
        ),
        let createSymbol = dlsym(library, "CGDisplayStreamCreateWithDispatchQueue"),
        let startSymbol = dlsym(library, "CGDisplayStreamStart"),
        let stopSymbol = dlsym(library, "CGDisplayStreamStop"),
        let showCursorSymbol = dlsym(library, "kCGDisplayStreamShowCursor"),
        let minimumFrameTimeSymbol = dlsym(library, "kCGDisplayStreamMinimumFrameTime"),
        let queueDepthSymbol = dlsym(library, "kCGDisplayStreamQueueDepth"),
        let showCursorKey = showCursorSymbol
            .assumingMemoryBound(to: Optional<CFString>.self)
            .pointee,
        let minimumFrameTimeKey = minimumFrameTimeSymbol
            .assumingMemoryBound(to: Optional<CFString>.self)
            .pointee,
        let queueDepthKey = queueDepthSymbol
            .assumingMemoryBound(to: Optional<CFString>.self)
            .pointee else {
            return nil
        }
        self.library = library
        self.create = unsafeBitCast(createSymbol, to: CGStreamCreateFn.self)
        self.start = unsafeBitCast(startSymbol, to: CGStreamStartFn.self)
        self.stop = unsafeBitCast(stopSymbol, to: CGStreamStopFn.self)
        self.showCursorKey = unsafeBitCast(showCursorKey, to: NSString.self)
        self.minimumFrameTimeKey = unsafeBitCast(minimumFrameTimeKey, to: NSString.self)
        self.queueDepthKey = unsafeBitCast(queueDepthKey, to: NSString.self)
    }

    deinit {
        dlclose(library)
    }
}

 struct PendingCaptureFrame {
    let pixelBuffer: CVPixelBuffer
    let pts: CMTime
    let duration: CMTime
    let callbackNs: UInt64
    let captureWallMs: UInt64
}


