# 시스템 아키텍처

문서 상태: 제안안 0.1  
선행 결정: ADR-0001부터 ADR-0004  
중요: Viewer UI shell과 transport 구현은 Phase 1/2 실험 전까지 provisional이다.

## 1. 아키텍처 목표

1. 원격 source 하나를 Galaxy XR의 독립 Home Space 창 하나에 매핑한다.
2. 영상 hot path를 JavaScript, JSON, UI event loop에서 분리한다.
3. 한 source 또는 한 Activity의 실패가 다른 stream window를 중단하지 않는다.
4. 지연이 누적되기보다 프레임을 버리도록 bounded queue를 사용한다.
5. 플랫폼 capture/encode/decode를 교체 가능한 port 뒤에 둔다.
6. 보안 주체, network session, source 승인, Activity task를 명시적으로 연결한다.
7. 모든 성능 주장은 단계별 timestamp와 외부 glass-to-glass 측정으로 검증한다.

## 2. 전체 구조

```text
macOS / Windows Host
┌────────────────────────────────────────────────────────────────────┐
│ Host UI (Tauri + TS, provisional)                                  │
│   source picker / pairing / status / diagnostics                   │
│          │ Rustra typed commands and low-rate events               │
│          ▼                                                         │
│ Host Core (Rust)                                                    │
│   Pairing  SourceRegistry  Session  QualityPolicy  Metrics          │
│          │                         │                                │
│          │ CapturePort             │ TransportControl               │
│          ▼                         ▼                                │
│ Native Capture -> HW Encoder -> VideoPacketizer -> Secure Transport│
│ ScreenCaptureKit / WGC    VideoToolbox / Media Foundation           │
└───────────────────────────────────┬────────────────────────────────┘
                                    │ local LAN
                      reliable control + real-time video
                                    │
Galaxy XR Viewer                    ▼
┌────────────────────────────────────────────────────────────────────┐
│ Shared App Process                                                 │
│   Thin Android Binder / Rust Core / Secure Transport / Catalog     │
│           │                 │                  │                    │
│           │ lease A         │ lease B          │ lease C            │
│           ▼                 ▼                  ▼                    │
│ StreamActivity A      StreamActivity B    StreamActivity C         │
│ Decoder A + Surface   Decoder B + Surface Decoder C + Surface      │
│     IDE window           terminal window      browser window       │
│                                                                    │
│ HubActivity: pair, browse sources, open/close/diagnose windows      │
└────────────────────────────────────────────────────────────────────┘
                                    │
Android XR Home Space               ▼
              [IDE]       [Terminal]       [Browser]       [Hub]
```

## 3. 프로세스와 소유권

### 3.1 Host 프로세스

초기 배포는 한 사용자 세션에서 실행되는 tray/menu bar 애플리케이션을 가정한다.

Host shell 책임:

- 시스템 source picker 표시
- 화면 기록 권한 안내
- QR/페어링 승인 UI
- 현재 capture 표시
- 명시적 stop all
- 진단 bundle 내보내기

Host core 책임:

- 승인된 source registry
- 장치 identity와 capability
- transport session
- stream lifecycle
- quality controller
- per-source capture/encoder resource
- typed error와 metric

Host는 daemon/root 권한을 요구하지 않는다. 로그인 화면, secure desktop, 다른 사용자 session을 캡처하지 않는다.

### 3.2 Viewer 앱 프로세스

공유 singleton 성격의 `SessionService`가 network connection을 소유한다. 이름은 구현 언어에 따라 바뀔 수 있지만 역할은 유지한다.

`HubActivity`:

- paired Host 목록
- 연결 상태
- source catalog
- source를 새 창으로 여는 action
- 열린 stream window 목록
- capability와 benchmark 진단

`StreamActivity`:

- Intent의 opaque `source_id`와 `instance_id` 읽기
- SessionService bind
- source lease 획득
- decoder와 Surface 연결
- 창 크기/초점/가시성 보고
- 오류 overlay
- task 종료 시 lease 반환

`SessionService` 또는 동등한 얇은 Android binder:

- Rust viewer core의 process handle 소유
- Activity bind/unbind와 Surface callback을 Rust에 전달
- OS가 요구하는 foreground/background lifecycle 연결

다음 실제 로직은 Rust viewer core가 담당한다.

- 장치 키와 pairing 저장소 접근
- Host connection 1개 유지
- source마다 video channel demux
- stream window lease count 관리
- source catalog 캐시
- Host와 protocol version negotiation
- Activity 복원 시 권한 재검증

### 3.3 source lease

source가 열려 있는지 Activity 하나의 callback만으로 판단하지 않는다. 명시적 lease를 사용한다.

