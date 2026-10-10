# Wi-Fi 양방향 KM(마우스·키보드 공유) 설계 — 승인 대기 안

문서 상태: 설계 제안안. **구현 승인 전까지 코드 변경 없음.**
기준일: 2026-09-25.
근거 문서: `docs/research/2026-09-24-wifi-km-android-feasibility.md`([링크](../../research/2026-09-24-wifi-km-android-feasibility.md)), `docs/research/2026-09-25-wifi-km-macos-feasibility.md`([링크](../../research/2026-09-25-wifi-km-macos-feasibility.md)), 격차 조사 `docs/research/2026-09-25-wifi-km-full-parity-gap.md`([링크](../../research/2026-09-25-wifi-km-full-parity-gap.md), `2026-09-25-wifi-km-full-parity-gap.json`), 근거 JSON `docs/research/2026-09-24-wifi-km-android-evidence.json`, `docs/research/2026-09-25-wifi-km-macos-evidence.json`.

> **2026-09-25 정정**: 최종 리뷰에서 본 설계가 원래 목표(Universal Control 수준 양방향 마우스·키보드 경계 이동)를 축소했음이 확인됐다. "보이는 커서는 root/uinput의 유일 경로"라는 주장은 scrcpy UHID + 무선 ADB 1차 근거에 의해 반증됐고, 본 문서는 이를 반영해 정정됐다(wifi-km-full-parity-gap). 접근성-only 축소 모드는 사용자가 명시적으로 수용하는 경우에만 선택 가능한 폴백이다.

> **2026-10-03 전송 계약 승인·모션 이벤트 정정**: 사용자는 Wi-Fi 직접 앱 간 통신을 기본 전송 경로로 명시 승인했고, 무선 ADB는 기본으로 요구·승인 요청하지 않기로 했으며, USB는 선택적 미래 확장(지금 구현 안 함)으로 기록됐다 — [wifi-km-optional-usb-roadmap](2026-10-03-wifi-km-optional-usb-roadmap.md). §13-Q1(옵션 B 무선 ADB + UHID)은 기본 경로에서 제외된다. 또한 연결 기기 실측([wifi-km-connected-device-validation](../../research/2026-10-03-wifi-km-connected-device-validation.md) §5)에서 Android API 34+ `AccessibilityService.onMotionEvent`와 `AccessibilityServiceInfo.setMotionEventSources`가 확인돼, 이전의 "프레임워크 수준 전역 모션 필터는 공개 API 부재로 불가"라는 범주적 주장은 철회된다(§0·§2 정정 반영, §14도 참조). 원래 목표는 유지되며 축소 모드는 별도 명시 수용이 필요하다.

## 0. 권고 요약 (결정 요청)

**권고(정정): 원래 목표 — 실제 마우스 커서(보이는 포인터·hover)와 키보드의 양방향 경계 이동 — 를 유지하되, 그 공식 구현 경로를 무선 ADB + UHID 가상 HID(옵션 B) 후보로 복원한다. Wi-Fi 로컬 직접 연결 위에 기존 secure-channel·입력 평면 계약을 재사용한다. USB·root는 전제로 하지 않는다(무선 페어링은 Wi-Fi 범주). 옵션 B 승인 여부가 최우선 사용자 결정(§13-Q1)이며, 승인 전까지 구현 착수는 없다. 접근성-only(옵션 A)는 사용자가 축소를 명시 수용할 때만 선택하는 폴백이다.**

