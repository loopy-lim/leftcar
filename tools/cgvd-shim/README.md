# CGVD shim (실험 — R-015 논골 유지)

`tools/cgvd-spark/` 스파크에서 확정한 private `CGVirtualDisplay` API를
tablet-display의 `CgvdProvider`(Rust)가 서브프로세스로 호출하기 위한 최소
Swift 실행 파일. **실험이지 정식 기능이 아니다** — 기본 프로바이더 승격은
스파크 evidence에 근거한 별도 ADR 없이는 없다
(`docs/09-risk-register.md` R-015, `docs/research/2026-09-03_cgvd-spark-results.md`).

private API를 직접 부른다: macOS 버전이 오르면 언제든 파손될 수 있고 그 때
이 shim은 `probe`가 `MISSING`을 답하는 것으로 조용히 무능해진다. 파손 시
대안은 ADR-0005의 BetterDisplay 래퍼다.

## 빌드

```zsh
cd tools/cgvd-shim && swift build -c release
# 바이너리: .build/release/cgvd-shim — CgvdProvider 기본 탐색 경로
# (provider.rs DEFAULT_SHIM_PATH: tools/cgvd-shim/.build/release/cgvd-shim)와 일치
```

## stdout 계약 (응답마다 한 줄)

| 서브커맨드 | 출력 | 종료 코드 |
|---|---|---|
| `probe` | `EXISTS` / `MISSING` | 항상 0 (MISSING도 정상 답) |
| `inspect --display-id=<id>` | `INSPECT <displayID> <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight> <x> <y>` | 0 — 별도 fresh process에서 공개 CoreGraphics 상태를 한 번 조회 |
| `create` | `READY <displayID> <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight>` | 0 — 등록·논리 모드·backing pixel까지 확인 후 출력하고 stop/EOF까지 상주 |
| 상주 세션 stdin | `PLACE <requestID> <x> <y>` | `PLACED <requestID> <displayID> <x> <y> <width> <height> <primaryBefore> <primaryAfter>` — 요청 좌표와 실제 bounds가 일치하고 주 디스플레이가 유지된 경우에만 성공 |
| 상주 세션 stdin | 잘못되거나 적용 실패한 `PLACE` | `FAILED <requestID> <detail>` — 요청별 한 줄 응답, 응답이 없거나 실패하면 Rust provider도 성공으로 처리하지 않음 |
| 상주 세션 stdin | `RESIZE <width> <height> <scale>` | `RESIZED <logicalWidth> <logicalHeight> <pixelWidth> <pixelHeight>` — CGVirtualDisplaySettings를 새 모드(hiDPI=scale==2, 60Hz)로 재적용하고 도달한 관측 모드를 READY와 같은 형식으로 답한다. 뒤이은 `PLACE`는 새 논리 크기를 기대 bounds로 검증한다 |
| 상주 세션 stdin | 잘못되거나 적용 실패한 `RESIZE` | `FAILED resize displayID=<id> <detail>` — PLACE와 같은 요청별 한 줄 실패다. **프로세스를 종료하지 않는다**: 세션이 곧 디스플레이 수명이므로 한 요청 실패로 세션 전체를 잃지 않고 계속 받는다. descriptor.maxPixels 생성 크기 초과 등은 apply가 거절할 수 있고 그 경우 mode/settings detail로 답한다 |
| `create` | `FAILED registration timeout displayID=<id>` | 1 — 2초 내 활성 등록 미확인 |
| `create` | `FAILED mode timeout displayID=<id>` | 1 — 2초 내 요청 크기 모드 미도달 |
| `create` | `UNAVAILABLE session` | 1 — GUI 로그인 세션 밖 실행 (분류 A) |
| `create` | `NOACTIVE` | 1 — 활성 화면 0개, 덮개 개방/외장 모니터 필요 (분류 B) |
| `create` | `FAILED displayID=0` | 1 — 세션·화면 정상인데 생성 실패, API 문제 (분류 C) |
| `create` | `FAILED private API missing on this macOS` | 1 — 클래스 부재 (probe가 MISSING인 상태로 create 실행) |
| `create` | `FAILED settings displayID=<id>` | 1 — 생성은 됐는데 요청 크기 모드 적용 실패 (고아 추적용 id) |
| `create` | `FAILED usage: ...` | 2 — 플래그 오류 |
| `remove` | `FAILED remove requires a live create session` | 2 — Rust provider가 해당 create 프로세스 stdin에 `stop`을 보냄 |

계약 외 텍스트를 stdout에 쓰지 않는다. Rust는 첫 `READY` 뒤에도 세션 응답을
계속 읽으며 각 `PLACE`와 같은 requestID의 한 줄만 해당 요청의 결과로 인정한다.
진단 3분류(A 세션 밖 / B 클램쉘·헤드리스 / C API 문제)는 스파크의 분류를
그대로 따른다.

