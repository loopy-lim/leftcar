# Leftcar v1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** docs/01–10 + ADR-0001..0004에 따라 Leftcar 제품 저장소를 구현한다. 자동 검증 가능한 모든 증거(E1–E3, 가능하면 E4)를 완성하고, 실기기 의존 증거(E5–E7)는 harness와 판정 script를 남기고 pending으로 기록한다.

**Architecture:** ADR-0002 의존성 규칙(`domain <- media-model <- session/host-core/viewer-core`, video hot path는 TS/Rustra/JSON 밖)을 cargo workspace + pnpm workspace로 강제한다. 제품 로직은 전부 Rust(domain/media-model/network-protocol/session/host-core/viewer-core/transport-api/transport-quic/diagnostics)와 TypeScript(UI)로 작성하고, Kotlin은 Activity/Intent/Surface shim으로 제한한다. Rustra는 pin `11ff71f`로 제어 계약에만 사용한다.

**Tech Stack:** Rust 1.95 (workspace, proptest, quinn), pnpm 10 / TypeScript / React Native + Tauri, Android SDK/NDK(r26), Java 17.

---

## 환경 제약과 증거 정책 (구현자가 반드시 먼저 읽을 것)

1. **Galaxy XR 실기기 없음.** E5(실기기), E6(종단간), E7(장시간 계측)은 이 환경에서 생성 불가다. docs/README.md 검증 수준 규칙에 따라 이를 "구현됨, 증거 pending"으로 명시하고, 실기기 판정 script/harness를 `tools/`에 남긴다. E5+를 달성했다고 표현하지 않는다.
2. **디스크 17Gi 여유.** Gradle/NDK/RN 대량 캐시를 피한다. Rust 빌드는 `cargo check` 우선, Android Rust는 `cargo build --target aarch64-linux-android`로 검증하고 전체 Gradle 빌드는 마지막에 1회 시도.
3. **ScreenCaptureKit 실화면 캡처는 권한 UI가 필요**하므로 자동 검증 대상이 아니다. FakeCapture/FakeEncoder/FakeDecoder 경로가 CI 증거이고, 실제 SCK/VideoToolbox adapter는 compile + 구조 검사까지만 한다 (docs/08 H16–H18이 실기기 단계).
4. **24주 로드맵의 물리적 순서는 유지하되**, 이 세션은 "구현 + 자동 검증"을 수행하고 Gate 판정(G1–G6)은 증거 보고서로 대체한다.
5. 커밋은 task 단위로 자주. 메시지 prefix: `feat:`, `test:`, `chore:`, `docs:`.

## 진행 매핑 (roadmap H → plan Stage)

| Stage | Roadmap tasks | 증거 수준 |
| --- | --- | --- |
| S0 bootstrap | H00 | E1 |
| S1 guardrails | H01, H03(CI) | E1 |
| S2 domain | H10(일부), H25/H28–H32의 domain 층 | E1 |
| S3 media/network | H10 | E1,E2 |
| S4 transport | H10–H14 (bake-off는 simulated만) | E2 |
| S5 control contract | H02, H26 | E1,E2,E4 |
| S6 session | H22–H24, H27 (로직), H32 | E1,E2 |
| S7 host core + mac | H15–H20 (fake 경로 완성, 실SCK 구조) | E1,E2,E3 |
| S8 viewer core + shim | H05–H09 중 로직/구조, H29 | E1,E2,E3 |
| S9 diagnostics | H37 | E1 |
| S10 apps(TS) | H04, H15 UI, H38 일부 | E1,E3 |
| S11 CI+문서정합 | H03, H52 | E0–E3 보고 |

---

## Stage 0: 저장소와 workspace bootstrap (H00)

**Files:** Create `.gitignore`, `Cargo.toml`, `rust-toolchain.toml`, `package.json`, `pnpm-workspace.yaml`, `clippy.toml`, `deny.toml`(생략 가능), `apps/`, `crates/`, `packages/`, `tools/`, `tests/`.

**Step 0.1** `git init` 후 `.gitignore`:

```gitignore
/target
node_modules/
dist/
*.log
.DS_Store
apps/viewer-android/android/.gradle/
apps/viewer-android/android/app/build/
apps/viewer-android/build/
*.apk
*.aab
artifacts/
```

