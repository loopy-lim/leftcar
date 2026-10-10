# 입력 전용 Mac ↔ Android 공유 디자인·UX 감사

2026-10-03. 읽기 전용 소스 감사. 기준 checkout: `/private/tmp/leftcar-km-20261003`, HEAD `72ff99000ab1981f9773a0b90aad3ee7ca42faad` 및 현재 미커밋 구현. 소스 수정·설치·권한 변경·기기 조작·테스트 재실행 없음.

**결론: 중요한 UX 문제 7개(P1 3개, P2 4개).** 기존 소유권·동의·선택 표현은 유지할 수 있으나, OFF/미확인 상태를 실제 실패와 구분하고 오류 원인 및 종료 확인 상태를 보존해야 한다. 실제 레이아웃·테마·TalkBack/VoiceOver 수용 검증은 이 보고서에서 완료를 주장하지 않는다.

## 기준과 증거 경계

- `ui-design`의 절제·위계·복구·접근성 원칙과 Loop `docs/design/design-system.md`, `ui-ux-guidance.md` §9를 읽었다. **Leftcar 값의 정본은 `packages/ui-tokens`**이다. Loop의 별도 색상·크기·반경·CLI 줄 예산을 Leftcar에 이식하지 않는다.
- Android 화면, 홈/카탈로그 진입, `km-session`, `use-km-model`, 실제 native 상태/오류 경로, Host 승인 패널·기기 행·상태 생산자, 공통 primitives/tokens/KO·EN을 검토했다.
- 현재 Host AX가 “TB522FU 두 방향 허용, Mac 권한 둘 다 허용, 공유 꺼짐, 준비되지 않음/권한 확인 경고”를 동시에 보인다는 Root의 실제 앱 실측 전달이 있다. 감사자가 직접 캡처한 증거는 아니다. 아래 F1은 해당 실측과 독립적으로 소스에서 원인을 확인했다. Root의 CUA 화면 캡처는 두 번 모두 전체 흰색으로 나와 유효한 픽셀 증거가 없으며 재시도를 중지했다. 사용자 Android Start/물리 왕복 확인은 대기 중이다.
- 기존 `input-sharing-view.test.tsx`, `KmConsentPanel.test.tsx`, `km-session.test.ts`, 공통 대비 테스트와 native 복구 보고서를 **읽었으며 실행하지 않았다**. 정적 렌더 문자열·모의 renderer는 실제 픽셀, 줄바꿈, native 키보드 조작이나 화면 읽기 증거가 아니다.

## 우선순위별 발견과 제안

### F1 · P1 · 정상 OFF에 실패 안내가 계속 표시된다

**증거:** Host `src-tauri/src/km.rs:248–277`은 활성 세션이 없으면 Android capability를 모두 false인 기본값으로 만들고 이를 `readiness_reason`에 전달한다. `:839–861`은 해당 값을 미확인으로 구분하지 않고 실제 capability 부족으로 해석한다. `src/pairing/KmConsentPanel.tsx:59`는 모든 `blockedReason`을 `t.km.hostBlocked`로 치환한다. 따라서 Mac 권한과 두 방향 승인이 모두 정상이어도, 아직 Android에서 시작하지 않은 OFF가 “양방향 승인·입력 권한을 확인” 경고가 된다. 정상 사용자가 이미 허용한 설정을 반복 확인하게 된다.

**제안:** 세션 꺼짐, 상대 지원 미확인, 실제 시작 차단을 별도 상태로 전달한다. 정상 OFF는 “Android에서 공유를 시작하세요 / Start sharing on Android”, 미확인은 조용한 설명으로 표시한다. 실제 차단은 안정된 reason code로 KO/EN 원인과 해당 기기의 다음 행동만 표시한다. 잠금/secure input·동의 부족·capability 부족·연결 대기를 동일 권한 경고로 묶지 않는다. 임의 native 문자열을 그대로 노출하는 수정은 피한다.

**확인 필요:** 모든 승인/권한 허용 + OFF, 실제 Mac 권한 부족, secure input, 상대 미확인, 시작 연결 대기를 각각 검증한다. 기존 Host 테스트는 OFF+모든 권한 허용 조합을 다루지 않는다(`KmConsentPanel.test.tsx:7–35`).

### F2 · P1 · 시작/자동 중지 원인이 사라져 지원되는 복구 행동을 알 수 없다

