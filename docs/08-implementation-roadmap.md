# 24주 구현 로드맵

문서 상태: 실행 가능한 계획안 0.1  
계획 시작점: 빈 제품 저장소, Rustra는 외부 프로젝트  
구현 범위: 이 문서는 코드 구현을 포함하지 않는다. 다른 작업자가 순서대로 실행하기 위한 handoff다.

## 1. 일정 요약

24주는 인력 1-2명이 순차/부분 병렬로 일하는 기준의 **예상 창**이며 납기 약속이 아니다. 기술 gate 실패 시 다음 phase로 넘어가지 않는다.

| 주 | Phase | 결과 |
| --- | --- | --- |
| 1 | P0 Foundation | workspace, ADR, CI, Rustra pin |
| 2-3 | P1 Galaxy XR feasibility | RN multi-instance 4창, Rust NDK decoder |
| 4-5 | P2 Transport bake-off | WebRTC vs QUIC 실기기 선택 |
| 6-8 | P3 macOS single stream | 실제 Mac 창 1개를 Galaxy XR에 표시 |
| 9-11 | P4 Pairing/control/security | QR pairing, source capability, reconnect |
| 12-14 | P5 Multi-window product | Mac source 4개, 품질 allocator, 장애 격리 |
| 15-16 | P6 Performance/resilience | latency, soak, lifecycle, diagnostics |
| 17-19 | P7 Windows Host | WGC + Media Foundation 실제 Windows 경로 |
| 20-21 | P8 Hardening/packaging | 보안 검토, signed/internal packages |
| 22-23 | P9 Beta validation | 실사용, SLO, 회귀 수정 |
| 24 | P10 Handoff/release decision | 문서, 증거, go/no-go |

## 2. 공통 작업 카드 규칙

모든 작업은 다음 필드를 가진다.

- 선행 조건: 시작 전에 존재해야 하는 코드/결정/장치
- 대상: 주로 바꿀 파일/모듈
- Red: 먼저 실패해야 하는 test 또는 판정 script
- 산출물: 작업이 만드는 것
- 검증: 실행할 명령과 실험
- 수용 기준: 완료의 객관적 조건
- fallback: 실패 시 허용된 다음 행동
- 인계: 다음 작업자가 알아야 할 정보

구현자는 실제 scaffold가 생기면 명령 이름을 package script로 고정하고 이 문서에 반영한다.

## 3. Gate

### G0 계획 승인

- 원격 입력은 Host의 스트림별 명시 승인과 별도 네이티브 데이터 경로만 사용한다.
- Home Space multi-instance가 기본 UX다.
- TypeScript/Rust 중심, 최소 Kotlin shim이다.
- macOS 먼저, Windows 두 번째다.
- v1은 로컬 LAN, H.264, window/display capture다.

### G1 Galaxy XR vertical slice

- 같은 Leftcar APK의 stream window 4개를 동시에 열 수 있다.
- 네 창 모두 비초점 상태에서도 synthetic content가 갱신된다.
- Rust NDK `AMediaCodec` 4개가 실제 Surface로 H.264 1080p30을 10분 재생한다.
- Rustra RN 실제 경로 `addNumbers -> 42`가 동작한다.
- Kotlin shim에 domain/network/codec policy가 없다.

G1 실패 시 macOS capture 개발을 시작하지 않는다.

### G2 transport 선택

- WebRTC/QUIC 후보의 동일 조건 결과가 있다.
- 선택 후보가 보안, 복구, S1 p95, 다중 stream 기본 기준을 만족한다.
- ADR-0004가 선택 결과로 갱신된다.

### G3 macOS 단일 stream

- 실제 앱 창 -> ScreenCaptureKit -> VideoToolbox -> 선택 transport -> Rust AMediaCodec -> Galaxy XR Surface가 연결된다.
- S1 10분과 기본 glass-to-glass 결과가 있다.

### G4 macOS v1 candidate

- 네 앱 창을 네 Home Space 창에 표시한다.
- F4 profile, 장애 격리, 60분 soak를 통과한다.
- pairing/source revoke/stop all을 통과한다.

### G5 Windows candidate

- 실제 Windows 앱 창의 E6 경로와 lifecycle stress를 통과한다.

### G6 beta/release

- 필수 NFR, security checklist, packaging, rollback, documentation이 통과한다.

## 4. P0 Foundation, 1주

### H00 Workspace bootstrap

