// CGVD shim — tablet-display CgvdProvider가 서브프로세스로 호출하는 바이너리.
// EXPERIMENT ONLY (R-015): 기본 프로바이더 승격은 스파크 evidence에 근거한
// 별도 ADR 없이는 없다. 근거: docs/research/2026-09-03_cgvd-spark-results.md
//
// stdout 계약 (반드시 한 줄 — Task 4 parse_cgvd_line이 이 형식을 파싱한다):
//   probe   -> "EXISTS" | "MISSING"
//   inspect --display-id=<id>
//           -> "INSPECT <displayID> <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight> <x> <y>"
//   create  -> "READY <displayID> <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight>" |
//              "UNAVAILABLE <detail>" | "FAILED <detail>"
//   PLACE <requestID> <x> <y>
//           -> "PLACED <requestID> <displayID> <x> <y> <width> <height> <primaryBefore> <primaryAfter>" |
//              "FAILED <requestID> <detail>"
//   RESIZE <width> <height> <scale>
//           -> "RESIZED <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight>" |
//              "FAILED resize displayID=<id> <detail>" — 세션 유지, 프로세스는 종료하지 않는다
//   remove  -> "FAILED remove requires a live create session"
// argv 되돌림 금지: 개행 섞인 인자가 계약을 두 줄로 깨뜨린다.
//
// 종료 코드: 0 성공 / 1 환경·엔진 실패 / 2 사용법 오류

import Foundation
import CGVD

/// 실패 출력은 성공 경로와 같은 stdout 한 줄 계약을 따른다. 진행 로그는 없다 —
/// Rust 쪽이 stdout 첫 줄만 읽기 때문에 계약 외 텍스트는 곧 파싱 오류이다.
func fail(_ line: String, code: Int32) -> Never {
    print(line)
    exit(code)
}

// MARK: - probe (세션 무관 — SSH·자동화 셸에서도 판정 가능)

/// create이 실제로 쓰는 4종. descriptor만 확인하면 본체나 Settings가 빠진
/// 상태를 오판하므로 전부 있어야 "생성 표면이 산다"로 본다.
let requiredClasses = [
    "CGVirtualDisplay",
    "CGVirtualDisplayDescriptor",
    "CGVirtualDisplaySettings",
    "CGVirtualDisplayMode",
]

func runProbe() -> Never {
    // NSClassFromString은 objc 런타임 조회라 심볼 직접 참조가 없다 — API가
    // 파손된 macOS에서도 dyld 크래시가 아니라 MISSING이라는 정상 답을 낸다.
    let missing = requiredClasses.filter { NSClassFromString($0) == nil }
    // MISSING은 실패가 아니라 유의미한 답이므로 두 경우 모두 exit 0.
    print(missing.isEmpty ? "EXISTS" : "MISSING")
    exit(0)
}

func runInspect(_ arguments: [String]) -> Never {
    guard arguments.count == 3,
          let rawID = arguments[2].split(separator: "=", maxSplits: 1).last,
          arguments[2].hasPrefix("--display-id="),
          let displayID = UInt32(rawID), displayID != 0 else {
        fail("FAILED usage: cgvd-shim inspect --display-id=<id>", code: 2)
    }
    guard CGDisplayIsActive(displayID) != 0 else {
        fail("FAILED inactive displayID=\(displayID)", code: 1)
    }
    guard let mode = CGDisplayCopyDisplayMode(displayID) else {
        fail("FAILED mode unavailable displayID=\(displayID)", code: 1)
    }
    let bounds = CGDisplayBounds(displayID)
    print(
        "INSPECT \(displayID) \(mode.width) \(mode.height) \(mode.pixelWidth) \(mode.pixelHeight) "
            + "\(Int(bounds.origin.x)) \(Int(bounds.origin.y))"
    )
    exit(0)
}

// MARK: - create

struct CreateOptions {
    var name = "Leftcar Virtual"
    var width = 1920
    var height = 1200
    var scale = 1
    var serial: UInt32 = 0
}

