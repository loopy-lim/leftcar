# 4K60 인코더 드롭 복구와 선택형 실험 프로필 설계

## 상태와 문서 관계

- 상태: 사용자 방향 승인 완료, 문서 검토 대기
- 작성일: 2026-08-28
- 목표 플랫폼: Apple Silicon macOS Host + Android Viewer
- 이 문서는
  `docs/superpowers/specs/2026-08-27-4k-ave-hybrid-low-latency-design.md`의
  단일 세션 후보 정책을 대체한다.
- 기존 H.264 RTVC 실기기 결과는 4K 약 48.55fps, 1440p 약 60fps였다.
  과거 standalone 4K 약 64fps 결과는 VideoToolbox의 dropped callback을
  유효 출력으로 잘못 계산했으므로 acceptance 근거에서 제외한다.

## 목표

3840x2160 해상도를 유지하면서 고변화 화면에서도 캡처, 유효 인코더 출력,
Android 렌더를 지속 60fps에 가깝게 만든다. 테스트마다 선택한 인코더 전략과
실제 적용 결과를 기록해 동일 바이너리에서 재현 가능한 A/B 비교를 지원한다.

구체적인 목표는 다음과 같다.

1. VideoToolbox의 정상적인 `frameDropped` callback을 장애로 오인해 IDR을
   반복 요청하는 복구 피드백 루프를 제거한다.
2. 인코더 제출과 패킷화를 서로 독립적으로 계측하고 스케줄링한다.
3. 4K 해상도를 낮추지 않고 bitrate rate control, per-frame Base QP,
   encoder-owned pixel buffer 입력을 선택해서 비교한다.
4. Host가 지원하는 실험 프로필만 Viewer에 광고하고, 요청 프로필과 실제 적용
   프로필을 Host 상세 화면과 stats에 남긴다.
5. 단일 세션이 4K60을 달성하지 못할 때만 수평/수직 이중 인코더 프로브를
   실행하고, 처리량 게이트를 통과한 분할 방식만 제품 wire로 확장한다.

## 비목표

- 4K 요청을 1440p나 1080p로 자동 축소하지 않는다.
- software encoder를 사용하지 않는다.
- 실행 중인 VideoToolbox session의 rate-control 방식을 hot swap하지 않는다.
- 측정 전에 이중 인코더 wire, Android 이중 decoder, GPU compositor를 만들지
  않는다.
- 짧은 Host FPS 표본만으로 glass-to-glass 또는 장시간 4K60 완료를 주장하지
  않는다.

## 확인된 원인과 제약

### VideoToolbox dropped callback 처리 오류

`VTCompressionSessionEncodeFrame`의 output handler는 `status == noErr`이면서
`sampleBuffer == nil`, `infoFlags`에 `frameDropped`가 설정된 callback을 반환할
수 있다. 이는 rate control 또는 인코더 부하에 따른 프레임 드롭이며 session
출력 장애가 아니다.

현재 코드는 callback의 `infoFlags`를 버리고 `sampleBuffer == nil`을 모두 출력
실패로 처리한다. 일반 프레임 드롭에도 recovery IDR을 요청하므로 고변화
화면에서 다음 피드백 루프가 생길 수 있다.

```text
encoder frame drop
  -> output failure로 오분류
  -> CSD reset + forced IDR
  -> 큰 access unit과 인코더/전송 부하
  -> 추가 frame drop
```

정상 드롭은 별도 카운터로 기록하고 복구를 요청하지 않는다. 단, 이미 네트워크
복구를 위해 명시적으로 요청한 keyframe 자체가 드롭된 경우에만 기존 cooldown
후 복구를 재시도한다.

### 인코더와 패킷화 슬롯 결합

현재 유효한 encoder callback 이후 `completeEncodeSlot()`은 packetization이
끝난 뒤 호출된다. 그 결과 `encodeInFlight`가 VideoToolbox 작업뿐 아니라 AU
변환과 packetization 대기시간까지 포함한다.

output callback에서 generation과 sample 유효성을 확인한 직후 encoder 슬롯을
반환한다. packetization은 별도의 `packetizationInFlight` 카운터와 기존 bounded
queue로 관리한다. 이 변경은 모든 실험 프로필에 항상 적용한다.

### BaseFrameQP 제약

현재 M1 Max H.264 RTVC session은
`kVTCompressionPropertyKey_SupportsBaseFrameQP=true`를 보고한다. Apple SDK
계약상 `kVTEncodeFrameOptionKey_BaseFrameQP`를 사용하면 다음 조건을 지켜야
한다.

- session의 모든 제출 프레임에 Base QP를 지정한다.
- 표준 rate control과 `AverageBitRate`, `DataRateLimits`, `Quality`, QP limit
  속성은 무시된다.
