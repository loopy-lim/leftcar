# Wi-Fi 기반 Android 전역 KM(키보드·마우스) 타당성 조사

**날짜:** 2026-09-24
**성격:** 조사 보고서(구현 아님). 제품 코드 수정 없음.
**결론:** **부분 가능(PARTIAL).** Universal Control/Synergy 수준의 OS 전역 양방향 KM을
접근성 + Wi-Fi만으로 달성하는 것은 불가능하다. Android 물리 키보드 → Mac 방향은
기존 KeyBridge 접근성 경로로 사실상 완성되어 있고, Mac → Android 방향은
root/시그니처 권한 없이는 "보이는 커서"와 진짜 포인터 이벤트 시맨틱을 얻을 수 없다.

---

## 1. 검수한 저장소 사실 (verified source facts)

| 사실 | 근거 경로 |
| --- | --- |
| Leftcar는 `IRemoteInputOwnership.acquire()/release()` AIDL로 `dev.loopy.keybridge`의 원격 입력 소유권 게이트를 제어한다 | `apps/viewer-expo/android/app/src/main/aidl/dev/loopy/keybridge/remote/IRemoteInputOwnership.aidl` |
| 바인딩은 명시적 ComponentName + `BIND_AUTO_CREATE`이고, `onServiceDisconnected/onBindingDied/onNullBinding`에서 UNAVAILABLE로 복귀한다 | `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/KeyBridgeInputAdapter.kt` |
| 소유권 상태 기계는 3상태(LOCAL_ANDROID / ACQUIRING_REMOTE / REMOTE_MAC)이며, 캡처 상태에서 경계 이탈·Esc·포커스 상실·단절 시 항상 해제 이펙트를 반환한다 | `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/InputOwnershipController.kt` |
| 포인터 캡처는 `target.hasWindowFocus()` 확인 + `SDK_INT >= O` 가드 후 `requestPointerCapture()`로 요청하고, 상대 이동은 `AXIS_RELATIVE_X/Y`로 읽는다 | `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt` (848~860, 1082~1083행 부근) |
| 스트림 창은 `windowSoftInputMode="stateAlwaysHidden|adjustNothing"` — 소프트 IME를 정책상 쓰지 않는다 | `apps/viewer-expo/android/app/src/main/AndroidManifest.xml` |
| KeyBridge 접근성 서비스가 앱보다 먼저 외부 키 이벤트를 소비하며, down/up 시퀀스 소유자(RemotePassthroughGate)로 통과를 관리한다는 설계 | `docs/superpowers/specs/2026-09-14-android-input-ownership-and-disconnect-safety-design.md` §2.2, §6 |

주의: KeyBridge 접근성 서비스의 **실제 구현 소스는 이 저장소 밖**에 있다. 이번 조사에서
직접 검수한 것은 AIDL 계약과 Leftcar 측 어댑터뿐이다.

## 2. 방향별 평가

### 2.1 Android 물리 키보드 → Mac (완료된 방향)

- AccessibilityService의 `FLAG_REQUEST_FILTER_KEY_EVENTS`로 하드웨어 키를
  포커스 윈도우보다 먼저 관찰하고 소비(억제)할 수 있다. USB/root 불필요.
- 기존 KeyBridge가 이미 이 경로를 구현했고 Leftcar는 AIDL 게이트로 제어한다.
- 제약: Power 키·일부 시스템 전용 키는 관찰·소비가 불가능하다(설계 문서 §4 불변식 4와 일치).
- **판정: 이 방향은 접근성만으로 충분하며 Wi-Fi 전송만 남아 있다.**

### 2.2 Mac → Android (미해결 방향)

| 능력 | 공개 API | root/시그니처 |
| --- | --- | --- |
| 보이는 커서 이동 | ✕ (가상 포인터 장치 필요) | ○ (uinput / `INJECT_EVENTS`) |
| 클릭·드래그·스크롤 | △ `dispatchGesture`(API 24+, 좌표 기반 제스처) | ○ |
| hover | ✕ | ○ |
| 마우스 홀드 시맨틱 | △ 연속 스트로크(`willContinue`) 흉내 | ○ |
| 임의 KeyEvent 주입 | ✕ (IME 텍스트·`globalAction` 부분만) | ○ (`adb shell input keyevent`) |
| 전역 포인터 관찰·억제 | ✕ | ○ |

핵심 구조적 제약 두 가지:

1. **접근성 키 필터에는 포인터 대응물이 없다.** 마우스 이벤트는 포인터 아래
   윈도우로만 전달되므로, Leftcar 외부의 커서 위치를 관찰하거나 물리 마우스
   입력을 억제할 공개 API가 없다.
2. **보이는 커서는 커널 레벨 가상 입력 장치 문제다.** `dispatchGesture`는
   포인터 이벤트가 아니라 제스처를 주입하므로 화면의 커서가 움직이지 않고
   hover가 발생하지 않는다. Universal Control의 "커서가 넘어가는" 느낌은
   root(uinput) 또는 시스템 시그니처 `INJECT_EVENTS` 없이는 재현 불가능.

