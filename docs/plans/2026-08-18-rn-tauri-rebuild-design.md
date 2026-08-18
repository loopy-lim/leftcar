# Leftcar 재구축 설계 — RN 멀티윈도우 뷰어 + Tauri 호스트

날짜: 2026-08-18
상태: 승인됨 (브레인스토밍 완료)

## 배경

기존 `apps/host-mac`(단일 파일 Swift 메뉴바 앱)은 동작은 하지만 좌측 아키텍처(Rust 크레이트, Rustra 제어평면)를 전부 우회하고, 창 선택이 없으며, 하드코딩된 IP로 단일 스트림만 지원한다. 이를 다음으로 교체한다:

- **뷰어**: React Native (기존 `apps/viewer-expo`, Expo 57 / RN 0.86) — 여러 소스를 OS 멀티윈도우 창으로 동시에 보는 뷰어
- **호스트**: Tauri v2 데스크톱 앱 — 화면 공유 오케스트레이션 (제어 서버 + 세션 관리 + UI)

## 핵심 결정

| 항목 | 결정 |
|---|---|
| 화면 공유 방향 | Mac → Android (기존과 동일) |
| 연결 구조 | **제어평면은 뷰어가 pull** (Rustra JSON), **비디오는 호스트가 push** (ADR-0002 유지) |
| 비디오 파이프라인 | 증명된 자산 재사용: `native/macos-capture-shim`(SCK→H.264) + `native/android-viewer`(TCP→AMediaCodec→Surface) |
| Tauri 역할 | 제어 서버 + 세션 매니저 + UI. 캡처는 기존 shim FFI 호출 (방식 A) |
| 멀티윈도우 | 소스당 1개 안드로이드 OS 창 (기존 설계 "one task per source" 유지) |
| 소스 선택 | v1은 디스플레이 전체 캡처. 창(SCWindow) 선택은 다음 단계 (ADR-0003의 창 캡처는 후속) |
| 발견 | mDNS(NSD) 자동 발견 + IP 직접 입력 병행 |
| fps | **90fps 기본, 120fps 상한** (성공 기준: 단일 스트림 90–120fps, 2스트림 동시 안정 — 90 마인드) |
| 플랫폼 | macOS 먼저 (멀티스트림 달성 후), Windows는 같은 트레잇 뒤 DXGI 백엔드로 2단계 |
| 기존 앱 | `apps/host-mac`은 새 앱 완성 후 삭제 |

## 전체 아키텍처

```
┌─ macOS (이후 Windows) ──────────────┐      ┌─ Android ────────────────────┐
│  apps/host-desktop (Tauri v2)       │      │  apps/viewer-expo (RN 0.86)  │
│  ┌──────────────┐  ┌─────────────┐  │      │  ┌─────────────────────────┐ │
│  │ 웹 UI (TS)    │  │ Rust 백엔드  │  │      │  │ RN UI: 호스트 발견,      │ │
│  │ 상태/통계/제어 │←─│             │  │      │  │ 소스 목록, 재생/정지      │ │
│  └──────────────┘  │ ① 제어 서버  │◄─┼──────┼─────────┘ pull (Rustra JSON) │
│                    │  (catalog,   │  │ mDNS│  ┌─────────────────────────┐ │
│  ┌──────────────┐  │  start/stop) │  │ ───►│  │ NSD로 호스트 자동발견    │ │
│  │ macos-       │←─│             │  │      │  └─────────────────────────┘ │
│  │ capture-shim │  │ ② 세션 매니저 │  │      │  ┌─────────────────────────┐ │
│  │ (기존 dylib)  │  │ (멀티 인스턴스)│  │      │  │ native/android-viewer    │ │
│  │ SCK→H.264    │  └──────┬──────┘  │      │  │ TCP 수신→AMediaCodec     │ │
│  └──────────────┘         │ push    │      │  │ →Surface (기존 jni.rs)    │ │
│                           ╞═════════╪══════╪═► 포트 5000+n, 스트림당 1개   │
└────────────────────────────┘ 비디오 평면  │  소스당 1개 OS 창(멀티윈도우)  │
                                             └─────────────────────────────┘
```

## 섹션 2: Tauri 호스트 앱 (`apps/host-desktop`)

### Rust 백엔드 (src-tauri)

