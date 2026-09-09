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

@_cdecl("leftcar_capture_has_persistent_access_v1")
public func leftcarCaptureHasPersistentAccessV1() -> Int32 {
    hasPersistentContentCaptureEntitlement() ? 1 : 0
}

@_cdecl("leftcar_capture_list_displays")
public func leftcarCaptureListDisplays() -> UnsafeMutablePointer<CChar> {
    guard hasScreenCaptureAccess() else {
        setLastError("screen-recording permission is not granted to Leftcar Host")
        return UnsafeMutablePointer<CChar>(strdup("[]"))
    }
    // Source cards only need stable display metadata. Using CoreGraphics here
    // keeps catalog refresh read-only; ScreenCaptureKit consent is requested
    // exactly once, when the viewer starts a stream.
    if let catalog = coreGraphicsCatalogJSON() {
        setLastError("")
        return UnsafeMutablePointer<CChar>(strdup(catalog))
    }
    setLastError("CoreGraphics returned no active displays")
    return UnsafeMutablePointer<CChar>(strdup("[]"))
}



/// The single start ABI is `leftcar_capture_start_v8` (see
/// CaptureShim+UdpStabilityExports.swift): identical parameter set to the
/// retired v2..v7 sequence plus the trailing viewer-generated 32-byte media
/// key. There is no plaintext media path anymore, so legacy start exports are
/// removed rather than kept as unsealed fallbacks.
 func startCaptureSession(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backend: CaptureBackendKind,
    mediaTransport: MediaTransportKind = .udp,
    contentMode: StreamContentMode = .interactive,
    encoderExperiment: EncoderExperiment = .auto,
    udpStability: AppliedUdpStability = .legacy,
    mediaKey: Data
) -> UInt32 {
    guard hasScreenCaptureAccess() else {
        setLastError("screen-recording permission is not granted to Leftcar Host")
        return 0
    }
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
        targetLabel: "\(ipStr):\(port)",
        width: width,
        height: height,
        fps: fps,
        backend: backend,
        mediaTransport: mediaTransport,
        contentMode: contentMode,
        requestedEncoderExperiment: encoderExperiment,
        udpStability: udpStability,
        mediaKey: mediaKey
    )

    // Register before setup: the stream configuration consults the registry
    // to decide system-audio ownership, so the session (and its handle) must
    // already be visible when setupScreenCaptureKit builds that
    // configuration. Every failure path below removes the entry again.
    let handle = withRegistry { reg in
        let h = nextHandle
        nextHandle += 1
        session.sessionHandle = h
        reg[h] = session
        return h
    }

    // Establish the media socket first. Capture callbacks can then be accepted
    // immediately without losing the initial CFG/IDR while the viewer listener
    // is still racing to bind its port.
    let connected = session.connectSocket()
    guard connected else {
        removeFromRegistry(handle)
        return 0
    }

    let started: Bool
    switch backend {
    case .screenCaptureKit:
        // The persistent-content-capture entitlement grants VNC-style
        // no-reconsent capture; ordinary Screen Recording consent supports
        // the same SCK path — including its system-audio plane. Only a Mac
        // with neither consent falls through to the error below.
        guard hasPersistentContentCaptureEntitlement() || hasScreenCaptureAccess() else {
            setLastError(
                "screen-recording permission is not granted to Leftcar Host"
            )
            removeFromRegistry(handle)
            session.stop()
            return 0
        }
        let displayIDs = activeDisplayIDs()
        guard Int(displayIndex) < displayIDs.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displayIDs.count) displays)")
            removeFromRegistry(handle)
            session.stop()
            return 0
        }
        // Approved VNC-style builds reconnect directly to the requested
        // display after Screen Recording permission has been granted. Builds
        // without approval never show a picker; the Host advertises the
        // automatic CGDisplayStream backend instead.
        let selection = requestPersistentDisplayFilter(
            displayID: displayIDs[Int(displayIndex)],
            timeout: 15
        )
        guard let filter = selection.filter else {
            setLastError(selection.error ?? "screen capture returned no display")
            removeFromRegistry(handle)
            session.stop()
            return 0
        }
        started = session.setupScreenCaptureKit(filter: filter)
    case .cgDisplayStream:
        let displayIDs = activeDisplayIDs()
        guard Int(displayIndex) < displayIDs.count else {
            setLastError("displayIndex \(displayIndex) out of range (\(displayIDs.count) displays)")
            removeFromRegistry(handle)
            session.stop()
            return 0
        }
        started = session.setupCGDisplayStream(displayID: displayIDs[Int(displayIndex)])
    }
    guard started else {
        removeFromRegistry(handle)
        session.stop()
        return 0
    }

    session.startPerformanceLogging()
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
    transferSystemAudioOwnership(afterRemoving: session)
    return 0
}

/// Stop with a viewer-visible reason. `reasonCode` uses the LCT1 wire codes:
/// 2 = host operator forced the stop, 3 = ordinary stop/shutdown. The viewer
/// closes its window on receipt instead of waiting for a media timeout.
@_cdecl("leftcar_capture_stop_v3")
public func leftcarCaptureStopV3(handle: UInt32, reasonCode: Int32) -> Int32 {
    let session = withRegistry { reg in
        reg.removeValue(forKey: handle)
    }
    guard let session else {
        setLastError("no such handle \(handle)")
        return 1
    }
    if reasonCode > 0 {
        session.notifyViewerTermination(
            code: UInt8(clamping: reasonCode),
            reason: reasonCode == 2 ? "host operator stopped the stream" : "stream stopped"
        )
        transferSystemAudioOwnership(afterRemoving: session)
        return 0
    }
    session.stop()
    transferSystemAudioOwnership(afterRemoving: session)
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

@_cdecl("leftcar_capture_input_permission_v1")
public func leftcarCaptureInputPermissionV1() -> Int32 {
    CGPreflightPostEventAccess() ? 1 : 0
}

@_cdecl("leftcar_capture_screen_permission_v1")
public func leftcarCaptureScreenPermissionV1() -> Int32 {
    hasScreenCaptureAccess() ? 1 : 0
}

@_cdecl("leftcar_capture_request_input_permission_v1")
public func leftcarCaptureRequestInputPermissionV1() -> Int32 {
    CGRequestPostEventAccess() ? 1 : 0
}

@_cdecl("leftcar_capture_set_input_enabled_v1")
public func leftcarCaptureSetInputEnabledV1(handle: UInt32, enabled: Int32) -> Int32 {
    let session = withRegistry { $0[handle] }
    guard let session else {
        setLastError("input session handle not found: \(handle)")
        return -1
    }
    if !session.setInputEnabled(enabled != 0) {
        setLastError("Accessibility input permission is not granted to Leftcar Host")
        return -2
    }
    setLastError("")
    return 0
}

@_cdecl("leftcar_capture_set_quality_v1")
public func leftcarCaptureSetQualityV1(handle: UInt32, qualityPercent: Int32) -> Int32 {
    guard qualityPercent == 0 || (25...50).contains(qualityPercent) else {
        setLastError("quality override must be 0 (auto) or between 25 and 50")
        return -2
    }
    let session = withRegistry { $0[handle] }
    guard let session else {
        setLastError("quality session handle not found: \(handle)")
        return -1
    }
    let quality = manualQualityHintFromSliderPercent(qualityPercent)
    if let error = session.setQualityOverride(quality) {
        setLastError(error)
        return -3
    }
    setLastError("")
    return 0
}
