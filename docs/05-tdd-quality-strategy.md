# TDD와 품질 전략

문서 상태: 제안안 0.1  
적용 범위: 모든 제품 코드, native shim, protocol, 성능 회귀  
원칙: 테스트 통과와 실기기 동작은 서로 다른 증거다.

## 1. 목표

Leftcar의 TDD는 단순한 unit test 비율 목표가 아니다. 다음 실패를 구현 전에 구체적인 executable contract로 만든다.

- 잘못된 source가 다른 stream window에 표시됨
- 한 source의 backlog가 다른 source 지연을 키움
- 창 resize/복원 중 decoder와 Surface lifetime이 꼬임
- 네트워크 손실 뒤 오래된 frame이 늦게 표시됨
- 페어링되지 않은 Viewer가 frame을 받음
- Rust/TypeScript 계약이 drift함
- Kotlin shim에 business logic이 자라남
- 테스트에서는 빠르지만 실제 Galaxy XR에서 latency가 누적됨

## 2. TDD 기본 루프

모든 작업 카드에서 다음 순서를 기록한다.

1. Red
   - 한 가지 관찰 가능한 동작을 이름으로 쓴다.
   - 실패하는 가장 작은 자동 테스트를 추가한다.
   - platform 실험이면 실패 baseline 또는 재현 script를 먼저 만든다.
2. Green
   - 테스트를 통과하는 최소 구현을 작성한다.
   - 범위를 벗어난 abstraction과 최적화를 추가하지 않는다.
3. Refactor
   - 중복과 책임 경계를 정리한다.
   - public contract와 metric 이름을 안정화한다.
4. Local gate
   - 변경 crate/package의 빠른 테스트를 실행한다.
5. Integration gate
   - 해당 경계의 contract/integration test를 실행한다.
6. Evidence gate
   - emulator 또는 실기기가 필요한 항목이면 증거 수준과 결과를 남긴다.
7. Handoff
   - 결과, 남은 가설, 실행 명령, artifact 위치를 기록한다.

Red 테스트 없이 시작할 수 있는 예외:

- 공식 SDK가 기기에서 존재하는지만 확인하는 time-boxed spike
- build system bootstrap
- 사람이 승인해야 하는 시스템 permission picker

예외에서도 성공/실패 판정 script와 결과 schema를 먼저 작성한다.

## 3. 테스트 계층

### L0 정적/구조 검사

목적:

- dependency 방향
- 금지 API
- generated code drift
- secret pattern
- Kotlin shim 책임 경계
- formatting/lint/type error

예:

```text
domain_has_no_platform_dependencies
video_plane_has_no_rustra_dependency
generated_types_do_not_contain_video_payload
viewer_contract_has_no_remote_input_command
kotlin_shim_imports_only_allowlisted_packages
handwritten_kotlin_contains_no_network_or_codec_policy
```

Kotlin shim의 정확한 line count를 품질 기준으로 삼지 않는다. allowlist import, JNI/TurboModule API snapshot, mutation-free adapter test로 책임을 제한한다.

### L1 순수 unit test

대상:

- state machine
- quality budget allocator
- retry/backoff
- source lease
- error mapping
- protocol version negotiation
- frame drop policy
- packet fragment assembly
- redaction

실행 시간 목표: 전체 10초 이내.

### L2 property/model test

대상과 invariant:

| 대상 | invariant |
| --- | --- |
| source lease | acquire/release 순서와 중복에 관계없이 count가 음수가 되지 않음 |
| stream epoch | 이전 epoch frame은 절대 새 decoder에 전달되지 않음 |
| fragment assembler | 입력 순서/중복/손실에 관계없이 메모리 상한 유지 |
| budget allocator | 총 bitrate/pixel budget 초과 없음 |
| command idempotency | 같은 request ID는 side effect 한 번 |
| catalog revision | stale mutation이 최신 source를 덮지 않음 |
| retry | 최대 delay와 총 시도 budget 준수 |
| redaction | 임의 문자열의 title/path/token이 diagnostic에 남지 않음 |

Rust `proptest` 또는 동등 도구를 사용한다. concurrency 구조는 `loom` 적용 가능성을 검토한다.

### L3 component test

실제 component와 fake port를 연결한다.

- Host core + FakeCapture + FakeEncoder + FakeTransport
- Viewer core + FakeTransport + FakeDecoder
- React screen + mocked Rustra client
- stream launcher TS spec + fake native module
- Rust NDK wrapper는 host-side fake C ABI로 lifecycle 검증

### L4 contract/cross-language test

검증 항목:

- Rustra generated TS clean regeneration
- Rust input/output fixture를 TS가 decode
- TS command helper payload를 Rust가 decode
- runtime contract hash
- network protocol golden vector
- stable error mapping
- C/JNI ownership과 null/error behavior

