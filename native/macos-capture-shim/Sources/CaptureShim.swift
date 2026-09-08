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

// MARK: - C ABI surface (v2, handle-based)

 let registryLock = NSLock()
 var registry: [UInt32: CaptureSession] = [:]
 var nextHandle: UInt32 = 1
 let leftcarPerformanceLogger = Logger(
    subsystem: "leftcar.ll3.kr",
    category: "performance"
)


 func withRegistry<T>(_ body: (inout [UInt32: CaptureSession]) -> T) -> T {
    registryLock.lock()
    defer { registryLock.unlock() }
    return body(&registry)
}

/// Remove a session from the handle table (startup failure and stop paths).
/// The removed session, when present, is returned by `withRegistry`; the
/// stop paths already hold their own reference, so discard it here.
 func removeFromRegistry(_ handle: UInt32) {
    _ = withRegistry { $0.removeValue(forKey: handle) }
}
 var lastErrorUTF8: UnsafeMutablePointer<CChar> = UnsafeMutablePointer<CChar>(strdup(""))

 func setLastError(_ message: String) {
    free(lastErrorUTF8)
    lastErrorUTF8 = UnsafeMutablePointer<CChar>(strdup(message))
}

 func hasScreenCaptureAccess() -> Bool {
    // Do not call CGRequestScreenCaptureAccess from the control request. On a
    // release bundle with a new TCC identity macOS may wait for user input
    // while this call is running, which leaves getCatalog stuck on
    // "loading". Permission must be granted explicitly in System Settings;
    // catalog/start then fail immediately with a useful error until it is.
    // CGPreflightScreenCaptureAccess is a non-UI query; keeping it on the
    // control caller avoids synchronously waiting for Tauri's AppKit thread.
    CGPreflightScreenCaptureAccess()
}

 let persistentContentCaptureEntitlement =
    "com.apple.developer.persistent-content-capture" as CFString

/// Apple's persistent-content-capture entitlement is restricted to approved
/// remote-desktop/VNC apps. Read the entitlement from the running task instead
/// of trusting a build flag: an unsigned development binary must never claim
/// that it can bypass the system picker.
 func hasPersistentContentCaptureEntitlement() -> Bool {
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

 final class PersistentDisplayFilterRequest: @unchecked Sendable {
     let lock = NSLock()
     let completed = DispatchSemaphore(value: 0)
     var selectedFilter: SCContentFilter?
     var failure: String?

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

 func requestPersistentDisplayFilter(
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
