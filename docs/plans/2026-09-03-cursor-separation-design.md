---
date: 2026-09-03
topic: "커서 분리 — 위치 스트림 옵트인 설계 (LCD1)"
status: approved
---

# 설계: 커서 분리 — 위치 스트림 옵트인 (LCD1)

**날짜**: 2026-09-03
**근거**: `docs/research/2026-08-25_remote-screen-display-pipelines.md` (업계 표준, Leftcar 미달), `docs/research/2026-09-03_opendisplay-comparison.md` (참고 구현 확보)
**범위**: 이번 구현은 커서 **위치** 스트림. 모양(shape) 동기화는 와이어 포맷에 여지만 설계만 포함하고 구현하지 않는다.

## 결정 사항 (브레인스토밍 확정)

1. **범위**: 위치 먼저, 모양은 설계만 (2단계 분리)
2. **채널**: 기존 제어 채널 확장 (UDP 사이드 채널 아님) — 미디어/제어 소켓이 이미 분리돼 있고 TCP 폴백이 공짜로 호환
3. **좌표 원천**: CGEvent Tap (물리 마우스/트랙패드 + 원격 주입 전부 커버)
4. **전송 방식**: 방식 A — 정규화 좌표 상태 스트림 + 뷰어 오버레이 렌더 (손실 시 다음 업데이트로 자연 수렴)
5. **옵트인**: 세션당 토글로 채용 여부 선택 (`LCDON`/`LCDOFF` — 구버전 호스트는 무시하므로 하위 호환)

## 아키텍처

```
[호스트 macOS]                                [뷰어 Android]
CGEvent Tap (마우스 이동 감시)                제어 소켓 수신 루프
  └─ 물리 마우스/트랙패드 이동 ─┐                      │
                                ├─→ lastCursorPos ─→  LCD1 패킷 (제어 소켓)
  원격 입력 주입(기존) ─────────┘        (변경 시만)     │
                                                  ▼
                                     CursorScheduler (coalescing, 2×FPS)
                                                  │
                                                  ▼
                                     커서 오버레이 (SurfaceView 위 뷰 계층)
```

- **호스트**: `showsCursor = false`는 LCD1 스트림이 활성인 동안만. 스트림 중단 시 자동 원복(커서 소실 방지).
- **뷰어**: `LCDON`을 보내도 호스트가 LCD1을 안 보내면 오버레이를 표시하지 않음(자동 폴백 — 구버전 호스트 호환).
- **원칙**: LCI1 포인터 경로의 거울상. 신뢰성 계층 없음(최신 값이 곧 정답).

## 와이어 포맷

### 옵트인 명령 (뷰어→호스트, 제어 채널)

プレ인텍스트 명령 자리(기존 `IDR`/`BYE`와 동일 토큰 인증 프레임):

| 명령 | 의미 |
|---|---|
| `LCDON` | 커서 위치 스트림 요청 |
| `LCDOFF` | 커서 위치 스트림 중지 요청 |

- 구버전 호스트: 미인식 → 무시 (안전)
- 호스트 승낙 시: `LCD1` 패킷 흐름 시작 + capture `showsCursor=false` 전환
- 거부/미지원 시: LCD1 미전송 — 뷰어는 이를 폴백 조건으로 사용

### 위치 패킷 (호스트→뷰어, 제어 채널)

```
오프셋  크기  필드
0      4    magic "LCD1"
4      4    sequence (u32 BE, 호스트 세션당 1부터)
8      2    x (u16 BE, 정규화 0..=65535)
10     2    y (u16 BE, 정규화 0..=65535)
12     1    visibility (0=숨김, 1=표시)
13     1    shape id (예약, 0=기본 화살표 — 이번 구현은 항상 0)
14     N    세션 토큰 (기존 제어 채널과 동일)
```

- 총 14 + 토큰 길이. `visibility=0`은 커서가 화면 밖이거나 숨김 상태일 때 (뷰어는 오버레이 숨김).
- 시퀀스는 모노토닉 증가하되 **재정렬 불필요** — 뷰어는 시퀀스가 이전보다 큰 경우만 수용(stale drop), 손실 시 다음 패킷이 상태를 덮어씀.