- 선행 조건: G0 초안 합의
- 대상: `Cargo.toml`, `package.json`, `pnpm-workspace.yaml`, `rust-toolchain.toml`, `apps/`, `crates/`, `packages/`
- Red: `tools/check-workspace-boundaries`가 필요한 디렉터리와 script 부재로 실패
- 산출물: Rust workspace, pnpm workspace, React Native bare app, Tauri host shell placeholder
- 검증:
  - `cargo metadata --no-deps`
  - `pnpm install --frozen-lockfile`
  - `pnpm typecheck`
  - Android debug compile
- 수용 기준: 빈 shell build, lockfile commit, toolchain/version 명시, generated/target 제외 정책
- fallback: RN/Tauri 생성 CLI 결과를 별도 temp에서 검토 후 필요한 파일만 이동
- 인계: 정확한 Rust, Node, pnpm, RN, Android Gradle Plugin, NDK version 기록

### H01 Architecture guardrails

- 선행 조건: H00
- 대상: `tools/architecture-check/`, root test scripts
- Red:
  - domain crate에 임시 platform dependency를 넣으면 test 실패
  - Kotlin shim에 `java.net`, codec policy symbol을 넣으면 test 실패
- 산출물: dependency graph 검사, Kotlin import allowlist, generated code drift 검사
- 검증: `pnpm test:architecture`, `cargo test -p architecture-tests`
- 수용 기준: ADR-0002 dependency rule을 CI에서 강제
- fallback: custom parser보다 `cargo metadata`, `rg`, ESLint restricted imports 조합 사용
- 인계: allowlist 변경에는 ADR review가 필요하다고 CI message에 표시

### H02 Rustra compatibility pin

- 선행 조건: H00
- 대상: `crates/control-contract/`, `packages/control-generated/`, dependency manifests
- Red:
  - Rust command `add_numbers` contract test
  - generated TS `addNumbers` type test
  - runtime contract hash mismatch test
- 산출물: 검증된 Rustra commit/tag pin, generated package, fake engine
- 검증:
  - Rustra upstream 자체 gate
  - `cargo test -p control-contract`
  - `pnpm test:contract`
  - clean regeneration diff
- 수용 기준: Host test adapter와 RN adapter 계획 경로 모두 `20 + 22 = 42`; pin과 source URL 기록
- fallback: package registry가 불완전하면 vetted git commit과 local generated runtime을 명시적으로 pin
- 인계: 2026-08-17 조사 시 public `main`은 `11ff71f5...`, 로컬 `feat/event-sink`은 그보다 앞서 있었으므로 local branch를 암묵적으로 참조하지 말 것

### H03 CI skeleton

- 선행 조건: H00-H02
- 대상: `.github/workflows/`, root scripts
- Red: intentional generated drift와 failing Rust test로 CI failure 확인
- 산출물: PR fast lane, main lane skeleton, artifact naming
- 검증: local CI equivalent script와 first remote run
- 수용 기준: Rust/TS/Android compile/contract/architecture checks가 별도 job으로 원인 표시
- fallback: device job은 manual placeholder로 두되 required evidence 문서를 연결
- 인계: device test 부재를 green CI가 숨기지 않게 job summary에 E-level 표시

## 5. P1 Galaxy XR feasibility, 2-3주

### H04 Multi-instance TypeScript UI shell

- 선행 조건: H00, Galaxy XR 접근
- 대상:
  - `apps/viewer-android/src/HubApp.tsx`
  - `apps/viewer-android/src/StreamApp.tsx`
  - `apps/viewer-android/specs/StreamWindowLauncherSpec.ts`
- Red: mocked launcher가 source마다 unique launch handle/document URI를 받는 TS test
- 산출물: Hub source list와 source별 독립 stream app root
- 검증: Jest/React Native Testing Library, typecheck
- 수용 기준: source A/B/C/D가 unique `instance_id`로 launch 요청됨; same source 중복 정책 test
- fallback: launch behavior는 native fake로 유지하고 H05에서 실기기 연결
- 인계: TS state에는 key/token을 넣지 않음

### H05 Thin Android multi-instance shim

- 선행 조건: H04
- 대상: `android/.../shim/HubActivity.kt`, `StreamActivity.kt`, launcher module, manifest
- Red: instrumentation test가 4 unique task/Activity instance를 기대하고 실패
- 산출물:
  - `PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI=true`
  - document/multiple task launch
  - RN initial props에 opaque launch handle 전달
- 검증: emulator instrumentation 후 Galaxy XR 수동/자동 확인
- 수용 기준: Galaxy XR Home Space에서 같은 APK 창 4개 이동/resize/close; Hub close 후 stream 창 유지
- fallback:
  1. launchMode/document URI/task affinity 수정
  2. RN shared host/surface registration 수정
  3. 두 번 실패 뒤 G1 review. Full Kotlin 전환은 자동 fallback 아님
