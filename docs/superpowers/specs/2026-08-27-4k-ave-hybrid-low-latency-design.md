# 4K AVE/RTVC 하이브리드 저지연 인코더 설계

## 상태와 문서 관계

- 상태: 사용자 승인 완료, 구현 계획 작성 완료
- 작성일: 2026-08-27
- 목표 플랫폼: Apple Silicon macOS Host + Lenovo TB710FU Android Viewer
- 이 문서는 `2026-08-26-4k-hevc-low-latency-design.md`의 wire/Android HEVC
  호환 경로는 유지하면서, 4K 기본 인코더 선택과 VideoToolbox 속성 정책을
  교체한다.
- `docs/superpowers/plans/2026-08-27-encoder-throughput-1440p60.md`의
  “low-latency rate control 유지”와 “4K HEVC 우선” 항목은 현재 실기기 및
  capability 근거와 충돌하므로 이 문서가 해당 항목을 대체한다.

## 목표

Leftcar가 3840×2160 화면을 최대한 60fps에 가깝게 캡처·인코딩·전송·렌더하고,
고변화 화면에서도 오래된 프레임이 쌓여 반응 지연이 증가하지 않게 한다.

구체적인 1차 목표는 다음과 같다.

1. 4K `video` 프로필에서 일반 Apple 하드웨어 인코더(AVE)를 사용해 Host와
   Android의 지속 출력·렌더를 먼저 55fps 후보 게이트 이상으로 올리고,
   최종 180초 평균을 각각 59fps 이상으로 만든다.
2. 1080p/1440p `interactive` 프로필과 4K RTVC 대조군의 기존 저지연 동작을
   보존한다.
3. 실제 선택된 인코더 ID, hardware 여부, preset/profile, 적용·거부된 속성을
   Host 상세 지표에서 직접 확인할 수 있게 한다.
4. 4K 처리량을 높이더라도 software pipeline 지연 p95가 50ms를 넘어서 계속
   증가하지 않게 한다.

“거의 0에 가까운 레이턴시”는 물리적으로 0ms라는 뜻으로 사용하지 않는다.
이 문서의 검증 범위에서는 캡처 timestamp부터 Android decoder 입력까지의
software pipeline age와 RTT를 측정한다. photon-to-photon 지연은 고속 카메라
측정 전까지 별도 미검증 항목으로 남긴다.

## 현재 근거

### 실기기 4K 기준선

2026-08-27 설치본에서 3840×2160 H.264 60fps 세션을 열고 `Option+0`으로
고변화 배경 화면을 재현했다.

- 단계별 FPS: capture 57 / encode submit 46 / encode output 46
- Android 실제 render: 46fps
- encode output interval p95: 23.9ms
- capture→encode p95: 38.3ms
- capture queue wait 관측값: 6.2ms
- encode output p95: 22.9ms
- packetization p95: 0.4ms
- network send 관측값: 1.6ms
- receiver RTT / decoder-side wire age: 11ms / 7ms
- Host network queue: 0B / oldest 0.0ms
- Android output drop / decoder input drop: 0 / 0

캡처 callback은 60fps에 근접하지만 VideoToolbox 출력이 한 프레임 예산
16.67ms를 넘으면서 submit과 output이 약 46fps로 제한된다. packetization,
Host network queue, Android decoder는 이 출력률을 따라가므로 현재의
1차 병목은 Host 인코더 경로다.

### VideoToolbox capability 근거

동일한 M1 Max에서 `VTCopyVideoEncoderList`,
`VTSessionCopySupportedPropertyDictionary`, 실제 session 생성으로 확인한 결과:

- `EnableLowLatencyRateControl=true`
  - H.264 encoder ID: `com.apple.videotoolbox.videoencoder.h264.rtvc`
  - 지원 preset: `VideoConferencing`
  - `PrioritizeEncodingSpeedOverQuality`, `MaximumRealTimeFrameRate`,
    `SuggestedLookAheadFrameCount`, `Quality`, `MaxFrameDelayCount` 설정을
    `-12900`으로 거부했다.