## 컴포넌트

### 호스트 (macOS shim)

1. `CaptureSession+Cursor.swift` (신규)
   - CGEvent Tap 설치/해제 (`CGEvent.tapCreate` — mouseMoved/dragged/otherMouseDragged)
   - 변경 시만 `lastCursorPos` 갱신 + dirty 플래그
   - 전송 타이머: `InputScheduler.polling_rate_hz()` 동일 정책(2×FPS, 30..=240Hz) — 소켓 루프 틱에서 drain
   - `encodeCursorPacket()` — 위 포맷
   - 옵트인 상태 관리: `cursorStreamEnabled` + `showsCursor` 원복 보장 (세션 종료·LCDOFF·피드백 무음 모두)
2. `CaptureSession+Backend.swift`
   - `showsCursor`를 고정 `true`가 아니라 세션 상태에서 읽어오도록 변경 (최초 생성 시 `true` 유지 — LCD1 협상 전 기본)
3. `CaptureSession+ViewerControl.swift`
   - `LCDON`/`LCDOFF` 파싱 (UDP + TCP 경로 모두)
   - `consumeViewerControl` 루프에서 커서 패킷 drain 전송

### 뷰어 (Android + Rust)

4. `input_protocol.rs`에 커서 평면 추가 (별도 파일 `cursor_protocol.rs` 권장 — 입력 프로토콜과 분리)
   - `CursorState` 파서, `LCDON/LCDOFF` 인코더, stale-drop 시맨틱
5. JNI 경로: Rust가 파싱한 커서 상태를 Kotlin으로 콜백 (기존 이벤트 콜백 패턴 재사용)
6. `StreamActivity`/HUD 레이어
   - 옵트인 UI: 설정 토글 1개 ("원격 커서 로컬 표시" — 기본 꺼짐)
   - 오버레이 뷰 (작은 커서 이미지, `visibility` 반영)
   - `LCDOFF` 전송 경로 (토글 끌 때와 세션 종료 시)

## 오류 처리

| 상황 | 동작 |
|---|---|
| 구버전 호스트가 LCDON 무시 | 뷰어가 LCD1 수신 없음 확인(타임아웃) → 오버레이 미표시, capture 커서는 원래대로 비디오에 심김(변화 없음) |
| LCD1 스트림 끊김 (네트워크) | 뷰어는 마지막 좌표 유지하되 1초 타임아웃 후 오버레이 페이드아웃. 호스트는 feedback 무음 종료 경로에서 showsCursor 원복 |
| 호스트 크래시 | showsCursor는 프로세스 상태라 자동 원복(새 세션이 다시 true로 시작) |
| 터치 사용자 (마우스 없음) | CGEvent 탭이 좌표를 안 줌 — visibility=0 유지, 오버레이 표시 안 함 |
| 보안 | 기존 세션 토큰 인증 프레임 그대로 사용 — 새 인증 경로 없음 |

## 테스트

- **Rust 유닛**: `cursor_protocol.rs` — 인코딩/파싱/stale-drop/토큰 인증 라운드트립 (기존 `input_protocol.rs` 테스트 스타일 준수)
- **Swift 유닛**: 패킷 인코딩 바이트 정합 (Rust 테스트와 동일 벡터), 옵트인 상태 전이(LCDON→스트림 시작→LCDOFF→원복)
- **CI**: 기존 cargo/swift 테스트 워크플로에 자동 포함
- **실기 검증은 별도 게이트**: T11 검증 6과 마찬가지로 E3 등급 기록 후 실기 확인 (이 설계의 완료 조건이 아님)

## 명시적 비목표 (이번 구현)

- 커서 모양 비트맵 전송 (shape id 필드만 예약)
- Galaxy XR 시스템 커서 상호작용 (기기 미보유 — R-017)
- Wi-Fi에서 120Hz 특화 (OpenDisplay와 달리 2×FPS 정책 그대로 — 추후 측정 후 조정)