- 인계: task dump, manifest, OS build, screen recording을 redacted artifact로 보관

### H06 RemoteSurface Fabric spec and shim

- 선행 조건: H04-H05
- 대상:
  - `specs/RemoteSurfaceNativeComponent.ts`
  - generated Codegen output
  - allowlisted Kotlin SurfaceView manager
  - Rust JNI surface registry
- Red: attach/detach callback order test와 instance crossing negative test
- 산출물: TS `<RemoteSurface>`가 `instance_id`별 native Surface를 Rust에 전달
- 검증:
  - RN Codegen task
  - fake native instrumentation
  - 네 창에 다른 solid color/counter surface
- 수용 기준: 4개 Surface가 섞이지 않고 resize/destroy/recreate; `ANativeWindow` ref 균형
- fallback: SurfaceView 문제 시 TextureView spike를 별도 commit에서 비교
- 인계: generated code 수정 금지, TS spec과 shim API snapshot 첨부

### H07 Rust NDK AMediaCodec single decoder

- 선행 조건: H06, NDK pin
- 대상: `crates/viewer-core`, `native/android-viewer`, NDK bindings
- Red:
  - fake NDK function table lifecycle test
  - delta-before-keyframe drop test
  - Surface 없이 configure 금지 test
- 산출물: H.264 Annex B/AVCC 명시적 parser, codec config, AMediaCodec decoder, ANativeWindow output
- 검증: Galaxy XR에서 synthetic 1080p60 10분
- 수용 기준: hardware codec identity 기록, frame visible, no software fallback without state, repeated start/stop 100회
- fallback: Java MediaCodec을 policy 없이 얇게 호출하는 비교 spike는 허용하되 기본 언어 결정은 재승인
- 인계: NDK API level, codec name, low-latency support, native ownership table

### H08 Four-decoder and multi-resume proof

- 선행 조건: H05-H07
- 대상: viewer diagnostic/test harness
- Red: 4 stream instance model test와 resource exhausted behavior
- 산출물: golden H.264 4개를 네 stream window에 매핑하는 lab mode
- 검증: Galaxy XR 1080p30 x4 10분, focus rotation, resize 100회
- 수용 기준: 네 counter 진행, 다른 창 닫아도 유지, crash/ANR/decoder stall 없음, memory slope 없음
- fallback: 1440p60 x1 + 720p15 x3 또는 decoder instance count 축소를 제품 변경안으로 제시
- 인계: G1 result report 작성

### H09 Rustra RN real adapter proof

- 선행 조건: H02, H04
- 대상: RN Rustra native module integration, `HubApp.tsx`
- Red: mock은 42지만 real adapter test가 native wiring 전 실패
- 산출물: Galaxy XR RN UI에서 Rust `add_numbers` 호출
- 검증: emulator와 Galaxy XR에서 `20 + 22 = 42`, contract hash mismatch negative test
- 수용 기준: real JSI/FFI/native route 증거; string mock/JS 계산 금지
- fallback: Rustra RN adapter integration defect는 Rustra에 최소 reproduction과 fix plan을 먼저 작성
- 인계: Rustra pin, generated hash, native symbols, log

### G1 review task

- H05-H09 결과를 한 report에 모은다.
- 모두 통과해야 P2로 간다.
- 일부 profile만 실패하면 scope/SLO 변경을 제품 책임자가 승인한다.

## 6. P2 Transport bake-off, 4-5주

### H10 Media/network model

- 선행 조건: G1
- 대상: `crates/media-model`, `network-protocol`, `transport-api`
- Red: epoch/order/bounded queue/property tests
- 산출물: `EncodedFrame`, fragment header, transport trait, simulated link
- 검증: Rust unit/proptest/fuzz smoke
- 수용 기준: old epoch/drop, size cap, incomplete timeout, key/config dependency 명시
- fallback: 실제 codec bytes 대신 synthetic payload로 model을 먼저 고정
- 인계: wire는 아직 제품 확정이 아니라고 표시

### H11 QUIC prototype

- 선행 조건: H10
- 대상: `transport-quic`, Android Rust build
- Red: simulated loss/reorder/outage tests
- 산출물: authenticated test session, reliable control, datagram video, IDR feedback
- 검증: pre-encoded stream Mac -> Galaxy XR, network profiles
- 수용 기준: memory bound, reconnect, 1/2/4 stream metric, no plaintext
- fallback: FEC 없이 먼저 측정; 목표 미달 원인을 한 번만 개선한 뒤 결과 고정
- 인계: custom packetization와 congestion assumptions 문서

