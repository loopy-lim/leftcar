# 4K60 수직 분할 이중 인코더와 저비용 Dual Surface 설계

## 상태와 문서 관계

- 상태: dual AVE steady-state 구현 완료, split 복구/flow-control 교정 설계 승인
- 작성일: 2026-08-29
- 목표 플랫폼: Apple Silicon macOS Host + Android Viewer
- 선택한 방식: `splitVertical` + UDP 포트 2개 + Android `SurfaceView` 2개
- 이 문서는
  `docs/superpowers/specs/2026-08-28-4k60-encoder-experiments-design.md`의
  Phase B/C 분할 후보를 구체화한다.
- post-encode queue와 IDR gap 처리 규칙은
  `docs/superpowers/specs/2026-08-29-4k60-split-flow-control-recovery-design.md`가
  이 문서보다 우선한다.
- 기존 3840x2160 단일 RTVC 실기기 결과는 `rateControl` 약 48.66fps,
  `adaptiveQp` 약 48.77fps, `encoderPool` 약 41.53fps였다. Android 렌더 FPS가
  Host 유효 출력과 거의 같고 network queue가 누적되지 않았으므로 첫 병목은
  Wi-Fi나 Android 디코더가 아니라 단일 4K VideoToolbox session 처리량이다.

## 결정 요약

3840x2160 화면은 한 번만 캡처한다. 캡처된 NV12 프레임을 좌우
1920x2160 타일로 나눈 뒤, 두 개의 H.264 AVE hardware encoder에 같은 PTS와
frame sequence로 병렬 제출한다. 각 타일은 기존 UDP/FEC wire를 그대로 사용해
서로 다른 포트로 보낸다.

초기 RTVC 설계는 동시 실행 시 두 session 합계가 약 37~48fps에 머물러
폐기했다. 제품 capability는 exact AVE H.264 session 두 개에 forced-IDR probe
frame을 동시에 제출하고 양쪽 valid callback까지 확인할 때만 `splitVertical`을
광고한다. session create만 성공한 lazy allocation 상태는 capability 증거로
인정하지 않는다. RTVC pair는 진단용 환경 변수에서만 유지한다.

Android는 두 개의 hardware `MediaCodec` decoder를 각각 `SurfaceView`에 직접
연결한다. 앱 소유 OpenGL compositor, `SurfaceTexture`, CPU readback을 사용하지
않는다. 동일 frame sequence의 decoder output을 최대 한 frame period 동안
기다린 뒤 `AMediaCodec_releaseOutputBufferAtTime`으로 같은 표시 시각을
예약한다. 서로 다른 Surface이므로 완전한 원자적 합성을 주장하지 않으며,
실기기에서 좌우 표시 차이가 16.7ms 이내인지 계측한다.

Viewer의 platform capability query는 software codec을 제외하고 타일별
`1920x2160@60`과 `maxSupportedInstances >= 2`를 만족하는 exact codec name을
선택한다. 이 이름을 JNI를 통해 두 native worker에 전달하며 split decoder는
MIME type fallback을 허용하지 않는다. 두 번째 instance 또는 actual codec-name
검증이 실패하면 split session 전체가 fail closed한다.

## 목표

1. 실제 source와 전체 표시 해상도를 3840x2160으로 유지한다.
2. 고변화 화면에서도 Host의 두 타일 유효 인코더 출력과 Android의 joined
   presentation을 지속 60fps에 가깝게 만든다.
3. 정상 상태에서는 좌우가 같은 frame sequence를 표시하고, 순간 지연에도
   좌우 표시 차이를 최대 한 frame period인 16.7ms로 제한한다.
4. 한 타일의 손실이나 decoder 장애가 화면 반쪽의 장기 손상, 검정 화면 또는
   무제한 지연으로 이어지지 않게 pair 단위로 복구한다.
5. Android에서 기존 hardware decoder-to-Surface 경로를 유지해 애플리케이션
   소유 GPU pass와 frame copy를 추가하지 않는다.