- rate-control 목적의 frame drop은 발생하지 않는다.
- 애플리케이션이 결과 bitrate와 품질 제어 책임을 가진다.

따라서 Base QP는 실행 중에 bitrate 모드와 전환하지 않고 별도 session
프로필로 시작한다.

## 선택 가능한 실험 프로필

wire 값은 `EncoderExperiment` enum으로 고정한다. `auto`를 제외한 명시적
실험은 다른 경로로 자동 fallback하지 않는다.

| ID | Viewer 표시 | 인코더 입력과 rate control | 용도 |
| --- | --- | --- | --- |
| `auto` | 자동 | 현재 검증된 기본 프로필 선택 | 일반 사용 기본값 |
| `rateControl` | 비트레이트 기준선 | direct NV12 + RTVC rate control | 기존 동작 대조군 |
| `adaptiveQp` | 적응형 QP | direct NV12 + 모든 프레임 Base QP | rate-control drop 제거 실험 |
| `encoderPool` | 인코더 버퍼 | encoder-owned pool + RTVC rate control | 입력 surface A/B |

후속 이중 인코더 프로필 ID는 `splitHorizontal`과 `splitVertical`로 예약한다.
초기 구현에서는 Host capability에 포함하지 않으므로 Viewer에 표시되지 않는다.
실캡처 프로브가 해당 방향의 처리량 게이트를 통과하고 제품 wire가 구현된
시점에만 Host가 광고한다.

### `auto` 정책

첫 배포에서 `auto`는 `rateControl`을 적용한다. `adaptiveQp`가 실기기 180초
게이트를 통과하기 전에는 자동 기본값으로 승격하지 않는다. 승격 여부와
무관하게 명시적 `rateControl`은 회귀 대조군으로 유지한다.

### `adaptiveQp` 정책

- session 시작 전에 Base QP 지원을 확인하고, 지원하지 않으면 명시적인 시작
  오류를 반환한다.
- 초기 QP는 32, 허용 범위는 26부터 42까지다.
- 기존 Host 품질 slider 25부터 50은 QP 42부터 26으로 선형 매핑한다. slider가
  낮을수록 높은 QP와 낮은 bitrate를 사용한다.
- 자동 상태에서 1초 window에 encoder drop이 있거나, 유효 출력이 55fps보다
  낮거나, encode submit call p95가 16.67ms보다 크면 QP를 2 높인다.
- 세 개의 연속 1초 window에서 encoder drop이 0이고, 유효 출력이 59fps 이상,
  callback p95가 18.5ms 이하, network oldest age가 16.67ms 이하이면 QP를 1
  낮춘다.
- 모든 제출 프레임에 그 시점의 QP를 넣는다. keyframe에도 생략하지 않는다.
- 실제 QP와 변경 횟수를 stats에 기록한다.

QP 조정은 공간 해상도와 목표 FPS를 바꾸지 않는다. QP가 42에 도달한 뒤에도
유효 출력이 55fps 미만이면 품질을 더 낮추거나 해상도를 바꾸지 않고 해당
실험을 실패로 판정한다.

## 설정과 capability 전달

### Control contract

`StartStreamInput`에 `encoderExperiment`를 추가하고 기본값을 `auto`로 둔다.
구버전 Viewer는 필드를 보내지 않아도 기존처럼 시작된다.

`CatalogView`에는 다음 정보를 가진 `encoderExperiments` 배열을 추가한다.

```text
EncoderExperimentInfo {
  id: string
  label: string
  hint: string
  requiresReconnect: true
}
```

Host는 현재 platform과 shim capability에서 실제 시작 가능한 프로필만
광고한다. Viewer가 광고되지 않은 값을 요청하면 `unsupported encoder
experiment: <id>`로 실패한다.

### Viewer 선택 UI

- Android Viewer의 스트림 품질 선택 아래에 `인코더 실험` 고급 영역을 둔다.
- Host가 두 개 이상의 프로필을 광고하고, 선택 해상도가 4K일 때만 표시한다.
- 기본 선택은 `auto`다.
- 프로필을 바꾸면 다음 연결부터 적용된다는 문구를 표시한다.
- 연결 자동 복구와 USB/Wi-Fi 전환은 원래 선택을 보존해 같은 실험 프로필로
  session을 다시 시작한다.
- 새 React Native UI는 저장소의 Uniwind 설정을 사용하고 조건부 클래스 조합은
  `clsx`와 `tailwind-merge` 또는 기존 공용 helper로 제한한다.

### Host Desktop 표시

Desktop에서 session 설정을 바꾸지는 않는다. 연결 상세 화면에 다음을
표시한다.