### H12 WebRTC prototype

- 선행 조건: H10
- 대상: `transport-webrtc`, native build integration
- Red: same transport conformance suite
- 산출물: paired test signaling, video track/source mapping, keyframe feedback
- 검증: H11과 동일 access units/profile/network
- 수용 기준: 같은 metric schema와 decoder path 사용
- fallback: prebuilt/library 선택이 reproducible하지 않으면 build/package risk로 기록
- 인계: libwebrtc version, build flags, hidden buffer 설정 가능 범위

### H13 Bake-off runner

- 선행 조건: H11-H12
- 대상: `tools/benchmark-runner`, benchmark template
- Red: incomplete manifest를 거부하는 schema test
- 산출물: 한 명령으로 후보별 동일 run, JSON/CSV/histogram
- 검증: clean/normal/busy/bad/outage, W1/W4, 10분/60분 일부
- 수용 기준: 재현 seed, environment, raw/redacted metrics 저장
- fallback: true glass-to-glass 장비가 없으면 software stage 결과와 한계를 분리
- 인계: [성능 문서](06-benchmark-device-validation.md) 형식 준수

### H14 Transport decision

- 선행 조건: H13
- 대상: ADR-0004, dependencies, transport selection feature
- Red: selected transport 없으면 product build가 실패하는 config test
- 산출물: 승자, 탈락 이유, v1 supported mode
- 검증: G2 checklist
- 수용 기준: 보안/correctness/S1/복구/4 stream 결과와 복잡도 판정
- fallback: 둘 다 실패하면 P3 중단. profile 또는 SLO를 다시 승인하고 bake-off 반복
- 인계: 제품 build에서 loser prototype을 기본 dependency로 남기지 않음

## 7. P3 macOS single stream, 6-8주

### H15 Host Tauri + Rustra shell

- 선행 조건: G2, H02
- 대상: `apps/host-desktop`, host control contract
- Red: Tauri real adapter `addNumbers -> 42`, contract mismatch
- 산출물: menu bar/tray shell, permission/source/pairing placeholder UI
- 검증: `cargo test`, TS test, Tauri dev/build
- 수용 기준: 실제 WebView -> rustra_dispatch -> Rust 42; app close cleanup
- fallback: Host UI shell만 native로 바꾸지 말고 Tauri integration defect 최소화
- 인계: local URL/process 증거와 package는 별도 수준으로 보고

### H16 ScreenCaptureKit permission/picker

- 선행 조건: H15, macOS 15+ Mac
- 대상: `macos-capture`, Swift/C ABI shim, Host UI
- Red: FakeCapture permission/source state tests
- 산출물: system content picker, window/display source descriptor, revoke
- 검증: 실제 권한 deny/grant/restart/revoke, 앱 창/디스플레이 선택
- 수용 기준: 사용자가 고른 source만 callback; Host indicator; window title log redaction
- fallback: custom picker보다 official `SCContentSharingPicker` 유지
- 인계: OS version별 권한 동작과 native handle lifetime

### H17 ScreenCaptureKit frame adapter

- 선행 조건: H16
- 대상: capture frame port
- Red: invalid/incomplete sample, resize, burst, source unavailable tests
- 산출물: CMSampleBuffer/IOSurface lifetime-safe bounded sink
- 검증: synthetic window 1080p60/1440p60, callback allocation/copy metric
- 수용 기준: capture callback block 없음, latest-frame policy, resize epoch
- fallback: 첫 구현 한 번의 GPU/CPU copy 허용하되 metric과 제거 task 생성
- 인계: pixel format/color space/cursor behavior

### H18 VideoToolbox H.264 encoder

- 선행 조건: H17
- 대상: `macos-encode`
- Red: config/key/delta sequence, no frame reorder, restart epoch tests
- 산출물: realtime hardware H.264 access units, IDR request, bitrate/profile update
- 검증: codec identity, 1/2/4 session capability exploration
- 수용 기준: hardware use confirmed, 1080p60 stable, queue bound, SPS/PPS recoverable
- fallback: software encoder는 lab fallback일 뿐 product silent fallback 금지
- 인계: properties 성공/실패와 실제 output profile

### H19 Loopback media vertical slice

- 선행 조건: H10, H17-H18
- 대상: Host pipeline integration and test decoder
- Red: source close/resize/backpressure integration tests
- 산출물: actual capture -> encode -> selected transport loopback -> frame digest/decoder
- 검증: same Mac loopback 10분
- 수용 기준: epoch/reconfigure/restart와 bounded memory
- fallback: network를 loopback으로 고정하여 capture/encode 문제 분리
- 인계: per-stage timing baseline

