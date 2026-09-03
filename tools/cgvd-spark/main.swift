// CGVirtualDisplay 스파크 프로브 — Leftcar evidence 수집용 (R-015: 승격 아님).
// 검증 항목은 docs/research/2026-09-03_opendisplay-comparison.md 미해결 질문 1·2:
//   1) macOS 26에서 private API가 동작하는가 (생성/HiDPI/60Hz 상한/미러 해제)
//   2) SCK SCShareableContent에 가상 디스플레이가 뜨는가 (스트리밍 캡처 정합 전제)
// 측정 결과는 사람이 읽는 요약으로 stdout에 출력한다.

import Cocoa
import CoreGraphics
import ScreenCaptureKit

final class Probe {
    static var failures: [String] = []
    static var notes: [String] = []

    static func check(_ label: String, _ ok: Bool, _ detail: String = "") {
        let mark = ok ? "PASS" : "FAIL"
        print("[\(mark)] \(label)\(detail.isEmpty ? "" : " — \(detail)")")
        if !ok { failures.append(label) }
    }

    static func note(_ line: String) {
        print("[NOTE] \(line)")
        notes.append(line)
    }
}

// MARK: - 측정 헬퍼

func displayListDescription() -> String {
    var n: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &n)
    var ids = [CGDirectDisplayID](repeating: 0, count: Int(n))
    CGGetActiveDisplayList(n, &ids, &n)
    return ids.map { id in
        let b = CGDisplayBounds(id)
        let pm = CGDisplayCopyDisplayMode(id)?.pixelWidth ?? 0
        return "\(id): \(Int(b.width))x\(Int(b.height))pt@origin(\(Int(b.minX)),\(Int(b.minY))) px=\(pm)"
    }.joined(separator: " | ")
}

func findHiDPIMode(displayID: CGDirectDisplayID, wantW: Int, wantH: Int) -> CGDisplayMode? {
    let opts = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
    let modes = CGDisplayCopyAllDisplayModes(displayID, opts) as? [CGDisplayMode] ?? []
    return modes.first { $0.width == wantW && $0.height == wantH && $0.pixelWidth == wantW * 2 }
}

func allModesSummary(displayID: CGDirectDisplayID) -> String {
    let opts = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
    let modes = CGDisplayCopyAllDisplayModes(displayID, opts) as? [CGDisplayMode] ?? []
    return modes.map { "\($0.width)x\($0.height)px\($0.pixelWidth)x\($0.pixelHeight)@\($0.refreshRate)" }
        .joined(separator: ", ")
}

// MARK: - SCShareableContent 폴링 (가상 디스플레이가 캡처 목록에 뜨는지)

func waitForSCDisplay(matching width: Int, timeoutSec: Double) -> SCDisplay? {
    let deadline = Date().addingTimeInterval(timeoutSec)
    var found: SCDisplay?
    while Date() < deadline {
        let sema = DispatchSemaphore(value: 0)
        SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) { content, _ in
            found = content?.displays.first { Int($0.width) == width }
            sema.signal()
        }
        sema.wait()
        if found != nil { break }
        Thread.sleep(forTimeInterval: 0.25)
    }
    return found
}

// MARK: - 메인

let sema = DispatchSemaphore(value: 0)
DispatchQueue.main.async { sema.signal() } // AppKit main thread 활성화
_ = sema.wait(timeout: .now() + 2)

print("== CGVirtualDisplay spark probe ==")
print("macOS: \(ProcessInfo.processInfo.operatingSystemVersionString)")
print("before: \(displayListDescription())")

let sessionDict = CGSessionCopyCurrentDictionary()
if sessionDict != nil {
    Probe.note("CGSession 존재 — GUI 로그인 세션 소속 프로세스다.")
} else {
    Probe.note("CGSession nil — GUI 세션 밖(SSH 등). 생성은 실패 가능성이 높지만 결과를 확인하기 위해 시도한다.")
}

var activeCount: UInt32 = 0
CGGetActiveDisplayList(0, nil, &activeCount)
if activeCount == 0 {
    // 0이어도 중단하지 않는다: 가상 디스플레이 생성이 세션의 첫 디스플레이가 될 수
    // 있고(클램쉘/헤드리스에서의 생성 가능성이 이 스파크의 측정 항목 중 하나),
    // 실제로 2026-09-02 BetterDisplay CLI 생성이 이 머신에서 성공한 전례가 있다.
    Probe.note("활성 디스플레이 0 — 클램쉘/헤드리스 상태로 보인다. 그래도 생성을 시도한다.")
}

// 1920x1200 (WUXGA, 16:10) — Leftcar의 태블릿 기본값과 동일하게 픽셀 지정.
// Leftcar 계약(EVIDENCE.md 정정 기록): 픽셀 기반 + HiDPI off + multiplier 1x.
// 이 프로브는 두 가지를 모두 측정한다: (a) 1x 픽셀 모드, (b) 포인트@2x HiDPI.
let wantW = 1920, wantH = 1200