**Step 0.2** root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
  "crates/domain",
  "crates/media-model",
  "crates/network-protocol",
  "crates/session",
  "crates/transport-api",
  "crates/transport-quic",
  "crates/host-core",
  "crates/macos-capture",
  "crates/macos-encode",
  "crates/viewer-core",
  "crates/diagnostics",
  "crates/control-contract",
  "tools/architecture-check",
]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "Proprietary"

[workspace.dependencies]
domain = { path = "crates/domain" }
media-model = { path = "crates/media-model" }
network-protocol = { path = "crates/network-protocol" }
session = { path = "crates/session" }
transport-api = { path = "crates/transport-api" }
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bytes = "1"
proptest = "1"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "time", "sync", "net"] }
uuid = { version = "1", features = ["v4"] }
```

`rust-toolchain.toml`: `1.95.0`. 각 crate는 빈 `src/lib.rs` + 위 의존성만으로 skeleton 생성. `cargo metadata --no-deps && cargo check --workspace` 통과를 확인.

**Step 0.3** `pnpm-workspace.yaml`:

```yaml
packages:
  - "packages/*"
  - "apps/*"
```

root `package.json` (scripts는 docs/05 §7의 목표 인터페이스):

```json
{
  "name": "leftcar",
  "private": true,
  "scripts": {
    "typecheck": "tsc -b",
    "test": "vitest run",
    "test:contract": "vitest run -c vitest.contract.config.ts",
    "test:architecture": "tsx tools/architecture-check/ts.ts",
    "rustra:generate": "cargo run -p control-contract --bin generate"
  }
}
```

**Step 0.4** commit `chore: workspace bootstrap (H00)`.

## Stage 1: architecture guardrails (H01)

**Files:** Create `tools/architecture-check/`(cargo crate), `tools/architecture-check/ts.ts`, `tests/architecture/` 대신 crate 내부 tests.

**Step 1.1 Red test** (`tools/architecture-check/tests/dependency_rules.rs`): `cargo metadata --no-deps`를 파싱해:

- `domain`은 workspace 외부 crate 의존 금지(thiserror/serde 허용 allowlist)
- `media-model`→`domain` 허용, `domain`→`media-model` 금지
- `session`/`host-core`/`viewer-core`→`domain`,`media-model` 허용
- `domain`이 `tauri|winit|objc2|windows|jni|android` import 시 fail
- `control-contract`→`domain` 허용, 역방향 금지
- TS 측: `packages/control-generated` 내 `EncodedFrame|VideoPacket|NalUnit` 문자열 금지, `apps/viewer-android/src` 내 `sendKey|injectInput|clipboard` 금지 (`ts.ts`가 regex 검사)

**Step 1.2** 구현 후 `cargo test -p architecture-check` green. 일부러 위반 넣어 fail 확인(Red→Green).

**Step 1.3** commit `test: architecture dependency guardrails (H01)`.

## Stage 2: domain crate — TDD 핵심 (docs/03 §9, docs/04 §3–4, docs/05 §5)

**Files:** Create `crates/domain/src/{ids,source,profile,budget,lease,phase,error,redact}.rs`.

**Step 2.1 ids.rs** — opaque newtype:

```rust
macro_rules! opaque_id { ($name:ident) => {
    #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct $name(pub String);
    impl $name { pub fn generate() -> Self { Self(uuid::Uuid::new_v4().to_string()) } }
} }
opaque_id!(HostId); opaque_id!(DeviceId); opaque_id!(SourceId);
opaque_id!(StreamInstanceId); opaque_id!(SessionId);
```

Test: `id_display_never_leaks_in_debug`는 아니고(디버그 출력은 필요), `generated_ids_are_unique`, `id_rejects_empty`.

**Step 2.2 lease.rs** — source lease (docs/05 §5.2 테스트 이름 그대로):

```rust
pub struct LeaseTable { leases: HashMap<SourceId, HashSet<StreamInstanceId>>, stop_debounce: Duration }
pub enum LeaseEvent { SourceStarted(SourceId), SourceStopped(SourceId) }
```

Tests(TDD 순서): `acquire_release_never_negative`(proptest: 임의 acquire/release 순서에서 count>=0), `first_lease_starts_source`, `last_lease_stops_after_debounce`, `double_release_is_noop`, `release_unknown_source_is_noop`.

**Step 2.3 profile.rs + budget.rs** — QualityProfile(focus/normal/background_visible/suspended + Custom{max_width,max_height,max_fps,max_bitrate_bps})과 budget allocator. docs/03 §9.3 우선식:

```rust
priority = visibility_weight * focus_weight * requested_quality * health_penalty
```

Allocator 입력: 창 목록(visible, focused, requested_quality, health_penalty), 총 pixel rate 상한, 총 bitrate 상한. 출력: 창별 profile. Tests(docs/05 §5.5 이름 사용): `focused_large_window_receives_focus_profile`, `unfocused_visible_window_keeps_playing`, `small_window_downgrades_after_hysteresis`, `rapid_focus_changes_do_not_thrash_encoder`(hysteresis window 내 재배치 금지), `thermal_severe_caps_total_pixel_rate`, `visible_streams_receive_minimum_fair_share`, `hidden_stream_can_suspend`. proptest: `allocator_never_exceeds_total_budget`.

**Step 2.4 phase.rs** — PairingState, StreamPhase enum (docs/04 §4와 동일 변형) + 허용 전이 테이블. Tests: `phase_transitions_match_product_requirements`(01 §6 그래프: suspended→negotiating 재개, any→source_unavailable 등).

**Step 2.5 error.rs** — ErrorScope(Command/Source/Stream/Session/Host/Viewer), RecoveryAction, `LeftcarError` + docs/04 §8 stable error code 테이블 전부 상수로. Tests: `all_stable_errors_have_user_recovery_mapping`, `unknown_code_maps_to_diagnostics`.

**Step 2.6 redact.rs** — allowlist 기반 redactor. 입력 문자열에서 title/path/token/IP 유출 방지. Tests(docs/07 §18): `diagnostics_redact_title_path_token_and_ip` — `~/Documents/secret.txt`, `pairing_token=abc`, `192.168.0.5` 등이 출력에 없음, 허용 패턴(codec, duration, error code)은 통과.

**Step 2.7** `cargo test -p domain` green, commit `feat: domain core with TDD (lease/budget/phase/error/redact)`.

## Stage 3: media-model + network-protocol (docs/03 §6, docs/05 §5.4)

**Files:** `crates/media-model/src/{frame,fragment,assemble,backpressure}.rs`, `crates/network-protocol/src/{envelope,version,wire}.rs`.

**Step 3.1 frame.rs** — `EncodedFrame`(docs/03 §6.1 필드 그대로: session_id, source_id, stream_epoch, frame_id, kind, codec, capture_time_host_ns, encode_done_host_ns, width, height, payload: Bytes), `FrameKind::{Key,Delta,Config}`, `StreamEpoch(u32)`.

**Step 3.2 fragment.rs** — packetizer: frame → fragments(MTU), fragment header{source_id, epoch, frame_id, kind, frag_index, frag_count, checksum}. 상한(docs/07 §13): frame 16MiB, incomplete per source 2.

**Step 3.3 assemble.rs** — assembler. Tests(docs/05 §5.4 이름 그대로): `delta_before_config_is_dropped`, `delta_before_keyframe_is_dropped`, `old_epoch_is_dropped_after_resize`, `duplicate_fragment_does_not_duplicate_output`, `incomplete_frame_expires`, `newer_complete_frame_can_supersede_older_incomplete_delta`, `keyframe_loss_requests_idr_with_rate_limit`, `queue_never_exceeds_configured_bytes`, `fragment_flood_stays_within_memory_bound`(proptest: 무한 fragment 스트림에도 메모리 상한 유지).

**Step 3.4 backpressure.rs** — docs/03 §6.3 표 그대로 bounded stage queue. `LatestFrameSlot`(capture→encoder: 최신 1장), `BoundedAuQueue`(encoder→packetizer 2 AU), 시간 예산 slot. Tests: `latest_frame_slot_drops_stale`, `au_queue_drops_oldest_delta_keeps_key`, `queue_never_exceeds_configured_bytes`.

**Step 3.5 envelope.rs (network-protocol)** — ClientHello/ServerHello/ControlEnvelope(docs/03 §5.2 필드). JSON spike 구현(제품 wire는 미정 Q-004). version negotiation: range 교차. Tests: `version_negotiation_picks_highest_common`, `version_mismatch_is_fatal`, `oversized_control_message_allocates_nothing_large`(256KiB 초과 입력에서 0 alloc 보장 — 파서가 길이 prefix 먼저 검사), golden vector(`tests/golden/*.json`에 hex fixture).

**Step 3.6 fuzz smoke** — `network-protocol/fuzz/` 대신 proptest 기반 `fuzz_smoke.rs`: `arbitrary_bytes_never_panic_envelope_parser`(길이 상한 내 파싱이 panic/abort 없이 Err).

**Step 3.7** `cargo test -p media-model -p network-protocol` green, commit.

## Stage 4: transport-api + simulated + QUIC + loopback (H10–H14 simulated)

**Files:** `crates/transport-api/src/lib.rs`(Transport trait + SimulatedLink + LinkProfile), `crates/transport-quic/src/lib.rs`.

**Step 4.1 Transport trait**:

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self, addr: SocketAddr) -> Result<TransportConnection>;
}
#[async_trait]
pub trait TransportConnection: Send + Sync {
    async fn send_control(&self, bytes: Bytes) -> Result<()>;
    async fn send_video(&self, source: SourceId, bytes: Bytes) -> Result<()>;
    async fn recv(&self) -> Result<TransportEvent>; // Control(Bytes) | Video(SourceId, Bytes) | Closed
}
```

**Step 4.2 SimulatedLink** — LinkProfile{base_delay,jitter,loss_rate,duplicate_rate,reorder_rate,bandwidth_bps,outage_schedule,mtu} + deterministic seed RNG(docs/05 §4.4). docs/05 §9.1의 5개 profile(clean-lan/normal-wifi/busy-wifi/bad-wifi/outage)을 preset으로. Tests: `outage_schedule_blocks_all`, `loss_seed_is_reproducible`, `reorder_never_drops_complete_frames`.

**Step 4.3 transport-quic** — quinn 사용. `transport-quic`은 `transport-api` 구현: 제어는 reliable bi-stream, 영상은 DATAGRAM. 로컬 loopback(`127.0.0.1`) 테스트만. Tests: `quic_loopback_delivers_control_in_order`, `quic_datagram_video_reaches_receiver`, `quic_reconnect_after_close`.

**Step 4.4 transport-webrtc** — crate를 만들지 않고 `transport-api`에 `webrtc.rs` stub + 문서 주석으로 "libwebrtc 빌드 위험 R-007, 실기기 bake-off H12에서 결정" 명시. product build는 `--features transport-quic`가 기본 선택(ADR-0004 미확정 상태 그대로). config test: selected transport 없으면 `host-core`의 `build_info`가 실패(`H14 Red` 재현).

**Step 4.5 L5 loopback 통합** — `tests/integration/loopback.rs`: `FakeCapture → FakeEncoder → transport(Simulated/QUIC) → assembler → FakeDecoder → frame digest`. 검증(docs/05 §5): handshake, 1/2/4 source multiplex, disconnect/reconnect, IDR request, source close isolation, bounded queue, shutdown idempotency. commit.

## Stage 5: control-contract via Rustra pin (H02, H26)

**Files:** `crates/control-contract/Cargo.toml`(rustra git pin), `src/{host,viewer,common}.rs`, `src/bin/generate.rs`, 생성 출력 `packages/control-generated/`.

**Step 5.1 pin**:

```toml
[dependencies]
rustra = { git = "https://github.com/loopy-lim/rustra", rev = "11ff71f5e2b5a0c563d50525eef82b0a05768c1f" }
```

(네트워크 불가 시 `path = "/Users/loopy/dev/ll3/rustra-bridge/crates/rustra"` + 주석으로 git pin 명시. 원칙: 공개 main pin.)

**Step 5.2 common.rs** — docs/04 §3–4 타입 전부(`#[bridge_type]` IDs, SourceDescriptor, PairingState, StreamPhase, QualityProfile, CustomQualityProfile).

**Step 5.3 host.rs** — docs/04 §5 command 전부(get_capture_permission_state, request_source_selection, list_approved_sources, revoke_source, begin/cancel/approve/reject_pairing, list_paired_devices, revoke_device, get_host_snapshot, set_source_policy, stop_source, stop_all_streams, export_redacted_diagnostics) + HostEvent. 내부 로직은 domain/host-core 위임, command는 thin wrapper.

**Step 5.4 viewer.rs** — docs/04 §6 command 전부 + ViewerEvent.

**Step 5.5 generate.rs** — `Package::builder("leftcar.host.control")`/`("leftcar.viewer.control")` → `packages/control-generated/`에 TS 생성 + contract hash 파일.

**Step 5.6 contract tests** — `cargo test -p control-contract`: `add_numbers_20_22_is_42`(호스트 adapter 경로), `viewer_contract_does_not_expose_input_commands`, `video_payload_type_is_absent_from_generated_typescript`(생성 TS 파일 regex), `pairing_offer_view_never_contains_private_key`, `generated_contract_hash_matches_runtime`, `duplicate_request_id_is_idempotent`(CommandMeta). TS 측 `packages/control-generated/__tests__/contract.test.ts`: 생성 파일 import + `addNumbers` 타입 존재 + `EncodedFrame` 부재.

**Step 5.7** clean regeneration 확인(`git diff --exit-code -- packages/control-generated`), commit.

## Stage 6: session crate — pairing/auth/capability/reconnect (H22–H25, H27, H32 로직)

**Files:** `crates/session/src/{pairing,capability,revocation,reconnect,identity}.rs`.

**Step 6.1 identity.rs** — `DeviceIdentity` 추상(키 핸들만, raw bytes 없음). Trait `SecureStore`(put/get/delete opaque handle). FakeSecureStore로 테스트. `key_export_is_impossible`(API에 raw getter 없음을 컴파일 타임 구조로 보장하는 문서 테스트).

**Step 6.2 pairing.rs** — PairingOffer{version, host_public_fingerprint, ephemeral_offer_id, single_use_secret, expiry, address_hints, human_code}. Clock trait 주입(docs/05 §4.1). Tests(docs/05 §5.1 + docs/07 §18 이름 그대로): `new_offer_expires_after_two_minutes`, `expired_offer_cannot_create_device_identity`, `replayed_offer_is_rejected`, `host_rejection_leaves_no_partial_device`, `pairing_cancel_zeroizes_ephemeral_secret`(zeroize crate 사용), `same_offer_concurrent_requests_approve_at_most_one`, `short_human_code_alone_cannot_authenticate`.

**Step 6.3 capability.rs** — `view_catalog`, `view_source(source_id, revision, expiry)` capability 객체. 존재하지 않는 capability 목록(send_keyboard 등)은 타입 레벨 부재 + `unknown_input_like_command_is_denied`(프로토콜 enum에 input 변형 없음을 reflection 테스트). Tests: `paired_peer_cannot_view_unapproved_source`, `guessed_source_id_fails_authorization`, `expired_capability_requires_renewal`, `capability_binds_revision`.

**Step 6.4 revocation.rs** — revoke가 모든 활성 stream 종료. Tests: `revocation_closes_existing_streams`, `revoked_device_cannot_resume_old_session`, `revoke_device_invalidates_existing_session_commands`.

**Step 6.5 reconnect.rs** — version negotiation + exponential backoff with jitter + stale epoch 폐기 + duplicate request_id 멱등. Clock 주입. Tests: `backoff_respects_max_delay_and_budget`, `reconnect_resumes_or_cleanly_restarts`(seamless 우선, 실패 시 clean+IDR), `five_second_outage_recovers_within_budget`, `old_frame_never_rendered_after_reconnect`.

**Step 6.6** commit.

## Stage 7: host-core + macos facades (H15–H20 fake 경로)

**Files:** `crates/host-core/src/{registry,orchestrator,fakes}.rs`, `crates/macos-capture/src/{lib.rs,scstream_stub.rs}`, `crates/macos-encode/src/{lib.rs,vt_stub.rs}`.

**Step 7.1** CapturePort/EncodePort trait(docs/03 §7.1 시그니처) 정의는 `media-model`에, 구현체는 각 crate.

**Step 7.2 fakes.rs** — FakeCapture(script: fixed color, counter pattern, resize sequence, no-change interval, source disappears, permission revoked, timestamp discontinuity, burst), FakeEncoder(config/keyframe/delta chain/delayed output/dropped input/restart epoch/hw unavailable). docs/05 §4.2–4.3 그대로.

**Step 7.3 registry.rs** — approved source registry + revision CAS. Tests: `unapproved_source_cannot_start`, `stale_revision_mutation_rejected`, `permission_revoke_stops_all_affected_sources`, `one_capture_failure_does_not_stop_other_sources`, `stop_all_is_idempotent`.

**Step 7.4 orchestrator.rs** — capture→encode→packetize→transport pipeline 구성, budget allocator 연동, teardown 순서(docs/03 §13) idempotency. Tests: `teardown_is_idempotent`, `session_close_stops_all_sources_concurrently`, `no_orphan_capture_after_close`(T-11 대응), `epoch_increments_on_encoder_restart`.

**Step 7.5 macos-capture/macos-encode** — 실제 SCK/VideoToolbox 호출은 `#[cfg(feature = "real")]` 뒤에 두고 feature off 기본. feature off 상태에서는 facade type만 존재. 주석으로 실제 API 경로(SCContentSharingPicker→SCStream→IOSurface→VTCompressionSession, realtime=true, AllowFrameReordering=false)와 권한 흐름 문서화. compile check만.

**Step 7.6** loopback 통합(Stage 4의 L5 테스트가 orchestrator 사용하도록 승격). commit.

## Stage 8: viewer-core + lease/task + NDK 구조 (H05–H09, H29 중 로직)

**Files:** `crates/viewer-core/src/{demux,lease_registry,window_state,c_abi,fakes}.rs`, `native/android-viewer/`(C ABI).

**Step 8.1 demux.rs** — transport 수신 → source별 채널 분할 → assembler → decoder 입력. Tests: `four_sources_demux_without_cross_talk`(Multi-source Identity, docs/06 §4.5), `stale_epoch_dropped_after_surface_recreate`, `decoder_input_only_after_config_and_keyframe`.

**Step 8.2 lease_registry.rs** — domain lease와 연결된 viewer 측 registry: acquire(source, instance) → SessionService 요청, release → Host stop after debounce. Tests(docs/05 §5.3): `unique_source_opens_unique_document_task`, `same_source_focuses_existing_window_by_default`, `task_removal_releases_exactly_one_lease`, `hub_close_keeps_stream_tasks_alive`, `last_stream_close_allows_session_idle`, `restored_task_reauthenticates_before_decode`.

**Step 8.3 window_state.rs** — Activity lifecycle event 입력(create/start/resume/focusGain/focusLoss/surface*/pause/stop/configurationChange/taskRemove/processDeath) → StreamPhase + 정책 출력. permutation proptest(docs/05 §6.2): `decoder_output_never_configured_without_surface`, `no_permanent_lease_without_task`, `one_instance_never_detaches_another_surface`, `no_double_free_on_repeated_stop_destroy`, `visible_unfocused_activity_can_play`, `stopped_window_suspends_after_grace_period`.