- 요청 실험 프로필
- 실제 적용 실험 프로필
- 적용 실패 또는 `auto` 선택 이유
- 현재 Base QP
- encoder frame drop과 valid output FPS
- encode submit call p95와 encoder callback p95
- encoder/packetization in-flight

이 구조는 session 시작 권한을 Viewer에 유지하면서 Desktop에서 결과를 즉시
확인할 수 있게 한다.

### FFI 호환성

macOS shim에 `leftcar_capture_start_v6`를 추가해 기존 v5 인자 뒤에
`encoderExperiment`를 전달한다. Rust FFI는 v6를 우선 사용한다.

- 요청이 `auto`이면 v6가 없는 구형 shim에서 v5로 호환 fallback할 수 있다.
- 명시적 실험이면 v6가 없을 때 실패한다. 다른 프로필을 실행하고 성공으로
  표시하지 않는다.

## 콜백과 계측 데이터 흐름

```text
Viewer profile + encoderExperiment
  -> Host capability validation
  -> start_v6
  -> RTVC session + selected input/rate-control policy
  -> EncodeFrame submit call timing
  -> output callback
       frameDropped -> count + release encoder slot, no ordinary IDR
       valid sample -> release encoder slot -> packetization queue
       OSStatus error -> existing startup fallback or bounded recovery
  -> UDP/FEC/AOAP transport
  -> Android decoder + render feedback
  -> Host stats + Desktop inspector
```

`framesEncoded`처럼 제출 성공을 세는 값과 유효 출력은 분리한다. 성능 판정은
sample buffer가 존재하고 `frameDropped`가 아닌 callback만 사용한다.

## 추가 stats 계약

Swift stats JSON, Rust `StatsInfo`, control response, Desktop `SessionRow`에 다음
필드를 같은 이름으로 전달한다.

- `encoderExperimentRequested`
- `encoderExperimentApplied`
- `encoderExperimentFallbackReason`
- `encoderFrameDrops`
- `encoderFrameDropFps`
- `validEncodeOutputFps`
- `encodeSubmitCallP50Us`
- `encodeSubmitCallP95Us`
- `encoderCallbackP50Us`
- `encoderCallbackP95Us`
- `packetizationInFlight`
- `baseFrameQp`
- `baseFrameQpChanges`

기존 `encodeOutputFps`는 호환성을 위해 유지하되 유효 sample 기준으로 바로잡고,
새 UI는 의미가 명시된 `validEncodeOutputFps`를 우선 표시한다. 문서의 과거
standalone 64fps 수치는 무효 근거로 표시한다.

## 복구 정책

1. 일반 encoder `frameDropped`는 dependency chain 손상으로 간주하지 않고
   IDR이나 CSD 재전송을 요청하지 않는다.
2. 명시적 recovery keyframe callback이 dropped인 경우 recovery gate를 해제하고
   기존 750ms cooldown 뒤 한 번 재요청한다.
3. `VTCompressionSessionEncodeFrame`의 동기 반환 오류와 callback OSStatus 오류는
   기존 startup generation/fallback 규칙을 따른다.
4. UDP AU 손실, network queue overflow, Android decoder의 keyframe 요청은 기존
   네트워크 복구 경로를 유지한다.
5. encoder drop과 network loss 카운터를 합치지 않는다.

## 단계별 구현 경계

### Phase A: 단일 세션 정확성

- callback drop 의미 수정
- encoder와 packetization slot 분리
- 선택형 `auto`, `rateControl`, `adaptiveQp`, `encoderPool`
- control/FFI/stats/Viewer/Desktop 전달
- 4K 실제 화면 A/B

Phase A가 단일 제품 변경 단위다.

### Phase B: Host 전용 실캡처 분할 프로브

Phase A의 세 단일 세션 후보가 최종 4K60 게이트에 실패했을 때 시작한다.
실제 3840x2160 NV12 capture를 다음 두 방식으로 분리한다.

- `splitHorizontal`: 3840x1080 상단/하단 두 session
- `splitVertical`: 1920x2160 좌/우 두 session

프로브는 Android wire를 변경하지 않고 각 session의 valid output, encoder drop,
paired epoch와 callback latency만 측정한다. 난수 프레임이 아니라 `Option+0`으로
실제 고변화 화면을 사용한다.

### Phase C: 검증된 분할 방식의 제품화

Phase B에서 한 방향이 게이트를 통과한 경우에만 다음 별도 설계와 계획으로
진행한다.

- tile ID, epoch, geometry, codec-generation wire
- tile별 CSD/IDR/FEC와 pair-scoped recovery
- Android 이중 `MediaCodec`
- GPU compositor의 complete-epoch presentation
- Host capability 광고와 Viewer 선택 활성화