6. macOS에서 분할에 필요한 복사와 할당을 최소화하고, 측정 가능한 bounded
   queue만 허용한다.
7. 기능 추가를 계기로 7,271줄의 `CaptureShim.swift`, 2,511줄의 `jni.rs` 등
   긴 파일을 역할별 모듈과 폴더로 분리한다.

## 비목표

- 4K 요청을 1440p나 1080p로 자동 축소하지 않는다.
- 목표 FPS를 자동으로 30fps로 낮추지 않는다.
- software encoder나 software decoder를 fallback으로 사용하지 않는다.
- 첫 구현에서 OpenGL ES/Vulkan compositor를 만들지 않는다.
- 하나의 UDP 포트에 tile ID를 추가하는 wire v2를 만들지 않는다.
- 첫 구현에서 수평 분할이나 4분할을 제품 capability로 노출하지 않는다.
- 첫 검증 범위에서 AOAP/ADB TCP/USB에 이중 media channel을 추가하지 않는다.
  `splitVertical`은 direct Wi-Fi UDP에서만 시작 가능하며 다른 프로필의 기존
  transport 전환 동작은 변경하지 않는다.
- 두 개의 Surface가 항상 동일한 vsync에 원자적으로 latch된다고 가정하지
  않는다. 그 보장이 필요하다는 실측 결과가 나오면 별도 GL 설계로 전환한다.

## 전체 데이터 흐름

```text
ScreenCaptureKit 3840x2160 NV12 capture (한 번)
  -> bounded pair admission
  -> 좌/우 1920x2160 NV12 tile 준비
       -> left AVE H.264 encoder  --\
                                      EncodedPairAssembler
       -> right AVE H.264 encoder --/   -> 둘 다 유효할 때만 기존 UDP/FEC 전송
                                            left: viewer base port
                                            right: base port + 1
  -> 각 타일 MediaCodec input
  -> decoder output ready(frame sequence, PTS)
  -> PairPresentationCoordinator
       complete pair -> 동일 targetPresentNs로 두 Surface에 예약
       16.7ms timeout -> unmatched output 폐기, 마지막 정상 pair 유지
       dependency loss -> 양쪽 동시 CSD/IDR 복구
  -> SurfaceFlinger/hardware composer가 좌우 layer를 한 content rect에 배치
```

## 분할 방향과 프레임 정체성

### 수직 분할 고정

첫 제품 후보는 `splitVertical` 하나다.

- 전체: 3840x2160
- left tile: source rect `(0, 0, 1920, 2160)`
- right tile: source rect `(1920, 0, 1920, 2160)`
- 각 encoder 입력: 1920x2160 NV12

인코더 폭을 3840에서 1920으로 줄일 수 있고, Y plane과 CbCr plane 경계가 모두
짝수이며 일반적인 texture/stride 정렬에도 유리하므로 3840x1080 상하 분할보다
먼저 검증한다.

### pair 단위 admission

한쪽 encoder에만 frame을 제출하지 않는다. 두 encoder와 두 destination buffer가
모두 준비된 경우에만 해당 capture frame을 pair로 승인한다. 어느 한쪽이라도
capacity가 없으면 pending capture를 최신 frame으로 교체하고 이전 capture
pair 전체를 드롭한다. 이 정책은 좌우 sequence 발산보다 한 화면 전체의 최신성
유지를 우선한다.

승인된 capture pair는 다음 값을 공유한다.

- expanded monotonic `frameSequence`
- VideoToolbox `presentationTimeStamp`
- capture monotonic timestamp
- capture wall timestamp
- keyframe/recovery generation

각 포트의 기존 16-bit AU ID에는 같은 `frameSequence` 하위 값이 들어간다. 포트가
분리되어 있으므로 기존 `G`, `P`, `CFG`, `CF2` envelope에 tile ID를 추가하지
않는다. Android는 각 포트의 AU ID를 wrap-safe sequence로 확장해 동일한 pair key로
사용하고, MediaCodec input PTS도 이 sequence에서 계산한다. 한 타일에서 AU가
누락되어도 다음 타일의 PTS가 로컬 feed count 때문에 달라지지 않아야 한다.

