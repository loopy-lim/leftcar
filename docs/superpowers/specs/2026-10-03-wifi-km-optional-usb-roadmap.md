# Wi-Fi KM 선택적 USB 로드맵 — 승인된 전송 계약 기록

문서 상태: 사용자 승인 전송 계약의 기록 + 미래 선택 확장 로드맵. 기준일 2026-10-03. 제품 소스 변경 없음, 기기 설정 변경 없음, 무선 ADB 미사용·미요청.
근거 JSON: [2026-10-03-wifi-km-optional-usb-roadmap.json](2026-10-03-wifi-km-optional-usb-roadmap.json).
참조: [2026-10-03-wifi-km-connected-device-validation.md](../../research/2026-10-03-wifi-km-connected-device-validation.md), [2026-09-25-wifi-bidirectional-km-design.md](2026-09-25-wifi-bidirectional-km-design.md), [2026-09-25-wifi-km-full-parity-gap.md](../../research/2026-09-25-wifi-km-full-parity-gap.md).

## 1. 승인된 전송 계약 (사용자 명시 승인)

| 항목 | 값 | 의미 |
| --- | --- | --- |
| Wi-Fi 우선 | **예** | Mac ↔ 태블릿 Leftcar 앱 간 직접 Wi-Fi 통신이 유일한 기본 전송 경로다. 릴레이·무선 ADB를 경유하지 않는다. |
| 무선 ADB | **불요·기본 미요청** | 설계 §13-Q1(옵션 B 무선 ADB + UHID)은 기본 경로에서 제외됐다. 승인 요청을 반복하지 않으며, 사용자가 별도로 명시 승인하면 재등장한다. |
| USB | **선택적 미래 확장** | 지금 구현하지 않는다(`usb_implementation_now=false`). 착수는 사용자의 별도 승인이 전제다. |
| 입력 권한 분리 | **예** | 전송(네트워크·케이블) 승인과 Android/macOS OS 입력 권한 승인은 별개의 결정이며, 어느 한쪽이 다른 쪽을 활성화하지 않는다. |

이 계약은 원래 목표(양방향 실제 커서·키보드·경계 이동)를 축소하지 않는다. 다만 그 목표는 여전히 미충족이며(`original_goal_satisfied=false`), 본 문서는 전송 계약만을 기록한다.

## 2. 연결 기기 실측 결과 반영 (성공 주장 금지)

[wifi-km-connected-device-validation](../../research/2026-10-03-wifi-km-connected-device-validation.md)의 결과를 있는 그대로 반영한다:

- **LAN 도달성: 입증** — `ping 192.168.0.18` 4/4 성공(0% loss, rtt avg 154.2ms — 재측정 필요), mDNS 상호 발견(`leftcar-host:7777` ↔ TB710FU/Android API 36) 확인. Wi-Fi 직접 IP 경로가 살아 있다.
- **라이브 Leftcar 세션: blocked** — 태블릿 UI를 에이전트가 구동할 수단이 없어(USB 기기 부재 + 무선 ADB 금지) 동의된 앱 간 세션을 이번 실행에서 열지 못했다. 과거 UDP 세션 기록은 존재하나 기기 귀속은 단정 불가.
- **전체 완전-KM 물리 테스트: 전부 미수행** — mouse/keyboard/real_cursor_hover/edge_crossing/global_capture_suppression 모두 blocked_untested. 미수행을 성공으로 기재하지 않는다.

**최소 다음 인간 조치**: 태블릿에서 Leftcar Viewer를 열어 `leftcar-host`에 연결하고 세션을 승인한다(`source_grants.json`이 `dirty=true`이므로 재검토 프롬프트가 뜨면 확인). 새 OS 권한 불필요. 이것만으로 라이브 앱 간 UDP 전송이 재현된다 — 단 이는 **전송 계층 재현**일 뿐 완전-KM 달성이 아니다.

## 3. USB 확장 평가 (선택적 — 지금 구현하지 않음)

### 3.1 기존 usb-mux 크레이트의 실제 범위

`crates/usb-mux/src/lib.rs`는 **AOAP 벌크 링크의 프레이밍 계층**이다: `[channel:u8][length:u32 BE][payload]` 프레임, 채널 0=제어평면(newline-delimited), 채널 1=미디어 프레임, 16MiB 상한, 인코더·디코더 재구성과 유닛 테스트. 기기 열거·AOA 액세서리 핸드셰이크·입력 주입·권한 처리는 없다. 즉 USB 전송의 "말" 계층만 존재하고, USB 연결 자체를 만드는 호스트 구현은 이 크레이트에 없다.

### 3.2 핵심 원칙: USB 전송 ≠ OS 입력 권한

**USB 전송이 가능해져도 OS 입력 권한은 자동으로 부여되지 않는다.** Wi-Fi 세션 승인이 무선 디버깅 권한을 켜지 않는 것과 같은 분리 원칙이다. USB에서 입력 권한은 경로별로 다시 따진다:

| 경로 | ADB 필요 | 입력 권한 근거 | 상태 |
| --- | --- | --- | --- |
| AOA 앱 간 벌크 링크(전송만) | 불요 | USB 액세서리 모드(표준 `UsbManager` 경로 — [developer.android.com/develop/connectivity/usb/accessory](https://developer.android.com/develop/connectivity/usb/accessory)) | **전송 후보** — TB710FU 열거·대역폭 미실측, 호스트 측 구현 존재 미확인 |
| AOAv2 HID (USB 레벨 가상 HID 마우스·키보드) | 불요 | 호스트가 USB 레벨에서 HID 등록 — Android가 실제 물리 HID로 인식. scrcpy 문서: AOA는 "works at the USB level directly (so it only works over USB)" ([scrcpy keyboard.md](https://github.com/Genymobile/scrcpy/blob/master/doc/keyboard.md)) | **커서·hover 후보** — TB710FU AOAv2 HID 지원·OEM 동작 미실측, 호스트 libusb/HID 구현 없음 |
| UHID 가상 HID | **요(adb shell)** | AOSP `shell.te` "allow shell input injection" (`/dev/uhid`) — full-parity-gap §2.2 | **기본 계약 제외** — 무선 ADB 미승인 상태이며 adb shell이 전제. 별도 명시 승인 시에만 재검토 |

결론: USB를 나중에 선택하더라도 입력 권한은 별도 면이다. AOA HID가 무선-ADB 없는 실제 커서 후보로 유일하게 남지만, 그것은 아직 기기 실측도 구현도 없는 후보다. 검증 전에 설계 계약에 편입하지 않는다.

## 4. API 34+ 접근성 모션 이벤트 — 기존 범주적 주장 정정

[wifi-km-connected-device-validation §5](../../research/2026-10-03-wifi-km-connected-device-validation.md)에서 확인된 정정을 설계·격차 문서와 정리한다:

- **철회 대상 주장**: "프레임워크 수준 전역 모션 필터는 공개 API 부재로 불가, AccessibilityService는 키 필터만 제공"(full-parity-gap §2.3 표 및 설계 §0).
- **1차 근거**:
  - `AccessibilityService.onMotionEvent(MotionEvent)` — API 34 추가, "allows an accessibility service to observe generic MotionEvents" ([개발자 문서](https://developer.android.com/reference/android/accessibilityservice/AccessibilityService#onMotionEvent(android.view.MotionEvent))).
  - `AccessibilityServiceInfo.setMotionEventSources(int)` — API 34, "**MotionEvents from sources in getMotionEventSources() are not sent to the rest of the system**" ([개발자 문서](https://developer.android.com/reference/android/accessibilityservice/AccessibilityServiceInfo#setMotionEventSources(int))) → 구독 소스에 대해 **관찰과 억제가 동시에 성립**한다.
  - 허용 소스에 `SOURCE_MOUSE`, `SOURCE_MOUSE_RELATIVE`, `SOURCE_TOUCHPAD`, `SOURCE_TRACKBALL` 등이 명시된다 ([InputDevice](https://developer.android.com/reference/android/view/InputDevice#SOURCE_MOUSE)).
- **정직한 한계(유지)**:
  - 동작·지연·OEM 차이(TB710FU = API 36, Lenovo)는 **기기 실측 전**이다. "전역 억제가 실제로 먹는다"는 실측 전까지 후보로만 기록한다.
  - **Mac → Android 주입 권한 격차는 유지**: leftcar.ll3.kr의 Input Monitoring 미승인(TCC 행 부재), Stage 2 전제 미충족. USB 전송도 이 격차를 해소하지 않는다.
  - TB710FU Viewer 측 접근성·IME 권한 상태는 미확인으로 남는다.

## 5. 목표 유지와 축소 모드 규율

- **원래 목표 유지**: 양방향 실제 마우스·키보드·경계 이동의 완전 수용 기준은 그대로 보존한다. 본 로드맵은 이를 축소한 문서가 아니다.
- **축소 모드 규율**: 제스처/키보드-only 축소 모드는 원래 목표를 충족하지 못하므로, 채택에는 **새로운 명시적 사용자 수용**이 필요하며 무답변 기본 수용은 금지된다(설계 §13-Q3 원칙).

## 6. 다음 단계 (요약)

1. **최소 사용자 조치(전송 계약 내)** — 태블릿에서 Viewer를 열어 `leftcar-host`에 연결·승인. 라이브 앱 간 UDP 전송 재현(전송 계층 한정).
2. **API 34 모션 관찰·억제 기기 실측 설계 준비** — `setMotionEventSources(SOURCE_MOUSE)`의 관찰·억제·지연·OEM 차이를 TB710FU에서 실측하는 계획(§13-Q2 KeyBridge 경로와 연결).
3. **KM 구현 승인 범위 갱신 제출** — Wi-Fi primary / no-ADB / USB optional 계약을 반영한 설계 승인 요청.
4. **USB 확장은 보류** — 사용자의 별도 승인 + TB710FU 액세서리 모드·AOAv2 HID 실측 + 호스트 구현 범위 결정이 착수 조건이다. 지금은 아무것도 구현하지 않는다.