**증거:** Android `app/input-sharing.tsx:31–32`는 `model.error`가 있으면 native reason 안내를 숨기고, 없으면 모든 reason을 일반 `nativeStopped` 문구로 대체한다. `src/km-session.ts:88–96`은 일반 prepare/Host/native 거절을 모두 `startFailed`로 압축한다. 실제 native `KmRuntime.kt:125–136,156,371,415–418`은 권한·KeyBridge 연동 부재·helper 종료·캡처 상실·정리 실패를 구체적으로 구분하지만 UI까지 전달되지 않는다. KeyBridge 연동 실패는 “활성화한 채 지원 업데이트”가 필요하다고 명시한다(`:133`). 그런데 공통 `i18n.ts:41,710`의 unsupported 안내는 다른 입력 접근성 서비스 비활성화를 포괄적으로 제안한다. `docs/input-sharing-setup.md`의 “KeyBridge는 켜둔 채 사용”과 필요한 연동 API 설명도 화면에서 찾을 수 없다.

**제안:** native/Host 실패를 코드·복구 대상·다음 행동으로 정규화하고 하나의 오류 presenter로 표시한다. KeyBridge 물리 키 연동 부족은 “KeyBridge를 유지하고 연동 지원 버전으로 업데이트”로 안내한다. 다른 서비스 충돌과 분리한다. ordinary start/stop cause를 소실하지 않되 session key/token/임의 진단 문자열은 렌더하지 않는다. cleanupFailed는 “종료 미확인”을 유지하고 현재 필요한 수동 접근성 복구 행동과 재시도 Stop을 제공한다.

**확인 필요:** KeyBridge 미연동 + Android 물리 키 장치, 지원하지 않는 mouse observation/capture, Host 승인 부족, Wi-Fi/인증 실패, 회전/권한 상실, native cleanup 실패를 KO/EN으로 검증한다. 기존 테스트 `km-session.test.ts:50–58`은 상세 거절을 generic으로 바꾸는 동작만 확인한다.

### F3 · P1 · 원격 Stop 미확인은 Refresh/재진입에서 지워지고 Retry Stop이 실제 원격 재시도를 하지 않는다

**증거:** `KmSession.stopSession`은 `src/km-session.ts:111–119`에서 active peer를 먼저 제거하고 stopKm 실패 여부와 무관하게 peer를 닫는다. 이후 Stop은 native Stop만 시도하므로 해당 원격 stopKm을 재시도하지 않는다. `use-km-model.ts:17–20`은 Refresh 시 오류를 무조건 null로 바꾼다. native Stop은 성공하고 stopKm만 실패한 경우 native 상태는 OFF이므로 reconcile도 미확인을 복구하지 않는다. `input-sharing.tsx:73–74`는 error가 지워지면 Start로 돌아간다. 재진입 시 PanelState error도 null로 초기화된다(`use-km-model.ts:11`). watchdog은 원격 종료 확인의 증거가 아니다.

**안전성 경계:** native `KmRuntime.kt:390–391`은 Stop에서 별도의 입력 transport를 닫으며 `KmDuplex.kt:69–72`는 TCP socket을 닫는다. Host는 입력 read 실패 또는 500ms peer heartbeat timeout에서 run을 마치고(`km.rs:786–803`), `:359–368,420–433`에서 release_all/local owner/backend stop을 수행한다. 따라서 이 발견은 **원격 입력이 무기한 유지된다는 주장이나 native fail-safe 결함이 아니다.** UI의 종료 확인 지속성과 “재시도 Stop”의 실효성 결함이다. 실제 OS 입력 복원 수용은 별도 검증이다.

**제안:** 앱 전역 세션 모델에 종료 확인 결과를 유지하고, 단순 권한 Refresh로 지우지 않는다. 원격 확인이 실패하면 기기/세션 식별자를 안전하게 보존하여 인증된 재연결 후 stop/status 확인을 수행하거나 “로컬 입력 복원됨 · 원격 종료 확인 필요”로 정확히 안내한다. Retry Stop은 이 미해결 상태를 실제 해결해야 한다. 자동 재시작 없이 확인 후에만 OFF/Start로 전환한다.