## macOS 저비용 분할과 이중 인코더

### NV12 tile 준비

제품 기본 경로는 IOSurface/Metal-compatible `CVPixelBufferPool` 두 개와 하나의
`MTLBlitCommandEncoder`를 사용한다.

1. capture NV12의 Y/CbCr plane을 `CVMetalTextureCache` texture로 연다.
2. left/right pool에서 재사용 가능한 1920x2160 destination을 하나씩 얻는다.
3. 한 command buffer에서 Y plane 두 영역과 CbCr plane 두 영역을 총 네 번
   region blit한다.
4. CPU `waitUntilCompleted`를 호출하지 않는다. command buffer completion에서
   같은 pair의 두 VideoToolbox 제출을 예약한다.
5. capture source와 destination pair는 completion/callback까지 명시적으로
   retain하고 종료 시 generation으로 늦은 callback을 무시한다.

이는 full-frame CPU copy, GPU compute shader, 색공간 변환, scale을 피한다.
Metal blit도 시스템 GPU/display 자원을 사용할 수 있으므로 비용이 0이라고
주장하지 않고 `splitPreparationP95Us`와 실기기 GPU/전력 표본을 기록한다.

구현 전 짧은 feasibility probe에서 원본 stride를 보존한 zero-copy plane view가
두 RTVC session 모두에서 hardware encode와 60fps를 안정적으로 유지하는지도
검사할 수 있다. source buffer lock을 encoder callback까지 유지하거나
VideoToolbox 내부 staging을 유발하면 즉시 폐기한다. 제품 경로는 측정 결과가
없는 zero-copy 가정 대신 bounded Metal blit을 기준으로 한다.

### encoder pair

- 동일한 capture stream 아래 `TileEncoder.left`와 `TileEncoder.right`를 둔다.
- 두 session 모두 exact H.264 AVE hardware encoder, `RealTime=true`, frame reorder
  비활성, expected frame rate 60, 동일 GOP/recovery 정책을 적용한다.
- 두 session의 callback/submit state와 in-flight counter는 분리하지만 pair
  admission과 recovery generation은 공유한다.
- `EncodedPairAssembler`는 같은 frame sequence의 두 callback을 타일당 하나씩
  보관한다. 둘 다 valid sample일 때만 packetization queue 두 개에 함께 넘긴다.
  한쪽이 `frameDropped`, OSStatus error, callback timeout이면 반대쪽 sample도
  전송하지 않고 폐기한다.
- 전송하지 않은 encoded frame을 다음 delta frame이 참조하지 않게 callback pair
  실패는 양쪽 force-keyframe을 예약한다. 두 IDR callback이 모두 유효할 때만
  CSD와 함께 전송해 dependency chain을 다시 같은 경계에서 시작한다.
- callback barrier는 sequence별로 최대 한 frame period만 기다리고 타일당 최대
  두 sequence의 sample만 보관한다. 이는 한 encoder가 최대 한 frame 앞서는 것은
  허용하면서도 그 이상 누적되지 않게 한다. 가장 오래된 incomplete pair는
  폐기해 sample buffer가 encoder throughput을 역으로 막지 않게 한다.
- packetization 이후 network queue도 좌/우 access unit을 원자적 pair 하나로
  보관한다. 초기 구현의 post-encode latest-wins 정책은 wire AU gap과 반복 IDR
  복구를 만들 수 있어 폐기한다. 승인된 capture-to-send lease, send-time AU ID,
  recovery-boundary discard 규칙은 별도 flow-control 교정 설계를 따른다.
- 각 타일은 전체 목표 bitrate의 50%를 초기 ceiling으로 사용한다. 정적인 타일이
  ceiling을 모두 소비하지 않는 것은 허용한다. 첫 구현에서 복잡도 기반 bitrate
  재분배는 넣지 않고, per-tile 실제 bitrate를 계측해 후속 필요성을 판단한다.
