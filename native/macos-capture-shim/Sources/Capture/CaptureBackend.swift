import Foundation
import AppKit
import CoreGraphics

 enum CaptureBackendKind: String {
    case screenCaptureKit
    case cgDisplayStream

    static func parse(_ value: String?) -> CaptureBackendKind? {
        guard let value else { return .screenCaptureKit }
        switch value.lowercased() {
        case "sck", "screencapturekit": return .screenCaptureKit
        case "cg", "cgdisplaystream": return .cgDisplayStream
        default: return nil
        }
    }
}

 enum MediaTransportKind: String {
    case udp
    case tcp
    case usb
    case adbTcp

    var usesTCP: Bool {
        self == .tcp || self == .usb || self == .adbTcp
    }

    static func parse(_ value: String?) -> MediaTransportKind? {
        guard let value else { return .udp }
        switch value.lowercased() {
        case "udp": return .udp
        case "tcp", "wifitcp", "wifi-tcp": return .tcp
        case "usb", "aoap": return .usb
        case "adbtcp", "adb-tcp": return .adbTcp
        default: return nil
        }
    }
}

 enum StreamContentMode: String {
    case interactive
    case video

    static func parse(_ value: String?) -> StreamContentMode? {
        guard let value else { return .interactive }
        switch value.lowercased() {
        case "interactive", "latency": return .interactive
        case "video", "movie": return .video
        default: return nil
        }
    }
}

struct NativePixelSize: Equatable {
    let width: Int
    let height: Int
}

struct NativePixelModeCandidate {
    let logicalWidth: Int
    let logicalHeight: Int
    let pixelWidth: Int
    let pixelHeight: Int
}

private struct ManagedDisplayMode {
    let displayUUID: String
    let generation: UInt64
    let logicalWidth: Int
    let logicalHeight: Int
    let pixelWidth: Int
    let pixelHeight: Int
}

private let managedDisplayModesLock = NSLock()
private var managedDisplayModes: [CGDirectDisplayID: ManagedDisplayMode] = [:]

func registerManagedDisplayMode(
    displayID: CGDirectDisplayID,
    generation: UInt64,
    logicalWidth: Int,
    logicalHeight: Int,
    pixelWidth: Int,
    pixelHeight: Int
) -> Bool {
    guard displayID != 0, generation != 0, logicalWidth > 0, logicalHeight > 0,
          pixelWidth >= logicalWidth, pixelHeight >= logicalHeight else { return false }
    guard let uuid = CGDisplayCreateUUIDFromDisplayID(displayID)?.takeRetainedValue(),
          let uuidString = CFUUIDCreateString(nil, uuid) as String? else { return false }
    managedDisplayModesLock.lock()
    managedDisplayModes[displayID] = ManagedDisplayMode(
        displayUUID: uuidString,
        generation: generation,
        logicalWidth: logicalWidth,
        logicalHeight: logicalHeight,
        pixelWidth: pixelWidth,
        pixelHeight: pixelHeight
    )
    managedDisplayModesLock.unlock()
    return true
}

func clearManagedDisplayMode(displayID: CGDirectDisplayID, generation: UInt64) {
    managedDisplayModesLock.lock()
    if managedDisplayModes[displayID]?.generation == generation {
        managedDisplayModes.removeValue(forKey: displayID)
    }
    managedDisplayModesLock.unlock()
}

func managedPixelSize(
    for displayID: CGDirectDisplayID,
    logicalWidth: Int,
    logicalHeight: Int
) -> NativePixelSize? {
    managedDisplayModesLock.lock()
    let mode = managedDisplayModes[displayID]
    managedDisplayModesLock.unlock()
    guard let mode else { return nil }
    guard CGDisplayIsActive(displayID) != 0,
          let uuid = CGDisplayCreateUUIDFromDisplayID(displayID)?.takeRetainedValue(),
          (CFUUIDCreateString(nil, uuid) as String?) == mode.displayUUID else { return nil }
    if logicalWidth == mode.logicalWidth, logicalHeight == mode.logicalHeight {
        return NativePixelSize(width: mode.pixelWidth, height: mode.pixelHeight)
    }
    if logicalWidth == mode.logicalHeight, logicalHeight == mode.logicalWidth {
        return NativePixelSize(width: mode.pixelHeight, height: mode.pixelWidth)
    }
    return nil
}