**확인 필요:** native Stop 성공 + stopKm 실패 → Refresh → panel remount → Retry Stop. 기존 테스트 `km-session.test.ts:82–98`은 native 실패/cleanupFailed 복구를 다루지만 이 원격 실패 조합과 UI 오류 지속성은 다루지 않는다. native `cleanupFailed`를 유지하는 현재 수정 자체는 긍정적으로 확인했다.

### F4 · P2 · 자동 중지 후 홈 진입점이 사라져 중지 이유/복구를 찾기 어렵다

**증거:** `src/km-entry.ts:2–3`은 연결이 없고 state가 OFF이면 entry를 숨긴다. `InputSharingEntry.tsx:11–16`은 reason을 저장하지 않는다. native 권한/helper/연결 상실로 OFF+reason이 되었을 때 기본 연결도 끊어져 있으면 홈에서 공유 상태와 복구 화면 접근이 사라진다. 카탈로그의 별도 entry도 `model.host` 조건이다(`app/catalog.tsx:699`). active/cleanupFailed 때 진입점을 유지하는 현재 처리는 적절하다.

**제안:** 최근 중지 원인과 미해결 종료 상태도 앱 전역 모델에서 받아 홈 entry를 유지하고 상태를 짧게 덧붙인다. 해소/확인 후 일반 OFF 표현으로 돌아간다. 입력 공유 설정 접근을 영상 카탈로그 조회 성공에 의존시키지 않는다. 시작은 계속 명시적 사용자 행동이어야 한다.

### F5 · P2 · 권한 작업/중지에도 “시작 연결 중”과 Cancel이 나타난다

**증거:** `use-km-model.ts:32–48`은 Start/Stop/Shizuku 요청/설정 열기에 하나의 busy boolean을 사용한다. `input-sharing.tsx:74–75`는 비활성 상태의 모든 busy를 `startCheck`와 `cancelStart`로 표현한다. Shizuku 권한 요청 또는 cleanupFailed의 Stop 재시도 중에도 Start 취소 컨트롤이 나타날 수 있다. requestShizuku/openAccessibility 실패도 non-KmError이므로 `startFailed`로 표시된다(`use-km-model.ts:37`).

**제안:** 최소한 `starting/stopping/requestingPermission/openingSettings`를 구분한다. Cancel은 pending Start에만 표시한다. 각 작업에 맞는 짧은 진행/실패 문구와 복구 행동을 쓴다. 색상·스피너·새 토큰 추가는 필요 없다.

### F6 · P2 · Host 승인 오류가 1초 polling으로 사라지고 설정 열기 실패가 권한 조회 실패로 오인된다

**증거:** `KmConsentPanel.tsx:74`는 성공한 polling 조회마다 error를 지운다. `:80`은 1초 polling, `:90–91`은 저장 실패 후 busy를 해제하므로 saveFailed가 다음 tick에 사라질 수 있다. 설정 열기 실패도 `:93–95`에서 refreshFailed로 표시하며 다음 polling이 지운다. 사용자는 변경이 실패했는지, 설정 창이 열리지 않았는지 충분히 확인하기 어렵다.

**제안:** 조회 실패와 작업 실패를 분리한다. 승인 저장 오류는 다음 명시적 재시도나 해당 변경 확인까지 유지한다. 시스템 설정 열기 실패는 별도 문구로 유지하고 “다시 열기” 또는 수동 설정 경로를 제공한다. 일반 polling 성공으로 작업 오류를 지우지 않는다. alert는 실제 변화에만 알리도록 한다.

### F7 · P2 · 기기가 여러 대일 때 Host 접근성 이름에 대상 기기가 없다

**증거:** `PairedDeviceRow.tsx:84`의 펼침 버튼은 모든 행에서 동일한 공유 제목이며 `aria-label`에 device.name이 없다. 패널 `KmConsentPanel.tsx:25–26`의 두 switch도 방향 이름만 제공한다. 반면 revoke/approve는 기기 이름을 포함한다(`PairedDeviceRow.tsx:93,106`). 버튼/스위치 목록을 통해 이동하는 화면 읽기 사용자는 어떤 기기의 동의를 바꾸는지 구분하기 어렵다. 실제 화면 읽기 동작은 미검증이다.

**제안:** expansion과 direction switch 접근성 이름에 기기 이름을 포함하고, 패널을 기기 이름으로 labelled group/region으로 묶는다. Android 좌우 선택도 arrangement 라벨과 두 radio를 접근성 그룹으로 연결한다. 기존 체크 값/표시/44pt target은 유지한다. 실제 VoiceOver/TalkBack 및 하드웨어 키보드 탐색으로 순서·포커스·선택 읽기를 확인한다.

