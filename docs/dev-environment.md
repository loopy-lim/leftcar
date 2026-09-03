# 개발 환경 노트 (실측·테스트용)

상태: 상시 갱신 문서. 기기/세션 제약처럼 "문서로 남겨야 다음 사람(또는 에이전트)이
재시도하지 않는 것"을 기록한다. 2026-09-03 작성.

## 머신

- MacBook Pro, Apple M1 Max (32코어 GPU), macOS 26.6.2 (25G83), Xcode 26.2 (Swift 6.2.3, SDK macOS 26.2)
- 테스트 폰: Lenovo TB710FU, Android 16, 시리얼 `HA2D6EMP` (docs/EVIDENCE.md 실기 표본과 동일)

## 창 관리: Aerospace

- 이 머신은 **Aerospace**(타일링 창 관리자)를 쓰며, `option+1..9`로 화면(워크스페이스) 전환을 한다.
- 테스트 시 유의점:
  - 가상 디스플레이 프로브(`tools/cgvd-spark`)가 디스플레이를 잠깐 만들면 Aerospace가
    새 출력을 하나의 화면으로 인식한다. 프로브는 종료 전 스스로 정리하므로 수동 복구는 불필요.
  - UI 자동화(Accessibility/System Events)나 창 위치 가정이 필요한 검증은 Aerospace 단축키·
    워크스페이스 상태와 간섭할 수 있다 — 창 좌표 어설션은 전환 후 기준으로 잡을 것.

## 세션 제약: 자동화 셸은 WindowServer(GUI) 세션 밖이다

- Claude Code 자동화 셸(SSH 계열 세션, `com.apple.access_ssh` 소속)에서는:
  - `CGGetActiveDisplayList` → 0, `CGSessionCopyCurrentDictionary` → nil
  - `open`, Apple Events(System Events), `NSWorkspace.openApplication` → 전부
    `-10827 kLSNoExecutableErr`/`-1728`로 실패 (EVIDENCE.md 검증 6의 -10827 기록과 동일 근원)
- 따라서 **디스플레이 생성·모드 변경·스크린샷·창 조작이 필요한 실측은 사람이
  GUI 터미널(Ghostty 등)에서 직접 실행**해야 한다. 세션 번호(ps SESS)가 0으로 같아도
  SSH 유래 프로세스는 WindowServer 세션에 못 붙는다(launchd gui 도메인 spawn 필요).
- 세션과 무관한 확인은 가능하다: objc 런타임 클래스 존재 확인(`cgvd-exist`),
  컴파일, 단위 테스트, 파일/프로세스 관찰 등.

## 실측 절차 인덱스

| 대상 | 절차 | 실행 주체 |
|---|---|---|
| CGVirtualDisplay private API 존재 | `zsh tools/cgvd-spark/run-probe.zsh` 1단계 | 자동화 셸 가능 |
| CGVirtualDisplay 생성/모드/HiDPI/SCK | `zsh tools/cgvd-spark/run-probe.zsh` 2단계 (~80초) | **GUI 터미널에서 직접** |
| USB AOAP 물리 게이트 (T11) | `docs/usb-physical-validation.md` (검증 1~6) | **사람 + 폰 `HA2D6EMP`** |
| BetterDisplay CLI 계약 | ADR-0005 + `docs/EVIDENCE.md` 가상 디스플레이 실험 섹션 | GUI(호스트 앱) |
| 60분 soak·고모션·glass-to-glass | `docs/EVIDENCE.md` E4–E7 표 | 사람 + 기기 |

## Android 디바이스 확인 (자동화 셸에서 가능)

```zsh
adb devices            # 기기 인식 확인
adb shell getprop ro.build.version.release
adb logcat -d -s Leftcar   # 스트리밍 세션 로그 덤프
```
