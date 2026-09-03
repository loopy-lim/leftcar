# CGVirtualDisplay 스파크 (R-015 evidence 수집용)

macOS private `CGVirtualDisplay` API가 이 기기의 macOS에서 여전히 동작하는지
확인하는 하루 스파크. 근거는 `docs/research/2026-09-03_opendisplay-comparison.md`
(미해결 질문 1·2)와 `docs/research/2026-09-03_next-work-candidates.md` 3번 항목.

**이것은 실험이지 정식 기능 승격이 아니다** — R-015(`docs/09-risk-register.md`)의
v1 논골은 유지된다. 코드는 OpenDisplay(GPL-3.0)에서 복사하지 않았고, 공개된
API 시그니처 지식을 참고해 0부터 작성했다.

## 구성

- `CGVD.h` — private API 최소 선언 (직접 작성)
- `cgvd-exist.swift` — 세션 무관 실측: 클래스 존재·메서드 수 (SSH/자동화 셸에서 가능)
- `main.swift` — GUI 세션 실측: 생성 → 1x 모드 → 120Hz 시도 → HiDPI @2x →
  SCK `SCShareableContent` 정합 → 회전(모드 재적용) → 60초 관찰(롤백 감시) →
  동시 2개 생성/소멸
- `run-probe.zsh` — 둘 다 빌드·실행

## 실행

**생성 실측은 반드시 터미널 앱(Ghostty/Terminal 등, GUI 세션)에서.**
SSH·Claude 자동화 셸은 WindowServer 세션에 붙지 못해 활성 디스플레이가
0으로 보인다(프로브가 exit 2로 이를 판정한다).

```zsh
zsh tools/cgvd-spark/run-probe.zsh
```

## 판정 기준 (질문 1)

| 관측 | 의미 |
|---|---|
| 클래스 4종 존재 | API 미파손 (2026-09-03, macOS 26.6.2에서 이미 확인) |
| 생성 성공 + displayID ≠ 0 | private API가 26에서도 실동작 |
| 활성 디스플레이 0 상태에서 생성 성공 | "덮개 닫힘 모드" 직접 생성 경로 열림 |
| 활성 디스플레이 0 상태에서 생성 실패 | 생성 선행("만들고 닫는다") 설계 확정 근거 |
| 120Hz 요청 → 60으로 강제 | 60Hz 상한 확인 |
| `pixelWidth == width*2` 모드 존재·적용 | HiDPI @2x 경로 유효 |
| SCShareableContent에 등장 | Leftcar SCK 캡처 경로와 정합 (질문 2 전제) |
| 60초 관찰에서 모드·origin 무롤백 | OpenDisplay류 연속 집행 루프 필요성 재평가 자료 |

## 제품 시나리오: 덮개 닫힘(클램쉘) 모드 — 태블릿이 유일 화면

Sidecar가 지원하는 "덮개 닫고 태블릿만으로 사용"을 Leftcar가 흉내 낼 구조.
서드파티 검증 절차(BetterDisplay 더미, Duet):

1. 덮개 열림 상태에서 앱이 가상 디스플레이를 **먼저 생성** (크기 자유 — 모드
   재적용으로 resize 가능, 2026-09-03 실측으로 `Settings.rotation` 표면도 확인)
2. 덮개를 닫으면 내장 패널은 꺼지지만 가상 디스플레이가 "외부 디스플레이"로
   세상에 남아 세션 생존 (클램쉘 절전 미트리거)
3. 앱이 `caffeinate -s`(AC) assertion 유지, 필요 시 관리자 1회
   `pmset disablesleep` 헬퍼로 덮개 닫힘 절전 이중 방어
4. 태블릿이 USB/Wi-Fi로 가상 디스플레이 스트리밍 + 역방향 입력

**실측 방법**: 덮개 연 상태로 run-probe.zsh 실행 → 60초 관찰 중간에 덮개를
닫는다. 관찰 로그에 가상 디스플레이가 유일 출력으로 생존하는지 기록된다.
이 결과가 "덮개 닫힘 모드" 기능 ADR의 1차 evidence가 된다.

2026-09-03 상태: 활성 디스플레이 0(클램쉘 닫힘+화면 공유)에서의 생성은
SSH 세션에서 displayID=0 실패. GUI 세션에서 0개 생성이 되는지는 새 프로브
(조기 종료 제거)로 판별 대상 — 성공 시 "직접 생성" 경로도 열린다.

## 2026-09-03 실측 기록 (자동화 셸, 세션 무관 부분)

- macOS 26.6.2 (25G83), M1 Max
- 클래스 4종 모두 존재: `CGVirtualDisplay`(21 methods),
  `CGVirtualDisplayDescriptor`(34), `CGVirtualDisplaySettings`(12),
  `CGVirtualDisplayMode`(7)
- 알려진 헤더에 없던 표면: `CGVirtualDisplaySettings.rotation`/`isReference`/
  `refreshDeadline`, `CGVirtualDisplayMode.transferFunction`,
  descriptor의 `redPrimary`/`greenPrimary`/`bluePrimary`/`whitePoint`/
  `displayInfo`/`serialNumber`(별칭)
- GUI 세션 부재로 생성 실측은 미수행 — 터미널에서 run-probe.zsh로 수행할 것