## 상태별 검토

| 상태 | 현재 확인된 표현/행동 | 판정·필요 개선 |
| --- | --- | --- |
| 초기 설정/권한 조회 | Android 조회 중, Shizuku 설정 안내/요청, 접근성 설정/Refresh; Host 조회 전 switch 비활성 | 소스상 복구 컨트롤 있음. F5/F6의 작업 의미 수정 필요 |
| 권한 허용 + 지원 미확인 | Android는 시작 시 지원 확인 설명, Start는 두 방향 동의와 권한 필요 | capability는 prepare 이후 확인하므로 사전 false 자체로 Start를 차단하면 안 됨. Host는 F1 |
| OFF | Android/Host 공유 꺼짐 | 정상 OFF를 오류로 보여주는 Host F1, 이유 있는 OFF는 F2/F4 |
| 시작 대기 | Android 지원 확인/연결, Start busy, Cancel; 화면 이탈 pending 취소 | 상태 취소 경로 존재. 작업 phase 구분 F5. 실제 지연/포커스 미검증 |
| armed | 양쪽 “켜짐 · 가장자리 이동 대기”, Android Stop, arrangement 잠금 | 문구·행동 소스와 테스트 일치. 실제 edge 체감 미검증 |
| Mac → Android | 양쪽 현재 입력 소유권 문구 | 색 없이 이해 가능. 실제 두 플랫폼 전환/읽기 미검증 |
| Android → Mac | 양쪽 현재 입력 소유권 문구 | 위와 동일 |
| 동의 철회 | 한 방향 철회가 전체 세션 중지라는 KO/EN 설명, Android active toggle 철회 시 stop, Host 저장 명령 분리 | 의미가 명확함. 장시간 실패와 복구 표시는 F2/F3/F6 |
| native 종료 실패 | persistent cleanupFailed, Stop 유지, permission recovery 유지 | 기존 정리 성공 오인 수정은 확인. 구체적 수동 복구는 F2; 실제 helper/로컬 복원 미검증 |
| 원격 종료 실패 | transient stopFailed만 표시 | F3, 확인 상태 지속/실효성 있는 retry 필요 |

## 토큰·밀도·선택·접근성·국제화 검토

- **토큰/테마:** scoped 네 컴포넌트에 hex/rgba, 임의 그림자/전환/반경 선언은 발견하지 않았다. 공유 `recipes.ts`의 role class를 사용한다. typography 최소 12, action min target 44는 Leftcar tokens/recipes의 기존 정의다. 공통 대비 테스트는 두 테마의 text role×canvas/surface/subtle 조합을 다룬다. 이번 감사는 테스트 통과나 실제 대비 렌더를 새로 주장하지 않는다.
- **강조/표면:** Android primary는 Start 또는 Stop 한 개, 다른 동의/좌우/설정은 secondary/ghost다. Host에는 승인 switch와 secondary/ghost만 있다. 강한 색 면을 추가할 이유가 없다. notice는 공유 중립 surface/border를 쓴다. 일상 OFF 경고 제거가 새로운 장식보다 먼저다.
- **선택:** Android 동의는 KO/EN Approved/Not approved 텍스트와 checked state, 좌우는 체크 표시와 radio checked state를 같이 쓴다. busy/active 비활성화 때 class가 같아져도 선택 marker/text는 남는다(`input-sharing-view.test.tsx:89–112`). Host는 standard switch 위치 및 aria-checked를 사용한다. 실제 라이트/다크에서 thumb 위치·focus 가시성은 미검증이다.
- **밀도/compact·tablet:** Android ScrollView + `w-full max-w-2xl` + wrapping 가능한 Action label, Host row flex-wrap/min-w-0는 소스상 대응이다. native header와 본문 제목이 중복되고, 모든 정상 권한에도 접근성 설정 버튼/긴 안전 설명이 항상 보인다. 먼저 검증할 시각 후보이며, 캡처 없이 “과밀/잘림/좋음”으로 판정하지 않는다. compact에서 Stop과 emergency 접근성, tablet에서 세로 여백/최대 폭을 실측해야 한다. 정상 권한의 설정을 접는 개선은 복구 접근성을 확인한 뒤 결정한다.
- **KO·EN:** km 공통 문자열은 두 언어 모두 정의되고 양방향/철회/현재 owner 의미가 일치한다. 기존 Android 테스트는 KO 동의와 EN owner/failed cleanup을 다루고 Host 테스트는 EN만 다룬다. native 원문은 한국어 문자열도 포함하므로 그대로 화면에 붙이는 수정은 EN 품질을 해결하지 못한다. 원인 code의 두 언어 번역이 필요하다.
- **키보드/화면 읽기:** 실제 DOM button/switch, RN Pressable roles/checked/disabled/busy, status live region, error alert, focus class가 있다. 터치 target은 기존 44/48 기준이다. 실기기 RN keyboard focus·radio group navigation·TalkBack alert 전달·Host VoiceOver 목록 읽기는 미검증이며 F7을 포함해 확인해야 한다. 본문 heading은 Label/Span이므로 semantic heading/group을 보완할 후보가 있다.
- **모션:** 이 feature 자체의 새 animation/timer는 없다. 공통 Action busy indicator는 기존 primitive다. Host의 1초 timer는 상태 조회이며 decorative motion이 아니다. 화면 읽기 반복/Reduce Motion 실측은 없다.