Rustra의 기존 cross-wire 방식처럼 hand-crafted byte만 비교하지 않고 실제 양쪽 encoder/decoder를 사용한다.

### L5 process integration test

실제 process와 loopback network를 사용한다.

```text
synthetic capture -> software/test encoder -> transport -> test decoder -> frame digest
```

검증:

- session handshake
- 1/2/4 source multiplex
- disconnect/reconnect
- IDR request
- source close isolation
- bounded queue
- shutdown idempotency

CI에서 hardware codec을 요구하지 않도록 deterministic synthetic backend를 둔다.

### L6 Android emulator/instrumentation

검증:

- HubActivity와 StreamActivity task 생성
- source별 unique document URI
- configuration change 복원
- task close lease release
- multiple resumed activity behavior
- Surface create/destroy callback ordering
- React Native initial props
- Rustra `addNumbers -> 42` 실제 RN 경로
- TS Fabric spec과 generated Android interface 연결

에뮬레이터 codec 결과는 성능 증거가 아니다.

### L7 물리 플랫폼 통합

장치:

- 실제 Mac
- 실제 Windows PC
- Galaxy XR

검증:

- 실제 시스템 picker와 권한
- ScreenCaptureKit/WGC frame
- hardware encode/decode
- Home Space multi-instance
- Surface composition
- thermal/power
- Wi-Fi

이 계층은 nightly/manual gate일 수 있지만 release에는 필수다.

### L8 광학 종단간 성능

고속 카메라 또는 photodiode 계측으로 실제 화면 변화와 Galaxy XR 표시 사이를 잰다. software timestamp만으로 대체할 수 없다.

## 4. 테스트 가능한 port

### 4.1 Clock

```rust
trait Clock {
    fn monotonic_now(&self) -> MonoTime;
    fn wall_now(&self) -> WallTime;
}
```

재시도, lease grace period, frame timeout은 system clock을 직접 읽지 않는다.

### 4.2 Capture

`FakeCapture`는 다음 script를 지원한다.

- 고정 color frame
- frame ID가 그려진 moving pattern
- resize sequence
- no-change interval
- source disappears
- permission revoked
- timestamp discontinuity
- burst frame callback

### 4.3 Encoder

`FakeEncoder`는 payload가 아니라 frame dependency를 모델링한다.

- config
- keyframe
- delta chain
- delayed output
- dropped input
- encoder restart/epoch
- hardware unavailable

golden H.264 fixture는 공개 또는 프로젝트가 직접 생성한 synthetic pattern만 사용한다. 실제 사용자 화면을 fixture로 저장하지 않는다.

### 4.4 Transport

`SimulatedLink` 설정:

```rust
struct LinkProfile {
    base_delay: Duration,
    jitter: Duration,
    loss_rate: f64,
    duplicate_rate: f64,
    reorder_rate: f64,
    bandwidth_bps: u64,
    outage_schedule: Vec<TimeRange>,
    mtu: usize,
}
```

seed를 결과에 기록하여 실패를 재현한다.

### 4.5 Decoder

`FakeDecoder`는 다음을 기록한다.

- configure calls
- Surface attach/detach
- input frame/epoch
- keyframe requirement
- output time
- resource exhausted
- malformed bitstream

## 5. 상태 머신 테스트 목록

### 5.1 Pairing

```text
new_offer_expires_after_two_minutes
expired_offer_cannot_create_device_identity
host_rejection_leaves_no_partial_device
approval_binds_presented_fingerprint
replayed_offer_is_rejected
revoked_device_cannot_resume_old_session
pairing_cancel_zeroizes_ephemeral_secret
```

### 5.2 Host session

```text
unapproved_source_cannot_start
approved_source_starts_once_for_first_lease
second_viewer_lease_reuses_policy_without_duplicate_capture_when_allowed
last_lease_stops_capture_after_debounce
permission_revoke_stops_all_affected_sources
one_capture_failure_does_not_stop_other_sources
stop_all_is_idempotent
```

### 5.3 Viewer window

```text
unique_source_opens_unique_document_task
same_source_focuses_existing_window_by_default
restored_task_reauthenticates_before_decode
surface_destroy_requests_decoder_detach
surface_recreate_requests_config_and_idr
focus_loss_does_not_pause_visible_stream
stopped_window_suspends_after_grace_period
task_removal_releases_exactly_one_lease
hub_close_keeps_stream_tasks_alive
last_stream_close_allows_session_idle
```

### 5.4 Video ordering

```text
delta_before_config_is_dropped
delta_before_keyframe_is_dropped
old_epoch_is_dropped_after_resize
duplicate_fragment_does_not_duplicate_output
incomplete_frame_expires
newer_complete_frame_can_supersede_older_incomplete_delta
keyframe_loss_requests_idr_with_rate_limit
queue_never_exceeds_configured_bytes
```