### H20 Mac to Galaxy XR single stream

- 선행 조건: H07, H14, H19
- 대상: Host/Viewer session integration
- Red: end-to-end synthetic identity test, source authorization stub
- 산출물: 실제 Mac 앱 창 한 개가 Galaxy XR stream window 한 개에 표시
- 검증: S1 10분, resize, close, Wi-Fi outage
- 수용 기준: E6 증거, visible text/motion, reconnect, cleanup
- fallback: capture replay와 live capture를 비교해 병목 layer 격리
- 인계: G3 report, exact build/device/network

### H21 First glass-to-glass baseline

- 선행 조건: H20
- 대상: latency tool/artifact
- Red: manifest missing fields fail
- 산출물: 200 sample baseline과 stage trace
- 검증: [측정 절차](06-benchmark-device-validation.md)
- 수용 기준: 결과를 목표/관측으로 구분; 미달도 기록
- fallback: 고속 카메라 불가 시 software estimate로 G3 기능은 통과 가능하나 G6 성능은 미통과
- 인계: 우선 병목 1-2개만 지정

## 8. P4 Pairing/control/security, 9-11주

### H22 Device identity secure storage

- 선행 조건: G3
- 대상: `session`, platform secure store adapters
- Red: key export 금지, reinstall/reset/revoke tests
- 산출물: Host/Viewer long-term identity와 opaque handle
- 검증: Mac Keychain, Android Keystore physical tests
- 수용 기준: TS/Kotlin에 private bytes 없음; backup 복제 정책 확인
- fallback: dev in-memory key는 debug only compile flag
- 인계: algorithm/library/security review notes

### H23 QR pairing

- 선행 조건: H22
- 대상: Host pairing commands, Viewer scanner/consumer UI
- Red: expiry/replay/MITM/concurrent offer tests
- 산출물: 2분 single-use QR, Host confirmation, device record
- 검증: success/reject/expire/replay on physical devices
- 수용 기준: QR만으로 자동 승인 안 됨; fingerprint binding
- fallback: camera scanner 이슈 시 manual transfer는 dev-only, security 약화 금지
- 인계: redacted protocol transcript

### H24 Authenticated session/revocation

- 선행 조건: H23
- 대상: selected transport auth layer
- Red: unpaired/revoked/downgrade/replay tests
- 산출물: mutual authenticated reconnect와 revoke
- 검증: packet capture, active revoke
- 수용 기준: revoke가 열린 모든 stream을 중단; plaintext 없음
- fallback: auth를 signaling/UI trust로 대체하지 않음
- 인계: threat mapping T-01-T-05

### H25 Source capability

- 선행 조건: H16, H24
- 대상: Host source registry, network protocol
- Red: guessed/stale/unapproved source requests
- 산출물: device/session/source/revision bound capability
- 검증: negative integration and physical source revoke
- 수용 기준: approved source만 start; source revoke가 다른 source를 유지
- fallback: v1은 Host당 single paired Viewer로 scope 축소 가능하나 source auth는 제거 불가
- 인계: catalog revision semantics

### H26 Rustra product contracts

- 선행 조건: H15, H23-H25
- 대상: [제어 계약](04-rustra-control-contracts.md)의 command/event
- Red: contract tests, mock UI component tests
- 산출물: Host/Viewer generated TS client, typed error/recovery
- 검증: clean generation, runtime hash, actual Tauri/RN flows
- 수용 기준: frame type/input command 부재 architecture test
- fallback: event push 문제 시 snapshot polling을 낮은 빈도로 사용, media는 금지
- 인계: stable error table과 UI copy

### H27 Version/reconnect state machine

- 선행 조건: H24-H26
- 대상: session/network protocol
- Red: version mismatch, 5s outage, stale epoch, duplicate request tests
- 산출물: negotiate, backoff, resume/restart, user state
- 검증: simulated and physical outage
- 수용 기준: 1초 목표 복구 또는 명확 degraded, old frame 없음
- fallback: seamless resume 대신 clean reconnect + IDR를 우선
- 인계: compatibility matrix

## 9. P5 Multi-window product, 12-14주

### H28 Four-source Host multiplex

- 선행 조건: G3, H25
- 대상: host-core scheduler, transport mapping
- Red: source isolation/fairness/backpressure tests
- 산출물: source 4 capture/encode/channel
- 검증: synthetic then real Mac windows
- 수용 기준: source ID 혼선 없음, 한 source stop/resize가 다른 source 중단 안 함
- fallback: shared encoder 최적화보다 독립 pipeline correctness 우선
- 인계: CPU/GPU/pixel budget baseline