- low-latency rate control을 지정하지 않은 일반 하드웨어 경로
  - H.264 encoder ID: `com.apple.videotoolbox.videoencoder.ave.avc`
  - hardware accelerated: true
  - `HighSpeed` preset과 speed priority, maximum realtime FPS, look-ahead 0,
    quality hint를 지원했다.

따라서 현재 코드는 4K에서도 RTVC를 강제한 뒤 RTVC가 지원하지 않는 속성들을
설정하고 있다. 코드에 속성이 존재하는 것과 실제 인코더에 적용되는 것은
같지 않다.

standalone Swift 프로세스의 후속 throughput probe는 현재 시스템 상태에서
session 생성 `-12903`을 받았지만, 같은 시점에 설치된 Host는 4K session을
정상 생성했다. 이 probe는 처리량 acceptance 근거로 사용하지 않고 실제 Host
통합 경로에서 A/B한다.

## 검토한 접근

### 선택: 프로필 기반 AVE/RTVC 하이브리드

기존 `contentMode`를 실제 인코더 경로 선택에 사용한다. 4K `video`는 AVE
H.264, `interactive`와 1440p 이하는 RTVC H.264를 사용한다. 같은 바이너리에서
Viewer의 “동영상 우선”과 “선명한 화면/균형” 프로필로 A/B할 수 있어 별도
숨은 환경 변수나 wire 변경이 필요 없다.

장점은 변경 범위가 Host에 집중되고 기존 H.264 decoder/wire를 그대로 쓰며,
실제로 지원되는 AVE 속성을 사용할 수 있다는 점이다. 위험은 AVE가 RTVC보다
내부 buffering 지연이 클 수 있다는 점이며, 처리량과 age를 함께 acceptance로
묶어 방지한다.

### 보류: AVE HEVC 우선

AVE HEVC도 동일한 일반 하드웨어 속성 경로를 사용할 수 있지만, 기존 4K HEVC
실기기 결과는 H.264보다 느렸다. H.264 AVE가 최종 4K60 조건을 달성하지 못할
때 동일한 계측으로 비교하는 두 번째 후보로만 둔다. HEVC를 기본으로 다시
바꾸려면 실제 Host/Android output과 latency가 H.264 AVE보다 좋아야 한다.

### 후속 별도 설계: tiled/multi-session 4K

4K를 두 개 이상의 encoder session으로 나누면 hardware 병렬성을 더 사용할
가능성이 있지만 wire, Android decoder 수, Surface 합성, recovery 경계가 모두
바뀐다. 단일 AVE가 목표에 실패한 것이 입증되기 전에는 구현하지 않는다.

## 아키텍처

### 인코더 경로 정책

순수 함수 `encoderSessionPolicy(width:height:contentMode:)`가 session 생성 전에
다음 결정을 반환한다.

| 해상도/모드 | 기본 경로 | 기본 코덱 | 목적 |
| --- | --- | --- | --- |
| 3840×2160 이상 + `video` | AVE | H.264 | 4K 처리량과 60fps 우선 |
| 3840×2160 이상 + `interactive` | RTVC | H.264 | 같은 바이너리의 저지연 대조군 |
| 2560×1440 이하 + `interactive` | RTVC | H.264 | 기존 1440p/1080p 반응성 보존 |
| sub-4K + `video` | RTVC | H.264 | 검증되지 않은 범위의 회귀 최소화 |

4K AVE가 최종 4K60 처리량과 지연 acceptance를 모두 통과하면 후속 정책
단계에서 4K `interactive`도 AVE로 승격할 수 있다. 통과 전에는 두 프로필을
합치지 않는다.

### AVE 발견과 명시적 선택

하드코딩된 encoder ID 하나에만 의존하지 않는다.