func parseCreateOptions(_ arguments: [String]) -> CreateOptions? {
    var options = CreateOptions()
    for argument in arguments.dropFirst(2) {
        let pair = argument.split(separator: "=", maxSplits: 1)
        guard pair.count == 2 else { return nil }
        switch String(pair[0]) {
        case "--name":
            // 빈 이름은 Rust 쪽 validate_name과 같은 이유로 거부한다.
            let name = String(pair[1])
            if name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return nil }
            options.name = name
        case "--width":
            // maxPixels는 scale을 반영하므로 UInt32 오버플로 트랩(=계약 붕괴)을
            // 파싱 단계에서 잘라낸다.
            guard let width = Int(String(pair[1])), width > 0,
                  width <= Int(UInt32.max / 2) else { return nil }
            options.width = width
        case "--height":
            guard let height = Int(String(pair[1])), height > 0,
                  height <= Int(UInt32.max / 2) else { return nil }
            options.height = height
        case "--scale":
            guard let scale = Int(String(pair[1])), scale == 1 || scale == 2 else { return nil }
            options.scale = scale
        case "--serial":
            guard let serial = UInt32(String(pair[1])) else { return nil }
            options.serial = serial
        default:
            return nil
        }
    }
    return options
}

/// 조건이 참이 될 때까지 최대 timeout초 동안 interval 간격으로 묻는다.
/// 고정 대기보다 정확하다 — 빠른 환경에선 즉시 통과, 느린 환경에선 여유를
/// 주고, 끝내 확인이 안 되면 "된 셈 치고" 진행하지 않는다.
func poll(upTo timeout: TimeInterval, interval: TimeInterval = 0.1, _ condition: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while true {
        if condition() { return true }
        if Date() >= deadline { return false }
        Thread.sleep(forTimeInterval: interval)
    }
}

/// 생성자가 반환했다는 것은 등록 보장이 아니다 — WindowServer 등록을 폴링으로
/// 확인한다.
func pollRegistration(displayID: CGDirectDisplayID) -> Bool {
    poll(upTo: 2.0) { CGDisplayIsActive(displayID) != 0 }
}

/// 적용을 요청한 모드가 실제 표시 상태가 됐는지 확인한다. 시스템이 다른
/// 배율·모드를 강제하면 요청 픽셀이 안 맞으므로 확인 실패로 답한다.
func pollRequestedMode(displayID: CGDirectDisplayID, width: Int, height: Int, scale: Int) -> Bool {
    poll(upTo: 2.0) {
        guard let mode = CGDisplayCopyDisplayMode(displayID) else { return false }
        return mode.width == width && mode.height == height
            && mode.pixelWidth == width * scale && mode.pixelHeight == height * scale
    }
}

func writeResponse(_ line: String) {
    FileHandle.standardOutput.write("\(line)\n".data(using: .utf8)!)
}

func runPlaceCommand(
    _ command: String,
    displayID: CGDirectDisplayID,
    expectedWidth: Int,
    expectedHeight: Int
) {
    let fields = command.split(whereSeparator: { $0.isWhitespace })
    guard fields.count == 4,
          fields[0] == "PLACE",
          let requestID = UInt64(fields[1]),
          let x = Int32(fields[2]),
          let y = Int32(fields[3]) else {
        writeResponse("FAILED 0 invalid PLACE command")
        return
    }

    let primaryBefore = CGMainDisplayID()
    var configuration: CGDisplayConfigRef?
    let beginResult = CGBeginDisplayConfiguration(&configuration)
    guard beginResult == .success, let configuration else {
        writeResponse("FAILED \(requestID) begin code=\(beginResult.rawValue)")
        return
    }
    let configureResult = CGConfigureDisplayOrigin(configuration, displayID, x, y)
    guard configureResult == .success else {
        CGCancelDisplayConfiguration(configuration)
        writeResponse("FAILED \(requestID) configure code=\(configureResult.rawValue)")
        return
    }
    let completeResult = CGCompleteDisplayConfiguration(configuration, .forSession)
    guard completeResult == .success else {
        writeResponse("FAILED \(requestID) complete code=\(completeResult.rawValue)")
        return
    }

    var observed = CGRect.null
    let reachedRequestedBounds = poll(upTo: 2.0) {
        observed = CGDisplayBounds(displayID)
        return Int32(observed.origin.x) == x && Int32(observed.origin.y) == y
            && Int(observed.width) == expectedWidth && Int(observed.height) == expectedHeight
    }
    let primaryAfter = CGMainDisplayID()
    guard reachedRequestedBounds else {
        writeResponse("FAILED \(requestID) bounds timeout actual=\(Int(observed.origin.x)),\(Int(observed.origin.y))")
        return
    }
    guard primaryAfter == primaryBefore else {
        writeResponse("FAILED \(requestID) primary changed before=\(primaryBefore) after=\(primaryAfter)")
        return
    }
    writeResponse(
        "PLACED \(requestID) \(displayID) \(Int(observed.origin.x)) \(Int(observed.origin.y)) "
            + "\(Int(observed.width)) \(Int(observed.height)) \(primaryBefore) \(primaryAfter)"
    )
}