- ABR 압력은 두 타일의 loss, oldest age, decoder pressure 중 더 나쁜 값을 사용해
  pair 전체에 동일한 비율로 적용한다. 한쪽만 품질 단계가 달라지지 않게 한다.
- 한쪽 encoder가 startup 또는 runtime fatal error를 내면 나머지 한쪽만 계속
  보내지 않고 split session 전체를 명시적 오류로 종료한다.

## 두 포트와 기존 media wire 재사용

`StartStreamInput.viewerPort`를 left/base port로 유지하고 right port는
`viewerPort + 1`로 결정한다. `splitVertical` 시작 시 다음을 검증한다.

- base port가 65534 이하이다.
- base와 base+1 UDP receiver가 Host 시작 전에 모두 bind되었다.
- Host의 같은 unconnected UDP socket에서 nonce challenge를 두 주소에 보내고,
  base와 base+1 양쪽 source port의 echo를 모두 확인한 뒤에만 capture를 시작한다.
- 두 endpoint가 같은 paired Host만 허용한다.
- 두 포트 중 하나라도 prepare에 실패하면 Activity와 Host stream을 시작하지
  않는다.

각 포트는 기존 fragment reassembly, Reed-Solomon parity, latency timestamp,
challenge/authentication, receiver feedback을 독립적으로 유지한다. Host는
`recvfrom` source port로 left/right feedback을 분류한다. protocol byte를
변경하지 않아 단일 stream 경로의 회귀 위험을 줄인다.

기존 `LCF1` receiver feedback은 nonce 뒤가 아니라 nonce 앞의 optional trailing
fields로 joined FPS, pair-ready p95/max, sync timeout, unmatched drop을 확장한다.
기존 Host는 34-byte core만 읽고 추가 bytes를 무시할 수 있으며 새 Host는 source
port별 rendered FPS와 base feedback의 joined fields를 함께 기록한다.

입력 이벤트와 cursor 좌표는 left/base endpoint 하나만 사용한다. right endpoint는
media와 tile feedback 전용이다. Host 종료 통지는 두 endpoint에 전송하되 Viewer는
logical split session 하나로 coalesce한다.

## Android Dual Surface 렌더링

### layout

`SplitStreamLayout`은 전체 16:9 content rect를 먼저 계산한 다음 그 rect를 정확한
정수 x 좌표에서 좌우로 나눈다. 각 `SurfaceView`가 독립적으로 aspect-fit을 하지
않게 해 중앙 seam, 1px gap, 중복 scale을 방지한다.

- left view: content rect의 왼쪽 절반
- right view: content rect의 나머지 폭
- 두 view 모두 opaque, 동일 z-order policy, source tile 크기 1920x2160
- 배경은 검정이며 마지막 정상 buffer를 새 pair가 올 때까지 유지
- pointer/touch 좌표는 두 view가 아니라 전체 content rect 기준으로
  3840x2160 source 좌표에 매핑

앱이 OpenGL texture를 합성하지 않는다. decoder는 `ANativeWindow`에 직접
출력하고 SurfaceFlinger/hardware composer가 두 layer를 배치한다. 기기 compositor가
필요에 따라 GPU를 사용할 수는 있지만 앱 소유 texture copy나 render pass는 없다.

### decoder와 presentation coordinator

현재 decoder의 `feed_au_status()`가 준비된 output을 즉시 렌더하는 결합을
분리한다.

- `queue_access_unit(au, pts)`
- `dequeue_output(timeout) -> ReadyOutput { index, pts }`
- `release_output_at(index, target_ns)`
- `discard_output(index)`

left/right decoder worker는 자기 `MediaCodec` API를 한 thread에서 순서대로
호출한다. 중앙 `PairPresentationCoordinator`는 codec output index 자체를
release하지 않고 worker에 명령만 보낸다.

동기화 규칙은 다음과 같다.

1. 같은 PTS의 left/right output이 준비되면 다음 display tick에 가까운 동일한
   `targetPresentNs`를 계산한다.