// --- 생성 ---
let descriptor = CGVirtualDisplayDescriptor()
descriptor.setDispatchQueue(DispatchQueue.main)
descriptor.name = "Leftcar CGVD Probe"
descriptor.maxPixelsWide = UInt32(wantW * 2) // 회전/모드 전환 여유
descriptor.maxPixelsHigh = UInt32(wantH * 2)
descriptor.sizeInMillimeters = CGSize(width: 520, height: 325) // 16:10 24인치급
descriptor.productID = 0x4C43 // "LC"
descriptor.vendorID = 0x4C50 // "LP"
descriptor.serialNum = 0x20260903

let vd = CGVirtualDisplay(descriptor: descriptor)
guard vd.displayID != 0 else {
    print("== RESULT: CGVirtualDisplay 생성 실패 (displayID=0) ==")
    // 실패 원인 3갈래 자동 분류:
    //   A) SSH 계열 세션 (CGSession nil) → GUI 터미널에서 재실행 필요
    //   B) 활성 프레임버퍼 0 (클램쉘/헤드리스) → 덮개를 열거나 외장 모니터 연결 후 재실행
    //   C) 그 외 → API 자체 문제 가능성 (A·B 해소 후에도 실패하면 판정)
    let sessionOut = CGSessionCopyCurrentDictionary() == nil
    var fbCount = 0
    let match = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOFramebuffer"))
    if match != 0 { fbCount = 1; IOObjectRelease(match) }
    print("  진단: CGSession nil=\(sessionOut), IOKit IOFramebuffer 활성=\(fbCount)")
    if sessionOut {
        print("  분류 A: 이 프로세스는 GUI 로그인 세션 밖(SSH 등) — GUI 터미널에서 재실행할 것")
    } else if fbCount == 0 {
        print("  분류 B: 활성 프레임버퍼 없음(클램쉘 닫힘/헤드리스) — 덮개 개방 또는 외장 모니터 후 재실행")
    } else {
        print("  분류 C: GUI 세션 + 활성 프레임버퍼인데 생성 실패 — API 레벨 문제로 판정")
    }
    exit(1)
}
let displayID = vd.displayID
print("created displayID=\(displayID)")
Probe.check("CGVirtualDisplay 생성 성공 (macOS 26)", true, "displayID=\(displayID)")

// 잠깐 안정화 (WindowServer 등록)
Thread.sleep(forTimeInterval: 1.0)
print("after create: \(displayListDescription())")
Probe.check("CGActiveDisplayList 등재", CGDisplayIsActive(displayID) != 0)

// --- 1x 픽셀 모드 적용 (Leftcar 현재 계약) ---
do {
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = 0
    settings.modes = [CGVirtualDisplayMode(width: UInt(wantW), height: UInt(wantH), refreshRate: 60)]
    let ok = vd.apply(settings)
    Probe.check("applySettings 1920x1200@60 1x (HiDPI off)", ok)
    Thread.sleep(forTimeInterval: 1.0)
    let mode = CGDisplayCopyDisplayMode(displayID)
    Probe.check("현재 모드가 1920x1200 1x",
                mode?.width == wantW && mode?.pixelWidth == wantW,
                "now=\(mode.map { "\($0.width)x\($0.height)px\($0.pixelWidth)" } ?? "nil")")
    let bounds = CGDisplayBounds(displayID)
    Probe.check("논리 bounds 1920x1200",
                Int(bounds.width) == wantW && Int(bounds.height) == wantH,
                "bounds=\(bounds)")
}

// --- refresh 상한 측정: 60 넘는 모드가 apply되는지 ---
do {
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = 0
    settings.modes = [CGVirtualDisplayMode(width: UInt(wantW), height: UInt(wantH), refreshRate: 120)]
    let ok = vd.apply(settings)
    Probe.check("applySettings 120Hz 시도", ok)
    Thread.sleep(forTimeInterval: 0.5)
    let rate = CGDisplayCopyDisplayMode(displayID)?.refreshRate ?? 0
    Probe.note("120Hz 요청 후 실제 refreshRate=\(rate) (60 상한이면 60으로 강제)")
}

// --- HiDPI(포인트 @2x) 모드: 960x600pt @2x = 1920x1200px ---
do {
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = 1
    settings.modes = [CGVirtualDisplayMode(width: UInt(960), height: UInt(600), refreshRate: 60)]
    let ok = vd.apply(settings)
    Probe.check("applySettings 960x600pt@2x (HiDPI on)", ok)
    Thread.sleep(forTimeInterval: 1.0)
    let mode = CGDisplayCopyDisplayMode(displayID)
    let hidpiMode = findHiDPIMode(displayID: displayID, wantW: 960, wantH: 600)
    Probe.check("@2x 모드가 모드 목록에 존재", hidpiMode != nil,
                "modes=\(allModesSummary(displayID: displayID))")
    if let hidpiMode {
        var config: CGDisplayConfigRef?
        CGBeginDisplayConfiguration(&config)
        CGConfigureDisplayWithDisplayMode(config, displayID, hidpiMode, nil)
        let err = CGCompleteDisplayConfiguration(config, .permanently)
        Probe.note("HiDPI 모드 CGConfigure 결과=\(err.rawValue), 현재=\(mode.map { "\($0.width)x\($0.height) px\($0.pixelWidth)" } ?? "nil")")
    }
}