/// 상주 세션의 RESIZE — create와 같은 논리로 모드를 재적용한다. apply 실패나
/// 모드 미도달은 fail()로 죽지 않는다: 한 요청의 실패가 프로세스(=디스플레이
/// 수명)를 끝내면 Rust가 세션 전체를 잃으므로, PLACE와 같은 한 줄 FAILED로
/// 답하고 루프는 계속한다. 반환값은 적용된 논리 크기 — 호출자가 PLACE의
/// 기대 bounds를 갱신하는 데 쓴다.
func runResizeCommand(
    _ command: String,
    virtualDisplay: CGVirtualDisplay,
    displayID: CGDirectDisplayID
) -> (width: Int, height: Int)? {
    let fields = command.split(whereSeparator: { $0.isWhitespace })
    // 크기 상한은 create의 --width/--height와 같다 — maxPixels 반영 오버플로
    // 트랩을 파싱 단계에서 잘라낸다.
    guard fields.count == 4,
          fields[0] == "RESIZE",
          let width = Int(fields[1]), width > 0, width <= Int(UInt32.max / 2),
          let height = Int(fields[2]), height > 0, height <= Int(UInt32.max / 2),
          let scale = Int(fields[3]), scale == 1 || scale == 2 else {
        writeResponse("FAILED resize displayID=\(displayID) invalid RESIZE command")
        return nil
    }

    // descriptor.maxPixels는 생성 시 크기 기준이므로 그보다 큰 RESIZE는 apply가
    // 거절할 수 있다 — 거절 시 아래 FAILED가 정직한 답이다.
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = scale == 2 ? 1 : 0
    settings.modes = [
        CGVirtualDisplayMode(width: UInt(width), height: UInt(height), refreshRate: 60)
    ]
    guard virtualDisplay.apply(settings) else {
        writeResponse("FAILED resize displayID=\(displayID) settings")
        return nil
    }
    guard pollRequestedMode(displayID: displayID, width: width, height: height, scale: scale) else {
        writeResponse("FAILED resize displayID=\(displayID) mode timeout")
        return nil
    }
    guard let mode = CGDisplayCopyDisplayMode(displayID) else {
        writeResponse("FAILED resize displayID=\(displayID) mode unavailable")
        return nil
    }
    // READY와 같은 관측값 나열 — Rust 파서가 같은 형식 코드를 재사용한다.
    writeResponse("RESIZED \(mode.width) \(mode.height) \(mode.pixelWidth) \(mode.pixelHeight)")
    return (width: width, height: height)
}

