---
date: 2026-09-03T19:40:00+09:00
researcher: loopy-lim
git_commit: 70b81e7 (스파크 도구 기준)
branch: main
repository: leftcar
topic: "CGVirtualDisplay 스파크 실측 결과 — macOS 26 private API 생존 확인과 헤드리스 생성 불가 확정"
tags: [research, spark, cgvirtualdisplay, virtual-display, clamshell, headless, evidence]
status: complete
last_updated: 2026-09-03
last_updated_by: loopy-lim
---

# 리서치: CGVirtualDisplay 스파크 실측 결과 (2026-09-03)

**날짜**: 2026-09-03 19:40 +0900
**연구자**: loopy-lim
**기기**: MacBook Pro, Apple M1 Max (32코어 GPU), macOS 26.6.2 (25G83), Xcode 26.2 (Swift 6.2.3)
**도구**: `tools/cgvd-spark/` (커밋 `70b81e7`)
**근거 문서**: `docs/research/2026-09-03_opendisplay-comparison.md` 미해결 질문 1·2, `docs/research/2026-09-03_next-work-candidates.md` 3번

## 연구 질문

OpenDisplay가 쓰는 private `CGVirtualDisplay` API가 이 머신의 macOS 26에서 동작하는가 (질문 1). Leftcar의 SCK 캡처 경로와 정합하는가 (질문 2).

## 요약

스파크 목표 중 **API 생존과 설계 방향은 확정**됐다. 실제 생성 성공 실측은 물리 활성 화면이 필요해 다음 세션으로 남는다.

1. **private API 4종 모두 macOS 26.6.2에 생존** — `CGVirtualDisplay`(21 메서드), `CGVirtualDisplayDescriptor`(34), `CGVirtualDisplaySettings`(12), `CGVirtualDisplayMode`(7). "macOS 업데이트 시 파손" 리스크(리서치 §5)는 현 버전에서 실제화되지 않았다.
2. **기존 공개 헤더에 없던 신규 표면 실측** — `CGVirtualDisplaySettings.rotation`(회전을 모드 재적용 없이 settings에서 직접 지정 가능성), `isReference`, `refreshDeadline`, `CGVirtualDisplayMode.transferFunction`(색상 전송 함수 지정 가능성), descriptor의 `redPrimary`/`greenPrimary`/`bluePrimary`/`whitePoint`/`displayInfo`/`serialNumber`(별칭). OpenDisplay가 쓰지 않는 기능이라 후속 실험 가치가 있다.
3. **완전 헤드리스에서 생성 불가 확정** — 클램쉘 닫힘 + HDMI 모니터 전원 꺼짐(IOKit에 EDID 없음, 활성 `IOFramebuffer` 0개) 상태에서 `CGVirtualDisplay(descriptor:)`가 `displayID=0`으로 실패. 백그라운드 디스패치 큐 A/B로도 동일. 세션 종류와 무관하게 **활성 화면 0개는 생성 자체를 막는다**.
4. **같은 상태에서 BetterDisplay CLI도 abort(134)** — GUI 상주 앱과의 XPC가 이 상태에서 막힌다. 2026-09-02 CLI 생성 성공 전례와의 차이는 세션이 아니라 활성 화면 유무다.
5. **설계 확정**: Leftcar "덮개 닫힘 모드"(태블릿이 유일 화면)는 Sidecar/BetterDisplay/Duet과 같은 **"만들고 닫기"** 구조여야 한다 — 활성 화면이 있을 때 가상 디스플레이를 먼저 생성하고, 이후 덮개가 닫혀도 가상 디스플레이가 외부 출력으로 남아 세션이 생존한다. 0디스플레이에서 직접 생성하는 경로는 없다(업계 전부 동일 전제, OpenDisplay에도 `awaitingWake` 처리만 존재).

## 상세 분석

### 1. 실측 방법

`tools/cgvd-spark/`의 두 도구로 나눠 측정했다.

- `cgvd-exist.swift` — 세션과 무관(SSH/자동화 셸에서 가능). objc 런타임에 클래스 존재와 메서드 수를 묻는다.
- `cgvd-spark`(main.swift) — GUI 세션 실측용. 생성 → 1x 모드 → 120Hz 시도 → HiDPI @2x → SCK `SCShareableContent` 정합 → 회전(모드 재적용) → 60초 관찰(롤백 감시) → 동시 2개 생성/소멸. 실패 시 원인을 3분류(A 세션 밖 / B 클램쉘·헤드리스 / C API 문제)로 자동 진단한다.