`create` 플래그: `--name=<n> --width=<w> --height=<h>` (기본값
"Leftcar Virtual", 1920x1200). 빈 이름·0 이하 크기는 사용법 오류(exit 2)다.

## 실행 예

```zsh
.build/release/cgvd-shim probe    # 어떤 세션에서든 가능 (클래스 존재만 묻는다)
.build/release/cgvd-shim create   # 반드시 GUI 세션(터미널 앱)에서
```

`create`는 성공 응답 후에도 CGVirtualDisplay 객체를 보유한 채 stdin을
기다린다. 같은 세션에 `PLACE <requestID> <x> <y>`를 보내면 디스플레이를 만든
소유 프로세스가 배치를 적용한다. Rust provider는 세션별 요청을 직렬화하고
3초 안에 일치하는 requestID, displayID, 실제 bounds, 보존된 주 디스플레이를
담은 `PLACED`를 받은 경우만 성공으로 처리한다. 같은 세션의
`RESIZE <width> <height> <scale>`는 생성과 같은 apply 경로를 재사용해 모드를
바꾸고, 도달한 관측 모드를 `RESIZED <w> <h> <pw> <ph>`로 답한다 — 스트림
재시작 없이 가상 화면 크기를 조정하는 호스트 resize(Task 3 provider.rs)가 이
명령을 쓴다. RESIZE 실패는 세션을 끊지 않는다. 호출자가 `stop` 한 줄을 보내거나
stdin을 닫으면 객체를 해제하고 종료하므로, displayID별 프로세스 수명과 제거
대상이 일치한다.

**create 실측은 반드시 터미널 앱(Ghostty/Terminal 등)에서.** SSH·Claude 자동화
셸은 WindowServer 세션에 붙지 못해 `UNAVAILABLE session`이 나온다. 이것은
버그가 아니라 분류 A의 정상 동작이다.

## 구현 노트

- **헤더 임포트**: `Sources/cgvd-shim/include/module.modulemap`으로 `module
  CGVD { header "CGVD.h" }`를 노출하고 Swift에서 `import CGVD`로 쓴다.
  스파크는 `swiftc -import-objc-header`를 썼지만 SwiftPM에는 모듈맵이 정석이고,
  `cSettings`의 `-I` 플래그가 그 경로를 건넨다.
- **스레드**: 스파크는 `DispatchQueue.main` + AppKit main thread 세마포어로
  runloop을 살렸다. CLI에는 runloop이 없으므로 전용 직렬 큐로 단순화했다.
  스파크 A/B가 확인한 큐 무관성은 **실패 경로**(헤드리스 displayID=0이 두
  큐에서 동일)뿐이다 — 이 큐에서의 성공 경로는 실측 대상이며, 고정 대기 대신
  등록·모드 폴링(최대 2초, 100ms 간격)으로 확인해 OK의 의미를 "생성자
  반환"이 아니라 "등록+요청 모드 확인"으로 강화했다.
- **argv 되돌림 없음**: 개행이 섞인 서브커맨드를 그대로 출력하면 계약 한
  줄이 두 줄로 깨진다. `FAILED unknown subcommand`로 고정해 출력했다.
- **시리얼**: Rust provider가 세션마다 0이 아닌 UInt32 시리얼을 생성해
  전달한다. 이름 해시나 고정값을 사용하지 않아 반복 생성이 충돌하지 않는다.
- **`probe`는 4종 전부 확인**: descriptor만 보면 create이 쓰는 나머지 클래스가
  빠진 macOS를 EXISTS로 오판한다. `NSClassFromString` 조회라 파손 시에도
  크래시가 아니라 `MISSING`이라는 정상 답을 낸다. `create`도 직접 참조 전에
  같은 조회로 먼저 잘라낸다 — 클래스가 없으면 nil 이니셜이 함정에 빠지기
  때문이다. 링크는 `-weak_framework CoreGraphics`로 약하게 묶어 파손된
  macOS에서도 바이너리 자체는 로드된다(`nm -m`으로 weak external 확인).
- **수명/제거**: CGVirtualDisplay에는 공개된 파괴 호출이 없으므로 create
  프로세스가 객체를 보유한다. Rust는 displayID별 프로세스를 stop하고 정상
  종료를 기다리며, provider Drop도 남은 세션을 정리한다.
- `Package.swift`의 `.unsafeFlags`는 이 패키지를 다른 SwiftPM 패키지의
  의존성으로 못 쓰게 만들지만, 독립 실행 파일이라 무해하다.