### H29 Stream window lease integration

- 선행 조건: H08, H27-H28
- 대상: Viewer lease/task registry
- Red: lifecycle permutation, Hub close, process restore tests
- 산출물: task와 Host source lifecycle 연결
- 검증: 4 windows open/close/restore physical
- 수용 기준: orphan lease/capture 없음, same source policy 정확
- fallback: process restoration에서 자동 재생 대신 안전한 “다시 연결” UI 허용
- 인계: task dump and lease trace

### H30 Window-aware quality allocator

- 선행 조건: H28-H29
- 대상: domain quality policy, window state bridge
- Red: budget/fairness/hysteresis property tests
- 산출물: focus/normal/background/suspend profile
- 검증: focus rotate, resize, thermal signals
- 수용 기준: thrash 없음, visible minimum, total budget 상한
- fallback: automatic policy가 불안정하면 explicit per-window profiles로 beta
- 인계: default weights와 observed profile transition

### H31 Source resize/reconfigure

- 선행 조건: H28-H30
- 대상: capture/encoder/protocol/decoder epoch
- Red: late old epoch, Surface recreate, config before IDR tests
- 산출물: app window resize end-to-end
- 검증: 50 source resize + 100 Viewer window resize
- 수용 기준: stretched/wrong source/dead decoder 없음
- fallback: resize debounce와 fixed output resolution
- 인계: SPS/PPS/epoch trace

### H32 Failure isolation

- 선행 조건: H28-H31
- 대상: error scopes/watchdogs
- Red: capture fail, encoder fail, transport subchannel fail, decoder fail injection
- 산출물: window-local error and retry
- 검증: physical source close and decoder resource exhaustion
- 수용 기준: 다른 세 stream 계속 재생
- fallback: session-wide reconnect는 auth/transport-wide failure만 허용
- 인계: incident/error code mapping

### H33 G4 multi-window E6

- 선행 조건: H28-H32
- 대상: E2E scenario/runbook
- 산출물: Mac 앱 창 4개 -> Galaxy XR 창 4개
- 검증: M4/F4 30분, focus/close/resize/outage
- 수용 기준: 기능, isolation, identity, no input, secure transport
- fallback: F4 adaptive profile로 resource 맞춤; W4 자체 실패는 product review
- 인계: G4 interim report

## 10. P6 Performance/resilience, 15-16주

### H34 Queue/copy optimization

- 선행 조건: H33
- 대상: measured top bottleneck only
- Red: allocation/copy/queue invariant benchmark
- 산출물: latest-frame path, zero/low-copy improvement
- 검증: before/after same run
- 수용 기준: p95 또는 resource 개선, correctness 유지
- fallback: 개선 없는 복잡한 path revert
- 인계: flame/trace and statistical comparison

### H35 Latency and network tuning

- 선행 조건: H34
- 대상: encoder/transport/decoder tuning
- Red: regression benchmark profile
- 산출물: no reordering, IDR pacing, jitter/drop policy
- 검증: network matrix and glass-to-glass
- 수용 기준: NFR-001 or documented blocker/re-scope
- fallback: resolution/FPS profile 조정 후 재승인
- 인계: each knob effect table

### H36 Lifecycle/soak hardening

- 선행 조건: H33
- 대상: shutdown/reconnect/resource ownership
- Red: 100x/60m stress scenario
- 산출물: idempotent teardown, memory/latency stability
- 검증: 60분 S1/F4, sleep/wake, process kill
- 수용 기준: NFR-002/006/008, no orphan capture
- fallback: unsupported lifecycle condition을 사용자 UI로 명시하고 safe stop
- 인계: heap/handle/codec counts timeline

### H37 Diagnostics

- 선행 조건: H26, H33
- 대상: `diagnostics`, Host/Viewer UI
- Red: sensitive string redaction property tests
- 산출물: 1Hz overlay, redacted export, run manifest
- 검증: export review and schema tests
- 수용 기준: useful stage/error metrics, no title/path/token/IP/frame
- fallback: field denylist가 아니라 allowlist로 축소
- 인계: artifact retention policy

### H38 Recovery UX

- 선행 조건: H32, H37
- 대상: TS Hub/Stream UI
- Red: every stable error has recovery component/action test
- 산출물: pairing, permission, source gone, reconnect, decoder exhausted UI
- 검증: RN component tests and Galaxy XR human pass
- 수용 기준: raw error만 보이는 상태 없음; 다른 창을 가리지 않음
- fallback: action 불가능한 오류는 safe close/diagnostics
- 인계: Korean/English copy source

