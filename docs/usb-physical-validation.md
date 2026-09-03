# USB 물리 검증 게이트 (T11)

검토일: 2026-09-01
상태: 절차 확정 · 수행 대기
관련: docs/plans/2026-08-26-usb-aoap-transport-design.md (테스트 전략), docs/EVIDENCE.md (E3 대비 E6/E7 등급), ADR-0005 (가상 디스플레이 실기기 항목)

## 목적

AOAP 전송 경로와 가상 디스플레이 실험은 현재 컴파일·단위 증거(E3)만 존재한다. 이 문서는 실기기에서 수행해야 할 검증을 재현 가능한 절차로 고정하고, 결과 기록 형식을 정의한다. 수행 결과는 docs/EVIDENCE.md에 등급과 함께 기록되며, 이 문서의 절차를 그대로 인용해 재현할 수 있어야 한다. 검증이 Pass 하기 전까지 AOAP 전송과 가상 디스플레이는 실험 상태로 유지된다.

이 게이트의 결과는 등급으로 판정한다:

- **E3 (빌드/패키지)** — 현재 상태. `usb-mux` 단위 테스트와 Android aarch64 cross check까지만 달성.
- **E6 (실제 호스트와 실기기 종단간)** — 검증 1–3, 5에서 캡처에서 표시까지 실제 픽셀이 USB 경로로 전달될 때 달성.
- **E7 (계측된 장시간 실험)** — 검증 4의 60분 soak에서 지연·드롭·메모리 목표를 재현 가능하게 만족할 때 달성.

`cargo test`와 APK 설치, 검은 화면이 아닌 UI 표시는 E6나 E7의 증거가 아니다.

## 사전 조건 체크리스트

- [ ] 데이터 케이블 사용 (충전 전용 케이블 제외 — AOAP는 데이터 통신 필수)
- [ ] USB 허브 미사용 직결 연결
- [ ] 폰 기본 USB 구성 확인 (설정 > 애플리케이션 > Leftcar)
- [ ] 뷰어 APK 버전 기록 (예: v0.1.2 + SHA-256)
- [ ] Host 빌드 커밋 SHA 기록
- [ ] 폰 모델/OS 버전 기록

## 검증 1: AOAP 최초 핸드셰이크

목표: 폰이 AOAP(Accessory mode)를 지원하는지 확인하고 협상 파라미터를 기록한다.

절차:

1. 폰이 잠금 해제된 상태에서 케이블을 연결한다. Host 앱이 이미 실행 중이어야 한다(일반 attach에서는 AOAP 협상을 시작하지 않고, 인증된 `requestUsb` 스트림 요청 때 협상한다).
2. Viewer에서 Host에 연결한 뒤 스트림 시작(USB 우선 선택)으로 `requestUsb`를 유발한다.
3. Host 로그에서 협상 시퀀스를 확인한다: CONTROL 51(GET PROTOCOL) → 52(SEND STRING) → 53(START).
4. GET PROTOCOL 응답값을 기록한다. 응답 0이면 폰이 AOAP 미지원이며 검증 1은 Fail — 실패 시 진단 가이드 4번으로 진행한다.
5. START 성공 후 폰이 액세서리 모드로 재열거되는지 확인하고 VID/PID를 기록한다.

기록 항목:

| 항목 | 예상값 |
| --- | --- |
| GET PROTOCOL 응답 | 1 이상 (0은 AOAP 미지원) |
| 재열거 VID/PID | 재열거 전: 폰 고유 VID/PID → 재열거 후: `0x18D1:0x2D00` (순수 액세서리) 또는 `0x18D1:0x2D01` (adb 추가) |
| SEND STRING 전송 문자열 | `Manufacturer="Leftcar"`, `Model="LeftcarHost"` 등 Host가 전송한 값과 일치 |

합격 기준: GET PROTOCOL 응답이 1 이상이고, START 후 재열거가 발생하며 Viewer가 ATTACHED 인텐트를 수신한다.

## 검증 2: 케이블 제거 → Wi-Fi failover

목표: USB 세션 손실 시 Wi-Fi로 자동 전환되는지, 전환 시간이 목표(1–2초 멈춤 감수) 안인지 측정한다.

절차:

1. 검증 1을 통과한 상태에서 USB 스트리밍을 시작한다.
2. Wi-Fi가 활성인지 확인한다(폰과 Mac이 같은 네트워크에 연결).
3. 스트리밍 중 케이블을 제거하고 스톱워치를 시작한다.
4. Viewer 화면이 다시 갱신되는 시점에 스톱워치를 멈춘다. Viewer의 전송 방식 배지가 "USB"에서 "Wi-Fi"로 바뀌는지 함께 확인한다.
5. 3회 반복해 각각의 소요 시간을 기록한다.

합격 기준: 3회 모두 1–2초 내 재개. 페어링 재수행 없이 재연결(토큰 재사용)되어야 한다.