**Step 8.4 c_abi.rs + native/android-viewer** — docs/05 §8.2의 함수 6개(`leftcar_viewer_process_start`, `leftcar_stream_attach_surface`, `leftcar_stream_surface_changed`, `leftcar_stream_detach_surface`, `leftcar_stream_update_window_state`, `leftcar_stream_release`)를 `#[no_mangle] extern "C"`로. Surface는 opaque handle(u64)로만 취급(실 JNI는 Kotlin shim). fake NDK function table로 lifecycle 테스트: `attach_once_detaches_at_most_once`, `null_surface_rejected`, `callback_crossing_instances_rejected`, `panic_does_not_cross_c_abi`(catch_unwind).

**Step 8.5 decoder fake** — FakeDecoder(configure calls, surface attach/detach, input frame/epoch, keyframe requirement, output time, resource exhausted, malformed bitstream). 실제 `AMediaCodec` 경로는 `aarch64-linux-android` target으로 `cargo check`하는 것으로 구조 검증(NDK 빌드는 Stage 10).

**Step 8.6** commit.

## Stage 9: diagnostics (H37)

**Files:** `crates/diagnostics/src/{metrics,allowlist,bundle}.rs`.

**Step 9.1** metric 구조(codec, profile, w/h, fps, duration, size, count, error_code, opaque id hash). 1Hz summary aggregator.