func runCreate(_ arguments: [String]) -> Never {
    guard let options = parseCreateOptions(arguments) else {
        fail("FAILED usage: cgvd-shim create [--name=<n>] [--width=<w>] [--height=<h>]", code: 2)
    }

    // 분류 A: GUI 로그인 세션 밖이면 WindowServer 접근 자체가 불가능하다.
    // NOACTIVE보다 먼저 검사해야 "세션 문제"와 "화면 없음"을 구분할 수 있다.
    guard CGSessionCopyCurrentDictionary() != nil else {
        fail("UNAVAILABLE session", code: 1)
    }

    // 약한 바인딩(README 구현 노트) 덕에 이 바이너리는 파손된 macOS에서도
    // 로드되지만, 클래스가 없으면 아래 직접 참조가 nil 메시징이 된다. Swift는
    // nil 이니셜 결과를 함정으로 처리하므로 계약 한 줄로 먼저 잘라 둔다.
    guard requiredClasses.allSatisfy({ NSClassFromString($0) != nil }) else {
        fail("FAILED private API missing on this macOS", code: 1)
    }

    // 분류 B: 활성 화면 0개는 생성 자체를 막는다 (스파크 2026-09-03 확정 —
    // 클램쉘 닫힘+모니터 전원 꺼짐에서 displayID=0, BetterDisplay CLI abort 동일).
    var activeCount: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &activeCount)
    guard activeCount > 0 else {
        fail("NOACTIVE", code: 1)
    }

    // descriptor는 스파크 main.swift의 생성 코드를 그대로 축소해 이식했다:
    // maxPixels는 회전/모드 전환 여유로 2배, 실제 크기는 아래 settings로 고정.
    let descriptor = CGVirtualDisplayDescriptor()
    let queue = DispatchQueue(label: "dev.leftcar.cgvd-shim.create")
    descriptor.setDispatchQueue(queue)
    descriptor.name = options.name
    descriptor.maxPixelsWide = UInt32(options.width * options.scale)
    descriptor.maxPixelsHigh = UInt32(options.height * options.scale)
    descriptor.sizeInMillimeters = CGSize(width: 520, height: 325) // 16:10 24인치급
    descriptor.productID = 0x4C43 // "LC"
    descriptor.vendorID = 0x4C50 // "LP"
    // Rust가 세션마다 생성한 시리얼을 전달한다. 0은 유효한 관리 식별자가
    // 아니므로 명시적으로 거부한다.
    guard options.serial != 0 else { fail("FAILED serial must be non-zero", code: 2) }
    descriptor.serialNum = options.serial

    // CLI에는 runloop이 없으므로 스파크의 main 큐 대신 전용 직렬 큐를 쓴다.
    // 스파크 A/B는 실패 경로(헤드리스 displayID=0)에서만 큐 무관을 확인했다 —
    // 이 큐에서의 성공 경로는 실측 대상이며, 등록 폴링(아래)이 그 검증을 대신한다.
    let virtualDisplay = CGVirtualDisplay(descriptor: descriptor)
    let displayID = virtualDisplay.displayID
    guard displayID != 0 else {
        // 분류 C: 세션(A)과 활성 화면(B)이 정상인데도 실패 — API 레벨 문제.
        fail("FAILED displayID=0", code: 1)
    }

    // 모드는 논리 크기로 요청하고 hiDPI가 backing pixel을 결정한다. 요청
    // 값을 backing 크기로 미리 곱하면 HiDPI에서 다시 두 배가 된다.
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = options.scale == 2 ? 1 : 0
    settings.modes = [
        CGVirtualDisplayMode(width: UInt(options.width), height: UInt(options.height), refreshRate: 60)
    ]
    guard virtualDisplay.apply(settings) else {
        // 프로세스 종료로 생성한 디스플레이를 정리하고 적용 실패를 보고한다.
        fail("FAILED settings displayID=\(displayID)", code: 1)
    }

    // 생성자 반환과 apply 참은 등록 보장이 아니다 — WindowServer 등록과
    // 논리/픽셀 모드 도달을 모두 확인한 뒤에만 READY를 말한다.
    guard pollRegistration(displayID: displayID) else {
        fail("FAILED registration timeout displayID=\(displayID)", code: 1)
    }
    guard pollRequestedMode(displayID: displayID, width: options.width, height: options.height, scale: options.scale) else {
        fail("FAILED mode timeout displayID=\(displayID)", code: 1)
    }

    guard let mode = CGDisplayCopyDisplayMode(displayID) else {
        fail("FAILED mode unavailable displayID=\(displayID)", code: 1)
    }
    let ready = "READY \(displayID) \(mode.width) \(mode.height) \(mode.pixelWidth) \(mode.pixelHeight)\n"
    FileHandle.standardOutput.write(ready.data(using: .utf8)!)

    // CGVirtualDisplay의 수명은 이 객체가 살아 있는 동안 관리한다. Rust가
    // stop을 보내거나 stdin이 EOF가 되면 반환하면서 객체를 해제한다.
    // RESIZE는 PLACE의 기대 bounds가 따라가야 할 현재 논리 크기를 바꾼다 —
    // var로 추적해 리사이즈 뒤 PLACE 검증이 새 모드를 본다. PLACE 파서는
    // fields[0] == "PLACE"로 정확히 일치를 보므로 RESIZE 줄을 잘못 먹지 않고,
    // dispatch 순서만 RESIZE를 먼저 둔다.
    var currentWidth = options.width
    var currentHeight = options.height
    withExtendedLifetime(virtualDisplay) {
        while let command = readLine(strippingNewline: true) {
            let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed == "stop" { break }
            if trimmed.hasPrefix("RESIZE") {
                if let resized = runResizeCommand(
                    trimmed,
                    virtualDisplay: virtualDisplay,
                    displayID: displayID
                ) {
                    currentWidth = resized.width
                    currentHeight = resized.height
                }
                continue
            }
            runPlaceCommand(
                trimmed,
                displayID: displayID,
                expectedWidth: currentWidth,
                expectedHeight: currentHeight
            )
        }
    }
    exit(0)
}

// MARK: - 진입

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    fail("FAILED usage: cgvd-shim <probe|create|remove> [options]", code: 2)
}

switch arguments[1] {
case "probe":
    runProbe()
case "inspect":
    runInspect(arguments)
case "create":
    runCreate(arguments)
case "remove":
    fail("FAILED remove requires a live create session", code: 2)
default:
    // 서브커맨드를 그대로 되돌리지 않는다 — argv에 개행이 섞이면 계약 한 줄이
    // 두 줄로 깨져 호출자의 파싱을 오염시킨다 (리뷰에서 재현된 사례).
    fail("FAILED unknown subcommand", code: 2)
}