1. `VTCopyVideoEncoderList`로 H.264 encoder를 열거한다.
2. `kVTVideoEncoderList_IsHardwareAccelerated=true`인 항목만 남긴다.
3. codec이 일치하는 후보 중 `PerformanceRating`이 가장 높은 항목을 선택한다.
4. 선택한 `EncoderID`를
   `kVTVideoEncoderSpecification_EncoderID`로 session 생성에 전달한다.
5. `RequireHardwareAcceleratedVideoEncoder=true`를 함께 지정해 software fallback을
   금지한다.
6. 생성 후 `EncoderID`와 `UsingHardwareAcceleratedVideoEncoder`를 다시 읽어
   요청과 실제 선택이 일치하는지 확인한다.

RTVC 경로는 기존처럼 `EnableLowLatencyRateControl=true`를 사용한다. AVE
경로에는 이 키를 넣지 않는다. 두 키를 동시에 사용해 encoder 선택을
VideoToolbox에 모호하게 맡기지 않는다.

### 속성 적용 계약

session 생성 직후 supported property dictionary와 supported preset dictionary를
읽는다. 속성은 다음 세 부류로 처리한다.

#### 공통 필수 속성

- `RealTime=true`
- `AllowFrameReordering=false`
- H.264 Main profile + CABAC(4K AVE)
- `ExpectedFrameRate=60`
- hardware encoder 확인

필수 속성 설정 또는 prepare가 실패하면 해당 후보를 폐기하고 fallback으로
간다. 4K AVE에서 Main profile이 실패했을 때 성능이 악화된 Baseline/CAVLC로
몰래 내려가지 않는다.

#### AVE 선택 속성

지원되는 경우 다음 순서로 적용한다.

1. `HighSpeed` preset
2. `RealTime=true`, `AllowFrameReordering=false` 재적용
3. `PrioritizeEncodingSpeedOverQuality=true`
4. `MaximumRealTimeFrameRate=60`
5. `SuggestedLookAheadFrameCount=0`
6. `MaximizePowerEfficiency=false`
7. 초기 `Quality=0.25`
8. 계산된 `AverageBitRate`와 `DataRateLimits`로 preset bitrate 덮어쓰기

`MaxFrameDelayCount`는 현재 AVE/RTVC probe에서 writable 속성이 아니므로
설정 대상에서 제거하고 지원 여부만 계측한다.

#### RTVC 선택 속성

RTVC가 광고하는 `VideoConferencing` preset과 공통 필수 속성만 적용한다.
speed priority, maximum realtime FPS, generic quality처럼 지원되지 않는 키는
호출하지 않는다. 동적 화질 제어는 RTVC에서 실제 적용 가능한 bitrate scale을
사용하고, generic quality hint와 구분해 표시한다.

각 선택 속성은 `applied`, `unsupported`, `rejected(status)` 중 하나로 기록한다.
지원 dictionary에 없는 키는 `unsupported`이며 오류로 취급하지 않는다.
지원한다고 광고했지만 설정에 실패한 키는 `rejected`이며 상태 코드와 함께
노출한다.

### fallback 순서

4K `video` session 시작의 fallback은 다음과 같다.

1. AVE H.264 hardware
2. 기존 RTVC H.264 hardware

AVE H.264가 생성되지만 runtime 처리량이 낮다는 이유로 session 중간에 RTVC로
자동 교체하지 않는다. encoder 교체는 codec configuration과 keyframe 경계를
새로 만들어야 하므로, 현재 session을 명시적으로 재시작하는 별도 동작이다.

AVE HEVC는 H.264 AVE가 최종 4K60 조건에 실패했을 때만 동일한 실험 매트릭스로
평가한다. 평가 전에는 자동 fallback 체인에 넣지 않는다.

### 계측 전달

Swift stats JSON에는 이미 encoder ID, hardware flag, preset, profile이 있지만
Rust FFI와 UI 경계에서 버려지고 있다. 다음 필드를 전체 경로로 전달한다.

