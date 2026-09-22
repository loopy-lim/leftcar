// VirtualDisplayBridgeProbe: 가상 디스플레이 브리지 물리 검증 CLI.
//
// 단독 실행 파일로 CGVirtualDisplayBridge.o와 함께 링크된다(빌드 스크립트
// vdisp-test 모드). GUI 터미널에서 사람이 직접 실행한다 — SSH/자동화 셸에는
// GUI 세션이 없어 생성이 불가능하다(설계 §E-게이트 전제).
//
// 사용:
//   probe                     — 클래스·GUI 세션·활성 디스플레이 검사
//   create <w> <h> <scale>    — 가상 디스플레이 생성(기본 1280 800 2)
//   status                    — 브리지 레지스트리 상태
//   list                      — 활성 디스플레이 전체(우리 것 포함, 경계·모드)
//   destroy                   — 제거(사라질 때까지 폴링)
//   cycle <n>                 — create/destroy n회 반복(누적·좀비 검사)
//   linger                    — 생성 후 stdin 대기(kill -9 실험용)

import Foundation
import CoreGraphics

// nohup/리다이렉션 환경에서도 즉시 보이게 한다(kill -9 실험 관찰용).
setvbuf(stdout, nil, _IONBF, 0)

func symbol(_ name: String) -> UnsafeMutableRawPointer? {
    guard let handle = dlopen(nil, RTLD_LAZY) else { return nil }
    return dlsym(handle, name)
}

func callStringFn(_ name: String) -> String? {
    typealias Fn = @convention(c) () -> UnsafeMutablePointer<CChar>?
    guard let sym = symbol(name), let raw = unsafeBitCast(sym, to: Fn.self)() else {
        return nil
    }
    defer { free(raw) }
    return String(cString: raw)
}

func probe() {
    guard let json = callStringFn("leftcar_vdisp_probe_v1") else {
        print("probe: bridge symbol missing")
        exit(2)
    }
    print("probe: \(json)")
}