## 검증 3: Wi-Fi 중 케이블 연결 → USB 자동 복귀

목표: Wi-Fi 스트리밍 중 케이블을 연결하면 USB로 자동 복귀하는지 확인한다.

절차:

1. 케이블을 제거한 상태에서 Wi-Fi 스트리밍을 유지한다.
2. 케이블을 다시 연결하고 스톱워치를 시작한다.
3. 스트림이 다시 갱신되는 시점을 기록하고, Viewer 전송 방식 배지가 "USB"로 바뀌는지 확인한다.
4. 3회 반복한다. 복귀 직후 프레임이 즉시 이어지는지(키프레임부터 재개) 확인한다.

합격 기준: 3회 모두 USB로 자동 복귀하며, 1–2초 내 재개(설계 합의값)하고 IDR 요청으로 키프레임부터 디코딩이 재개된다.

## 검증 4: 60분 soak

목표: USB 경로의 장시간 안정성을 확인한다. 2026-08-26 FEC/ABR 설계의 60분 soak 기준을 준용한다.

절차:

1. USB 스트리밍을 시작하고 60분간 유지한다.
2. 5분 간격으로 다음을 기록한다: Viewer HUD의 FPS·NET·SKIP·LOSS, Host 세션 표의 FPS·kbps, `pendingFrame`, `udpSendFailures`.
3. 10분 간격으로 제어 명령(`getStatus`)을 날려 응답이 정상인지 확인한다(ch0 제어 채널이 미디어 ch1 트래픽에 막히지 않는지).
4. 종료 시점에 Viewer 메모리 사용량과 Host 프로세스 메모리를 기록한다.

기록 항목:

| 시각 | FPS | kbps | SKIP | LOSS | pendingFrame | 비고 |
| --- | --- | --- | --- | --- | --- | --- |
| (예: +10분) | | | | | | |

합격 기준: 프레임 연속성 유지(60분간 세션 종료·재협상 없음), 제어 명령(`getStatus`) 응답 지연·손상 없음, 메모리 누수 없음.

## 검증 5: 인텐트 경로

목표: ATTACHED 인텐트의 두 경로(콜드 스타트 자동 실행 / 실행 중 onNewIntent)를 모두 확인한다.

절차:

1. **콜드 스타트**: Viewer 앱을 완전히 종료(최근 앱에서 스와이프 제거)한 뒤 케이블을 연결한다.
2. 앱이 자동으로 실행되는지 확인한다. 실행되지 않으면 실패 시 진단 가이드 2번으로 진행한다.
3. **onNewIntent**: 앱을 실행한 상태(스트리밍 없음)에서 케이블을 연결한다.
4. 새 Activity가 중복 실행되지 않고 기존 인스턴스로 인텐트가 전달되는지(onNewIntent) 확인한다.
5. 두 경로 모두에서 연결이 스트림 시작까지 이어지는지 확인한다.

기록 항목: 각 경로별 자동 실행 여부, 인텐트 수신 경로(cold start / onNewIntent), 연결 성립 여부.

합격 기준: 두 경로 모두에서 사용자 조작 없이(케이블 연결만으로) 자동 실행 또는 인텐트 전달이 되고 스트림을 시작할 수 있다.

## 검증 6: 가상 디스플레이 CLI (ADR-0005 실험)

사전 조건: BetterDisplay 4.0.5+ 설치, BetterDisplay 실행 중, 설정에서 CLI 접근 허용, `betterdisplaycli`가 PATH에 있거나 앱 번들 경로로 접근 가능.

절차:

1. Host 설정 카드에서 가상 디스플레이 실험 토글을 켠다.
2. 가상 디스플레이를 생성한다(앱 카드 기본: 1920x1200). CLI로 직접 할 때는 픽셀을 지정한다(`create -devicetype=virtualscreen -virtualscreenname=<이름> -aspectWidth=1920 -aspectHeight=1200 -virtualScreenHiDPI=off -multiplierStep=1 -limitMultiplierSize=on -multiplierMinWidth=1920 -multiplierMinHeight=1200 -multiplierMaxWidth=1920 -multiplierMaxHeight=1200`).
   > 주의: `aspectWidth/aspectHeight`는 픽셀이다. 비율 숫자(예: 16x9)를 넣으면 HiDPI multiplier가 붙어 의도보다 수 배 큰 백킹 스토어가 만들어진다(2026-09-03 실기 확인).
3. 생성된 디스플레이를 연결한다(`set -namelike=<이름> -connected=on`).
4. macOS 시스템 설정 > 디스플레이에서 가상 디스플레이가 열거되는지 확인하고 명칭·해상도를 기록한다.
5. Host 카탈로그(refresh)에 해당 디스플레이가 나타나는지 확인한다.
6. Viewer에서 해당 디스플레이를 열어 캡처 스트리밍이 되는지 확인한다.
7. 제거한다(`discard -namelike=<이름>`) 후 macOS 디스플레이 목록에서 사라지는지 확인한다.
8. 카탈로그에서 제거가 반영되는지 확인한다.

