import Foundation
import AppKit
import CoreGraphics

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

/// 활성 디스플레이 ID를 메인 디스플레이 우선으로 정렬해 돌려준다.
func sortedActiveDisplayIDs() -> [CGDirectDisplayID] {
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

func stableDisplaySourceID(_ displayID: CGDirectDisplayID) -> String? {
    guard let uuid = CGDisplayCreateUUIDFromDisplayID(displayID)?.takeRetainedValue(),
          let value = CFUUIDCreateString(nil, uuid) else { return nil }
    return "macos:display:\(value)".lowercased()
}

func coreGraphicsCatalogJSON() -> String? {
    let sortedIDs = sortedActiveDisplayIDs()
    guard !sortedIDs.isEmpty else {
        return nil
    }
    let entries: [[String: Any]] = sortedIDs.enumerated().map { index, displayID in
        let pixelSize = nativePixelSize(for: displayID)
        return [
            "index": index,
            "sourceId": stableDisplaySourceID(displayID) as Any? ?? NSNull(),
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
    sortedActiveDisplayIDs()
}