- `encoderMode`: `ave` 또는 `rtvc`
- `encoderID`
- `encoderHardwareAccelerated`
- `encoderPreset`
- `encoderProfile`
- `encoderAppliedProperties`
- `encoderUnsupportedProperties`
- `encoderRejectedProperties` (`key=status` 문자열)
- `encoderFallbackReason`

Host 상세 지표는 선택된 경로와 설정 결과를 FPS/latency 옆에 표시한다. 이 UI를
추가하므로 React Doctor `100 / 100`, desktop typecheck와 UI 테스트가 필수다.

## 데이터 흐름

```text
Viewer profile
  -> contentMode + 4K request
  -> pure encoderSessionPolicy
  -> AVE enumeration or RTVC low-latency selection
  -> supported-property plan
  -> VTCompressionSession create/prepare
  -> selected encoder verification + stats
  -> existing H.264 CFG/AU packetization
  -> existing UDP/FEC/recovery path
  -> existing Android low-latency H.264 decoder
  -> Surface render + authenticated feedback
```

wire packet, Android codec 구성, pairing, input, USB/Wi-Fi 전환 규약은 변경하지
않는다.

## 지연과 품질 정책

- AVE의 첫 목표는 backlog 없이 60fps frame budget을 맞추는 것이다.
- encode in-flight 상한은 4K에서 현재 3을 유지한다. 첫 실기기 A/B에서
  throughput을 확인하기 전에 1로 줄여 병렬성을 잃거나 5 이상으로 늘려
  latency를 숨기지 않는다.
- quality 0.25는 encoder 계산량을 줄이기 위한 시작점이며, bitrate ABR와 별개로
  실제 적용 여부를 표시한다.
- output이 55fps 이상이어도 capture/encode age가 계속 증가하거나 p95 50ms를
  넘으면 4K 저지연 목표를 통과한 것으로 보지 않는다.
- 처리량 통과 후에만 in-flight 2/3 A/B와 quality 0.25/0.5 A/B로 latency와
  화질을 회복한다.
- 사용자가 선택한 4K를 몰래 1440p로 낮추지 않는다.

## 오류 처리

- AVE hardware 후보가 없거나 create/mandatory property/prepare가 실패하면
  RTVC H.264를 한 번 시도하고 `encoderFallbackReason`을 남긴다.
- software encoder는 사용하지 않는다. 두 hardware 경로가 모두 실패하면
  session을 명시적 오류로 종료한다.
- 선택 속성 하나가 unsupported인 것은 session 실패가 아니다. advertised
  property가 rejected된 경우도 계측 후 계속할 수 있지만, 필수 속성이면
  session을 폐기한다.
- encoder ID/hardware 재확인이 요청과 다르면 해당 session을 사용하지 않는다.
- 기존 UDP loss, IDR recovery, stale live-edge 정책은 그대로 유지한다. AVE
  전환과 recovery 변경을 한 실험에 섞지 않는다.

## 테스트 전략

### TDD 정책 테스트

구현 전에 다음 assertion을 추가하고 실패를 확인한다.

1. 4K `video`는 AVE H.264 정책을 반환한다.
2. 4K `interactive`와 1440p `interactive`는 RTVC H.264를 반환한다.
3. AVE specification에는 exact encoder ID와 hardware requirement가 있고
   low-latency rate control key가 없다.
4. RTVC specification에는 low-latency rate control이 있고 AVE encoder ID가
   없다.
5. supported-property plan은 unsupported 키를 설정 목록에서 제외한다.
6. 4K AVE는 Baseline/CAVLC fallback을 만들지 않는다.
7. fallback 이유와 property 결과가 stats contract를 왕복한다.

### 정적 검증

- Swift policy test와 shim build
- Rust control-contract/FFI/control tests
- `cargo fmt --all -- --check`
- `cargo test -p viewer-decoder -p android-viewer`
- desktop typecheck와 관련 UI tests
- `npx -y react-doctor@latest . --verbose` 결과 `100 / 100`
- `git diff --check`

Android wire/decoder 코드는 변경하지 않는 것이 기본이다. 실제 회귀가 발견되지
않는 한 APK 재설치는 필요하지 않으며, Host만 새로 build/install한다.