**Step 9.2** allowlist 기반 직렬화 + redact(domain 재사용). Tests: `export_contains_no_title_path_token_ip_frame`, `run_scoped_hash_is_stable_within_run`, `metric_names_are_allowlisted`.

**Step 9.3** bundle writer(manifest.yaml + metrics.jsonl + redacted.log 형식, docs/06 §14 구조). commit.

## Stage 10: TypeScript apps (H04, H15 UI, H38)

**Files:** `packages/control-generated/`(생성됨), `packages/testing-mocks/`, `apps/host-desktop/`(Tauri), `apps/viewer-android/`(RN + Kotlin shim + Gradle).

**Step 10.1 packages/testing-mocks** — Rustra client mock(`@rustra/testing` 참고하되 자체 minimal mock`createMockClient({commandName: handler})`).

**Step 10.2 apps/host-desktop (Tauri)** — `pnpm create tauri-app` 없이 최소 수동 구성: `src-tauri/`(main.rs에서 control-contract host package + tauri adapter로 `addNumbers(20,22)=42` command 노출), React UI `src/`(source list mock, pairing placeholder, 상태 표시, stop all 버튼). Vitest: `addNumbers_client_returns_42`(mock), component test(페어링 상태 렌더링). `cargo check -p` 통과. 실행/스크린샷은 disk 상황에 따라 선택.