2. 두 worker가 `AMediaCodec_releaseOutputBufferAtTime`에 같은 값을 전달한다.
3. 한쪽이 먼저 준비되면 한 frame period에 4ms의 bounded scheduler allowance를
   더한 최대 20.7ms까지만 기다린다. 이 allowance는 이미 일치한 frame의
   표시를 늦추지 않으며, 더 큰 값은 MediaCodec output 보유량을 늘리므로 쓰지
   않는다.
4. timeout까지 pair가 완성되지 않으면 준비된 unmatched output을 render 없이
   폐기한다. 이후 늦게 도착한 반대쪽의 같은 PTS도 폐기한다.
5. 마지막으로 완성되어 표시된 pair는 Surface에 남으므로 한쪽만 여러 frame 앞서
   보이지 않는다. 다음 공통 PTS부터 정상 표시를 재개한다.
6. Android pending output은 타일당 최대 하나다. 새 output이 더 최신이면 오래된 unmatched
   output을 폐기해 decoder backlog가 interaction latency로 변하지 않게 한다.

NDK의 timed release API는 API 21부터 제공되고 현재 Android minSdk 24보다 낮으므로
추가 OS fallback은 필요하지 않다. 다만 서로 다른 Surface의 실제 latch 차이는
기기 compositor 동작에 좌우되므로 `pairReadyDeltaUs`, `scheduledPresentDeltaUs`,
가능한 실제 frame timestamp를 별도로 측정한다.

## pair-scoped 복구

1. 일반 단일-session encoder `frameDropped`는 기존처럼 network dependency
   손상으로 오인하지 않는다. `splitVertical`에서 pair admission 이후 한 타일만
   encoder drop되면 `EncodedPairAssembler`가 해당 pair의 양쪽 sample을 모두
   전송하지 않고 다음 capture pair에 paired IDR을 예약한다.
2. 한 타일 reassembler가 dependency loss를 감지하면 base endpoint를 통해
   pair recovery를 한 번 요청한다.
3. Host는 다음 승인 capture pair에서 양 encoder 모두 force-keyframe으로
   제출하고 두 타일의 CSD를 함께 갱신한다.
4. Viewer는 recovery generation 동안 unmatched delta output을 표시하지 않고,
   같은 sequence의 양쪽 IDR이 decode된 뒤 pair presentation을 재개한다.
5. 한 decoder가 fatal error, Surface loss, codec reconfigure를 만나면 한쪽만
   재생성하지 않고 두 decoder와 coordinator를 logical session 단위로 rebuild한다.
6. Surface resize 동안 기존 recovery suppression을 양쪽에 공통 적용한다.
7. recovery 요청은 기존 cooldown으로 coalesce해 양쪽 IDR이 반복 발생하는 증폭
   루프를 막는다.

## capability와 사용자 선택

- 기존 예약값 `splitVertical`을 Host capability에 추가하되 macOS hardware encoder
  probe를 통과한 경우에만 광고한다.
- Viewer가 보고하는 `maxConcurrentDecodersHint`가 2 미만이면 선택 목록에서
  숨긴다.
- 선택 UI에는 `4K 이중 인코더 · Wi-Fi 전용`임을 표시하고 reconnect가 필요함을
  유지한다.
- `splitVertical`은 실제 3840x2160 target과 direct UDP일 때만 시작한다.
- unsupported 기기, 다른 transport, 한쪽 port prepare 실패에서는
  `rateControl`로 조용히 fallback하지 않고 사용자에게 명확한 시작 오류를
  반환한다.
- 실기기 게이트를 통과하기 전에는 `auto`가 `splitVertical`을 선택하지 않는다.

## 모듈과 폴더 구조

기능 구현 전에 큰 파일을 동작 보존 방식으로 나눈다. 파일 이동과 기능 변경을
같은 patch에 섞지 않고, 각 extraction 뒤 기존 tests와 build를 통과시킨다.

### macOS Swift