실행은 `zsh tools/cgvd-spark/run-probe.zsh` (약 80초).

### 2. 측정 결과

| 항목 | 결과 |
|---|---|
| 클래스 4종 존재 | ✅ EXISTS (메서드 수 위 표) |
| 신규 표면 | ✅ rotation/isReference/refreshDeadline/transferFunction/색좌표/displayInfo |
| 헤드리스 생성 | ❌ displayID=0 (main 큐, 백그라운드 큐 모두) |
| 실패 원인 진단 | 분류 A+B 동시 충족: CGSession nil + IOFramebuffer 0 |
| BetterDisplay CLI | ❌ abort(134) — 같은 물리 상태 |
| 120Hz/HiDPI/SCK/회전/60초 관찰 | 미측정 (생성 선행 실패) |

### 3. 헤드리스 판별 근거

- `ioreg -r -k AppleClamshellState` → `AppleClamshellState = Yes` (덮개 닫힘)
- IOKit `"Description" = "DP or HDMI Adapter"` 커넥터는 인식되나 **EDID 부재** — 모니터 전원 꺼짐/절전
- `IOFramebuffer` 활성 인스턴스 0 — 시스템 전역에 활성 출력 없음
- `CGGetActiveDisplayList` 0, `CGSessionCopyCurrentDictionary` nil (SSH 계열 자동화 세션)
- `caffeinate -u`는 세션 밖에서 `UserIsActive` assertion 생성 자체가 거부됨

### 4. 세션 제약과 개발 환경 (재발 방지)

Claude Code 자동화 셸과 화면 공유(Screen Sharing)로 접속해 띄운 터미널은 **모두 GUI 로그인 세션 밖**이라 디스플레이 컨텍스트가 없다. `ps` 세션 번호가 0으로 같아도 마찬가지다. 디스플레이 생성·창 조작·스크린샷 실측은 **물리 활성 화면이 켜진 상태에서 사람이 직접** 실행해야 한다. 전체 절차 인덱스는 `docs/dev-environment.md`에 정리돼 있다.

### 5. 제품 시나리오에의 함의 — 덮개 닫힘 모드 (ADR 후보)

"덮개를 닫고 태블릿만으로 Mac을 조작"(Sidecar가 지원하는 시나리오)을 Leftcar가 구현하려면:

1. 활성 화면 존재 시(덮개 열림 또는 외장 모니터 켜짐) 앱이 가상 디스플레이를 **먼저 생성** — 크기는 자유(모드 재적용 resize), `Settings.rotation` 표면으로 회전 직접 지정 가능성도 있다
2. 덮개를 닫으면 내장 패널은 꺼지지만 가상 디스플레이가 "외부 출력"으로 남아 **세션 생존**
3. 앱이 `caffeinate -s`(AC 전원) assertion 유지, 필요 시 관리자 1회 `pmset disablesleep` 헬퍼로 절전 이중 방어
4. 태블릿이 USB/Wi-Fi로 스트리밍 + 역방향 입력

검증 방법은 `tools/cgvd-spark/README.md` "제품 시나리오" 절에 기록: 프로브 실행(60초 관찰 구간) 중간에 덮개를 닫아 관찰 로그로 생존 여부를 확인한다. 이 실측이 기능 ADR의 1차 evidence가 된다.

## 코드 참조

- `tools/cgvd-spark/CGVD.h` — private API 최소 선언 (OpenDisplay 코드 복사 아님, 공개 시그니처 참고 직접 작성)
- `tools/cgvd-spark/main.swift` — GUI 세션 프로브 (헤드리스 시도 + 원인 3분류 포함)
- `tools/cgvd-spark/cgvd-exist.swift` — 세션 무관 클래스 존재 실측
- `tools/cgvd-spark/run-probe.zsh` — 1·2단계 통합 실행
- `docs/dev-environment.md` — Aerospace·세션·클램쉘 제약 기록
- `docs/research/2026-09-03_opendisplay-comparison.md:70-82` — 연속 집행 루프 등 흡수 대상 운영 지식
- OpenDisplay 로컬 사본 `/tmp/opendisplay/Mac/MacSender.swift` — `awaitingWake`(상대 절전 대기) 처리 참조