- Android → Mac 방향은 접근성 키 필터(KeyBridge 경로)로 키보드가 사실상 완성되어 있다(android-feasibility §2.1, verified). 태블릿 물리 마우스의 전역 관찰·억제에 대해서는 **(2026-10-03 정정)** shell의 /dev/input 쓰기 neverallow(full-parity-gap §2.3)는 유효하나, "공개 API 부재" 주장은 철회된다 — API 34+에서 `AccessibilityService.onMotionEvent(MotionEvent)`("allows an accessibility service to observe generic MotionEvents")와 `AccessibilityServiceInfo.setMotionEventSources(int)`("MotionEvents from sources in getMotionEventSources() are **not sent to the rest of the system**" — 관찰과 억제가 동시에 성립, 허용 소스에 SOURCE_MOUSE 등 명시)가 존재한다([onMotionEvent](https://developer.android.com/reference/android/accessibilityservice/AccessibilityService#onMotionEvent(android.view.MotionEvent)), [setMotionEventSources](https://developer.android.com/reference/android/accessibilityservice/AccessibilityServiceInfo#setMotionEventSources(int))). 동작·지연·OEM 차이는 기기 실측 전이며 후보로만 기록한다(§2 (d), wifi-km-connected-device-validation §5).
- Mac → Android 방향에서 **"보이는 커서·hover가 root 없이 불가능하다"는 이전 주장은 철회된다.** scrcpy는 `--mouse=uhid`/`--keyboard=uhid`로 adb shell 권한(무선 ADB 포함, USB 불필요)만으로 Android에 진짜 물리 HID 장치를 만들어 실제 커서·hover를 제공하며, AOSP `shell.te`가 이를 명시 허용한다("allow shell input injection"). USB 전용인 것은 AOA이고 UHID는 아니다(full-parity-gap §2).
- macOS 측은 양방향 모두 플랫폼 수준 가능하고 주입은 이미 프로덕션 경로다(macos-feasibility §3). 리스크는 KM 상태 기계·자기 이벤트 구분·보안 입력 상호작용에 있다.
- 최종 판정은 기기 실측(android-feasibility §6, macos-feasibility §5, 추가: TB710FU에서 /dev/uhid 접근·무선 ADB 데몬 지연)에 의존한다. 이 설계는 승인 후 물리 검증 선행을 조건으로 하며, 현 시점 `original_goal_satisfied: false`, `physical_tests_passed: false`다.

## 1. 범위 구분: KM 모드와 화면 스트리밍은 다른 세션이다

사용자 의도의 "진짜 Android 앱 조작"은 기존 Mac 화면 스트리밍과 분리해야 한다.

| | 스트리밍(기존) | KM 모드(본 설계) |
| --- | --- | --- |
| 대상 화면 | Mac 디스플레이를 Android 창에 표시 | Android의 **자체 OS UI**(홈, 다른 앱)를 직접 조작 |
| 캡처 | ScreenCaptureKit 영상 | 없음 (영상 평면 미사용) |
| 입력 | Android 뷰어 UI → Mac 창 좌표 주입 | OS 전역 입력 기기 공유, 활성 대상이 양방향 전환 |
| 세션 | stream session(source lease) | **km session**(별도 capability `km_share`) |

KM 모드는 새 세션 종류이며 스트림 세션을 대체하지 않는다. 두 세션의 동시 실행은 허용하되 KM 활성 방향은 전역 상태이므로 어느 쪽이든 단일 소유 규칙(§5)을 따른다. 스트림 창의 `windowSoftInputMode=stateAlwaysHidden` 제품 정책과 충돌하지 않는다 — KM 모드는 스트림 창이 아니라 Android 시스템 IME가 정상 동작하는 자체 앱 화면을 다룬다(android-feasibility §2.4의 충돌 지적은 스트림 창에 한정).

## 2. Android 권한 경로 비교 (3옵션)

근거: android-feasibility §3 경로 비교 표.

### 옵션 A — 접근성 only (축소 폴백 — 사용자 명시 수용 필요)

- **역할**: 완전 패리티 목표를 충족하지 못하는 축소 폴백이다. 이전 설계의 "기본 권고"는 원래 목표를 조용히 축소한 것으로 철회된다(full-parity-gap §1). 사용자가 Q3(§13)에서 축소를 명시 수용한 경우에만 채택한다.
- **메커니즘**: KeyBridge 접근성 서비스(키 필터, `FLAG_REQUEST_FILTER_KEY_EVENTS`) + `dispatchGesture`(API 24+) + globalAction(Back/Home/Recents) + 자체 IME 커밋(§8).
- **커버**: Android 물리 키보드 → Mac 전역(완성). Mac → Android의 클릭·드래그·스크롤·텍스트·시스템 내비게이션.
- **미커버**: Android 화면의 보이는 커서, hover, 마우스 홀드 시맨틱 완전 재현, Leftcar 외부 물리 마우스 억제, Power 키, Android 측 마우스 경계 이동.
- **권한 수명**: 재부팅 유지. 배터리 최적화 예외 필요, Android 13+ 사이드로드는 restricted settings 해제 안내 필요.
- **USB/ADB/root**: 전부 불필요.

### 옵션 B — 무선 ADB + UHID 가상 HID (완전 패리티 후보 — 사용자 승인 전)

- **메커니즘**: Android 11+ 무선 디버깅(Wi-Fi 페어링, USB 불필요)으로 확립된 adb shell 세션 위에, scrcpy와 동일한 방식으로 `/dev/uhid`에 가상 HID 마우스·키보드를 생성한다(scrcpy `UhidManager.java` — shell uid로 `Os.open("/dev/uhid", O_RDWR)` 후 UHID_CREATE2). shell uid의 UHID 접근은 AOSP SELinux가 명시 허용한다(`shell.te`: "allow shell uhid_device:chr_file rw_file_perms" — 주석 "allow shell input injection").
- **커버**: **실제 커서·hover·상대 마우스 이동·휠·전체 문자/IME 키보드** — scrcpy 문서: UHID 모드에서 "the mouse pointer disappears from the computer and appears on the Android device instead", "it works over TCP/IP (wirelessly) (contrary to `--keyboard=aoa`)". 가상 커서 좌표를 자체 데몬이 소유하므로 **양방향 경계 이동 트리거** 구현도 가능하다(§4.2).
- **미커버**: Android 물리 마우스의 전역 억제(/dev/input 쓰기 neverallow). 물리 키보드 억제는 옵션 A의 접근성 필터로 보강 가능.
- **권한/수명**: 개발자 옵션 활성화 + 무선 디버깅 페어링. 무선 디버깅 토글의 재부팅 생존은 버전·OEM 의존 — 일반적으로 재부팅 후 꺼짐으로 알려져 있으나 단정하지 않고 TB710FU 실측 항목(PV)으로 둔다. 상주 데몬(app_process)의 배터리 최적화 생존도 실측 항목.
- **USB/root**: 불필요. USB는 폴백 연결 수단일 뿐 전제 아니다.
- **승인 조건**: 사용자 결정 §13-Q1. 기기 검증 전제: TB710FU에서 /dev/uhid 접근 허용 여부(scrcpy 문서 — 구형 Android 권한 오류 가능), 데몬 주입 지연 실측.
- **판정(이전 보류 판정 정정)**: "옵션 A 대비 얻는 것이 제한적"이라던 이전 판정은 커서·hover·경계 이동을 놓친 것이므로 철회된다. 옵션 B는 원래 목표를 만족하는 유일한 Wi-Fi 경로 후보다.

### 옵션 C — root/uinput 가상 입력 장치 (제외 권고)

- Universal Control 수준 완전 패리티에 이를 수 있으나 root 필수. 사용자가 USB/ADB/root를 승인 전제로 삼지 않기로 했으므로 **명시적 제외**. 향후 요구가 생기면 별도 결정.
- **정정**: 이전 설계가 "옵션 C가 완전 패리티의 유일 경로"라고 기술했으나, 이는 거짓이다 — 옵션 B(무선 ADB + UHID)가 root 없는 완전 커서 경로다(full-parity-gap §2). "유일 경로" 주장은 삭제됐다.

### 좌표·경계 감지 보완 수단의 위치

- Mac 쪽 경계 감지(전환 트리거): CGEventTap 관찰 — 옵션과 무관하게 macOS 권한만으로 가능(macos-feasibility §4.1).
- Android 쪽 "테블릿 → Mac" 경계 감지: 후보가 넷이다. (a) **옵션 B의 가상 UHID 커서** — 좌표를 자체 데몬이 소유하므로 경계 도달을 정확히 알고 Mac으로 캡처 해제를 요청할 수 있다(정확도 최상, 무선 ADB 승인 필요 — **2026-10 전송 계약에서 기본 미승인**, wifi-km-optional-usb-roadmap). (b) `TYPE_ACCESSIBILITY_OVERLAY` 테두리 스트립 hover 우회(기기 미검증). (c) shell의 `/dev/input` 읽기 허용(`r_file_perms`)을 이용한 물리 마우스 전역 관찰(SELinux 허용만 확인, 노드 식별·신뢰성·배터리 영향 미검증 — 후보로만 기록). (d) **(2026-10-03 신규 후보)** API 34+ 접근성 모션 관찰 — `setMotionEventSources(SOURCE_MOUSE)` + `onMotionEvent`로 물리 마우스 전역 관찰(및 억제) 후보. 무선 ADB 없이 접근성 권한만으로 가능한 후보이나 동작·지연·OEM 차이 기기 실측 전이다([onMotionEvent](https://developer.android.com/reference/android/accessibilityservice/AccessibilityService#onMotionEvent(android.view.MotionEvent)), [setMotionEventSources](https://developer.android.com/reference/android/accessibilityservice/AccessibilityServiceInfo#setMotionEventSources(int)) — wifi-km-connected-device-validation §5). (b)(c)(d) 검증 전, 그리고 (a) 승인 전에는 Android→Mac 마우스 경계 트리거는 미충족 상태다(full-parity-gap §3 매트릭스).

## 3. 전송·보안 재사용 (Wi-Fi 로컬 직접 연결)

KM 세션은 기존 인증·암호화 계약 위에 얹는다. 새로운 전송 계층을 만들지 않는다.

- **채널**: QR로 핀된 호스트 Ed25519 정체 키 핸드셰이크 → ChaCha20-Poly1305 AEAD 제어 평면(`crates/secure-channel`). KM은 새 capability `km_share`로 제어 평면에 등록.
- **입력 평면**: HKDF(`leftcar/media/v1`와 별도 info `leftcar/km/v1`)로 방향별 키 도출, 세션 난수 검증된 UDP. LCI1 의미론 재사용: 포인터 델타는 latest-wins(kind 1 대응), 키·버튼·휠·텍스트·제스처 커밋은 sequence/ACK reliable(kind 2–4 대응), release-all(kind 5) 계약 유지. LCS1 대응 `KM_STATE` 브로드캐스트로 활성 방향·억제 상태를 양쪽이 상시 공유.
- **기본 OFF·피어별 동의**:
  - Android → Mac(태블릿 키보드로 Mac 조작): 기존 §21 세션 입력 승인 계약 재사용. 세션 기본 OFF + 운용자 승인 시 기기별 상시 승인 기록 + OS 입력 권한 실패 시 fail-closed + 감사 로그(`input_auto_enabled` 등).
  - Mac → Android(Mac 입력으로 태블릿 조작): Android 측에 동일 계약의 신규 게이트를 만든다. 세션별 기본 OFF, 기기별 상시 승인(동일 재검토 규율), 감사 로그. 접근성·IME 권한과 별개로 **네트워크 세션 승인이 따로 필요**하다는 점이 핵심.
  - 방향별 동의는 독립이다. 한 방향 승인이 반대 방향을 활성화하지 않는다.

## 4. 양방향 전환과 경계 (edge crossing), 키보드 라우팅

### 4.1 배치와 스케일링 (device arrangement)

KM 배치는 사용자 설정이며 순수 함수로 도메인 크레이트에 둔다(테스트 가능).

```text
km_arrangement = {
  side: left | right,          # Mac 데스크톱 union의 어느 변이 태블릿과 맞닿는가
  edge_norm_start: f32,        # 공유 변 위 태블릿이 차지하는 구간 (0.0–1.0)
  edge_norm_end: f32,
  scale: f32,                  # Mac 좌표 → 태블릿 정규 좌표 속도 계수
  tablet_orientation: portrait | landscape
}
```

- Mac 커서가 공유 변을 `edge_norm_*` 구간에서 통과하면 전환 트리거. 진입 y 위치 → 태블릿 정규 y로 매핑.
- 전환 중 Mac 물리 포인터 델타 → 태블릿 정규 좌표의 **가상 KM 커서**를 이동. 옵션 B(무선 ADB+UHID)에서는 이 커서가 Android OS가 렌더링하는 실제 커서(hover 포함)다. 옵션 A(접근성 폴백)에서는 보이지 않는다(§2).
- multi-display 음수 원점은 기존 `currentInputBounds`/CGDisplayBounds union 재사용(macos-feasibility §4.3). 디스플레이 구성 변경 시 배치 재검증 실패하면 KM을 안전 해제.

### 4.2 상태 기계 (독점 소유권)

Host( Mac )가 소유하고 `KM_STATE`로 양쪽에 브로드캐스트한다. Android는 이 상태에만 따른다.

| 상태 | Mac 물리 입력 | 태블릿 키 필터 | 키보드 라우팅 대상 |
| --- | --- | --- | --- |
| `KM_OFF` | 정상 | OFF | 각자 로컬 |
| `KM_ARMED` | 정상 (경계 감시 탭만 활성) | OFF | 각자 로컬 |
| `KM_MAC_TO_ANDROID` | **억제 + Wi-Fi 전송** | OFF | Android (IME/globalAction 경유) |
| `KM_ANDROID_TO_MAC` | 정상 | **ON (KeyBridge 캡처 → 전송)** | Mac (CGEvent 주입) |

전이:

```text
KM_OFF -> KM_ARMED            : 양방향 동의 확인 + 권한 프리플라이트 통과
KM_ARMED -> KM_MAC_TO_ANDROID : Mac 포인터가 배치 변 통과 (CGEventTap 관찰)
KM_MAC_TO_ANDROID -> KM_ARMED : 탈출 콤보(§7) / 해제 조건(§9)
KM_ARMED -> KM_ANDROID_TO_MAC : 태블릿 측 경계 트리거(옵션 B: 가상 커서 경계 도달 / (b)(c) 후보 실측 통과 시) 또는 명시 토글(QS 타일·알림 버튼) — 폴백
KM_ANDROID_TO_MAC -> KM_ARMED : 동일 토글 재누름 / Mac 경계 역이동(Mac 측 관찰) / 탈출 조건
임의 -> KM_OFF                : 연결 단절·잠금·절전·revoke·stop all (항상 release-all 동반)
```

- **불변식 1**: 활성 방향은 최대 하나. `KM_MAC_TO_ANDROID`와 `KM_ANDROID_TO_MAC` 동시 진입 시도는 나중 요청을 거절하고 `KM_STATE`로 양쪽에 통보·감사 로그.
- **불변식 2**: `KM_MAC_TO_ANDROID`에서만 Mac 억제 탭이 이벤트를 소비하고, 그 외 상태에서 억제 탭은 관찰만 한다.
- **불변식 3**: 모든 상태 이탈은 release-all(양쪽)을 먼저 실행하고 상태를 바꾼다. 기존 3상태 소유권 컨트롤러(`InputOwnershipController`, LOCAL_ANDROID/ACQUIRING_REMOTE/REMOTE_MAC)의 해제-이펙트 패턴과 동일한 규율.
- Android 측 "테블릿 → Mac" 경계 트리거는 옵션 B 승인 시 가상 커서 경계 도달이 1순위이며, 스트립/evdev 관찰은 기기 검증 통과 시 추가 트리거다. 어떤 트리거도 검증되지 않으면 토글이 폴백으로 남는다 — 단, 토글-only는 원래 목표의 경계 이동 요건을 충족하지 못하므로 계약상 "미충족"으로 표기한다. 상태 기계 자체는 트리거가 추가되어도 변하지 않는다.

## 5. 루프 방지와 동시 로컬 입력

- **Mac 자기 이벤트 구분(미구현 — 설계 시접)**: 합성 이벤트는 KM 전용 `CGEventSource`로 태깅하고, 억제/관찰 탭 콜백에서 소스 식별자로 물리 이벤트만 처리. 미구현 상태가 macos-feasibility §4.6으로 기록돼 있으므로 Stage 2 검증 항목에 명시한다. 필터 실패 시(자기 이벤트가 억제되거나 루프 관측) 안전 해제.
- **Android 자기 이벤트**: IME `commitText`/`sendKeyEvent`와 `dispatchGesture` 주입은 하드웨어 키 필터 경로에 들어오지 않으므로 재캡처 루프가 구조적으로 없다. KeyBridge down/up 시퀀스 소유자 패턴이 로컬 통과를 관리한다.
- **동시 로컬 입력**: `KM_MAC_TO_ANDROID` 중에도 태블릿 터치스크린은 로컬로 산다(편집 필드 포커싱에 필요). 태블릿 물리 키보드는 활성 대상이 Android이므로 기본 동작(로컬)이 정답 — 필터 OFF. `KM_ANDROID_TO_MAC` 중 Mac 물리 입력은 정상(활성 대상이 Mac). 즉 항상 억제는 "비활성 대상 쪽에서 눌린 물리 입력"에만 적용되고, 본 설계에서 그 경우는 Mac이 비활성 대상인 `KM_MAC_TO_ANDROID` 한 가지뿐이다.
- **피드백 루프 감시**: `KM_MAC_TO_ANDROID` 중 Mac 억제 탭이 소비한 이벤트 수와 Android 도착 제스처 수의 급격한 불일치(예: 1초 내 억제 이벤트 ∝ 자기 주입 추정)가 관측되면 해제. 단순 카운터 기반 1차 방어 + 물리 검증에서 보정.

## 6. Mac → Android 입력 매핑 (옵션 A 세부)

| Mac 입력 | Android 수용 경로 | 비고 |
| --- | --- | --- |
| 문자 입력 | 자체 IME `commitText` (§8) | 편집 필드 포커스 필요 |
| Enter/Backspace/화살표/Tab | IME `InputConnection.sendKeyEvent` | 편집기 내비게이션만 |
| Esc / Home / Recents | `globalAction` (GLOBAL_BACK/HOME/RECENTS) | 시스템 내비게이션 부분집합 |
| 클릭·길게누름 | `dispatchGesture` 단일 스트로크 | 좌표는 가상 KM 커서 위치 |
| 드래그·홀드 | 연속 스트로크(`willContinue`) 흉내 | hover 없음, 앱별 호환성 unknown |
| 스크롤 | `dispatchGesture` 스왑 스트로크 | 속도 계수 = `km_arrangement.scale` |
| Power·OEM 예약 키 | 전달 불가 | android-feasibility 한계 3번과 일치 |

- 제스처 좌표는 정규 KM 커서 → 태블릿 픽셀 변환을 Android 어댑터가 최종 수행(디스플레이 회전 대응).
- 보이는 커서 부재는 옵션 A(접근성 폴백)에서만 적용되는 축소 계약이며, 사용자가 §13-Q3에서 명시 수용한 경우에만 채택된다. 그 경우 UI에 상시 표시한다: Mac 메뉴 바와 태블릿 KM 알림에 "커서 없이 조작 중" 상태. 옵션 B(무선 ADB+UHID)에서는 실제 커서가 존재하므로 이 표시는 불필요하다(full-parity-gap §2.1).

## 7. 비상 탈출과 눌린 키 해제 (failure_safety)

- **탈출 콤보**: `⌃⌥⌘+Esc`(기본, 설정 가능). Mac 억제 탭이 소비 경로에서 우선 검사하므로 `KM_MAC_TO_ANDROID` 중 항상 유효. 태블릿 토글도 탈출 경로.
- **release-all 전파**: 기존 kind 5 계약을 양방향으로 확장. Mac: `releaseInjectedInput` 재사용. Android: 진행 중 스트로크를 항상 UP 스트로크로 종료(`willContinue=false` 종료 스트로크, 상한 100ms) + IME composing 클리어. Android 어댑터는 "미종료 스트로크 슬롯"을 최대 1개로 유지하고 모든 teardown 경로가 종료를 보장하도록 JVM 테스트로 고정.
- **워치독**: Mac은 `KM_STATE`/ACK 부재 500ms → 자동 해제. Android는 1초 부재 → 키 필터 OFF + 로컬 복구(기존 `onBindingDied/onNullBinding` UNAVAILABLE 복구 패턴과 동일 규율).
- **Host stop all / revoke**: §11 계약 재사용 — 즉시 KM 해제, 승인 수명 무효화.

## 8. IME (Mac → Android 텍스트)

두 하위 접근 비교:

1. **자체 IME 서비스 (권고)**: Leftcar 앱 내 최소 `InputMethodService`. 사용자가 시스템 설정에서 1회 활성화(새 권한면). 네트워크에서 받은 텍스트를 앱 네트워크 세션 → 바인딩( KeyBridge AIDL과 같은 어댑터 패턴 ) → IME `commitText`. 의미론이 깨끗하고 비밀번호 필드 외 표준 경로.
2. **`ACTION_SET_TEXT` 접근성 액션**: 포커스 편집 노드 텍스트 교체. 설정 추가 없으나 내용 덮어쓰기·증분 타이핑 경합·비밀번호 필드 차단 문제. 실험 프로브로만 허용, 제품 경로에서 제외.

IME 미활성 상태에서는 키보드 라우팅이 globalAction 부분집합으로 제한되며 상태 UI가 이를 명시한다. 스트림 창의 `stateAlwaysHidden` 정책은 KM 모드와 무관(§1).

## 9. 단절·잠금·절전 복구

| 상황 | 동작 |
| --- | --- |
| Wi-Fi 단절 | §7 워치독 해제. 재접속은 새 핸드셰이크, KM 상태 자동 복귀 없음(사용자 재무장) |
| Mac 잠금(보안 입력) | 잠금 알림 수신 시 즉시 KM 해제. 잠금 중 합성 키 무시는 알려진 플랫폼 동작(macos-feasibility 한계 0) — KM이 잠긴 Mac에 키를 보내지 않는 것이 정책 |
| Mac 절전 | `KM_ARMED` 이상에서 IOPMAssention 계열 유지(SleepAssertion 재사용). 그래도 절전 진입 시 해제, 해제 상태로 복귀 |
| 태블릿 화면 꺼짐 | `KM_MAC_TO_ANDROID` 자동 해제(제스처 대상 상실) + release-all |
| 디스플레이 구성 변경 | 배치 재계산 실패 시 안전 해제 |
| 앱/프로세스 사망 | KeyBridge 바인딩 사망 경로 + 네트워크 워치독이 로컬 모드 복구 |

## 10. 재부팅과 권한 수명

- Mac: 손쉬운 사용·입력 모니터링 승인은 재부팅 유지. macOS 15+ 주기적 재승인 프롬프트는 미실측 리스크(macos-feasibility §5)로 물리 검증 항목.
- Android: 접근성 서비스 활성화는 재부팅 유지(배터리 최적화 예외 필요). Android 13+ 사이드로드 restricted settings 안내를 온보딩에 포함. IME 활성화도 유지.
- KM 동의(피어별 상시 승인)는 §21 dirty-flag 규율을 따른다. 비정상 종료 후에는 저장된 승인으로 자동 활성화하지 않고 재검토 요청(fail-closed).
- 옵션 B(무선 ADB+UHID)는 무선 디버깅 토글의 재부팅 생존이 버전·OEM 의존이다 — 일반적으로 재부팅 후 꺼짐으로 알려져 있으나 단정하지 않고 TB710FU 실측 항목으로 둔다. 온보딩에 개발자 옵션·페어링 절차 안내를 포함한다(사용자 승인 §13-Q1 전제).

## 11. 단계별 구현 시접 (승인 후 순서)

각 단계는 이전 단계 계약에 얹히며, 완료 판정은 저장소 검증 게이트로 한다.

- **Stage 0 — 계약**: `crates/control-contract`에 KM 메시지 종류(`KM_POINTER_DELTA`, `KM_GESTURE`, `KM_KEY`, `KM_TEXT`, `KM_GLOBAL_ACTION`, `KM_STATE`, release-all 확장)와 `km_share` capability 추가, Rustra 코드gen, golden vector. `crates/domain`에 KM 상태 기계 + `km_arrangement` 매퍼 순수 함수.
- **Stage 1 — Android → Mac**: 태블릿 KM 발신자(ARMED/토글 상태에서만 키 필터 게이트 ON → `KM_KEY` reliable), Mac 수신은 기존 `injectKey/injectText/injectPointerMove` 재사용. 동의는 §21 계약 재사용. KeyBridge AIDL(`IRemoteInputOwnership`) 변경 최소화.
- **Stage 2 — Mac 경계 감시·억제**: CGEventTap defaultTap(관찰/억제), 자기 이벤트 소스 태깅 필터, 배치 매퍼 연결, Host KM 상태 기계 + `KM_STATE` 브로드캐스트, 탈출 콤보, 절전·잠금 알림 해제. 억제 탭이 관찰 전용으로 강등되는 모든 오류 경로 fail-closed.
- **Stage 3 — Mac → Android 실행**: Q1(§13)에서 옵션 B(무선 ADB+UHID)가 승인된 경우: 무선 ADB 세션 관리 + UHID 가상 HID 마우스·키보드 어댑터(가상 커서 좌표 소유, 경계 트리거 포함)가 주 경로다. 옵션 A(축소 폴백)가 채택된 경우에 한해: Android KM 어댑터(`KeyBridgeInputAdapter` 패턴): `KM_GESTURE`→`dispatchGesture`(스트로크 종료 보장), `KM_GLOBAL_ACTION`. KeyBridge가 별도 저장소라 dispatchGesture 호출 주체( KeyBridge 확장 vs Leftcar 자체 접근성 서비스 )는 pending 결정(§13-Q2).
- **Stage 4 — 동의·UI**: Android 측 KM 게이트(세션 기본 OFF·상시 승인·감사 로그), QS 타일/알림 토글, Mac 메뉴바 상태. Host UI는 Tauri/React — 변경 시 `npx -y react-doctor@latest . --verbose` 100점 게이트 적용.
- **Stage 5 — IME**: 자체 IME 서비스 + 바인딩 브리지, `KM_TEXT` 연결.
- **Stage 6 — 물리 검증**: §12 체크리스트 통과를 최종 완료 조건으로 명시.

## 12. 실행 가능한 테스트 전략

자동(저장소 게이트):

```text
cargo test --workspace        # KM 상태 기계 property 테스트: 독점 소유 불변식,
                              # 모든 teardown 경로가 release-all 방출, 배치 매퍼
                              # (음수 원점, 회전, scale 경계값) 골든 테스트
cargo test -p secure-channel  # KM 방향 키 HKDF(info=leftcar/km/v1) 벡터, AEAD 왕복
cargo clippy --workspace --tests -- -D warnings
bun run test:contract         # Rustra KM 메시지 TS/Rust 교차 골든 벡터
bun run typecheck && bun run test
cargo fuzz/임의 KM 프레임 파서 # 상한 초과 allocation 거부 회귀(§13 상한 계약)
```

Android JVM(기존 `bun run verify js` 내):

```text
KM 게이트 상태 기계 테스트: arm/disarm/워치독 타임아웃 → 필터 OFF
AIDL 어댑터 단절 테스트: onServiceDisconnected/onBindingDied/onNullBinding → UNAVAILABLE
스트로크 종료 보장: teardown 시 미종료 스트로크 슬롯이 항상 UP으로 닫힘
globalAction/제스처 좌표 변환 테스트(회전·스케일)
```

macOS shim 단위(가짜 이벤트 소스): 탈출 콤보 파서, 자기 이벤트 필터(주입→탭 통과), 억제 상태별 소비/통과 행렬.

통합: fake transport로 KM 세션 전체(무장 → 양방향 키 → drop 시 release-all) — 실제 CGEventTap·dispatchGesture·기기 동작은 대체하지 않는다.

물리 검증(승인 후, 실기기):

- android-feasibility §6 전체: 경계 스트립 hover, dispatchGesture 실측 지연·앱 호환성(옵션 A 경로), OEM 예약 키 우선순위, 접근성 수명 + full-parity-gap §4 전체: UHID 생성 실측(PV10), 무선 디버깅 재부팅 생존·데몬 생존(PV11), 데몬 주입 지연(PV12), evdev 관찰 후보(PV13).
- macos-feasibility §5 전체: 승인 상태 억제+주입 동시 동작, macOS 15+ 재승인 프롬프트, Secure Event Input·잠금 상호작용, 경계 전환 레이턴시(AP 격리 포함).

## 13. 승인 필요 결정 (사용자 질문)

- **Q1. 무선 ADB 승인 (최우선 결정 → 2026-10 결정됨)**: 옵션 B(무선 ADB + UHID 가상 HID — Android 11+ 무선 디버깅 페어링, root·USB 불필요)를 완전 패리티 목표의 공식 구현 경로로 승인하는가? **(2026-10-03 사용자 결정)** 사용자는 Wi-Fi 직접 앱 간 통신을 기본으로 승인하고 무선 ADB는 기본으로 요구·승인 요청하지 않기로 했다 — 옵션 B는 기본 경로에서 제외되며, 별도 명시 승인 시에만 재검토한다. 남은 무선-ADB-불필요 후보는 API 34+ 접근성 모션 관찰·억제(§2 (d), 기기 실측 전)와 선택적 USB 확장(AOA HID 후보 — wifi-km-optional-usb-roadmap §3)이다. 원래 목표는 유지되며, 축소 모드 수용(Q3)은 별도 명시 수용 없이는 성립하지 않는다.
- **Q2. KeyBridge 확장 가능 여부**: KeyBridge 구현이 본 저장소 밖이다. `dispatchGesture`·게이트 확장을 KeyBridge 저장소에서 할 수 있는지, 아니면 Leftcar가 자체 접근성 서비스를 새로 갈지(권한 승인 UX가 2개가 되는 트레이드오프). 옵션 A(폴백) 채택 시에만 구현에 영향.
- **Q3. 축소 모드 수용 (조건부)**: Mac → Android에서 "보이는 커서 없음·hover 없음 + 토글 기반" 접근성-only 축소 모드를 v1 계약으로 수용하는가? 이 수용은 **옵션 B가 비승인된 경우에만 요구**되며, 무답변 시 "수용으로 간주"하지 않는다 — 축소 모드의 무기한 기본값화는 금지한다(full-parity-gap). 수용하지 않으면 해당 범위의 구현은 보류된다.
- **Q4. 탈출 콤보 기본값**: `⌃⌥⌘+Esc` 기본, 설정 가능 — 수용?
- **Q5. Android → Mac 진입 방식**: 옵션 B 승인 시 가상 커서 경계 도달을 1순위 트리거로, 스트립/evdev 관찰은 기기 검증 후 추가, 토글은 폴백 — 수용? 옵션 A만의 경우 토글-only는 경계 이동 요건 미충족으로 계약에 명시된다.
- **Q6. IME 배포**: 자체 IME 서비스를 설정에서 활성화하는 1회 절차를 수용하는가? (옵션 B의 UHID 키보드는 IME 절차 없이 전체 문자·IME를 커버한다 — 옵션 A의 보강 수단.)

## 14. 정직한 한계 요약

- 접근성만으로 Universal Control 수준의 완전한 전역 KM은 불가능하다. 그러나 **root/uinput이 유일한 완전 커서 경로라는 이전 주장은 철회됐다** — 무선 ADB(adb shell) + UHID 가상 HID가 root·USB 없는 완전 커서·hover 경로다(scrcpy 문서·소스, AOSP shell.te — full-parity-gap §2). 옵션 B 승인 전까지는 어떤 경로도 확정되지 않았다.
- 확정되지 않은 항목: Power·OEM 예약 키 전달, Android 측 마우스 경계 트리거의 실측(가상 커서 제외). 이들은 옵션 B 승인 여부와 무관한 구조적 한계다. **(2026-10-03 정정)** Android 물리 마우스 전역 억제는 더 이상 "불가"로 단정하지 않는다 — API 34+ `onMotionEvent`/`setMotionEventSources`가 관찰·억제 후보이며(/dev/input 쓰기 neverallow는 그대로 유효), 기기 실측 전까지는 후보로만 기록한다. Mac 측 주입 권한 격차(leftcar.ll3.kr Input Monitoring 미승인 — wifi-km-connected-device-validation §2)도 유지된다.
- 전송 계약(2026-10-03 사용자 승인): Wi-Fi 직접 앱 간 통신 우선, 무선 ADB 불요·기본 미요청, USB는 선택적 미래 확장(지금 구현 안 함), 전송 승인과 OS 입력 권한 승인은 별개다 — [wifi-km-optional-usb-roadmap](2026-10-03-wifi-km-optional-usb-roadmap.md). USB 전송 자체는 OS 입력 권한을 부여하지 않으며, AOA HID(AOAv2, ADB 불요, USB 전용 — scrcpy keyboard.md)는 무선-ADB 없는 실제 커서 후보이나 TB710FU 실측·호스트 구현 모두 부재다. 연결 기기 실측 결과는 LAN 도달성만 입증됐고 라이브 세션은 blocked, 전체 완전-KM 물리 테스트는 전부 미수행이다(wifi-km-connected-device-validation §3–§4).
- 본 설계의 모든 프레임워크 동작은 공식 문서·소스 인용 + 저장소 계약 검수이며 Lenovo TB710FU 기기 실측(/dev/uhid 접근, 무선 ADB 데몬 지연, 경계 전환 레이턴시)은 없다. KeyBridge 실구현 소스도 본 저장소 밖이다.
- 그러므로 현재 상태는 `original_goal_satisfied: false`, `physical_tests_passed: false`다. Stage 6 물리 검증 실패 시 옵션 재협의(B 경로 폴백 또는 범위 축소 — 축소는 Q3 명시 수용 필요)가 후속 결정이다. 이 문서는 설계 승인 전까지 구현을 시작하지 않는다.