```text
open window A
  -> acquire lease(source=A, instance=1)
  -> Host start source A

configuration change
  -> old Activity releases transient view binding
  -> retained task/session lease remains during grace period
  -> new Activity rebinds

close task
  -> release lease(source=A, instance=1)
  -> no leases remain
  -> Host stop source A after short debounce
```

기본 정책은 source당 stream window 하나다. 중복 창 기능이 승인되면 여러 viewer instance가 같은 encoded source를 구독하고 decoder만 별도로 가진다. Host encoder를 중복 생성하지 않는다.

## 4. 모듈 경계

### 4.1 제안 저장소 구조

```text
leftcar/
  Cargo.toml
  rust-toolchain.toml
  package.json
  pnpm-workspace.yaml

  apps/
    host-desktop/
      src/                         # Tauri/React TS UI
      src-tauri/                   # Rust app shell
    viewer-android/
      android/                     # 얇은 Activity/Intent/Surface Kotlin shim
      src/                         # React Native TypeScript UI
      specs/                       # TurboModule/Fabric TypeScript spec

  crates/
    domain/                        # 순수 상태, 정책, 오류, ID
    control-contract/              # Rustra commands와 codegen
    network-protocol/              # Host-Viewer wire schema와 version
    session/                       # pairing, auth, stream orchestration
    transport-api/                 # transport trait, fake transport
    transport-webrtc/              # bake-off 후보
    transport-quic/                # bake-off 후보
    media-model/                   # EncodedFrame, codec config, timestamps
    host-core/                     # source/encoder orchestration
    macos-capture/                 # ScreenCaptureKit adapter facade
    macos-encode/                  # VideoToolbox adapter facade
    windows-capture/               # Windows.Graphics.Capture adapter
    windows-encode/                # Media Foundation adapter
    viewer-core/                   # demux, quality, window lease
    diagnostics/                   # metrics and redacted bundles

  native/
    macos-capture-shim/            # Swift/C ABI가 필요할 때
    android-viewer/                # 최소 Kotlin/JNI glue와 Rust NDK build

  packages/
    control-generated/             # Rustra generated TS, 수정 금지
    test-fixtures/                 # 비민감 synthetic media manifests

  tests/
    contract/
    integration/
    network/
    e2e/

  tools/
    latency-pattern/
    codec-probe/
    network-shaper/
    benchmark-runner/

  docs/
```

경로는 구현 시 조정할 수 있지만 다음 dependency rule은 고정한다.

```text
domain <- media-model <- session/host-core/viewer-core
domain <- control-contract
domain <- network-protocol
transport implementations -> transport-api
platform adapters -> ports defined by core crates
apps -> all orchestration

video hot path -X-> TypeScript/Rustra/JSON
network protocol -X-> UI-specific types
domain -X-> Tauri/React Native/Android/Apple/Windows SDK
Kotlin shim -X-> domain/session/network/quality policy
```

### 4.2 언어와 native 경계 예산

제품 로직은 TypeScript와 Rust로 구현한다.

| 영역 | 언어 | 이유 |
| --- | --- | --- |
| Host/Viewer UI | TypeScript | 팀의 주 언어, React Native/Tauri |
| contract | Rust + generated TypeScript | Rustra source of truth |
| session/network/media policy | Rust | 안전한 상태/메모리/동시성 관리 |
| Android decoder | Rust + NDK C API | `AMediaCodec`과 hot path를 TS/Kotlin에서 분리 |
| Activity/Intent/Surface bridge | 최소 Kotlin | Android component 등록과 RN native host 요구 |
| generated native interface | RN Codegen C++/Java | handwritten glue 축소 |

Kotlin 파일은 `apps/viewer-android/android/app/src/main/java/.../shim/` 아래로 제한한다. `shim`이 `domain` 결정을 내리지 못하도록 API와 architecture test를 둔다.

## 5. Control plane

Control plane은 두 경계로 나뉜다.

### 5.1 로컬 UI 경계

Rustra command와 event를 사용한다.

예:

- `listSources`
- `openSourceWindow`
- `closeSourceWindow`
- `getSessionSnapshot`
- `setStreamProfile`
- `beginPairing`
- `approvePairing`
- `revokeDevice`

저빈도 event:

- source catalog changed
- session state changed
- stream summary changed
- permission state changed

### 5.2 Host-Viewer network 경계

별도 protocol envelope을 사용한다.

```text
ClientHello
  protocol range
  device id
  app build
  codec capabilities digest

ServerHello
  selected protocol
  host id
  session id
  source catalog revision

ControlEnvelope
  protocol version
  session id
  request/event id
  monotonic sequence
  message kind
  payload
```

초기 spike는 디버깅 가능한 length-prefixed JSON을 허용한다. 제품 wire format은 schema evolution과 fuzz 결과를 보고 Protocol Buffers 또는 명시적 serde format으로 확정한다. rkyv archive를 검증 없이 네트워크 입력으로 직접 역직렬화하지 않는다.