## 아키텍처 인사이트

1. **가상 디스플레이의 생성 전제는 "활성 화면 1개"다.** 헤드리스 직접 생성은 macOS가 막으며, 이는 Sidecar/BetterDisplay/OpenDisplay 전부가 "만들고 닫기" 구조인 이유다. Leftcar의 BetterDisplay 래퍼(ADR-0005)도 이 전제를 따른다.
2. **R-015 판정에 필요한 다음 evidence는 2개로 좁혀졌다**: (a) 활성 화면 상태에서 CGVirtualDisplay 생성 성공 실측, (b) 생성 후 덮개 닫힘 생존 관찰. 둘 다 `run-probe.zsh` 한 번에 포함돼 있다.
3. **신규 표면(rotation, transferFunction)은 OpenDisplay가 쓰지 않는 기능** — 채택 시 BetterDisplay 래퍼 대비 우위가 될 수 있는 영역이다. 다만 private API 리스크는 동일하다.

## 관련 리서치

- `docs/research/2026-09-03_opendisplay-comparison.md` — 스파크의 근거 (미해결 질문 1·2 출처)
- `docs/research/2026-09-03_next-work-candidates.md` — 스파크를 3순위 과제로 선정한 문서
- `docs/EVIDENCE.md` (가상 디스플레이 실험 섹션) — 9/2 BetterDisplay CLI 실기 성공 전례

## 미해결 질문

1. 활성 화면 존재 시 생성 성공 여부와 120Hz/HiDPI/SCK 정합 실측 — **덮개 개방(또는 모니터 전원) 후 `run-probe.zsh` 1회**로 해소 예정. 절차·판정 기준은 README에 기록됨.
2. 덮개 닫힘 생존 관찰(클램쉘 모드) — 같은 실행 중 덮개를 닫는 것으로 측정.
3. `Settings.rotation`/`transferFunction`의 실제 효과 — 생성 성공 확인 후 별도 스파크 과제.
4. CGVirtualDisplay 자체 구현 전환 여부 — 위 1·2 결과를 evidence로 별도 ADR에서 판정 (R-015 논골 유지).

## 다음 세션 재개 가이드 (2026-09-03 기준)

중단 시점: 스파크 도구·결과 문서까지 커밋 완료(`408bb82`). 생성 실측만 물리 조작 대기.

**재개 조건**: 덮개를 열어 로그인하거나 외장 모니터 전원을 켠 뒤(HDMI 어댑터는 이미 인식됨 — 모니터가 EDID를 주기만 하면 됨).

**재개 절차 (순서대로)**:

1. `zsh tools/cgvd-spark/run-probe.zsh` — 출력 전체를 보존한다.
   - `before:` 뒤에 활성 디스플레이가 보이는지 먼저 확인 (0이면 여전히 헤드리스 — 진단 분류 참조)
   - 생성 성공 시 120Hz 상한/HiDPI @2x/SCK 정합/회전/60초 관찰이 자동 진행
2. **(선택, 클램쉘 모드 실측)** 60초 관찰 구간(`[NOTE] 60초 관찰 시작` 출력 직후)에 덮개를 닫는다 → 관찰 로그에 가상 디스플레이가 유일 출력으로 생존하는지 기록된다. 이것이 "덮개 닫힘 모드" ADR의 1차 evidence.
3. 결과를 이 문서에 추가하고 (프론트매터 `last_updated` 갱신) 다음을 판정한다:
   - 생성 성공 + 관찰 안정 → 미해결 질문 1·2 종료, CGVirtualDisplay 자체 구현 스파크 ADR 작성 착수
   - 생성 실패 + 분류 C → private API 한계 확정, ADR-0005(BetterDisplay 래퍼) 유지 기록
4. 병행 대기 작업: T11 검증 4(60분 USB soak)·검증 6 잔여(앱 UI 경유 VD) — `docs/usb-physical-validation.md` 절차 참조. 폰 `HA2D6EMP` 필요.

**금지**: R-015 논골 — 이 스파크가 성공해도 정식 기능 승격이 아니라 별도 ADR 판정 대상이다. GPL-3.0(OpenDisplay) 코드 복사 금지 원칙도 유지.
