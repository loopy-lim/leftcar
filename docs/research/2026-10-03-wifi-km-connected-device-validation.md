# 연결 기기 실측 — 태블릿·Mac 직접 Wi-Fi (무선 ADB 미사용)

문서 상태: 연결 기기 검증 결과. 기준일 2026-10-03. 제품 소스 변경 없음, 무선 ADB 사용 없음.
근거 JSON: [2026-10-03-wifi-km-connected-device-validation.json](2026-10-03-wifi-km-connected-device-validation.json).
정정 대상 문서: [2026-09-25-wifi-km-full-parity-gap.md](2026-09-25-wifi-km-full-parity-gap.md), [2026-09-25-wifi-bidirectional-km-design.md](../superpowers/specs/2026-09-25-wifi-bidirectional-km-design.md).

## 1. 결과 요약

**Wi-Fi 직접 연결 테스트: `blocked`** — 네트워크 계층의 직접 LAN 도달성은 입증됐으나(아래 3장), 동의된 Leftcar 앱 간 세션의 "이번 실행"은 태블릿 UI를 에이전트가 구동할 수단이 없어 수행하지 못했다(USB 기기 부재 + 무선 ADB 금지). 과거 전송 기록은 존재한다. 미수행을 성공으로 기재하지 않는다.

**원래 목표(양방향 실제 커서·키보드 경계 전환)는 여전히 미충족** — `original_goal_satisfied: false`. 이번 작업으로 달성 여부가 바뀌지 않았으며, 대신 API 34+ 접근성 모션 관찰이라는 새로운 무선-ADB-불필요 후보 경로가 확인됐다(5장).

## 2. 연결된 기기 식별 (사전 가정 없이)

### Mac (Host)

- macOS **26.6.2 (25G83)**, Leftcar Host 실행 중(PID 67539), `*:7777` 리슨, mDNS `_leftcar._tcp` `leftcar-host` 광고.
- LAN 주소: `192.168.0.133` (en0), `192.168.0.154` (en9). USB로 연결된 기기 없음(`system_profiler SPUSBDataType` 비어 있음, `adb devices` 빈 목록).
- 권한(시스템 TCC DB, 읽기 전용 조회):

| 권한 | 대상 | 값 | 승인일 |
| --- | --- | --- | --- |
| Accessibility | leftcar.ll3.kr | 허용(2) | 2026-08-25 |
| ScreenCapture | leftcar.ll3.kr | 허용(2) | 2026-09-14 |
| ListenEvent(Input Monitoring) | **kr.ll3.keybridge** | 허용(2) | 2026-09-23 |
| ListenEvent | **leftcar.ll3.kr** | **행 없음(미승인)** | — |

### 태블릿 (Viewer)

- mDNS TXT로 식별: `_adb-tls-connect._tcp` `adb-HA2D6EMP-Qnp1Nz` → `name=TB710FU`, `api=36.0` (**Android API 36**), 주소 `192.168.0.18` (`Android.local.` A 레코드).
- Leftcar 페어링 기록 확인: `paired_devices.json`에 TB710FU (`mt3rf5l0-3jrargbs`, 2026-09-14 페어링).
- 태블릿이 스스로 무선 디버깅(`_adb-tls-connect`, 포트 37727)을 광고하고 있었다. **이번 작업에서 그 권한은 사용하지 않았다**(금지 사항). 광고 자체는 기기가 이미 켜 둔 상태의 수동 관찰일 뿐이다.
- 태블릿 측 Viewer 접근성·IME 권한 상태는 기기 접근 수단이 없어 **미확인**.

## 3. Wi-Fi 직접 연결 증거

| 항목 | 명령 | 결과 |
| --- | --- | --- |
| 직접 LAN 도달성 | `ping -c 4 -t 5 192.168.0.18` | **4/4 성공, 0% loss**, rtt avg **154.2 ms** (min 104.9 / max 182.1 — 태블릿 Wi-Fi 절전 추정, 재측정 필요) |
| Mac 발견 | `dns-sd -L leftcar-host _leftcar._tcp local.` | `leftcar-host.local.:7777`, `product=leftcar` |
| 태블릿 발견 | `dns-sd -L adb-HA2D6EMP-Qnp1Nz _adb-tls-connect._tcp local.` + `dns-sd -G v4 Android.local.` | `api=36.0 name=TB710FU`, `192.168.0.18` |
| 과거 앱 간 UDP 전송 | `sessions.jsonl` | session 10: `transport=udp`, 2026-10-03 02:41:34 시작 → 02:46:08 종료(reason 3). 총 343세션 기록. 단 `dev:*` 식별자는 페어링 이름과 직접 매핑되지 않아 기기 귀속은 단정 불가 |
| **라이브 앱 간 세션** | — | **blocked** — 태블릿에서 Viewer를 여는 동작을 수행할 수단 없음. 제어 평면은 QR 핀 Ed25519 핸드셰이크 계약이라 외부 대역 내 시험 불가 |

수행하지 않은 것(의도적 회피): `adb connect/tcpip/pair`, 포트 37727 접속, `/dev/uhid` 접근, 설정 변경, 입력 주입.

## 4. 방향별 기능 평가 (전 항목 이번 세션 미실측)

### Mac → Android

| 기능 | 평가 | 상태 |
| --- | --- | --- |
| mouse | 보이는 커서 경로는 P2(무선 ADB+UHID) 승인 전제 — 이번 금지. 접근성 제스처 폴백 미구현 | blocked_untested |
| keyboard | 자체 IME(Stage 5)·globalAction 어댑터 미구현, Android 측 IME 활성화 별도 절차 | blocked_untested |
| real_cursor_hover | 일반 앱/접근성 권한으로는 전역 가시 커서·hover 없음 (P2 승인 시에만 후보) | impossible_under_ordinary_permissions_untested |
| edge_crossing | Mac 측 CGEventTap 트리거는 설계뿐. leftcar.ll3.kr의 Input Monitoring 미승인 | untested_not_implemented |
| global_capture_suppression | 억제 탭(defaultTap) 전제 — Input Monitoring 미승인 + Stage 2 미구현 | blocked_untested |

