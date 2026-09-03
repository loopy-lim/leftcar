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
cd tools/cgvd-shim && swift build
# 바이너리: .build/debug/cgvd-shim (배포는 --release)
```

## stdout 계약 (한 줄 — Rust `parse_cgvd_line`이 이 형식을 파싱한다)

| 서브커맨드 | 출력 | 종료 코드 |
|---|---|---|
| `probe` | `EXISTS` / `MISSING` | 항상 0 (MISSING도 정상 답) |
| `create` | `OK <displayID>` | 0 |
| `create` | `UNAVAILABLE session` | 1 — GUI 로그인 세션 밖 실행 (분류 A) |
| `create` | `NOACTIVE` | 1 — 활성 화면 0개, 덮개 개방/외장 모니터 필요 (분류 B) |
| `create` | `FAILED displayID=0` | 1 — 세션·화면 정상인데 생성 실패, API 문제 (분류 C) |
| `create` | `FAILED private API missing on this macOS` | 1 — 클래스 부재 (probe가 MISSING인 상태로 create 실행) |
| `create` | `FAILED settings` | 1 — 생성은 됐는데 요청 크기 모드 적용 실패 |
| `create` | `FAILED usage: ...` | 2 — 플래그 오류 |
| `remove` | `FAILED remove is not implemented yet by design (R-015 experiment scope)` | 3 |

계약 외 텍스트를 stdout에 쓰지 않는다 — Rust 쪽은 첫 줄만 읽는다.
진단 3분류(A 세션 밖 / B 클램쉘·헤드리스 / C API 문제)는 스파크의 분류를
그대로 따른다.

`create` 플래그: `--name=<n> --width=<w> --height=<h>` (기본값
"Leftcar Virtual", 1920x1200). 빈 이름·0 이하 크기는 사용법 오류(exit 2)다.

## 실행 예

```zsh
.build/debug/cgvd-shim probe    # 어떤 세션에서든 가능 (클래스 존재만 묻는다)
.build/debug/cgvd-shim create   # 반드시 GUI 세션(터미널 앱)에서
```

**create 실측은 반드시 터미널 앱(Ghostty/Terminal 등)에서.** SSH·Claude 자동화
셸은 WindowServer 세션에 붙지 못해 `UNAVAILABLE session`이 나온다. 이것은
버그가 아니라 분류 A의 정상 동작이다.

## 구현 노트

- **헤더 임포트**: `Sources/cgvd-shim/include/module.modulemap`으로 `module
  CGVD { header "CGVD.h" }`를 노출하고 Swift에서 `import CGVD`로 쓴다.
  스파크는 `swiftc -import-objc-header`를 썼지만 SwiftPM에는 모듈맵이 정석이고,
  `cSettings`의 `-I` 플래그가 그 경로를 건넨다.
- **스레드**: 스파크는 `DispatchQueue.main` + AppKit main thread 세마포어로
  runloop을 살렸다. CLI에는 runloop이 없으므로 전용 직렬 큐 + `queue.sync {}`
  배수 + 0.5초 안정화 대기로 단순화했다. 스파크는 성공/실패가 큐 선택과
  무관함을 이미 A/B로 확인했다(리서치 §2 표).
- **시리얼**: 상수 `0x20260903`. 이름 파생 해시보다 재현 가능성이 낫다.
- **`probe`는 4종 전부 확인**: descriptor만 보면 create이 쓰는 나머지 클래스가
  빠진 macOS를 EXISTS로 오판한다. `NSClassFromString` 조회라 파손 시에도
  크래시가 아니라 `MISSING`이라는 정상 답을 낸다. `create`도 직접 참조 전에
  같은 조회로 먼저 잘라낸다 — 클래스가 없으면 nil 이니셜이 함정에 빠지기
  때문이다. 링크는 `-weak_framework CoreGraphics`로 약하게 묶어 파손된
  macOS에서도 바이너리 자체는 로드된다(`nm -m`으로 weak external 확인).
- **remove 미구현은 설계다**: CGVirtualDisplay에는 공개된 파괴 호출이 없다.
  create이 OK를 출력한 뒤 이 프로세스는 곧 종료되는데, **객체 해제·프로세스
  종료 후에도 디스플레이가 남아 있는지는 미실측이다** — 스파크의 소멸 관측은
  헤드리스 생성 실패로 실행되지 않았다. 남아 있으면 spawn-and-exit 모델이
  그대로 유효하고, 사라진다면 상주 프로세스 설계가 필요하다. 둘 다 승격 ADR이
  판정할 사항이며, 1차 실측은 작업 10의 물리 생성 테스트다.
- `Package.swift`의 `.unsafeFlags`는 이 패키지를 다른 SwiftPM 패키지의
  의존성으로 못 쓰게 만들지만, 독립 실행 파일이라 무해하다.