func nativePixelModeCandidate(
    logicalWidth: Int,
    logicalHeight: Int,
    backingWidth: Double,
    backingHeight: Double
) -> NativePixelModeCandidate? {
    guard logicalWidth > 0,
          logicalHeight > 0,
          backingWidth.isFinite,
          backingHeight.isFinite,
          backingWidth > 0,
          backingHeight > 0,
          backingWidth.rounded(.towardZero) == backingWidth,
          backingHeight.rounded(.towardZero) == backingHeight,
          backingWidth < Double(Int.max),
          backingHeight < Double(Int.max) else {
        return nil
    }

    let pixelWidth = Int(backingWidth)
    let pixelHeight = Int(backingHeight)
    guard pixelWidth >= logicalWidth, pixelHeight >= logicalHeight else {
        return nil
    }
    let (pixelWidthRatio, pixelWidthRatioOverflow) = pixelWidth
        .multipliedReportingOverflow(by: logicalHeight)
    let (pixelHeightRatio, pixelHeightRatioOverflow) = pixelHeight
        .multipliedReportingOverflow(by: logicalWidth)
    guard !pixelWidthRatioOverflow,
          !pixelHeightRatioOverflow,
          pixelWidthRatio == pixelHeightRatio else {
        return nil
    }
    return NativePixelModeCandidate(
        logicalWidth: logicalWidth,
        logicalHeight: logicalHeight,
        pixelWidth: pixelWidth,
        pixelHeight: pixelHeight
    )
}

