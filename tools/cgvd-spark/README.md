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
| 120Hz 요청 → 60으로 강제 | 60Hz 상한 확인 |
| `pixelWidth == width*2` 모드 존재·적용 | HiDPI @2x 경로 유효 |
| SCShareableContent에 등장 | Leftcar SCK 캡처 경로와 정합 (질문 2 전제) |
| 60초 관찰에서 모드·origin 무롤백 | OpenDisplay류 연속 집행 루프 필요성 재평가 자료 |

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