## 11. P7 Windows Host, 17-19주

### H39 Windows shell/permission baseline

- 선행 조건: G4
- 대상: same Tauri Host, Windows config
- Red: platform adapter selection and Rustra real-adapter proof (H15 `addNumbers -> 42`)
- 산출물: Windows package/dev shell, source permission UI
- 검증: physical Windows build/run
- 수용 기준: mock이 아닌 real Tauri Rust command; no admin requirement
- fallback: packaging과 runtime proof 분리
- 인계: OS/GPU/build toolchain

### H40 Windows.Graphics.Capture adapter

- 선행 조건: H39
- 대상: `windows-capture`
- Red: fake frame pool resize/device lost/source close tests
- 산출물: system picker, window/display capture, D3D11 texture
- 검증: physical Windows app windows 1/4
- 수용 기준: yellow border/system consent, source isolation, resize
- fallback: Desktop Duplication은 display-only fallback
- 인계: minimized/protected/fullscreen behavior matrix

### H41 Media Foundation encoder

- 선행 조건: H40
- 대상: `windows-encode`
- Red: config/key/delta/restart and hardware fallback tests
- 산출물: low-latency H.264 hardware MFT path
- 검증: codec identity, D3D11 path, 1080p60/4x30
- 수용 기준: no silent software, no B-frame latency, recoverable config
- fallback: specific GPU unsupported state와 supported matrix 표시
- 인계: adapter/GPU/driver results

### H42 Windows to Galaxy XR E6

- 선행 조건: H41, existing session/transport/viewer
- 대상: end-to-end integration
- Red: platform parity scenario
- 산출물: real Windows window(s) in Home Space
- 검증: W1/W4, outage, resize, source close
- 수용 기준: G5 functionality and isolation
- fallback: v1 Windows profile를 lower resolution로 별도 선언 가능
- 인계: Mac 대비 metrics

### H43 Windows device-loss/soak

- 선행 조건: H42
- 대상: capture/encoder recovery
- Red: simulated device lost and resize storm
- 산출물: adapter recreation and user-visible error
- 검증: display mode change, sleep/wake, 60분
- 수용 기준: crash 없음, 다른 source 격리, handle leak 없음
- fallback: safe session restart with explicit state
- 인계: driver-specific limitations

## 12. P8 Hardening/packaging, 20-21주

### H44 Security review/fuzz

- 선행 조건: G4/G5 code freeze candidate
- 대상: auth/protocol/JNI/diagnostics
- Red: threat checklist gaps and fuzz corpus
- 산출물: closed findings, unsafe inventory, SBOM
- 검증: extended fuzz/sanitizer/negative physical tests
- 수용 기준: [보안 체크리스트](07-security-privacy.md) 통과
- fallback: unresolved high severity blocks beta
- 인계: finding ID, fix, regression test

### H45 Android internal package

- 선행 조건: H44
- 대상: signing, manifest, release config
- Red: release lint checks debug endpoint/exported components
- 산출물: signed internal AAB/APK, install runbook
- 검증: clean Galaxy XR install/update/rollback/data reset
- 수용 기준: multi-instance, Rustra, NDK libs in release; debug flags absent
- fallback: internal sideload first, Play track later
- 인계: checksum, version, ABI, symbols archive

### H46 macOS package

- 선행 조건: G4, H44
- 대상: signing/notarization/permissions
- Red: clean machine permission/install/update test plan
- 산출물: signed/notarized candidate
- 검증: separate Mac install, screen recording flow, uninstall/reinstall
- 수용 기준: package is not called E6 without real stream smoke
- fallback: internal signed build before updater
- 인계: entitlements and permission behavior

### H47 Windows package

- 선행 조건: G5, H44
- 대상: signing/installer/firewall/runtime deps
- Red: clean VM/PC install test
- 산출물: signed/internal installer and dependency manifest
- 검증: clean physical Windows install and stream smoke
- 수용 기준: no admin unless installer requires, clear firewall prompt
- fallback: portable internal artifact with explicit limitations
- 인계: hashes and runtime dependency proof

## 13. P9 Beta validation, 22-23주

### H48 Internal beta protocol

- 선행 조건: H45-H47
- 대상: beta guide, feedback schema
- Red: missing device/build/network info feedback is rejected/incomplete
- 산출물: install, pair, source, recover, diagnostics guide
- 검증: fresh tester follows without developer shell
- 수용 기준: no credentials/screens in feedback; E-level clear
- fallback: observed session with tester once, then guide revision
- 인계: known issues and supported profiles