func nativePixelSize(
    logicalWidth: Int,
    logicalHeight: Int,
    currentMode: NativePixelModeCandidate?,
    candidates: [NativePixelModeCandidate]
) -> NativePixelSize {
    let fallback = NativePixelSize(
        width: max(0, logicalWidth),
        height: max(0, logicalHeight)
    )
    guard logicalWidth > 0, logicalHeight > 0 else {
        return fallback
    }

    let (fallbackArea, fallbackOverflow) = logicalWidth.multipliedReportingOverflow(
        by: logicalHeight
    )
    guard !fallbackOverflow else {
        return fallback
    }

    var selected = fallback
    var selectedArea = fallbackArea
    for candidate in [currentMode].compactMap({ $0 }) + candidates {
        guard candidate.logicalWidth == logicalWidth,
              candidate.logicalHeight == logicalHeight,
              candidate.pixelWidth >= logicalWidth,
              candidate.pixelHeight >= logicalHeight else {
            continue
        }
        let (pixelWidthRatio, pixelWidthRatioOverflow) = candidate.pixelWidth
            .multipliedReportingOverflow(by: logicalHeight)
        let (pixelHeightRatio, pixelHeightRatioOverflow) = candidate.pixelHeight
            .multipliedReportingOverflow(by: logicalWidth)
        guard !pixelWidthRatioOverflow,
              !pixelHeightRatioOverflow,
              pixelWidthRatio == pixelHeightRatio else {
            continue
        }
        let (candidateArea, overflow) = candidate.pixelWidth.multipliedReportingOverflow(
            by: candidate.pixelHeight
        )
        guard !overflow, candidateArea > selectedArea else {
            continue
        }
        selected = NativePixelSize(
            width: candidate.pixelWidth,
            height: candidate.pixelHeight
        )
        selectedArea = candidateArea
    }
    return selected
}

 func nativePixelModeCandidate(_ mode: CGDisplayMode) -> NativePixelModeCandidate {
    NativePixelModeCandidate(
        logicalWidth: mode.width,
        logicalHeight: mode.height,
        pixelWidth: mode.pixelWidth,
        pixelHeight: mode.pixelHeight
    )
}

 func appKitBackingModeCandidate(
    for displayID: CGDirectDisplayID,
    logicalWidth: Int,
    logicalHeight: Int
) -> NativePixelModeCandidate? {
    let readCandidate: @MainActor () -> NativePixelModeCandidate? = {
        let screenNumberKey = NSDeviceDescriptionKey("NSScreenNumber")
        guard let screen = NSScreen.screens.first(where: { screen in
            guard let screenNumber = screen.deviceDescription[screenNumberKey] as? NSNumber else {
                return false
            }
            return CGDirectDisplayID(screenNumber.uint32Value) == displayID
        }) else {
            return nil
        }
        let backingRect = screen.convertRectToBacking(screen.frame)
        return nativePixelModeCandidate(
            logicalWidth: logicalWidth,
            logicalHeight: logicalHeight,
            backingWidth: Double(backingRect.width),
            backingHeight: Double(backingRect.height)
        )
    }
    if Thread.isMainThread {
        return MainActor.assumeIsolated {
            readCandidate()
        }
    }
    return DispatchQueue.main.sync {
        readCandidate()
    }
}

 func nativePixelSize(for displayID: CGDirectDisplayID) -> (width: Int, height: Int) {
    let logicalWidth = CGDisplayPixelsWide(displayID)
    let logicalHeight = CGDisplayPixelsHigh(displayID)
    let currentMode = CGDisplayCopyDisplayMode(displayID).map(nativePixelModeCandidate)
    if currentMode == nil,
       let managed = managedPixelSize(
           for: displayID,
           logicalWidth: logicalWidth,
           logicalHeight: logicalHeight
       ) {
        return (managed.width, managed.height)
    }
    let options = [kCGDisplayShowDuplicateLowResolutionModes as String: true] as CFDictionary
    let matchingModes = (CGDisplayCopyAllDisplayModes(displayID, options) as? [CGDisplayMode] ?? [])
        .filter { mode in
            mode.width == logicalWidth && mode.height == logicalHeight
        }
        .map(nativePixelModeCandidate)
    let appKitMode = appKitBackingModeCandidate(
        for: displayID,
        logicalWidth: logicalWidth,
        logicalHeight: logicalHeight
    )
    let selected = nativePixelSize(
        logicalWidth: logicalWidth,
        logicalHeight: logicalHeight,
        currentMode: currentMode,
        candidates: matchingModes + [appKitMode].compactMap { $0 }
    )
    return (selected.width, selected.height)
}

 func coreGraphicsCatalogJSON() -> String? {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else {
        return nil
    }
    var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(count))
    var filled = count
    guard CGGetActiveDisplayList(count, &displayIDs, &filled) == .success else {
        return nil
    }
    let mainDisplayID = CGMainDisplayID()
    let sortedIDs = displayIDs.prefix(Int(filled)).sorted { lhs, rhs in
        if lhs == mainDisplayID { return true }
        if rhs == mainDisplayID { return false }
        return lhs < rhs
    }
    let entries: [[String: Any]] = sortedIDs.enumerated().map { index, displayID in
        let pixelSize = nativePixelSize(for: displayID)
        return [
            "index": index,
            "name": "Display \(index)",
            "width": pixelSize.width,
            "height": pixelSize.height,
        ]
    }
    guard let data = try? JSONSerialization.data(withJSONObject: entries),
          let json = String(data: data, encoding: .utf8) else {
        return nil
    }
    return json
}

 func activeDisplayIDs() -> [CGDirectDisplayID] {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else {
        return []
    }
    var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(count))
    var filled = count
    guard CGGetActiveDisplayList(count, &displayIDs, &filled) == .success else {
        return []
    }
    let mainDisplayID = CGMainDisplayID()
    return displayIDs.prefix(Int(filled)).sorted { lhs, rhs in
        if lhs == mainDisplayID { return true }
        if rhs == mainDisplayID { return false }
        return lhs < rhs
    }
}