- `control_server`: TCP 리스너 (기본 7777). 뷰어가 pull. Rustra JSON 명령:
  - `get_catalog` → 사용 가능한 소스 목록 (v1: 디스플레이 목록 + 해상도/fps)
  - `start_stream {source_id, viewer_host, viewer_port}` → 세션 생성, shim 인스턴스 시작
  - `stop_stream {session_id}`
  - `get_status` → 활성 세션 목록 + 통계 (fps, kbps, 드롭)
- `session_manager`: 세션당 shim 인스턴스. `leftcar_capture_start`가 인스턴스 핸들을 반환하도록 shim 확장 (현재는 글로벌 단일 세션) — 다중 디스플레이/다중 뷰어 지원의 핵심 변경.
- `discovery`: mdns-sd 크레이트로 `_leftcar-host._tcp.local.` 서비스 광고 (포트 7777).
- 통계: shim의 frames/bytes 카운터를 폴링해 UI + `get_status`에 노출.

### 웹 UI

- 활성 스트림 목록 (소스, 대상, fps/kbps 실시간)
- 수동 시작/정지 (뷰어 없이 테스트용), 향후 창 선택 UI (v2)
- Tauri v2 IPC (`invoke`)로 백엔드 명령 호출. React + TypeScript.

### shim 확장 (`native/macos-capture-shim`)

- `leftcar_capture_start(viewer_ip, viewer_port, display_id?) -> u32 handle` — 세션 핸들 반환
- `leftcar_capture_stop(handle)`, `leftcar_capture_stats(handle) -> {frames, bytes}`
- 기존 단일 글로벌 상태를 `HandleTable`로 교체. 프로토콜(0x46 헤더 + CFG)은 그대로.

## 섹션 3: RN 뷰어 앱 (`apps/viewer-expo`)

### 화면

1. **호스트 화면**: NSD 검색 목록 + IP 직접 입력. 선택하면 제어 서버(TCP 7777)에 접속.
2. **소스 카탈로그 화면**: `get_catalog` 결과 목록. 각 항목에 "이 창에서 열기" 버튼.
3. **스트림 창**: 각 스트림이 안드로이드 OS 멀티윈도우의 별도 창. RN 화면(하나의 activity) + 네이티브 SurfaceView로 구현:
   - Expo의 단일 activity 제약을 피하기 위해 `android:launchMode` + `ActivityOptions.makeToPendingTransition`… 대신 **구현 상세**: 기존 `StreamActivity.kt`(shim)를 export하고, RN에서 `startActivity` 인텐트(`FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY` 아님, `launchMode="standard"`, `windowSoftInputMode`, `resizeableActivity=true`)로 여러 인스턴스를 띄운다. 각 인스턴스가 고유 포트(5000+n)에서 수신.
   - 스트림당 창 제목 = 소스 이름.

### 네이티브 모듈 (기존 자산 재사용)

- `leftcar-rustra` JNI 브리지 (제어 JSON invoke) — 기존 `invoke_json` 재사용
- `android-viewer` jni.rs — TCP 수신 → AMediaCodec → Surface. 변경: 하드코딩 5000 → 인텐트 엑스트라로 포트 수신, `attachSurface`는 그대로.

### nsd-react-native 또는 안드로이드 네이티브 NSD 모듈

- `NsdManager.discoverServices("_leftcar-host._tcp.local.")` 래핑 모듈 추가.

## 섹션 5: 오류 처리 & 검증

### 오류 처리

- 제어 연결 끊김: 세션은 유지(비디오 평면 독립), 뷰어 재접속 시 `get_status`로 상태 복원
- 비디오 연결 끊김: shim 전송 실패 시 세션 자동 stop + 뷰어에 오류 표시
- 포트 충돌: 5000+n 자동 할당, 실패 시 다음 포트
- 발견 실패: 수동 IP 입력 폴백 (항상 사용 가능)

### 검증 (성공 기준)

- **V1 (macOS)**: (a) 단일 스트림 90fps 안정 (1080p), 120fps 시도; (b) 2 스트림 동시 안정 재생 (마인드 90fps); (c) RN 뷰어에서 호스트 발견→카탈로그→창 2개
- **V2 (Windows)**: 같은 성공 기준, DXGI 백엔드
- 기존 `tests/` e2e 패턴 따름, `EVIDENCE.md`에 측정 기록 추가

## 명시적 비-목표 (v1)

- 창(SCWindow) 단위 캡처 선택 UI — v2 (shim은 display_id 이미 받도록 설계)
- 오디오
- 인증/암호화 (LAN 신뢰 환경 가정)
- WebRTC/QUIC 전환 (ADR-0004는 계속 유보)
