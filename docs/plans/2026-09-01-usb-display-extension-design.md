# USB 화면 확장 3종 설계 — 연결 상태 가시성 · 가상 디스플레이 · USB 물리 검증

날짜: 2026-09-01
상태: 승인됨 (섹션별 사용자 확인 완료)
접근: 단계적(접근 A) — 배지+문제해결 UI → USB 물리 검증 문서 → BetterDisplay 옵트인 실험
관련: 2026-08-26-usb-aoap-transport-design.md (AOAP 전송 경로), 09-risk-register.md R-015 (가상 디스플레이 v1 논골), DESIGN.md (모노크롬 디자인 토큰)

## 배경

USB(AOAP) 전송 경로는 이미 구현돼 있으나(2026-08-26), 세 가지 공백이 있다:

1. **연결 방식 가시성 부족** — 활성 스트림이 USB로 흐르는지 Wi-Fi로 흐르는지 UI에 표시되지 않는다. 데이터는 이미 존재한다(제어 계약 `media_transport`, `ActiveStream.mediaTransport`).
2. **"실행했는데 앱 창이 안 뜨는" 실사용 장애** — 사용자 보고: 설치는 성공했으나 실행 후 Host 창이 보이지 않았다. 코드상 원인: (a) main 창 닫기가 hide 동작이고 재개로는 트레이 아이콘만 존재(`lib.rs:120-127`), (b) 캡처 백엔드/포트 bind 실패 시 panic으로 무반응 종료(`lib.rs:36-37,48-50`), (c) 재서명 시 TCC 화면 기록 권한 초기화(README).
3. **AOAP 물리 검증 부재** — 구현 증거가 E3(컴파일/단위) 수준이고 실기기 검증(T11) 절차가 문서로 고정돼 있지 않다.

추가 목표: 드라이버 개발 없이 진짜 확장 모니터 경험을 제공하는 BetterDisplay 가상 디스플레이 연동 (옵트인 실험).

## 설계 1 — 연결 상태 표시 + 창 표시 개선 + 문제 해결 안내

### 1A. 스트림 카드 전송 방식 배지 (Viewer Android)

- 위치: `apps/viewer-expo/app/catalog.tsx`의 `ActiveStreamItem` — 해상도·FPS 행 옆.
- 데이터: `ActiveStream.mediaTransport` (`src/catalog-model-types.ts:28`) — 이미 존재, 추가 모델 변경 없음.
- 매핑: `usb → "USB"`, `udp → "Wi-Fi"`, `tcp → "Wi-Fi (TCP)"`, `adbTcp → "ADB"` (레거시).
- 자동 갱신: UDP↔USB 전환 시 `use-stream-controller.ts`의 `replaceRestartedStreamState`가 스트림 목록을 교체하므로 배지가 자동 갱신된다. 별도 구독 불필요.
- 스타일: DESIGN.md 모노크롬 토큰 — 배경 `--bg-surface-subtle`, 텍스트 `--text-secondary`. 색상·아이콘 과용 금지.

### 1B. Host 창 표시 개선 (Rust/Tauri)

- **시작 시 창 표시 보장**: `tauri.conf.json` windows[0]에 `visible: true` 명시 + setup hook에서 main 창 `show()` 보강(시작 race 대비).
- **panic 제거**: `platform_backend()` 실패와 control listener bind 실패 시 `panic!` 대신 오류 대화상자(rfd) 표시 후 정상 종료. 메시지 예: "포트 7777을 사용할 수 없습니다 — 다른 Leftcar Host가 실행 중일 수 있습니다."
- 동시 실행 방지 안내는 오류 메시지에 포함(README:85의 "설치본과 dev 동시 실행 금지" 정책을 사용자에게 노출).

### 1C. 문제 해결 가이드 확장 (Host + Viewer)

- **Viewer** (`host.tsx` troubleshoot 섹션 + i18n): "컴퓨터 앱 창이 안 뜨나요?" 항목 추가 — 닫기는 숨기기이며 macOS 메뉴 막대 트레이 아이콘 → "Leftcar Host 열기"로 재개함을 안내.
- **Host** (`App.tsx` troubleshoot 가이드 + i18n): 기존 4항목(Wi-Fi/AP 격리/방화벽/권한)에 5번째 항목으로 동일 내용 추가.
- i18n은 `packages/ui-tokens/src/i18n.ts` 한국어·영어 쌍으로 추가.

## 설계 2 — BetterDisplay 가상 디스플레이 연동 (옵트인 실험)

### 목표와 형태

- 드라이버 개발 없이 "창을 드래그해 옮길 수 있는" 확장 모니터 제공. BetterDisplay가 만든 가상 디스플레이는 macOS에서 진짜 모니터로 열거되므로 Leftcar의 기존 `list_displays`/캡처/스트리밍 경로가 **무수정으로** 동작한다.
- R-015(v1 논골)와의 정합: 일반 기능이 아닌 **옵트인 실험**(encoder-experiment 패턴 준용)으로 한정하고, ADR-000X로 근거를 기록한다.

### 동작 흐름

1. Host 설정에서 "가상 디스플레이 (실험)" 토글(기본 꺼짐).
2. Host가 `betterdisplaycli virtual -create=<WxHxF> --set-current` 실행.
3. 생성된 디스플레이가 카탈로그에 자동 반영(기존 refresh 경로).
4. Android에서 해당 디스플레이를 열면 기존 스트림과 동일하게 동작.