**Step 10.3 apps/viewer-android** — 최소 RN 수동 구성:
- `package.json`(react, react-native 최소 버전, metro config)
- `src/HubApp.tsx`: paired host 목록 → `list_remote_sources` → source 버튼 → `create_stream_launch` → `StreamWindowLauncher.open(handle)`(TurboModule 호출)
- `src/StreamApp.tsx`: initialProps의 launch handle → lease 획득 → `<RemoteSurface instanceId>`
- `specs/StreamWindowLauncherSpec.ts`, `specs/RemoteSurfaceNativeComponent.ts`(Codegen 스펙, docs/02 §9)
- `android/app/src/main/java/.../shim/`: `HubActivity.kt`, `StreamActivity.kt`(documentLaunchMode always, FLAG_ACTIVITY_NEW_DOCUMENT|MULTIPLE_TASK), `StreamWindowLauncherModule.kt`, `RemoteSurfaceManager.kt`(SurfaceView + JNI `leftcar_stream_attach_surface` 호출), `SessionService.kt`(bind/lease)
- `AndroidManifest.xml`: `PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI=true`, `resizeableActivity=true`
- Kotlin 테스트는 allowlist 관점 architecture check(Stage 1 ts.ts + cargo grep)로: shim이 `java.net`, codec policy symbol import 금지.