실기기 성능 run 동안 `adb logcat -v epoch -s LeftcarNative`를 함께 수집한다.
누적 frame counter와 timestamp 차이로 전체 평균 및 1초 구간 FPS를 계산하고,
각 `Rendered` 표본의 `captureAgeMs`로 p50/p95를 계산한다. Host UI의 반올림된
순간 숫자나 한 장의 HUD 화면만으로 성공을 판정하지 않는다.

### 실기기 매트릭스

같은 설치본, 같은 Display 0, 같은 TB710FU, 같은 Wi-Fi에서 수행한다.
고변화 화면은 항상 `Option+0`으로 이동한다.

| Run | 프로필 | 기대 경로 | 목적 |
| --- | --- | --- | --- |
| A | 선명한 화면 4K60 | RTVC H.264 | 같은 바이너리 대조군 |
| B | 동영상 우선 4K60 | AVE H.264 | 4K 처리량 후보 |
| C | 균형 1440p60 | RTVC H.264 | 저지연 회귀 확인 |
| D | AVE H.264 실패 시 별도 실험 | AVE HEVC | 두 번째 codec 후보 |

빠른 반복은 10초 warm-up + 30초 고변화 구간으로 수행한다. 최종 성능 판정은
60초 warm-up + 180초 steady 구간으로 다시 수행하고, latency creep는 10분
soak에서 확인한다.

## 성공 기준

55fps는 AVE 경로를 계속 튜닝할 가치가 있는지 판단하는 후보 게이트일 뿐,
4K60 완료 기준이 아니다. 4K AVE H.264는 다음 조건을 모두 만족해야 한다.

- 실제 source와 decoder가 3840×2160이다.
- Host encode output과 Android render의 180초 전체 평균이 각각 59fps 이상이다.
- 두 단계 모두 1초 rolling window의 p5가 55fps 이상이며 0fps 정지가 없다.
- encode output interval median이 16.7ms 이하이고 p95가 18.5ms 이하이며,
  encode output latency p95가 18.5ms 이하이다.
- capture timestamp→Android decoder age p95가 50ms 이하이며 시간에 따라
  단조 증가하지 않는다.
- Host network queue oldest age가 한 frame budget을 지속적으로 넘지 않는다.
- Android output drop과 decoder input drop이 0이다.
- 30초 고변화 구간의 receiver frame gap 증가는 2 이하이고, gap 뒤 화면이
  두 개의 출력 frame 안에 정상 복구된다.
- 실제 encoder ID가 AVE hardware이고 HighSpeed/speed priority 결과가 Host
  상세 지표에 표시된다.

1440p RTVC 회귀 조건은 Host/Android 55fps 이상, latency age p95 50ms 이하,
decoder input drop 0이다.

AVE가 55fps 후보 게이트를 달성해도 59fps 평균 또는 latency 조건을 실패하면
“4K60 저지연 완료”로 주장하지 않는다. H.264 AVE와 HEVC AVE가 모두 최종
처리량 조건을 실패하면 tiled/multi-session 설계를 별도 SPEC으로 시작한다.

## 변경 범위

예상 변경 파일:

- `native/macos-capture-shim/Sources/CaptureShim.swift`
- `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- `crates/control-contract/src/host.rs`
- `apps/host-desktop/src-tauri/src/ffi.rs`
- `apps/host-desktop/src-tauri/src/control.rs`
- `apps/host-desktop/src-tauri/tests/control_e2e.rs`
- `apps/host-desktop/src/sessionTypes.ts`
- `apps/host-desktop/src/SessionInspector.tsx`
- 관련 desktop UI test
- `docs/11-low-latency-investigation.md`

범위 밖:

- wire protocol 변경
- Android decoder/reassembly 변경
- USB/AOAP 구현
- mid-session encoder hot swap
- 자동 해상도 downgrade
- tiled/multi-session encode
- photon-to-photon 완료 주장