```text
native/macos-capture-shim/Sources/
  CaptureShim.swift                 # C ABI와 session registry만
  Capture/
    CaptureSession.swift            # capture lifecycle과 전체 orchestration
    CaptureBackend.swift            # SCK/legacy backend policy
  Encoder/
    EncoderPolicy.swift             # 순수 정책과 experiment parsing
    VideoToolboxEncoder.swift        # 단일 RTVC session wrapper
    AdaptiveQpController.swift
  Split/
    SplitGeometry.swift             # rect, plane offset, pair admission 순수 로직
    MetalNv12Splitter.swift          # pool/texture cache/blit lifetime
    EncodedPairAssembler.swift       # callback barrier와 bounded sample lifetime
    DualEncoderPipeline.swift        # 두 TileEncoder와 pair recovery
  Transport/
    MediaWire.swift                 # fragment/config/parity 생성
    ViewerTransport.swift           # UDP/TCP/USB send와 feedback
  Metrics/
    RollingMetrics.swift
    StreamStats.swift
```

Swift build command를 여러 source file을 안정적으로 받는 공용
`tools/build-macos-capture-shim.zsh`로 모은다. 개발 실행, policy test, 설치 build가
각자 `CaptureShim.swift` 하나만 컴파일하지 않게 같은 source 목록을 사용한다.

### Android Rust

```text
native/android-viewer/src/
  jni.rs                             # JNI-facing orchestration facade
  renderer/
    mod.rs
    session.rs                       # logical single/split session lifecycle
    network.rs                       # socket batch와 endpoint 처리
    tile_worker.rs                   # one socket + reassembler + decoder worker
    presentation_sync.rs             # host-testable pair state machine
    recovery.rs                      # pair-scoped recovery gate
    stats.rs
```

`crates/viewer-decoder/src/lib.rs`도 변경되는 NDK output 부분을 다음처럼 분리한다.

```text
crates/viewer-decoder/src/
  lib.rs
  annex_b.rs
  android/
    mod.rs
    ffi.rs
    decoder.rs
    output.rs                        # dequeue/timed release/discard
```

### Android Kotlin

```text
apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/
  StreamActivity.kt                  # lifecycle와 input forwarding
  StreamIntent.kt                    # extra parsing/validation
  view/
    AspectRatioSurfaceView.kt
    SplitStreamLayout.kt
    StreamHud.kt
```

새 파일은 한 책임을 가지며 가능한 한 800줄 미만을 목표로 한다. orchestration
특성상 이를 넘겨야 하면 새 책임 경계를 먼저 검토한다. 기존 긴 파일의 extraction
완료 목표는 `CaptureShim.swift` 1,500줄 이하, `jni.rs` 500줄 이하,
`StreamActivity.kt` 500줄 이하이다.

## stats와 진단

기존 전체 stats에 다음을 추가한다.

- `splitDirection`
- `splitPreparationP50Us`, `splitPreparationP95Us`
- `splitPairAdmissionDrops`
- `encodedPairCallbackP50Us`, `encodedPairCallbackP95Us`
- `encodedPairTimeouts`, `encodedPairDrops`
- `leftValidEncodeOutputFps`, `rightValidEncodeOutputFps`
- `leftEncoderFrameDrops`, `rightEncoderFrameDrops`
- `leftBitrateBps`, `rightBitrateBps`, `aggregateBitrateBps`
- `leftReceiverLoss`, `rightReceiverLoss`
- `leftRenderedFps`, `rightRenderedFps`, `joinedRenderedFps`
- `pairReadyDeltaP50Us`, `pairReadyDeltaP95Us`, `pairReadyDeltaMaxUs`
- `pairSyncTimeouts`, `unmatchedOutputDrops`
- `pairedRecoveryRequests`, `pairedRecoveryKeyframes`
- `splitTestInjectedDrops` (diagnostic environment가 설정된 run에서만 증가)

Desktop inspector는 전체 FPS 하나만 보여주지 않고 두 encoder와 joined
presentation을 함께 표시한다. Android HUD는 평상시 간결하게 joined FPS,
capture-to-render age, 현재 최대 tile skew만 보여주고 상세 계측은 펼침 영역이나
로그에 둔다.