### 의존성 정책

- BetterDisplay를 **번들하지 않는다**. 설치·실행 여부만 감지하고, 미설치 시 안내 메시지 + 다운로드 링크를 표시한다.
- 사전 요구: BetterDisplay v4.0.5+, macOS 12.3+, CLI 접근 허용(사용자가 BetterDisplay 설정에서 수동 허용).
- Windows 지원은 범위 밖 (macOS 전용, `#[cfg(target_os = "macos")]`).

### 구현 위치

- Host Tauri command 신규: `createVirtualDisplay(width, height, fps)` / `removeVirtualDisplay(name)` — `betterdisplaycli` 프로세스 실행, 실패 시 오류 문자열 반환.
- 기존 커맨드 등록부(`lib.rs` invoke_handler)에 추가.

### ADR-000X (경량)

- 결정: 가상 디스플레이는 서드파티 CLI 연동 옵트인 실험으로 제공한다.
- 근거: 자체 가상 디스플레이 드라이버는 IOKit/DriverKit 신규 프로젝트(R-015 "큰 새 프로젝트")인 반면, CLI 연동은 기존 캡처 경로 재사용으로 소규모 구현이 가능하다.
- 범위 제한: 번들 미포함, Windows 미지원, 실험 플래그 기본 꺼짐, 정식 승격은 물리 검증 후 별도 결정.

## 설계 3 — USB 물리 검증 게이트 (T11) 문서화

`docs/usb-physical-validation.md` 신규.

### 사전 조건 체크리스트

- 데이터 케이블(충전 전용 제외), 허브 미사용 직결, 폰 기본 USB 구성 확인, 뷰어 APK 버전 명기.

### 검증 절차 (2026-08-26 AOAP 설계의 수동 체크리스트를 실행 가능한 단계로 전개)

1. 최초 AOAP 핸드셰이크 — GET PROTOCOL 응답값, 재열거 VID/PID(0x18D1:0x2D00/0x2D01) 기록.
2. 케이블 제거 → Wi-Fi failover 소요 시간(목표 1–2초) 측정.
3. Wi-Fi 스트리밍 중 케이블 연결 → USB 자동 복귀 확인.
4. 60분 soak — 프레임 연속성, 제어 명령(getStatus) 응답 지연·손상 없음.
5. 인텐트 경로 — 앱 미실행 상태 케이블 연결 시 자동 실행, 실행 중 onNewIntent.

### 결과 기록 템플릿

- EVIDENCE.md 스타일: 기종/OS 버전/케이블 종류/각 항목 Pass·Fail + 측정값.

### 실패 시 진단 가이드 (증상 → 확인 순서)

- 연결 자체가 안 됨 → 충전 전용 케이블 여부, 허브 사용 여부.
- 뷰어 자동 실행 안 됨 → ATTACHED 인텐트 미수신(제조사 스킨), Manifest accessory filter 문자열 일치.
- 권한 대화상자 후 실패 → openAccessory 거부, fd 획득 실패 로그.
- 협상 실패 → 폰이 AOAP 미지원(GET PROTOCOL 0) → Wi-Fi 폴백 정상 동작 확인 + UI에 "USB 지원 안 됨" 노출 여부.

## 에러 처리

| 상황 | 동작 |
|---|---|
| betterdisplaycli 미설치/실행 실패 | 안내 메시지 + 링크. 실험 토글은 유지 |
| betterdisplaycli 실행되지만 가상 디스플레이 생성 실패 | 오류 문자열을 UI로 전달, 카탈로그 refresh 유도 |
| panic 제거 후 백엔드 초기화 실패 | 오류 대화상자 + 정상 종료(종료 코드 1) |
| USB 물리 검증 Fail | 결과를 EVIDENCE.md에 기록하고 AOAP은 실험 상태 유지(정식 전환 보류) |

## 테스트 전략

- **1A**: `catalog-model-types` 매핑 단위 테스트(jetpack 아님, vitest) — transport 문자열별 배지 레이블. React 변경 후 `npx -y react-doctor@latest . --verbose` 100/100 필수 + viewer-expo `tsc --noEmit`.
- **1B**: `bind_control_listener_at` 기존 테스트 유지, panic 경로는 대화상자 함수 분리로 단위 테스트 가능하게 설계.
- **1C**: i18n 키 쌍 존재 검사(기존 계약 테스트 패턴).
- **2**: `createVirtualDisplay` 명령의 CLI 인자 생성 로직 단위 테스트(프로세스 실행은 모킹). 실험 플래그 off일 때 커맨드 호출 없음 확인.
- **3**: 문서 검토 기반(자동화 없음). 실기기 게이트 수행 후 EVIDENCE.md 갱신.

## 범위 밖

- 자체 가상 디스플레이 드라이버(DriverKit) 개발
- Windows 가상 디스플레이(Indirect Display Driver)
- BetterDisplay 라이선스/Pro 기능의 자동화 검증
- AOAP의 Wi-Fi failover 백오프 정책 변경(기존 설계 유지)
- 공개 인터넷 전송(TLS/PAKE) — 기존 로드맵 유지