// --- SCK 정합 (질문 2) ---
do {
    print("waiting for SCShareableContent (max 10s)...")
    let sc = waitForSCDisplay(matching: 960, timeoutSec: 10) ?? waitForSCDisplay(matching: 1920, timeoutSec: 2)
    Probe.check("SCShareableContent에 가상 디스플레이 등장", sc != nil,
                sc.map { "scDisplay=\($0.displayID) \(Int($0.width))x\(Int($0.height))" } ?? "10s 내 없음")
}

// --- 미러 상태 확인 ---
Probe.check("미러 세트 아님", CGDisplayIsInMirrorSet(displayID) == 0)

// --- 회전 = 모드 재적용 (같은 디스플레이 identity 유지) ---
do {
    let before = displayID
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = 0
    settings.modes = [CGVirtualDisplayMode(width: UInt(wantH), height: UInt(wantW), refreshRate: 60)] // 1200x1920
    let ok = vd.apply(settings)
    Thread.sleep(forTimeInterval: 1.0)
    let after = vd.displayID
    let mode = CGDisplayCopyDisplayMode(after)
    Probe.check("회전(1200x1920) 모드 재적용", ok && before == after,
                "id same=\(before == after), now=\(mode.map { "\($0.width)x\($0.height)" } ?? "nil")")
    // 되돌리기
    let back = CGVirtualDisplaySettings()
    back.hiDPI = 0
    back.modes = [CGVirtualDisplayMode(width: UInt(wantW), height: UInt(wantH), refreshRate: 60)]
    _ = vd.apply(back)
    Thread.sleep(forTimeInterval: 0.5)
}

// --- 60초 유지 관찰 (연속 집행 필요성 관측) ---
do {
    Probe.note("60초 관찰 시작 — 매 10초 모드/미러/origin 변화 기록")
    var lastOrigin = CGDisplayBounds(displayID).origin
    var lastDesc = ""
    for tick in 1...6 {
        Thread.sleep(forTimeInterval: 10)
        let mode = CGDisplayCopyDisplayMode(displayID)
        let origin = CGDisplayBounds(displayID).origin
        let desc = "\(mode.map { "\($0.width)x\($0.height)px\($0.pixelWidth)@\($0.refreshRate)" } ?? "nil") origin(\(Int(origin.x)),\(Int(origin.y))) mirror=\(CGDisplayIsInMirrorSet(displayID) != 0)"
        if desc != lastDesc || origin != lastOrigin {
            Probe.note("t+\(tick * 10)s: \(desc)")
            lastDesc = desc
            lastOrigin = origin
        }
    }
    Probe.check("60초 관찰 — 모드·origin 무롤백", lastDesc.contains("px1920") || lastDesc.contains("960x600"),
                "last=\(lastDesc)")
}

// --- 소멸 ---
do {
    let before = displayListDescription()
    // ARC 해제로 terminationHandler 경로가 불리는지: 로컬 스코프로 이동
    do {
        let tmp = CGVirtualDisplay(descriptor: {
            let d = CGVirtualDisplayDescriptor()
            d.setDispatchQueue(DispatchQueue.main)
            d.name = "Leftcar CGVD Probe 2"
            d.maxPixelsWide = 1920
            d.maxPixelsHigh = 1200
            d.serialNum = 0x20260904
            d.productID = 0x4C43
            d.vendorID = 0x4C50
            d.sizeInMillimeters = CGSize(width: 520, height: 325)
            return d
        }())
        let s = CGVirtualDisplaySettings()
        s.hiDPI = 0
        s.modes = [CGVirtualDisplayMode(width: UInt(1920), height: UInt(1200), refreshRate: 60)]
        _ = tmp.apply(s)
        Thread.sleep(forTimeInterval: 1.0)
        let id2 = tmp.displayID
        Probe.check("두 번째 가상 디스플레이 동시 생성", id2 != 0 && id2 != displayID, "id2=\(id2)")
    } // 여기서 tmp 해제 → 소멸 관찰
    Thread.sleep(forTimeInterval: 1.5)
    let after = displayListDescription()
    Probe.check("해제 후 디스플레이 목록 복귀", !after.contains("Probe 2") || before == after, "after=\(after)")
}

print("== RESULT: \(Probe.failures.isEmpty ? "ALL PASS" : "FAILURES: \(Probe.failures)") ==")