func create(_ args: [String]) {
    let width = args.count > 2 ? UInt32(args[2]) ?? 1280 : 1280
    let height = args.count > 3 ? UInt32(args[3]) ?? 800 : 800
    let scale = args.count > 4 ? UInt32(args[4]) ?? 2 : 2
    typealias Fn = @convention(c) (UInt32, UInt32, UInt32, UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
    guard let sym = symbol("leftcar_vdisp_create_v1") else {
        print("create: bridge symbol missing")
        exit(2)
    }
    let name = "Leftcar Display"
    name.withCString { namePtr in
        guard let raw = unsafeBitCast(sym, to: Fn.self)(width, height, scale, namePtr) else {
            print("create: null return")
            exit(1)
        }
        defer { free(raw) }
        print("create \(width)x\(height)@\(scale)x: \(String(cString: raw))")
    }
}

func status() {
    guard let json = callStringFn("leftcar_vdisp_status_v1") else {
        print("status: bridge symbol missing")
        exit(2)
    }
    print("status: \(json)")
}

func listAllDisplays() {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success else {
        print("list: CGGetActiveDisplayList failed")
        exit(1)
    }
    var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
    var filled = count
    guard CGGetActiveDisplayList(count, &ids, &filled) == .success else {
        print("list: CGGetActiveDisplayList failed")
        exit(1)
    }
    let main = CGMainDisplayID()
    print("list: \(filled) active display(s), main=\(main)")
    for id in ids.prefix(Int(filled)) {
        let bounds = CGDisplayBounds(id)
        var modeText = "mode=none"
        if let mode = CGDisplayCopyDisplayMode(id) {
            modeText = "mode=\(mode.width)x\(mode.height) px=\(mode.pixelWidth)x\(mode.pixelHeight)"
        }
        print("  id=\(id)\(id == main ? " [main]" : "") active=\(CGDisplayIsActive(id)) online=\(CGDisplayIsOnline(id)) mirror=\(CGDisplayIsInMirrorSet(id)) bounds=\(Int(bounds.origin.x)),\(Int(bounds.origin.y)) \(Int(bounds.width))x\(Int(bounds.height)) \(modeText)")
    }
}

func destroy(exitAfter: Bool = true) {
    typealias Fn = @convention(c) (UInt32) -> Int32
    guard let sym = symbol("leftcar_vdisp_destroy_v1") else {
        print("destroy: bridge symbol missing")
        exit(2)
    }
    // status에서 displayId를 파싱하는 대신, 레지스트리의 유일한 슬롯을 얻는다.
    guard let json = callStringFn("leftcar_vdisp_status_v1"),
          let data = json.data(using: .utf8),
          let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let displays = parsed["displays"] as? [[String: Any]],
          let first = displays.first,
          let displayId = (first["displayId"] as? NSNumber)?.uint32Value else {
        print("destroy: no live virtual display")
        exit(0)
    }
    let rc = unsafeBitCast(sym, to: Fn.self)(displayId)
    print("destroy(\(displayId)): rc=\(rc) (0=gone, 1=not-ours, 2=still-active-timeout)")
    if rc != 0 { exit(1) }
    if exitAfter { exit(0) }
}

func cycle(_ args: [String]) {
    let times = args.count > 2 ? Int(args[2]) ?? 5 : 5
    for round in 1...times {
        print("cycle \(round)/\(times)")
        create(["argv0", "create", "1280", "800", "2"])
        // 모드 확정(~5s)이 진행 중인 상태에서 제거하면 회수가 늦는다 —
        // 확정을 기다렸다가 제거한다.
        usleep(6_000_000)
        // destroy는 exit 하지 않는 경로를 위해 직접 호출한다.
        typealias Fn = @convention(c) (UInt32) -> Int32
        if let sym = symbol("leftcar_vdisp_destroy_v1"),
           let json = callStringFn("leftcar_vdisp_status_v1"),
           let data = json.data(using: .utf8),
           let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let displays = parsed["displays"] as? [[String: Any]],
           let displayId = (displays.first?["displayId"] as? NSNumber)?.uint32Value {
            let rc = unsafeBitCast(sym, to: Fn.self)(displayId)
            print("  destroy rc=\(rc)")
            if rc != 0 {
                print("  FAIL: display did not disappear")
                exit(1)
            }
        } else {
            print("  FAIL: no display to destroy")
            exit(1)
        }
        usleep(300_000)
    }
    print("cycle: \(times) rounds ok")
}

func listModes(_ args: [String]) {
    let id = args.count > 2 ? CGDirectDisplayID(args[2]) ?? 0 : 0
    guard id != 0 else {
        print("modes: missing display id")
        exit(2)
    }
    let current = CGDisplayCopyDisplayMode(id)
    print("modes for \(id): current=\(current.map { "\($0.width)x\($0.height) px=\($0.pixelWidth)x\($0.pixelHeight)" } ?? "none")")
    let options = [kCGDisplayShowDuplicateLowResolutionModes as String: true] as CFDictionary
    guard let modes = CGDisplayCopyAllDisplayModes(id, options) as? [CGDisplayMode] else {
        print("  none")
        return
    }
    for mode in modes {
        let mark = (current != nil && mode.width == current?.width && mode.height == current?.height
                    && mode.pixelWidth == current?.pixelWidth) ? " *" : ""
        print("  \(mode.width)x\(mode.height) px=\(mode.pixelWidth)x\(mode.pixelHeight) rate=\(mode.refreshRate)\(mark)")
    }
}

func lingerWatch() {
    create(["argv0", "create", "1280", "800", "2"])
    // 생성 프로세스 안에서 보는 모드가 외부 프로세스와 다른지 관찰한다.
    for _ in 0..<20 {
        if let json = callStringFn("leftcar_vdisp_status_v1"),
           let data = json.data(using: .utf8),
           let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let displays = parsed["displays"] as? [[String: Any]],
           let first = displays.first {
            let bounds = first["bounds"] as? [String: Any]
            print("inproc: logical=\(first["logicalWidth"] ?? 0)x\(first["logicalHeight"] ?? 0) backing=\(first["backingWidth"] ?? 0)x\(first["backingHeight"] ?? 0) bounds=\(bounds?["width"] ?? -1)x\(bounds?["height"] ?? -1)@\(bounds?["x"] ?? 0),\(bounds?["y"] ?? 0)")
        } else {
            print("inproc: none")
        }
        Thread.sleep(forTimeInterval: 1)
    }
    print("linger-watch done; waiting for kill")
    while true {
        readLine()
    }
}

/// 같은 프로세스에서 생성→열거→제거→열거를 해본다: 제거 뒤에도 이 프로세스의
/// CGGetActiveDisplayList가 가상 디스플레이를 보이면(캐시 스테일) 카탈로그
/// 팬텀 문제가 있는 것이다.
func phantomCheck() {
    create(["argv0", "create", "1280", "800", "2"])
    print("after create, in-process list:")
    listAllDisplays()
    typealias Fn = @convention(c) (UInt32) -> Int32
    if let sym = symbol("leftcar_vdisp_destroy_v1"),
       let json = callStringFn("leftcar_vdisp_status_v1"),
       let data = json.data(using: .utf8),
       let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
       let displays = parsed["displays"] as? [[String: Any]],
       let displayId = (displays.first?["displayId"] as? NSNumber)?.uint32Value {
        let rc = unsafeBitCast(sym, to: Fn.self)(displayId)
        print("destroy rc=\(rc)")
    }
    print("after destroy, in-process list (t+0s):")
    listAllDisplays()
    Thread.sleep(forTimeInterval: 3)
    print("after destroy, in-process list (t+3s):")
    listAllDisplays()
}

/// 제거 지연의 성격 판별: 생성→제거(객체 해제) 후 이 프로세스와 외부
/// 프로세스가 각각 언제 디스플레이 소실을 보는지 비교한다.
func releaseWatch() {
    create(["argv0", "create", "1280", "800", "2"])
    typealias Fn = @convention(c) (UInt32) -> Int32
    if let sym = symbol("leftcar_vdisp_destroy_v1"),
       let json = callStringFn("leftcar_vdisp_status_v1"),
       let data = json.data(using: .utf8),
       let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
       let displays = parsed["displays"] as? [[String: Any]],
       let displayId = (displays.first?["displayId"] as? NSNumber)?.uint32Value {
        let rc = unsafeBitCast(sym, to: Fn.self)(displayId)
        print("release-watch: destroy rc=\(rc) (release done, poll result above)")
    }
    for round in 0..<10 {
        var count: UInt32 = 0
        CGGetActiveDisplayList(0, nil, &count)
        print("release-watch t+\(round * 2)s: in-process active count=\(count)")
        Thread.sleep(forTimeInterval: 2)
    }
}

func linger() {
    create(["argv0", "create", "1280", "800", "2"])
    print("linger: display alive; kill -9 this process to test WindowServer reclaim (E-1)")
    while true {
        readLine()
    }
}

let args = CommandLine.arguments
let command = args.count > 1 ? args[1] : "probe"
switch command {
case "probe": probe()
case "verify-mode":
    guard args.count == 6, let id = UInt32(args[2]), let width = Int(args[3]),
          let height = Int(args[4]), let scale = Int(args[5]),
          let mode = CGDisplayCopyDisplayMode(id) else { exit(2) }
    let bounds = CGDisplayBounds(id)
    guard Int(bounds.width) == width, Int(bounds.height) == height,
          mode.width == width, mode.height == height,
          mode.pixelWidth == width * scale, mode.pixelHeight == height * scale else {
        print("FAIL mode: bounds=\(bounds), mode=\(mode.width)x\(mode.height), pixels=\(mode.pixelWidth)x\(mode.pixelHeight)")
        exit(1)
    }
    print("PASS external mode: \(width)x\(height) @\(scale)")
case "verify-lifecycle", "verify-placement", "verify-placement-after-resize":
    let mainBefore = CGMainDisplayID()
    let modeArgs = args.count >= 5 ? Array(args[2...4]) : ["1280", "800", "2"]
    if command == "verify-placement-after-resize" {
        let previousMode = args.count >= 8 ? Array(args[5...7]) : ["1600", "1000", "1"]
        create([args[0], "create"] + previousMode)
        destroy(exitAfter: false)
    }
    create([args[0], "create"] + modeArgs)
    guard let json = callStringFn("leftcar_vdisp_status_v1"),
          let data = json.data(using: .utf8),
          let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let displays = payload["displays"] as? [[String: Any]],
          let displayID = (displays.first?["displayId"] as? NSNumber)?.uint32Value,
          let sym = symbol("leftcar_vdisp_destroy_v1") else { exit(2) }
    typealias DestroyFn = @convention(c) (UInt32) -> Int32
    if command == "verify-placement" || command == "verify-placement-after-resize" {
        typealias ArrangeFn = @convention(c) (UInt32, UInt32) -> Int32
        guard let arrange = symbol("leftcar_vdisp_arrange_v1") else {
            print("FAIL placement: arrangement is unavailable")
            exit(1)
        }
        let mainBounds = CGDisplayBounds(mainBefore)
        // Start with a negative origin: placing right first can settle a stale
        // desktop size and hide the first-placement gap after recreation.
        for side: UInt32 in [1, 2, 3, 0] {
            let rc = unsafeBitCast(arrange, to: ArrangeFn.self)(displayID, side)
            let bounds = CGDisplayBounds(displayID)
            let correctEdge = side == 0 ? bounds.minX == mainBounds.maxX
                : side == 1 ? bounds.maxX == mainBounds.minX
                : side == 2 ? bounds.maxY == mainBounds.minY
                : bounds.minY == mainBounds.maxY
            guard rc == 0, correctEdge, CGMainDisplayID() == mainBefore,
                  CGDisplayBounds(mainBefore) == mainBounds else {
                print("FAIL placement: side=\(side), result=\(rc), bounds=\(bounds)")
                let external = Process()
                external.executableURL = URL(fileURLWithPath: args[0])
                external.arguments = ["list"]
                try? external.run()
                external.waitUntilExit()
                exit(1)
            }
            // The creator's mode cache may be stale or empty after recreation.
            // A fresh process checks that placement did not revert Retina scale.
            let external = Process()
            external.executableURL = URL(fileURLWithPath: args[0])
            external.arguments = ["verify-mode", String(displayID)] + modeArgs
            try external.run()
            external.waitUntilExit()
            guard external.terminationStatus == 0 else { exit(1) }
        }
        print("PASS placement: all four sides; main display unchanged")
    }
    let result = unsafeBitCast(sym, to: DestroyFn.self)(displayID)
    var activeIDs = [CGDirectDisplayID](repeating: 0, count: 32)
    var activeCount: UInt32 = 0
    let listed = CGGetActiveDisplayList(32, &activeIDs, &activeCount) == .success
    let stillActive = activeIDs.prefix(Int(activeCount)).contains(displayID)
    guard result == 0, listed, !stillActive, CGMainDisplayID() == mainBefore else {
        print("FAIL lifecycle: destroy=\(result), active=\(stillActive), primaryPreserved=\(CGMainDisplayID() == mainBefore)")
        exit(1)
    }
    print("PASS lifecycle: display removed and primary preserved")
case "create": create(args)
case "status": status()
case "list": listAllDisplays()
case "modes": listModes(args)
case "destroy": destroy()
case "cycle": cycle(args)
case "linger": linger()
case "linger-watch": lingerWatch()
case "phantom": phantomCheck()
case "release-watch": releaseWatch()
default:
    print("unknown command: \(command)")
    print("usage: \(args[0]) probe|create <w> <h> <scale>|status|list|destroy|cycle <n>|linger")
    exit(2)
}