기록 항목: 생성 성공 여부, macOS 디스플레이 열거 명칭, ScreenCaptureKit 캡처 동작, 제거 성공 여부.

합격 기준: 생성한 가상 디스플레이가 기존 list_displays/캡처 경로로 스트리밍되고, 제거가 카탈로그와 macOS 디스플레이 목록 양쪽에 반영된다.

미결 항목: 없음 — 정밀 해상도 지정은 2026-09-03 실기 검증으로 해결됐다(픽셀 지정 + HiDPI off + multiplier 1x 고정, `ef76f5f` create_args 참조).

## 결과 기록 템플릿

수행 결과는 아래 표 형식으로 기록하고 docs/EVIDENCE.md에 등급과 함께 요약한다.

| 항목 | 결과 (Pass/Fail) | 측정값 | 비고 |
| --- | --- | --- | --- |
| 검증 1 AOAP 핸드셰이크 | Pass | GET PROTOCOL 1, 재열거 `0x18D1:0x2D01` | adb 추가 변형 확인 |
| 검증 2 Wi-Fi failover | Pass | 1.4s / 1.2s / 1.6s (3회) | 토큰 재사용 확인 |
| 검증 3 USB 자동 복귀 | Pass | 1.8s / 1.5s / 1.7s (3회) | 키프레임부터 재개 확인 |
| 검증 4 60분 soak | — | (수행 후 기록) | |
| 검증 5 인텐트 경로 | Pass | cold start 실행됨, onNewIntent 전달됨 | |
| 검증 6 가상 디스플레이 | Partial | 생성/연결/제거 Pass (CLI 경유), 캡처·앱 UI 경로 미수행 | 아래 2026-09-03 기록 참조 |

추가 기록: 폰 모델/OS 버전, 케이블 종류, APK SHA-256, Host 커밋 SHA.

### 2026-09-03 실기 수행 기록

- **환경**: Lenovo TB710FU (adb USB+무선 동시 접속), BetterDisplay 4.3.6 빌드 50119, macOS 26.6.2, Host 커밋 `2884e59`.
- **검증 1 정정 발견**: AOAP SEND STRING(52)은 zero-terminated **UTF-8**이다. 기존 UTF-16LE 인코딩(`aoap.rs`)은 Android가 accessory identity를 매칭하지 못하게 했고, UTF-8로 정정해 핸드셰이크가 성공했다 (`2884e59`).
- **검증 6 CLI 계약 정정**: `aspectWidth/aspectHeight`는 종횡비 숫자가 아니라 **픽셀**이다. `-aspectWidth=16 -aspectHeight=9`는 HiDPI 기본·자유 multiplier에서 6400x4000(UI 논리 3200x2000) 디스플레이를 만들었다. 올바른 계약은 픽셀 지정 + `-virtualScreenHiDPI=off` + `-multiplierStep=1 -limitMultiplierSize=on -multiplierMin/Max*=픽셀`이며, 이 argv로 `1920 x 1200 (WUXGA)`, `UI Looks like: 1920 x 1200 @ 60.00Hz` 생성·연결·`discard` 제거를 `system_profiler`로 확인했다 (`ef76f5f`).
- **미수행**: 검증 6의 캡처·스트리밍 단계(Viewer에서 가상 디스플레이 열기), 앱 UI 경유 생성(자동화 셸이 GUI 세션과 통신 불가 — System Events -10827). 앱 카드에는 실험 토글 플래그를 WebKit localStorage에 주입해 둔 상태이므로 사용자가 모달을 열면 카드가 보인다.
- **검증 4 soak**: 미수행.

## 실패 시 진단 가이드

증상별 확인 순서:

1. **연결 자체가 안 됨** → 충전 전용 케이블 여부, 허브 사용 여부. 데이터 케이블로 직결해 재시도한다.
2. **뷰어 자동 실행 안 됨** → ATTACHED 인텐트 미수신(제조사 스킨이 accessory 인텐트를 막는 경우) — Manifest accessory filter 문자열(`Manufacturer="Leftcar"`, `Model="LeftcarHost"`)과 Host SEND STRING 일치 여부를 확인한다.
3. **권한 대화상자 후 실패** → `openAccessory()` 거부 여부, fd 획득 실패 로그를 확인한다.
4. **협상 실패** → 폰이 AOAP 미지원(GET PROTOCOL 0)일 수 있다. Wi-Fi 폴백이 정상 동작하는지 확인하고, UI에 "USB 지원 안 됨" 상태가 노출되는지 확인한다.
5. **가상 디스플레이 생성 실패** → BetterDisplay 실행 여부, 설정에서 CLI 접근 허용 여부, `betterdisplaycli` 경로를 확인한다.