## 15항 체크리스트 요약

1–3 상태 어휘: 기존 state label 조회는 닫힌 switch/map이나 failure reason 분기가 양쪽에서 소실됨(F1/F2/F5). 4–5 위계: owner가 위에 있으나 정상 OFF의 허위 경고가 우선순위를 흐림(F1). 6–7 밀도: Leftcar spacing/표면을 유지하며 CLI 줄 예산은 비적용; 실제 렌더 판정 보류. 8 강조: scoped primary 한 개 유지. 9–10 색 외 의미: 텍스트·checked·marker로 의미 유지; CLI NO_COLOR/JSON은 비적용. 11 glyph: 기능적인 방향/선택 marker뿐이며 새로운 장식 없음. 12–13 문구/오류: 공통 KO/EN 상수 사용, 원인+다음 행동 부족(F1/F2/F5/F6); overflow factory는 이 표면에서 비적용. 14–15 일관성: 기존 Leftcar tokens/recipes 유지, 새 색/강조/어휘 도입 근거 불필요. 이 검토는 rendered QA와 runtime gate를 대체하지 않는다.

## 제안 실행·검증 순서

1. F1–F3의 상태 계약과 원인 code/종료 확인 모델을 먼저 확정한다. 기존 동의/입력 소유권 및 영상과 독립된 수명은 유지한다.
2. F4–F7을 같은 모델 위에서 구현한다. permission 상태는 Allowed/Required, session은 Off/Starting/On/Shutdown unconfirmed, operation failure는 발생한 행동 이름으로 표시한다. KeyBridge 설정은 사용자의 명시적 선택 없이 바꾸지 않는다.
3. 기존에 빠진 조합의 회귀 검증만 추가한다: 정상 OFF Host, 원격 Stop 실패의 Refresh/remount/retry, specific native/KeyBridge reason, permission/settings operation, 지속 Host 오류, 다중 기기 접근성 이름.
4. Root가 실제 Android compact/tablet와 Host 좁은/일반 폭에서 KO/EN×light/dark를 렌더하고 필요한 최소 캡처로 owner·선택·문구 줄바꿈·Stop/복구 접근성을 확인한다. armed/각 owner/철회/종료 실패와 keyboard/screen reader는 소스와 별도 증거로 기록한다.
5. React 변경이 승인되어 수행되면 사용자 AGENTS 규칙에 따라 repository root에서 React Doctor **100/100**, 수정 후 typecheck/관련 테스트를 실행한다. 현재 보고서는 audit-only이므로 재실행하지 않았다.

## 참고 기록

- `docs/research/2026-10-03-input-sharing-review/final-scoped-review.md`, `residual-recovery-report.md`: persistent native cleanup와 helper-death 복구의 기존 소스/테스트 경계. 해당 수정의 실기기 수용을 이 감사가 추가하지 않는다.
- 기억 조회는 이전 Leftcar 감사의 “장식 제거와 동시에 권한·오류·복구 유지, 소스와 실기기 증거 분리” 범위 원칙에만 사용했다. 현재 발견은 모두 위 현재 소스로 재확인했다.