### H49 Real workflow soak

- 선행 조건: H48
- 대상: Mac developer workflow, Windows monitoring workflow
- 산출물: 5 sessions x 60m minimum across profiles
- 검증: latency/thermal/reconnect/human text quality
- 수용 기준: no critical security/data/lifecycle bug; NFR results
- fallback: narrow supported profile rather than hide failures
- 인계: issue list prioritized by user impact

### H50 Regression closure

- 선행 조건: H49
- 대상: beta blocking defects only
- Red: each defect reproduction test
- 산출물: fixes and regression suite
- 검증: original environment and full relevant gate
- 수용 기준: no untested hotfix, no scope expansion
- fallback: known limitation with safe behavior and release block decision
- 인계: fix-to-test mapping

### H51 Final SLO run

- 선행 조건: H50
- 대상: benchmark artifacts
- 산출물: S1/F4 Mac, selected Windows profiles, 60m/optical/network results
- 검증: device lab lane
- 수용 기준: required NFR or explicit no-go
- fallback: no marketing extrapolation from partial profile
- 인계: immutable release candidate commit and artifacts

## 14. P10 Handoff/release decision, 24주

### H52 Documentation reconciliation

- 선행 조건: H51
- 대상: all docs, ADR status, runbooks, API docs
- Red: link checker, stale placeholder/search tests
- 산출물: architecture as built, supported matrix, operator/developer guide
- 검증: fresh contributor dry run
- 수용 기준: no plan claim presented as implemented evidence
- fallback: remaining gap explicitly marked
- 인계: next 90-day backlog

### H53 Release go/no-go

- 선행 조건: H44-H52
- 검토 자료:
  - security review
  - G1-G5 reports
  - final SLO
  - package install/rollback
  - known issues
- Go 기준: 필수 checklist 모두 통과, high severity 없음, support boundary 문서화
- No-go: security, capture persistence, W4 핵심, NFR-001/002, package recovery 중 미해결 필수 항목
- 산출물: signed decision record
- 인계: release tag/commit 또는 next corrective phase

## 15. 병렬화 가능 범위

안전한 병렬:

- H04 TS UI와 H07 Rust fake decoder foundation 일부
- H11 QUIC과 H12 WebRTC는 H10 contract가 고정된 뒤
- Host TS UI와 platform adapter fake tests
- Windows work는 G4 뒤 Mac hardening 일부와 병렬
- docs/security review와 performance tooling

병렬화 금지:

- H11/H12가 서로 다른 media/metric contract를 임의로 만드는 것
- capture와 encoder가 native buffer ownership 없이 독립 구현되는 것
- Rustra contract와 handwritten TS model이 별도로 진화하는 것
- G1 전에 macOS/Windows 전체 구현을 시작하는 것
- G2 전에 두 transport를 제품 코드에 영구 지원하는 것

## 16. 작업 중단/재설계 조건

즉시 중단:

- Galaxy XR multi-instance 4창이 공개 Android API로 재현되지 않음
- non-focused Home Space 창의 지속 업데이트가 시스템 정책상 불가능
- Rust NDK decoder 4개가 최소 adaptive profile에서도 불안정
- selected transport의 인증/암호화가 pairing identity에 bind되지 않음
- session 종료 뒤 capture가 남음

재설계 선택지 순서:

1. profile/동시 stream 수 조정
2. Overview 단일 창 fallback
3. SurfaceView/TextureView/native compositor 비교
4. React Native host/shim 구조 수정
5. 명시적 승인 뒤 Jetpack XR SpatialPanel 또는 다른 shell 조사

완전한 Kotlin 제품 재작성은 기본 fallback이 아니다.

## 17. 주간 보고 형식

```markdown
# Week N

## Outcome
- 완료한 사용자 관찰 가능 결과

## Evidence
- E-level
- command/run ID/artifact

## Metrics
- profile/device/network/result

## Failed hypotheses
- 가설, 결과, 영향

## Decisions
- ADR/change

## Next atomic tasks
- ID, owner, prerequisite

## Risks/blockers
- 필요한 권한/장치/결정
```

## 18. 완료 이후 90일 후보

v1 뒤에만 검토한다.

- Linux PipeWire Host
- HEVC/AV1 adaptive codec
- Overview mode
- optional Full Space SpatialPanel
- virtual display driver feasibility
- remote network/relay
- team pairing/device management
- audio

원격 입력의 실기기 지연·키보드 레이아웃·다중 모니터 좌표 검증은 별도 장치 게이트로 계속 추적한다.