## 6. Video plane

### 6.1 논리 frame

```rust
struct EncodedFrame {
    session_id: SessionId,
    source_id: SourceId,
    stream_epoch: u32,
    frame_id: u64,
    kind: FrameKind,          // key | delta | config
    codec: CodecProfile,
    capture_time_host_ns: u64,
    encode_done_host_ns: u64,
    width: u32,
    height: u32,
    payload: Bytes,
}
```

`stream_epoch`는 encoder restart, source resize, codec reconfiguration마다 증가한다. Viewer는 이전 epoch의 늦은 패킷을 버린다.

### 6.2 packetization 요구

- source와 epoch를 모든 fragment에서 식별
- frame length, fragment index/count, payload checksum 또는 AEAD 보호
- MTU 초과 fragmentation
- config/SPS/PPS를 keyframe과 함께 복구 가능하게 전달
- 늦은 delta frame 폐기
- frame assembly timeout
- 손실 뒤 decoder가 참조 불가능해지면 IDR 요청
- source별 sequence와 loss metric
- 메모리 상한이 있는 assembly map

### 6.3 backpressure 규칙

각 단계는 무제한 queue를 금지한다.

| 경계 | 기본 상한 | 넘을 때 |
| --- | --- | --- |
| capture -> encoder | 최신 1 frame 대기 | 이전 미인코딩 frame 폐기 |
| encoder -> packetizer | 2 access units | 가장 오래된 delta 폐기, key 보존 |
| packetizer -> network | 시간 예산 1 frame | source profile 낮춤, 늦은 delta 폐기 |
| network -> assembler | source당 2 incomplete frames | 오래된 frame 폐기 |
| assembler -> decoder | 최신 decodable access unit | 참조 안전성 확인 후 늦은 delta 폐기 |
| decoder -> Surface | MediaCodec/Surface 소유 | playout queue 추가 금지 |

정확한 수치는 실험에서 변경할 수 있지만 bounded와 “최신 화면 우선” 원칙은 유지한다.

## 7. Capture pipeline

### 7.1 공통 port

```rust
trait CapturePort {
    fn list_sources(&self) -> Result<Vec<CaptureSource>>;
    fn request_user_selection(&self) -> Result<ApprovedSource>;
    fn start(
        &self,
        source: ApprovedSource,
        config: CaptureConfig,
        sink: FrameSink,
    ) -> Result<CaptureHandle>;
}
```

frame은 CPU-owned generic byte vector보다 native texture/pixel buffer handle을 우선한다. 플랫폼 adapter가 lifetime을 명시하고 encoder가 consume한 시점을 반환한다.

### 7.2 macOS

```text
SCContentSharingPicker
 -> persistent approved source reference
 -> SCStream output queue per source
 -> CMSampleBuffer validation
 -> IOSurface/CVPixelBuffer
 -> VTCompressionSession
```

`SCStream` callback에서 network I/O나 무거운 allocation을 하지 않는다. bounded channel로 넘기고 즉시 반환한다.

### 7.3 Windows

```text
GraphicsCapturePicker
 -> GraphicsCaptureItem
 -> Direct3D11CaptureFramePool
 -> ID3D11Texture2D
 -> Media Foundation hardware encoder MFT
```

frame pool resize와 device lost를 stream epoch 전환으로 처리한다.

## 8. Decode pipeline

### 8.1 stream window 생성

```text
Hub selects source
 -> local core validates paired session and catalog revision
 -> creates opaque StreamLaunchToken (not secret-bearing Intent data)
 -> starts StreamActivity as new document task
 -> StreamActivity binds SessionService
 -> token exchanged for source lease
 -> SessionService asks Host to start/attach source
 -> codec config received
 -> MediaCodec created and configured with current Surface
 -> request IDR
 -> first frame rendered
 -> state becomes playing
```

### 8.2 Surface 재생성

```text
surfaceDestroyed
 -> mark output unavailable
 -> stop feeding decoder or detach where supported
 -> clear incomplete frames
 -> keep source lease during grace period

surfaceCreated
 -> reconfigure or recreate decoder
 -> increment viewer decode epoch
 -> request codec config + IDR
 -> render first frame
```

### 8.3 Activity 복원

OS process death 후 Intent에 있던 `source_id`를 신뢰하지 않는다.

1. persistent device identity를 secure storage에서 읽는다.
2. Host session을 재인증한다.
3. 현재 catalog에 source가 있고 승인 상태인지 확인한다.
4. 새 `StreamInstanceId`로 lease를 얻는다.
5. source가 없으면 복구 가능한 오류 UI를 보인다.