## 오류 처리

- 두 encoder 중 하나라도 hardware session 생성에 실패하면 startup 실패다.
- 두 decoder 중 하나라도 hardware codec 생성/configure에 실패하면 두 codec을
  해제하고 startup 실패다.
- 두 Surface 중 하나가 유효하지 않으면 decoder를 부분 attach하지 않는다.
- runtime 한쪽 fatal error는 logical pair 전체 teardown/reconnect를 유발한다.
- split buffer pool 고갈 시 추가 할당으로 queue를 키우지 않고 capture pair를
  드롭한다.
- Metal command buffer 오류는 해당 pair를 폐기하고 제한된 횟수 후 session을
  종료한다. CPU copy로 조용히 fallback하지 않는다.
- session 종료, resize, reconnect에서 두 port/socket/decoder/window reference가
  모두 해제되었는지 generation과 bounded wait로 검증한다.

## 테스트 전략

### TDD와 정적 테스트

구현 전에 다음 실패 테스트를 추가한다.

1. 3840x2160 vertical geometry가 Y와 CbCr plane에서 정확한 left/right rect를
   만든다.
2. 한 encoder capacity가 없으면 pair 전체 admission이 거부된다.
3. 두 encoder가 같은 frame sequence, PTS, keyframe generation을 받는다.
4. 한쪽 callback만 valid이면 어느 타일도 packetize하지 않고 paired IDR을
   예약한다.
5. 양쪽 callback이 valid인 pair만 두 packetization queue에 함께 전달된다.
6. split profile은 base/base+1 두 port가 준비되지 않으면 시작되지 않는다.
7. 기존 wire parser가 서로 다른 port에서 같은 AU ID를 독립적으로 조립한다.
8. presentation coordinator는 같은 PTS만 pair로 만들고 같은 target timestamp를
   두 worker에 보낸다.
9. 한쪽이 16.7ms 안에 도착하면 pair를 표시하고, 이후 도착하면 양쪽 unmatched
   output을 폐기한다.
10. wrap된 16-bit AU ID와 frame gap 후에도 두 tile PTS가 다시 일치한다.
11. 한 타일 loss가 한 번의 paired recovery만 만들고 다음 양쪽 IDR에서 gate가
   해제된다.
12. Surface 하나의 destroy/recreate가 두 decoder lifecycle을 함께 전환한다.
13. pointer 좌표가 두 Surface 경계에서도 전체 3840x2160 좌표로 연속 변환된다.
14. 단일 stream의 기존 immediate render와 transport 동작은 변경되지 않는다.
15. `LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER=N`은 완성된 N번째 pair에서 right AU
    하나만 억제하고 즉시 disarm되며, 환경값이 없을 때는 어떤 분기도 바꾸지 않는다.

필수 정적 검증은 Swift policy/build tests, Rust unit/integration tests, Android Kotlin
compile, Viewer TypeScript/tests, Host Desktop tests, repository typecheck,
`cargo fmt --all -- --check`, `git diff --check`를 포함한다.

React/React Native/TSX/style 또는 component behavior가 변경되면 저장소 root에서
`npx -y react-doctor@latest . --verbose`를 실행해 정확히 `100 / 100`을 받아야 한다.
수정 후 typecheck와 관련 tests를 다시 실행한다.

### 실기기 검증 단계

#### Gate 0: 독립 capability probe

- 실제 capture 기반 1920x2160 RTVC session 두 개를 동시에 시작한다.
- Android target device에서 1920x2160 H.264 decoder 두 개를 동시에 Surface에
  연결한다.
- 어느 한쪽이라도 hardware path를 제공하지 않으면 제품 구현을 계속하지 않고
  fail-closed 결과를 남긴다.

#### Gate 1: Host split throughput