### 2.3 경계 감지 (edge detection)

- **Leftcar 내부(Android → Mac):** 이미 구현됨. 캡처된 상대 이동이 정규화
  경계에서 바깥쪽이면 소유권 해제(`InputOwnershipController.moveCapturedPointer`).
- **Leftcar 외부(다른 앱에 포커스가 있을 때):** 공개 API로 불가능. 유일한 우회
  후보는 화면 테두리 수 px 폭의 `TYPE_ACCESSIBILITY_OVERLAY` 투명 스트립으로
  hover 이벤트를 받아 경계 통과를 감지하는 것이지만, 전체 화면 앱·게임·시스템
  UI에서의 동작이 기기 검증 없이는 알 수 없다(unknowns 참조).

### 2.4 IME와 포커스

- 하드웨어 키는 포커스 윈도우로만 전달되므로 Mac → Android 키보드는 (a) 자체
  IME 텍스트 주입, (b) `globalAction`(Back/Home/Recents), (c) ADB keyevent의
  세 경로뿐이다. (a)는 편집 필드 포커스를 요구하고 시스템 단축키를 재현하지
  못하며, 스트림 창의 `stateAlwaysHidden` 정책과 충돌한다.

## 3. 경로 비교

| 경로 | 커버 | 미커버 | USB/root | 수명 |
| --- | --- | --- | --- | --- |
| AccessibilityService(기존 KeyBridge) | Android KB → Mac 전역 억제·통과. Mac → Android 클릭/스크롤(globalAction 부분) | 보이는 커서, hover, 전역 포인터 관찰/억제, 임의 KeyEvent, Power 키 | 불필요 | 재부팅 후에도 유지(사용자가 설정에서 활성화; 배터리 최적화 예외 필요) |
| Wireless ADB (Android 11+) | shell uid로 tap/swipe/keyevent/text 주입, app_process 상주 데몬으로 지연 완화 가능 | 보이는 커서·hover·상대 마우스 스트림 없음, 명령당 지연 | 무선 페어링만으로 가능, root 불필요 | 재부팅마다 무선 디버깅 토글 재활성화 필요 |
| Root / uinput 가상 장치 | 진짜 가상 마우스·키보드. 전체 패리티의 유일 경로 | — | **root 필수(전제에서 제외)** | 재부팅 후에도 유지 |

## 4. 버전 제약

- `dispatchGesture` API 24+ / 포인터 캡처 API 26+(StreamActivity가
  `SDK_INT < O` 차단 — verified) / 무선 디버깅 API 30+(Android 11).
- Android 13+: 스토어 외 설치 앱의 접근성 서비스 활성화는 restricted settings로
  제한된다. 사이드로드 배포 시 사용자 안내가 필요하다.
- 대상 기기(Lenovo TB710FU)의 실제 API 레벨과 OEM 제한은 미확인.

## 5. 결론과 설계 시사점

1. **Android → Mac 방향은 접근성만으로 완전하다.** 기존 KeyBridge 아키텍처를
   유지하고 Wi-Fi 전송 계층만 연결하면 된다.
2. **Mac → Android 방향은 완전 패리티가 불가능하다.** 설계 결정이 필요한
   트레이드오프:
   - (A) 접근성 `dispatchGesture`로 클릭/스크롤/텍스트까지만 지원(커서 없음,
     완전 무선, 무 root) — 안정성 최선, 체감 열화.
   - (B) 무선 ADB를 선택적 고급 경로로 추가(키 이벤트 커버 확대) — 재부팅마다
     재설정 UX와 개발자 옵션 의존을 수용해야 함.
   - (C) 완전 패리티는 root 전제 — 요구 범위 밖으로 명시적 제외 권고.
3. **전역 억제 불가를 UX에 반영해야 한다.** Mac 조작 중 물리 마우스가 Android를
   건드리면 별도 포인터로 작동한다. 이는 Universal Control과 다른 명확한 차이점이며
   문서화·테스트 기준에 넣어야 한다.
4. Power 키·OEM 예약 키 불전달은 기존 설계(§4 불변식 4, §9)와 정합 — 변경 불필요.

## 6. 미수행 검증 (기기 실측 필요)

이 보고서의 프레임워크 동작은 모두 공식 문서 인용이며 **기기 테스트는 수행하지
않았다.** 다음을 Lenovo TB710FU에서 검증해야 한다:

1. `TYPE_ACCESSIBILITY_OVERLAY` 경계 스트립이 다른 앱 위 hover를 받는지
2. `dispatchGesture` 연속 스트로크의 실측 지연과 앱별 호환성
3. 무선 ADB `app_process` 데몬 주입 지연이 커서 품질을 만족하는지
4. OEM 예약 단축키가 접근성 필터보다 우선하는지
5. Android 버전과 OEM 배터리 최적화가 접근성 서비스 수명에 미치는 영향
