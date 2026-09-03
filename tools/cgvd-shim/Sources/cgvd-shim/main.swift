// CGVD shim — tablet-display CgvdProvider가 서브프로세스로 호출하는 바이너리.
// EXPERIMENT ONLY (R-015): 기본 프로바이더 승격은 스파크 evidence에 근거한
// 별도 ADR 없이는 없다. 근거: docs/research/2026-09-03_cgvd-spark-results.md
//
// stdout 계약 (반드시 한 줄 — Task 4 parse_cgvd_line이 이 형식을 파싱한다):
//   probe   -> "EXISTS" | "MISSING"
//   create  -> "OK <displayID>" (등록+요청 모드 확인 후) | "NOACTIVE" |
//              "UNAVAILABLE <detail>" | "FAILED <detail>"
//   remove  -> "FAILED remove is not implemented yet by design (R-015 experiment scope)"
// argv 되돌림 금지: 개행 섞인 인자가 계약을 두 줄로 깨뜨린다.
//
// 종료 코드: 0 성공 / 1 환경·엔진 실패 / 2 사용법 오류 / 3 remove 미구현(설계상)

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

// MARK: - create

struct CreateOptions {
    var name = "Leftcar Virtual"
    var width = 1920
    var height = 1200
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
            // maxPixels는 2배로 저장하므로 UInt32 오버플로 트랩(=계약 붕괴)을
            // 파싱 단계에서 잘라낸다.
            guard let width = Int(String(pair[1])), width > 0,
                  width <= Int(UInt32.max / 2) else { return nil }
            options.width = width
        case "--height":
            guard let height = Int(String(pair[1])), height > 0,
                  height <= Int(UInt32.max / 2) else { return nil }
            options.height = height
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
func pollRequestedMode(displayID: CGDirectDisplayID, width: Int, height: Int) -> Bool {
    poll(upTo: 2.0) {
        guard let mode = CGDisplayCopyDisplayMode(displayID) else { return false }
        return mode.width == width && mode.height == height
    }
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
    descriptor.maxPixelsWide = UInt32(options.width * 2)
    descriptor.maxPixelsHigh = UInt32(options.height * 2)
    descriptor.sizeInMillimeters = CGSize(width: 520, height: 325) // 16:10 24인치급
    descriptor.productID = 0x4C43 // "LC"
    descriptor.vendorID = 0x4C50 // "LP"
    // 상수 시리얼: 이름 파생 해시보다 단순함이 낫다 — 동일 시리얼 재생성의
    // 부작용은 아직 실측되지 않았고(작업 10 물리 실측 대상), 실험 범위에서는
    // 재현 가능한 고정값이 디버깅에 유리하다.
    descriptor.serialNum = 0x20260903

    // CLI에는 runloop이 없으므로 스파크의 main 큐 대신 전용 직렬 큐를 쓴다.
    // 스파크 A/B는 실패 경로(헤드리스 displayID=0)에서만 큐 무관을 확인했다 —
    // 이 큐에서의 성공 경로는 실측 대상이며, 등록 폴링(아래)이 그 검증을 대신한다.
    let virtualDisplay = CGVirtualDisplay(descriptor: descriptor)
    let displayID = virtualDisplay.displayID
    guard displayID != 0 else {
        // 분류 C: 세션(A)과 활성 화면(B)이 정상인데도 실패 — API 레벨 문제.
        fail("FAILED displayID=0", code: 1)
    }

    // Leftcar 계약은 픽셀 기반 1x (EVIDENCE.md 정정 기록) — HiDPI off,
    // 요청 크기 모드 정확히 1개. 적용 실패는 엔진 실패로 분류한다.
    let settings = CGVirtualDisplaySettings()
    settings.hiDPI = 0
    settings.modes = [
        CGVirtualDisplayMode(width: UInt(options.width), height: UInt(options.height), refreshRate: 60)
    ]
    guard virtualDisplay.apply(settings) else {
        // 디스플레이는 생성됐지만 요청 크기를 보장 못 한다 — OK로 거짓말하지
        // 않고 실패로 분류한다 (Task 4에서 EngineFailed로 매핑). displayID를
        // 남겨 호출자가 고아 디스플레이를 추적할 수 있게 한다 — remove는
        // 설계상 미구현이고 프로세스 종료 후 생존 여부도 미실측이다.
        fail("FAILED settings displayID=\(displayID)", code: 1)
    }

    // 생성자 반환과 apply 참은 등록 보장이 아니다 — WindowServer 등록과
    // 요청 모드 도달을 폴링으로 확인한 뒤에만 OK를 말한다. 타임아웃이면
    // "된 셈 치고" OK를 출력하지 않는다.
    guard pollRegistration(displayID: displayID) else {
        fail("FAILED registration timeout displayID=\(displayID)", code: 1)
    }
    guard pollRequestedMode(displayID: displayID, width: options.width, height: options.height) else {
        fail("FAILED mode timeout displayID=\(displayID)", code: 1)
    }

    print("OK \(displayID)")
    exit(0)
}

// MARK: - remove (의도적 미구현)

func runRemove() -> Never {
    // CGVirtualDisplay에는 공개된 파괴 호출이 없고, 객체 해제(또는 프로세스
    // 종료)가 디스플레이를 없애는지는 아직 실측되지 않았다 — 스파크의 소멸
    // 관측은 헤드리스 생성 실패로 실행되지 않았다(미해결 질문). 서브프로세스
    // 수명과 디스플레이 수명의 관계가 정해져야 remove 설계(상주 프로세스
    // 포함)가 가능하므로, 그 판정은 승격 ADR 범위다. 실험 범위 밖.
    fail("FAILED remove is not implemented yet by design (R-015 experiment scope)", code: 3)
}

// MARK: - 진입

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    fail("FAILED usage: cgvd-shim <probe|create|remove> [options]", code: 2)
}

switch arguments[1] {
case "probe":
    runProbe()
case "create":
    runCreate(arguments)
case "remove":
    runRemove()
default:
    // 서브커맨드를 그대로 되돌리지 않는다 — argv에 개행이 섞이면 계약 한 줄이
    // 두 줄로 깨져 호출자의 파싱을 오염시킨다 (리뷰에서 재현된 사례).
    fail("FAILED unknown subcommand", code: 2)
}