## 9. 품질 제어

### 9.1 입력 신호

- stream window pixel bounds
- window focus
- Activity visibility
- decoder queue delay
- render FPS와 drop
- network RTT/loss/jitter
- receiver bandwidth estimate
- host capture/encode time
- Galaxy XR thermal status
- battery/charging 상태

### 9.2 profile 예시

| profile | 해상도 | FPS | 용도 |
| --- | --- | --- | --- |
| focus | source 기준 최대 2560x1440 | 60 | 큰 초점 창 |
| normal | 최대 1920x1080 | 30 | 보이는 일반 창 |
| background-visible | 최대 1280x720 | 15 | 작거나 비초점인 창 |
| suspended | keyframe thumbnail 또는 0 | 0-1 | stopped/숨김 |

profile 변경은 hysteresis를 둔다. 창 focus가 흔들릴 때 매 frame encoder를 재설정하지 않는다.

### 9.3 budget allocator

Host는 모든 source의 총 pixel rate와 bitrate budget을 관리한다.

```text
priority = visibility_weight
         * focus_weight
         * requested_quality
         * health_penalty

allocate high profile to highest priority source
allocate remaining budget fairly
never starve a visible source below configured minimum without degraded state
```

정책은 pure domain function으로 구현해 property test한다.

## 10. 오류 모델

오류는 code, retryability, scope, user action을 가진다.

| scope | 예 | 격리 단위 |
| --- | --- | --- |
| source | 창 종료, permission revoked | 해당 stream window |
| codec | unsupported profile, decoder failure | 해당 source, fallback 가능 |
| session | auth expired, version mismatch | 같은 Host의 모든 창 |
| transport | link lost, congestion | session, 창별 degraded 표시 |
| app | secure storage corrupt | pairing reset 필요 |
| platform | no screen recording permission | Host 전체 source 불가 |

panic, exception, HRESULT/OSStatus를 문자열 하나로 평탄화하지 않는다. platform code는 redacted diagnostic에 포함하고 stable product error로 매핑한다.

## 11. 보안 경계

```text
Untrusted network bytes
 -> TLS/QUIC/WebRTC authentication
 -> bounded parser and version check
 -> authorized device/session
 -> approved source capability
 -> decoder input
```

decoder도 공격 표면이다. 페어링된 Host라고 해도 malformed access unit에서 process가 무너지지 않도록 입력 상한, codec config 검증, fuzz fixture, watchdog를 둔다.

## 12. 관측성

### 12.1 frame trace

sampled frame에 다음 timestamp를 기록한다.

- host capture callback
- host encode submit
- host encode output
- host network enqueue
- viewer network receive
- frame assembled
- decoder queue input
- decoder output callback
- frame released to Surface

서로 다른 장치의 monotonic clock은 직접 뺄 수 없다. clock offset/uncertainty를 추정해 software pipeline 분석에 사용하고, 최종 glass-to-glass는 외부 카메라로 측정한다.

### 12.2 metric labels

허용:

- opaque host/source/session ID의 실행 중 hash
- codec, profile, width, height, FPS
- duration, size, count, error code

금지:

- 창 제목 원문
- 앱 문서명
- 화면 pixel 또는 encoded frame
- IP, 인증서 private key, pairing token
- 사용자 계정명과 파일 경로

## 13. 종료 순서

정상 종료:

```text
StreamActivity close
 -> release source lease
 -> stop receiver subscription
 -> stop decoder input
 -> flush/release MediaCodec
 -> release Surface binding
 -> Host stop encoder
 -> Host stop capture
 -> release native buffers
```

세션 전체 종료:

```text
mark session closing
 -> reject new windows
 -> notify all stream windows
 -> stop all source pipelines concurrently
 -> close transport after bounded drain
 -> clear ephemeral secrets
 -> persist only approved device identity and non-sensitive settings
```

각 단계는 timeout과 idempotency를 가진다. `Drop` 또는 finalizer에만 자원 해제를 의존하지 않는다.

## 14. 기술 선택 게이트

| 선택 | 임시 기본 | 확정 증거 |
| --- | --- | --- |
| Viewer shell | React Native TypeScript + thin Kotlin shim | F-01, F-02, F-07 |
| Android renderer | Rust NDK `AMediaCodec` + `ANativeWindow`/SurfaceView | F-02, F-03, resize stress |
| transport | 미정 | F-08 bake-off |
| baseline codec | H.264 | 실제 Mac -> Galaxy XR S1/F4 |
| Host shell | Tauri | source picker, permission, Rustra real-adapter proof (H15 `addNumbers -> 42`), package proof |
| network wire | spike JSON, 제품 schema 미정 | version/fuzz/size tests |
| Windows capture | Windows.Graphics.Capture | physical Windows E6 |