### Android → Mac

| 기능 | 평가 | 상태 |
| --- | --- | --- |
| mouse | Leftcar 스트리밍 내 포인터 전송·CGEvent 주입은 프로덕션 경로(앱 창 한정). **전역** 관찰·억제는 API 34+ 모션 관찰이 새 후보(5장), 기기 실측 없음 | partial_candidate_untested |
| keyboard | KeyBridge 접근성 키 필터 경로 사실상 완성(기존 판정) + Mac 측 주입 권한 승인 확인. 실측은 이번에 없음 | ready_untested |
| real_cursor_hover | Mac 커서는 CGEvent 주입의 진짜 macOS 커서 — 방향상 문제 없음, 미실행 | untested |
| edge_crossing | 트리거 후보: 가상 커서(무선 ADB 필요)·스트립·evdev·**신규 API 34 모션 관찰** — 전부 미검증 | untested |
| global_capture_suppression | 키보드: KeyBridge(기존, ready). 마우스: API 34 `setMotionEventSources(SOURCE_MOUSE)` 후보 — "공개 API 불가" 판정 정정, 실측 전 | keyboard_ready_untested / mouse_candidate_untested |

## 5. 새 발견 — Android API 34+ 접근성 모션 이벤트 (기존 주장 정정)

기존 gap 문서의 "프레임워크 수준 전역 모션 필터는 공개 API 부재"라는 **범주적 주장은 API 34+에서 성립하지 않는다**:

- `AccessibilityService.onMotionEvent(MotionEvent)` — **API 34 추가**. "allows an accessibility service to observe generic MotionEvents".
- `AccessibilityServiceInfo.setMotionEventSources(int)` — **API 34**. 결정적 문구: "**MotionEvents from sources in getMotionEventSources() are not sent to the rest of the system**" → 구독 소스에 대해 관찰과 **억제**가 동시에 성립한다.
- 허용 소스 값에 `InputDevice.SOURCE_MOUSE`, `SOURCE_MOUSE_RELATIVE`, `SOURCE_TOUCHPAD`, `SOURCE_STYLUS`, `SOURCE_TRACKBALL` 등이 명시된다(터치스크린은 `TouchInteractionController` 별도 경로).
- TB710FU는 API 36이므로 API 존재는 확인. 동작·지연·OEM 차이는 **기기 실측 전**이며 KeyBridge 확장(설계 §13-Q2)과 연결된다.

영향: android_to_mac 마우스의 전역 관찰·억제와 Android 측 경계 감지가 무선 ADB 없이도 접근성 권한만의 후보 경로로 갱신됐다. `wifi-km-full-parity-gap`의 해당 셀은 후속 정정 대상.

## 6. 차단 요인

1. USB로 연결된 Android 기기 없음 → 태블릿 UI 구동 불가.
2. 무선 ADB 금지(이번 작업 제약) — 태블릿이 광고 중이어도 접속하지 않음.
3. KM 기능 자체가 제품에 미구현(설계 승인 대기) — 라이브 KM 테스트 대상 코드 부재.
4. leftcar.ll3.kr에 Input Monitoring 미승인 — Stage 2 권한 전제 미충족.
5. 태블릿 Viewer 권한 상태 확인 불가(기기 접근 수단 부재) — 미확인으로 기록.
6. 세션 로그 `dev:*` ↔ 기기 이름 매핑 부재로 과거 세션 귀속 단정 불가.

## 7. 다음 단계 (전송 권한과 Android OS 권한을 분리해 기재)

**최소 사용자 조치(전송 계층, 새 권한 불필요)** — 태블릿에서 Leftcar Viewer를 열어 `leftcar-host`에 연결한다. 페어링·화면·입력 승인은 이미 기록돼 있으나 `source_grants.json`이 `dirty=true`이므로 재검토 프롬프트가 뜨면 확인하면 된다. 이것만으로 라이브 앱 간 UDP 전송이 재현된다.

**Android OS 권한 결정(전송과 별개의 권한면)** — 완전 패리티 후보인 P2(무선 디버깅 페어링 + UHID) 승인 여부는 설계 §13-Q1의 사용자 결정이다. 앱 간 Wi-Fi 전송 승인과 무선 디버깅 권한은 서로 다른 결정이며, 한쪽 승인이 다른 쪽을 활성화하지 않는다.

**무선 ADB 없는 대안 실측** — TB710FU(API 36)에서 KeyBridge 확장으로 `setMotionEventSources(SOURCE_MOUSE)`의 관찰·억제·지연을 기기 실측한다(§13-Q2 연결).

**Stage 2 착수 전** — leftcar.ll3.kr에 Input Monitoring을 1회 부여한다.

**권고 구현 순서(설계 §11)** — Stage 0 계약(KM 메시지 + `km_share`) → Stage 1 Android→Mac 키보드(권한 이미 충족) → Stage 2 Mac 경계·억제 → Stage 3 Mac→Android(P2 승인 시 UHID / 비승인 시 Q3 축소 결정) → Stage 4 동의·UI → Stage 5 IME → Stage 6 물리 검증.

**도구** — `python3 tools/wifi-km-device-probe.py` (표준 라이브러리 전용, 읽기 전용, `--help` 동작 확인). Mac/태블릿 상태·mDNS·페어링·세션·TCC를 한 번에 수집한다.
