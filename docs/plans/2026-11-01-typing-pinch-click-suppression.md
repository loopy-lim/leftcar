# 타이핑 중 핀치 오클릭 억제 (GXR 손 추적 포인터)

날짜: 2026-11-01
상태: 구현

## 배경과 증상

사용자 관찰: Galaxy XR에서 키보드를 치는 동안 의도치 않은 핀치가 감지되어 클릭이 발생한다. 스트리밍 패널과 KM 경계 공유 두 모드 모두에서 나타난다.

원인: GXR 손 추적 포인터는 마우스 계열(`SOURCE_MOUSE` family) MotionEvent로 들어오고, 타이핑 손동작이 핀치(주 버튼 press/release)로 오탐된다. 앱에는 핀치 오탐과 실제 클릭을 구분할 신호가 없어 두 모드의 전달 경로가 이를 Mac으로 그대로 보낸다.

전달 경로(코드 근거):

- 스트리밍: `StreamActivity.forwardPointer`(캡처 전 직접 매핑), `forwardCapturedPointer`(포인터 캡처 후), 첫 클릭 활성화는 `activateRemoteInput`. 물리 키는 `dispatchKeyEvent`에서 라우팅하고 IME 커밋은 `TextInputRelay`로 전송된다.
- KM: `KmAccessibilityService.onMotionEvent`의 physical 포인터 장치가 `captureMouse`로 전역 캡처되고, `KmRuntime.rawMouseCallback.onMouse`의 버튼 전이가 호스트로 전송된다. GXR 물리 키는 `KmAccessibilityService.onKeyEvent` → `KmRuntime.key`로 전달된다.

## 설계 결정

| 항목 | 결정 |
| --- | --- |
| 억제 신호 | 마지막 키 활동(물리 키 DOWN, IME 텍스트·키페어 전송) 뒤 800ms |
| 억제 대상 | 포인터 버튼 press/release 전달만. 이동·휠·스크롤·커서 평면은 유지한다 |
| 쌍 보존 | 억제된 press의 release도 억제하고, 전송된 press의 release는 항상 전송한다 — 호스트에 눌린 버튼이 남지 않는다 |
| 비트마스크 일관성 | 억제 중 버튼은 전송 이벤트의 `buttonState`에서 0으로 유지한다 |
| 활성화 게이트 | 스트리밍 첫 클릭(포인터 캡처 획득)도 타이핑 창 안에서는 시작하지 않는다 |
| 구조 | 시계 주입 가능한 순수 정책 `input.TypingClickGuard` + 스트리밍·KM 각 와이어링 |
| 스레드 | KM은 main(onKeyEvent 기록)과 worker(rawMouseCallback 억제)가 같은 상태를 쓰므로 정책 메서드를 동기화한다 |
| 범위 밖 | KM macToAndroid 소유 중 GXR 로컬에서 시스템으로 디스패치되는 핀치 클릭. 접근성 관찰만으로는 소비할 수 없고 동적 grab 정책 변경이 필요해 후속 과제로 둔다 |

## 수락 기준과 작업

- AC-1 스트리밍: 키 활동 직후의 포인터 버튼 press/release는 호스트로 전송되지 않는다.
- AC-2 스트리밍: 타이핑 창 안의 패널 클릭은 원격 소유권 획득을 시작하지 않는다.
- AC-3 KM: GXR 물리 키 활동 직후의 포인터 버튼 전이는 호스트로 전송되지 않는다.
- AC-4 press→release 쌍 일관성: 실제 전송된 press의 release는 억제되지 않는다.
- AC-5 창 만료 뒤 클릭은 정상 전달되고, 이동·휠·스크롤은 항상 전달된다(기존 동작 회귀 없음).

- T1 `input.TypingClickGuard` 순수 정책과 단위 테스트(시계 주입, 창 경계, 쌍 억제, 버튼 정리)
- T2 `StreamActivity` 와이어링: 키 활동 기록, 두 전달 경로 버튼 억제와 buttonState 정리, 활성화 게이트 — Robolectric 회귀 테스트
- T3 `KmRuntime`·`KmAccessibilityService` 와이어링: 키 활동 기록과 `rawMouseCallback` 버튼 억제

## 검증

- V-1 `TypingClickGuardTest` 통과 (순수 JVM)
- V-2 스트리밍 Robolectric 테스트 — 타이핑 직후 클릭 억제, 창 만료 뒤 전달, 활성화 미시작 통과
- V-3 기존 viewer Android 단위 테스트 전체 통과(회귀 없음)와 아키텍처 검사에서 이 변경이 추가하는 신규 위반 0 — HEAD 기준 32개 기존 위반은 이 작업 이전 상태로 별도 후속
- 후속(범위 밖 기록): 억제된 press에도 로컬 탭 에코·HUD 표시가 유지되는 시각 불일치(F-4, 선택 개선), KM macToAndroid 소유 중 GXR 로컬 시스템 클릭 억제(동적 grab 정책)
- V-4 실기기(GXR) 타이핑 중 핀치 오클릭 소멸 — 실기기 핀치 재현은 사용자 환경이 필요하다. 이 계획의 코드 검증은 V-1~V-3까지이며 V-4는 사용자 확인 항목으로 남긴다

## 실행 증거 (2026-11-01 완료 시점)

| 검증 | 방법 | 실제 |
| --- | --- | --- |
| V-1·V-2·V-3 | `./gradlew :app:testDebugUnitTest --rerun-tasks` (독립 평가자 재실행 포함, env: ANDROID_HOME=/home/loopy/android-sdk, NDK 27.1.12297006) | BUILD SUCCESSFUL, 전체 304 tests / 0 failures — TypingClickGuardTest 8건 + StreamTypingClickTest 4건(캡처 경로 포함) PASS |
| V-3 아키텍처 | `bun run test:architecture` | exit 1(HEAD와 동일한 기존 위반 32건), 이 변경 파일 참조 위반 0건 |
| G3 코드 리뷰 | 독립 평가자 2회(초판 FAIL F-1 P2 → 유령 쌍 무효화 수정 후 재판정) | 최종 PASS — F-1~F-3 닫힘, F-4는 기록된 후속 |
| React Doctor | 해당 없음 판정 | 이 변경은 순수 Android Kotlin — React/TSX/스타일 미대상 |
| V-4 실기기 | 미수행 | BLOCKED — 사용자의 GXR 환경에서 확인 필요 (800ms 창 체감 포함) |
