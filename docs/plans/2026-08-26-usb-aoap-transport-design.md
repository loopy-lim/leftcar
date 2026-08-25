# USB(AOAP) 전송 경로 설계 — USB 우선 + Wi-Fi 보조

날짜: 2026-08-26
상태: 승인됨 (섹션 1·2 사용자 확인, 나머지는 문서로 일괄 제시)
관련: ADR-0002 (제어/미디어 평면 분리), ADR-0004 (전송 bake-off 유보), 2026-08-26-recovery-fec-abr-zerocopy-design.md (FEC/ABR)

## 배경 및 목표

현재 USB(`adbTcp`) 경로는 adb forward 기반으로 Host PATH에 `adb`가 필요하고, 제어 평면(7777)은 항상 Wi-Fi TCP다. 요구사항:

- 일반 사용자가 개발자 모드·USB 디버깅 없이 USB 케이블만으로 연결
- USB를 기본 경로, Wi-Fi를 보조 경로로 하는 이중 경로
- USB 뽑힘/삽입 시 자동 전환, 짧은 재연결(1–2초 멈춤 감수)
- USB 테더링은 라우팅 혼란 우려로 제외, adb 기반은 "무겁다" 판단으로 제외

## 접근법 비교

| 안 | 방식 | 결과 |
|---|---|---|
| A. AOAP | 안드로이드 액세서리 모드, bulk EP 직접 통신 | **채택** — 요구 3개 모두 충족 |
| B. adb 강화 | `adb reverse` 제어 평면 터널링 | 기각 — USB 디버깅/adb 설치 요구 유지 |
| C. USB 테더링 | RNDIS/NCM 유선 IP 링크 | 기각 — 라우팅 혼란, 캐리어 정책 리스크 |

AOAP는 전송 계층 "교체"가 아니라 기존 wire 프로토콜 위에 새 엔드포인트를 추가하는 것이므로 ADR-0004(QUIC/DTLS 등 교체 유보)와 정합.

## 아키텍처 개요

```
┌─────────────────────┐                    ┌──────────────────────┐
│  Host (Tauri/Rust)  │   USB 케이블       │  Viewer (폰)          │
│                     │ ═════════════════▶ │                      │
│ ┌─────────────────┐ │  AOAP bulk EP     │ ┌──────────────────┐ │
│ │ aoap_link       │ │  (제어+미디어     │ │ UsbAccessory      │ │
│ │ (nusb)          │ │   멀티플렉스)     │ │ BroadcastReceiver │ │
│ └────────┬────────┘ │                    │ └────────┬─────────┘ │
│ ┌────────▼────────┐ │                    │ ┌────────▼─────────┐ │
│ │ usb_mux         │ │                    │ │ usb_bridge        │ │
│ │ ch0=제어 ch1=미디어│                   │ │ (prepared_tcp     │ │
│ └────────┬────────┘ │                    │ │  동일 프레이밍)   │ │
│   control.rs /      │                    │   jni.rs 재사용      │
│   CaptureShim       │                    │                      │
└─────────────────────┘                    └──────────────────────┘
     + Wi-Fi 기존 경로 유지 (udp/tcp) ← 자동 전환 대상
```

핵심 결정:

1. **AOAP 핸드셰이크 Host 주도** — Viewer는 `USB_ACCESSORY_ATTACHED` 인텐트로 자동 실행(Manifest accessory filter). 사용자 조작은 케이블 꽂기뿐.
2. **단일 bulk 파이프 + 채널 멀티플렉싱** — AOAP는 bulk IN/OUT 2엔드포인트만 제공. 기존 길이 프리픽스(u32 BE) 프레임 앞에 1바이트 채널 ID(ch0=제어, ch1=미디어)를 추가한 mux. LCH1 인증·wire 프로토콜 unchanged.
3. **전송 옵션 확장** — `MediaTransport`에 `Usb` 변형 추가. `auto` 폴백 순서를 `usb → wifi-tcp → wifi-udp`로 재정의. 기존 `adbTcp`는 레거시 유지.
4. **전환 트리거는 USB attach/detach 이벤트** — Host nusb hotplug 감지 + Viewer `UsbManager` detach 브로드캐스트. 뽑히면 즉시 Wi-Fi 재연결. 백오프는 v0.2 Phase C 계획(300ms×2, 상한 5s) 공유.