### 5.5 Quality

```text
focused_large_window_receives_focus_profile
unfocused_visible_window_keeps_playing
small_window_downgrades_after_hysteresis
rapid_focus_changes_do_not_thrashes_encoder
thermal_severe_caps_total_pixel_rate
visible_streams_receive_minimum_fair_share
hidden_stream_can_suspend
```

## 6. 멀티 인스턴스 전용 검증

### 6.1 task identity

각 launch는 다음 tuple을 가진다.

```text
(host_id, source_id, stream_instance_id, document_uri, task_id)
```

테스트는 다음을 확인한다.

- 서로 다른 source가 같은 task로 합쳐지지 않는다.
- process death 후 secret 없이 source identity를 복구한다.
- revoked Host의 restored task가 decoder를 시작하지 않는다.
- task ID 변경이 network source identity를 바꾸지 않는다.

### 6.2 lifecycle permutation

다음 event를 model-based test로 순열 생성한다.

```text
activityCreate
activityStart
activityResume
focusGain/focusLoss
surfaceCreate/surfaceChange/surfaceDestroy
activityPause
activityStop
configurationChange
taskRemove
processDeath
```

invariant:

- Surface 없이 decoder output configure 금지
- task가 없는 permanent lease 금지
- 한 instance가 다른 instance Surface를 detach하지 않음
- stop/destroy 중복에도 double free 없음
- visible unfocused Activity는 재생 가능

### 6.3 Galaxy XR 수동 시나리오

1. Hub에서 네 source를 연다.
2. 네 창을 서로 다른 크기로 배치한다.
3. 각 창의 frame counter가 10분 동안 진행되는지 본다.
4. 한 창을 100회 resize한다.
5. 한 창을 닫고 다른 세 창이 유지되는지 본다.
6. Hub를 닫고 stream window가 유지되는지 본다.
7. Hub를 다시 열어 열린 창 목록이 일치하는지 본다.
8. headset sleep/wake 뒤 복구를 확인한다.

## 7. Rust/TS 계약 TDD

작업 순서:

1. Rust input/output/error type과 command signature를 작성한다.
2. Rust unit/compile-fail test를 작성한다.
3. TS generation snapshot을 갱신한다.
4. generated code가 clean인지 확인한다.
5. `@rustra/testing` mock으로 UI Red test를 작성한다.
6. 실제 RN/Tauri adapter의 `addNumbers -> 42`를 유지한다.
7. command를 실제 UI flow에 연결한다.

필수 gate:

```text
cargo test --workspace
bun run typecheck
bun run test
bun run rustra:generate
git diff --exit-code -- packages/control-generated
bun run test:contract
```

명령 이름은 scaffold에서 package script로 고정한다. 이 문서의 명령은 목표 인터페이스이며 현재 빈 저장소에서는 아직 실행되지 않는다.

## 8. React Native와 Rust NDK TDD

### 8.1 TypeScript 우선 spec

`RemoteSurfaceNativeComponent.ts`와 `StreamWindowLauncherSpec.ts`를 먼저 작성하고 Codegen 결과가 생성되는지 test한다.

### 8.2 Kotlin shim contract

shim은 다음 C ABI/JNI만 호출할 수 있다.

```text
leftcar_viewer_process_start
leftcar_stream_attach_surface
leftcar_stream_surface_changed
leftcar_stream_detach_surface
leftcar_stream_update_window_state
leftcar_stream_release
```

shim test:

- callback ordering을 fake native library가 기록
- null Surface 전달 금지
- attach 1회당 detach 최대 1회
- 다른 `instance_id`로 callback 교차 금지
- exception을 삼키지 않고 stable error event로 변환

### 8.3 Rust NDK decoder test

host unit test에서는 NDK function table을 fake로 주입한다.

physical device test에서는:

- synthetic H.264 SPS/PPS/IDR/delta fixture
- `AMediaCodec` create/configure/start/queue/release
- hardware codec identity
- low latency feature
- 1/2/4 instance
- Surface recreate
- malformed access unit
- repeated start/stop 100회

## 9. 네트워크 테스트

### 9.1 deterministic simulation

모든 PR에서 fixed seed profile을 실행한다.

| profile | delay | jitter | loss | bandwidth |
| --- | --- | --- | --- | --- |
| clean-lan | 1ms | 0.2ms | 0% | 1Gbps |
| normal-wifi | 4ms | 2ms | 0.1% | 200Mbps |
| busy-wifi | 12ms | 10ms | 1% | 40Mbps |
| bad-wifi | 25ms | 30ms | 3% | 15Mbps |
| outage | 4ms | 2ms | 100% for 5s | 200Mbps |

### 9.2 physical link