Vitest(RN 없이 TS 로직): launcher handle policy, 상태 머신 매핑. Gradle 전체 빌드는 disk 여유 확인 후 1회 시도(실패 시 blocker 기록).

**Step 10.4** commit 각 2건.

## Stage 11: CI + 문서 정합 + 증거 보고 (H03, H52)

**Files:** `.github/workflows/ci.yml`, `docs/EVIDENCE.md`, `README.md` 갱신, `tools/benchmark-runner/`(schema만), `docs/decisions/` 상태 갱신 없음(제안 유지).

**Step 11.1** ci.yml: jobs = rust-fast(fmt/clippy/test), ts(typecheck/test/contract/architecture), android-compile(if feasible), artifact naming. PR fast lane 15분 목표 구조만.

**Step 11.2** `docs/EVIDENCE.md`: 구현 항목 ↔ roadmap H ↔ 증거 수준(E1/E2/E3) ↔ 실행 명령 표. E5–E7 pending 항목 목록(실기기 4창, AMediaCodec 실디코드, glass-to-glass, bake-off, SCK 실화면)과 각 판정 script 진입점.

**Step 11.3** README "현재 상태" 갱신: 구현 시작됨, 자동 검증 항목 표기. docs/README.md 검증 수준 규칙 준수 문구 유지.

**Step 11.4** 최종 전체 게이트: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `pnpm typecheck && pnpm test && pnpm test:contract && pnpm test:architecture`, regeneration diff. 전부 green인 것을 출력로 확인하고 commit `chore: CI + evidence reconciliation (H03/H52)`.

---

## 완료 정의 (이 세션의 "모두 완성")

- [ ] workspace 전 crate가 ADR-0002 dependency rule을 통과
- [ ] docs/05 §5, docs/07 §18, docs/04 §11에 나열된 테스트 이름이 실제 테스트로 존재하고 전부 green
- [ ] L5 loopback 통합(FakeCapture→FakeEncoder→transport→assembler→FakeDecoder)이 Simulated + QUIC 양쪽에서 green
- [ ] Rustra pin에서 생성된 TS가 clean regeneration
- [ ] addNumbers 20+22=42가 Rust 계약 테스트로 증명됨
- [ ] Kotlin shim이 allowlist 검사를 통과
- [ ] EVIDENCE.md에 E1–E3 달성 항목과 E5–E7 pending 목록이 명시됨
- [ ] 모든 커밋이 위 게이트를 통과한 시점에서 작성됨