## 컴포넌트 상세

### Host 측

| 컴포넌트 | 위치 | 역할 |
|---|---|---|
| `aoap_link` | `apps/host-desktop/src-tauri/src/aoap.rs` 신규 | nusb 디바이스 열거 → AOAP 협상(CONTROL 51/52/53/54: GET PROTOCOL/SEND STRING/START) → 인터페이스 재열거 → bulk IN/OUT 확보. hotplug 이벤트 스트림 |
| `usb_mux` | `aoap.rs` 내부 | `[ch:u8][len:u32 BE][payload]` 프레임. ch0=제어(개행 JSON), ch1=미디어(L2 미디어 프레임) |
| `control.rs` 확장 | 기존 파일 | `MediaTransport::Usb` 추가, startStream 후보 순서 재정의, USB 세션 수명 관리 |
| CaptureShim | `native/macos-capture-shim` | 미디어 writer를 소켓·bulk OUT 공용 트레이트(`MediaWriter`)로 추상화 |
| Windows 백엔드 | `apps/host-desktop/src-tauri/src/windows_backend/` | `MediaWriter` 트레이트 채택 시 USB 쓰기 지원 겸용(WGC는 유지) |

### Viewer 측

| 컴포넌트 | 위치 | 역할 |
|--- cargo-lfmt-ignore
|---|---|---|
| `UsbAccessoryModule` | `apps/viewer-expo/android/.../usb/` 신규 | ATTACHED 인텐트 처리·자동 실행, `openAccessory()` → FileDescriptor → JNI 전달, detach 브로드캐스트 리스너 |
| `usb_bridge` | `native/android-viewer/src/usb_bridge.rs` 신규 | `prepared_tcp.rs` 동일 구조 — fd 기반 리더/라이터, 채널 demux, LCH1 에코 |
| JNI 확장 | `native/android-viewer/src/jni.rs` | `prepare_port`/`attach_port`에 fd 기반 변형 추가(바인딩 주소 정책 확장) |
| Manifest | `apps/viewer-expo/android/.../AndroidManifest.xml` | `USB_ACCESSORY_ATTACHED` 인텐트 필터 + accessory filter meta-data |

### 식별

AOAP SEND STRING 단계에서 Host가 `Manufacturer="Leftcar"`, `Model="LeftcarHost"` 등을 전송. Viewer Manifest filter가 이 문자열과 정확히 일치해야 자동 실행. 버전 문자열로 프로토콜 버전 협상 가능(`Version="1"`).

## 데이터 흐름

### 연결 수립 (USB)

1. 케이블 연결 → Host nusb가 디바이스 인식 → `aoap_link`가 AOAP 핸드셰이크 시도
2. 핸드셰이크 성공 → 폰이 액세서리 모드 재열거 → Viewer 앱이 ATTACHED 인텐트로 실행(이미 실행 중이면 onNewIntent)
3. Viewer `openAccessory()` → fd 획득 → `usb_bridge` 시작, ch0으로 `LCH1` 에코 대기
4. Host가 ch0에 제어 채널 시작 — 기존 pairing(`pair` 명령) 절차 그대로, 토큰 획득
5. `startStream` → Host가 ch1로 미디어 프레임 송신 개시. 미디어 핸드셰이크(LCH1 챌린지 에코)는 ch1에서 수행

### 자동 전환 (USB → Wi-Fi)

1. USB detach 이벤트(HotplugEvent::Left) → Host가 USB 세션 정리(forward 제거 상당의 cleanup), Viewer는 detach 브로드캐스트 수신
2. Host가 Wi-Fi 후보(wifi-tcp → wifi-udp)로 재시작, Viewer는 저장된 호스트 주소로 재연결
3. 재연결 시 획득 토큰 재사용(페어링 재수행 불필요) — 토픽 챌린지 nonce 재발급으로 도달성 증명 갱신
4. v0.2 Phase C 백오프(300ms부터 지수, 상한 5s) 적용