실제 Galaxy XR 결과는 simulator 결과와 분리한다. AP model, band, channel width, RSSI, distance, other traffic, power state를 기록한다.

### 9.3 protocol fuzz

fuzz target:

- handshake envelope
- length prefix
- control message
- video fragment header
- fragment assembler
- codec config validation
- diagnostic parser

성공 조건:

- panic/abort/UB 없음
- allocation 상한
- parser time 상한
- invalid input이 stable error/drop으로 끝남

## 10. 성능 테스트를 TDD에 넣는 방법

microbenchmark 수치를 일반 unit test의 hard threshold로 두지 않는다. noisy CI에서 flaky해진다.

세 계층으로 나눈다.

1. invariant
   - queue length, copy count, allocation count
   - 모든 PR에서 hard fail
2. controlled benchmark
   - dedicated runner에서 baseline 대비 회귀율
   - 10% 또는 통계적 기준 초과 시 경고/검토
3. product SLO
   - 실제 Mac + Galaxy XR에서 glass-to-glass
   - release blocking

예:

```text
capture_callback_performs_no_heap_allocation_after_warmup
encoded_frame_does_not_clone_payload_on_packetize
four_source_scheduler_stays_within_budget
viewer_queue_depth_never_exceeds_two_frames
```

## 11. CI 설계

### PR fast lane, 15분 목표

- format/lint/typecheck
- Rust unit/property with bounded cases
- TS unit/component
- contract regeneration/diff
- architecture rules
- synthetic loopback integration
- protocol smoke fuzz
- Android compile and Codegen generation

### main lane, 45분 목표

- full workspace tests
- sanitizer supported host tests
- extended property seeds
- Android emulator multi-task test
- package builds
- dependency/security audit

### nightly lane

- 60분 simulated network soak
- extended fuzz
- memory/leak tests
- macOS real capture if runner available
- Windows real capture if runner available

### device lab lane

- Galaxy XR multi-instance
- real hardware decode 1/2/4
- Mac and Windows E6
- glass-to-glass benchmark
- thermal soak

device lab 실패를 rerun 한 번으로 숨기지 않는다. environment failure와 product failure를 분류한다.

## 12. coverage 정책

전체 line coverage 숫자를 제품 목표로 삼지 않는다.

필수:

- domain state transition 100% branch 의도 검토
- auth/capability negative path 전부 명시
- parser invalid input family
- resource start/stop/error cleanup
- stable error code mapping
- Kotlin shim public method 모두 instrumentation coverage

video hardware driver 자체는 coverage보다 physical scenario matrix로 검증한다.

## 13. flaky test 정책

1. 첫 flaky 관측에서 issue와 seed/artifact를 남긴다.
2. 원인 없이 retry count만 늘리지 않는다.
3. 48시간 안에 deterministic fixture 또는 환경 격리를 시도한다.
4. quarantine은 owner, 만료일, 대체 gate가 있어야 한다.
5. security, data race, lifetime test는 quarantine할 수 없다.

## 14. fixture와 artifact 관리

저장 가능:

- synthetic color bars
- frame counter pattern
- generated H.264 test vectors
- redacted protocol hex
- benchmark CSV/JSON
- log schema sample

저장 금지:

- 실제 화면 캡처
- 창 제목과 문서명
- private IP가 포함된 raw log
- 인증서 private key
- pairing QR/token
- 사용자 계정/경로

## 15. Definition of Done

작업 하나는 다음을 모두 만족해야 완료다.

- [ ] acceptance criterion이 테스트 이름과 연결된다.
- [ ] Red 실패를 관측했거나 spike 판정표를 만들었다.
- [ ] 최소 구현이 Green이다.
- [ ] 관련 unit/property/contract/integration gate가 통과한다.
- [ ] resource cleanup/error path가 있다.
- [ ] metric과 redaction이 검토되었다.
- [ ] 플랫폼 주장이 있으면 필요한 evidence level이 충족되었다.
- [ ] 문서, command, artifact 경로가 handoff에 있다.
- [ ] 남은 가설과 fallback이 명시되었다.

## 16. 완료라고 말하면 안 되는 경우

- mock capture에서 그림이 나왔지만 실제 ScreenCaptureKit은 실행하지 않음
- H.264 파일을 재생했지만 네트워크 live stream은 연결하지 않음
- emulator에서 네 Activity를 열었지만 Galaxy XR에서 확인하지 않음
- `AMediaCodec` create 성공만 확인하고 실제 Surface frame을 보지 않음
- Rustra mock command가 42지만 RN native path는 실행하지 않음
- debug build가 되지만 signed/package artifact가 없음
- software timestamp가 낮지만 glass-to-glass를 측정하지 않음
- 1분 동작했지만 latency creep/thermal soak를 하지 않음