## 테스트 전략

### TDD와 정적 검증

구현 전에 다음 실패 테스트를 작성한다.

1. `frameDropped` callback은 encoder drop을 증가시키고 recovery IDR을 요청하지
   않는다.
2. recovery keyframe drop만 cooldown 재시도를 예약한다.
3. valid callback은 packetization 실행 전 encoder slot을 반환한다.
4. `auto` 누락 필드는 `rateControl` 적용으로 왕복한다.
5. 명시적 실험은 구형 v5 shim이나 unsupported capability에서 fail closed한다.
6. `adaptiveQp`는 모든 프레임에 QP를 넣고 압력/안정 window에서 2 증가/1 감소
   규칙과 26부터 42 범위를 지킨다.
7. Viewer의 자동 재연결과 USB/Wi-Fi 전환이 `encoderExperiment`를 보존한다.
8. stats의 요청/적용 프로필, drop, timing, QP가 Swift에서 Desktop까지
   왕복한다.

필수 검증 명령은 다음 범주를 모두 포함한다.

- Swift policy/unit tests와 macOS shim build
- Rust control-contract, FFI, control unit/e2e tests
- Viewer Expo TypeScript와 관련 Jest tests
- Host Desktop TypeScript와 UI tests
- 저장소 root에서 `npx -y react-doctor@latest . --verbose` 실행 및 `100 / 100`
- React Doctor 수정 후 repository typecheck와 관련 tests 재실행
- `cargo fmt --all -- --check`
- `git diff --check`

### 실기기 매트릭스

동일한 4K display, Android 기기, Viewer/Host build에서 각 프로필을 비교한다.

| Run | 프로필 | 구간 | 목적 |
| --- | --- | --- | --- |
| A | `rateControl` | 정적 30초 + 고변화 180초 | 기존 기준선 |
| B | `adaptiveQp` | 정적 30초 + 고변화 180초 | rate-control drop 제거 |
| C | `encoderPool` | 정적 30초 + 고변화 180초 | 입력 pool 효과 |
| D | Phase A 최상 프로필 | 고변화 600초 | latency creep와 정지 확인 |

각 run은 실제 적용 프로필, 3840x2160, target 60fps를 먼저 확인한다. Host
순간 HUD가 아니라 누적 valid output, Android render timestamp, drop 변화량을
사용한다.

## 성공 기준

Phase A의 4K60 완료 조건은 다음을 모두 만족하는 것이다.

- 실제 source, encoder, decoder가 3840x2160이다.
- 180초 Host valid encode output과 Android render 평균이 각각 59fps 이상이다.
- 두 단계의 1초 rolling FPS p5가 55fps 이상이고 0fps 정지가 없다.
- encoder frame drop이 steady 180초 구간에서 0이다.
- encode submit call p95와 encoder callback p95가 각각 18.5ms 이하이다.
- capture timestamp부터 Android decoder age p95가 50ms 이하이고 시간에 따라
  증가하지 않는다.
- packetization in-flight와 network oldest age가 지속적으로 증가하지 않는다.
- receiver frame gap 증가는 고변화 180초 동안 2 이하이며 손상 후 두 출력
  frame 안에 복구된다.
- recovery IDR은 encoder의 일반 frame drop 때문에 증가하지 않는다.
- 600초 soak에서 stream 종료나 장기 멈춤이 없다.

Phase B 분할 프로브는 두 session 각각의 valid output 평균 59fps 이상,
paired epoch 평균 59fps 이상, callback p95 18.5ms 이하, encoder drop 0을 모두
만족해야 Phase C 후보가 된다.

## 예상 변경 범위

- `native/macos-capture-shim/Sources/CaptureShim.swift`
- `native/macos-capture-shim/Tests/EncodePolicyTests.swift`
- `crates/control-contract/src/host.rs`
- `apps/host-desktop/src-tauri/src/backend.rs`
- `apps/host-desktop/src-tauri/src/ffi.rs`
- `apps/host-desktop/src-tauri/src/control.rs`
- `apps/host-desktop/src-tauri/tests/control_e2e.rs`
- `apps/host-desktop/src/sessionTypes.ts`
- `apps/host-desktop/src/SessionInspector.tsx`
- `apps/viewer-expo/src/stream-profile.ts`
- `apps/viewer-expo/src/launch-stream.ts`
- `apps/viewer-expo/app/catalog.tsx`
- 관련 Swift, Rust, TypeScript, React 테스트
- `docs/11-low-latency-investigation.md`

Android decoder, UDP/FEC wire, AOAP transport는 Phase A에서 변경하지 않는다.