### 자동 복귀 (Wi-Fi → USB)

1. USB attach 이벤트 → Host `aoap_link` 핸드셰이크 → 성공 시 진행 중 세션을 Wi-Fi에서 USB로 마이그레이션
2. 마이그레이션은 "새 세션 수립 + 기존 종료" — 프레임 파이프라인을 새 파이프로 전환. 1–2초 멈춤 감수(사용자 합의)
3. IDR 요청으로 즉시 키프레임부터 디코딩 재개

## 에러 처리

| 상황 | 동작 |
|---|---|
| AOAP 협상 실패(폰이 프로토콜 미지원 등) | USB 후보 실패로 처리, Wi-Fi 후보로 폴백. UI에 "USB 지원 안 됨" 상태 노출 |
| adbTcp와 달리 adb 서버 의존 없음 — adb 설치 여부와 무관하게 동작 | — |
| USB detach 중 미디어 송신 중 에러 | 즉시 세션 정리 후 Wi-Fi 폴백 (위 전환 흐름) |
| 폰에서 뷰어 앱 강제 종료 | bulk IO 에러 → Host 세션 종료, LCT1(종료 통지) 발행 후 정리 |
| mux 프레임 오류(길이 필드 이상) | 해당 채널 종료. ch0 종료 시 세션 전체 종료 |
| 토큰 만료·거부 | 기존 제어 프로토쉬 응답 그대로(`control.rs:683-731` 동일) |

## Windows 지원 갭

`windows_backend/mod.rs:148-171`은 현재 UDP 전용. USB 경로는 CaptureShim(macOS)과 같은 `MediaWriter` 트레이트를 채택하면 Windows 백엔드도 bulk OUT 쓰기를 지원할 수 있다. 단, v0.2 Phase B(Windows 물리 검증 E13)가 선행돼야 하며, 본 설계는 Windows USB 지원을 명시적 범위로 포함하지 않는다(후속 이슈로 분리).

## 테스트 전략

**Rust 단위 테스트**

- `usb_mux` 프레이밍 라운드트립: ch ID + len + payload 직렬화/파싱, 경계값(len=0, 최대 2MB, 잘못된 ch)
- `aoap_link` 모의 디바이스: 제어 전송(control transfer) 모킹으로 GET PROTOCOL/SEND STRING/START 시퀀스 검증, 협상 실패 분기
- `control.rs` 후보 폴백 순서: `usb → wifi-tcp → wifi-udp` 순서와 각 실패 분기 기존 테스트 패턴 확장

**통합 테스트 (macOS Host ↔ 실기기 폰)**

1. 케이블 연결만으로 뷰어 자동 실행 → 페어링 → 스트림 시작 E2E
2. 스트리밍 중 케이블 제거 → Wi-Fi 자동 전환 (1–2초 내 재개)
3. Wi-Fi 스트리밍 중 켕블 연결 → USB 자동 복귀
4. USB 스트리밍 60분 soak — 2026-08-26 FEC/ABR 설계의 60분 soak와 동일 기준
5. 멀티플렉스 무결성: 스트리밍 중 제어 명령(getStatus 등) 응답 지연·손상 없음

**수동 체크리스트**

- Manifest accessory filter 문자열과 Host SEND STRING 일치 여부
- 폰 이미 실행 중인 상태에서 케이블 연결(onNewIntent 경로)
- USB 허브/충전 전용 케이블(데이터 불가) 연결 시 안내 메시지

## 범위 밖

- QUIC/DTLS 전환 (ADR-0004 유보 유지)
- 제어 평면 TLS (v0.2 범위 밖 유지)
- Windows USB 지원 (후속 이슈)
- USB 테더링, adb 무선 디버깅
- 블루투스 등 다른 물리 계층