- 실제 3840x2160 capture를 Metal blit으로 좌우 분할한다.
- 정적 30초, `Option+0` 고변화 화면 180초를 측정한다.
- 두 encoder 각각 valid output 평균 59fps 이상이어야 한다.
- paired valid output 평균 59fps 이상, encoder drop 0이어야 한다.
- `splitPreparationP95Us`는 2.5ms 이하이고 시간에 따라 증가하지 않아야 한다.

#### Gate 2: end-to-end 화면 정확성

- Viewer가 실제 3840x2160 content rect와 1920x2160 decoder 두 개를 보고한다.
- joined presentation 평균 59fps 이상, rolling 1초 p5 55fps 이상이어야 한다.
- 정상 pair의 scheduled timestamp는 같아야 하고 측정된 좌우 차이는 모두
  16.7ms 이하여야 한다.
- 중앙 seam에 검정 선, 겹침, 다른 scale, 좌우 반전이 없어야 한다.
- high-motion checkerboard와 중앙을 가로지르는 이동 물체에서 장기 tear가 없어야
  한다.
- 한쪽 packet loss 주입 뒤 화면 반쪽이 지속해서 깨지지 않고 paired IDR 후 두
  output frame 안에 복구되어야 한다.

#### Gate 3: latency와 자원

- capture-to-render age p95 50ms 이하이며 10분 동안 증가 추세가 없어야 한다.
- pair sync wait p95 16.7ms 이하, pending output은 타일당 1을 넘지 않는다.
- Android에 앱 소유 GL context/render loop와 CPU frame copy가 없음을 코드와
  profiler로 확인한다.
- SurfaceFlinger layer와 frame timeline을 기록하고 단일 decoder 기준선 대비
  compositor/GPU 부담을 문서화한다. 수치가 높으면 GL을 추가하지 않고 먼저
  layer geometry, format, z-order를 수정한다.
- Host의 Metal split, 두 encoder, 전체 CPU/GPU/전력 표본을 단일 4K 기준선과
  비교해 `docs/11-low-latency-investigation.md`에 남긴다.

#### Gate 4: soak와 선택 노출

- `Option+0` 고변화 600초 동안 0fps 정지, stream 종료, queue 증가가 없어야 한다.
- 통과 후에만 Host가 `splitVertical` capability를 일반 Viewer에 광고한다.
- 통과 전에는 개발용 명시 플래그에서만 실행하며 `auto`에는 포함하지 않는다.

## 구현 순서

1. 긴 Swift/Rust/Kotlin 파일을 동작 보존 방식으로 분리하고 공용 build command를
   만든다.
2. pure split geometry, pair admission, presentation coordinator를 TDD로 만든다.
3. Mac dual RTVC와 Android dual decoder capability probe를 실행한다.
4. Metal NV12 splitter와 paired encoder callback/stats를 구현하고 Gate 1을
   통과시킨다.
5. 두 UDP receiver와 기존 wire를 연결한다.
6. Android `SplitStreamLayout`, decoder workers, timed Surface release를 구현한다.
7. pair-scoped recovery와 lifecycle teardown을 구현한다.
8. Viewer 선택 UI와 Desktop diagnostics를 연결한다.
9. 정적 품질 gate와 실기기 Gate 2~4를 실행하고 결과를 문서화한다.

## 중단과 후속 전환 조건

- dual RTVC 또는 dual hardware decoder가 capability probe에서 실패하면 두 Surface
  제품화를 중단한다. software fallback이나 해상도 하향으로 성공처럼 보이지
  않는다.
- 두 encoder가 각각 59fps에 도달하지 못하면 wire/Android 구현을 더 확장하지
  않고 Mac split preparation과 encoder scheduling을 다시 분석한다.
- 두 Surface 방식에서 실측 tile skew가 16.7ms를 넘거나 중앙 tear가 반복되면
  현재 경로를 숨기고, decoder-to-`SurfaceTexture` + 단일 OpenGL compositor를
  별도 설계로 작성한다.
- 성능 gate를 통과해도 4K가 아닌 profile과 기존 단일 stream의 회귀가 있으면
  capability를 광고하지 않는다.
