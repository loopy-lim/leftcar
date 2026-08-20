# 구현 증거 문서 (EVIDENCE)

기준일: 2026-08-17  
작성 근거: docs/README.md 검증 수준(E0–E7) 규칙. 이 문서는 달성한 증거와 대기 중인 증거를 구분한다. **E5 이상을 달성했다고 표기한 항목은 없다.**

## 요약

| 수준 | 상태 | 비고 |
| --- | --- | --- |
| E0 설계 | 달성 | docs/01–10, ADR-0001..0004 |
| E1 단위/property | 달성 | cargo test --workspace (전 crate) |
| E2 통합 | 달성 | L5 loopback (transport-api tests), C ABI 왕복 |
| E3 빌드 | 부분 달성 | Rust workspace + TS 전부; Android aarch64 check는 CI 잡 | 
| E4 에뮬레이터 | 미달성 | H05/H08 단계. 에뮬레이터 잡 미연결 |
| E5 Galaxy XR 실기기 | 부분 달성 | Galaxy XR는 없음; **동일 Android 16 실기기(TB710FU)에서** Expo RN 앱 구동, HW 디코더 1/4/6/8개 동시, multi-instance task 분리, 60/90fps 실측 |
| E11(신규) 페어링 + 미디어 출발지 검증 | 달성 | QR 페어링 + 토큰 인증 경로 유지, 미디어 역방향 peer 일치 검사 |
| E9(신규) Expo+Rustra 실기기 | 달성 | H09: JS → NativeModules.Rustra → JNI → rustra invoke_json으로 addNumbers(20,22)=42 + contract hash를 앱 화면에서 실측 (screenshot artifacts/device/h09-expo-rustra-proof.png) |
| E10(신규) RN 뷰어 + Tauri 호스트 재구축 | 달성 | v1 재구축: Tauri 호스트(제어 pull + 비디오 push) + RN 뷰어(OS 멀티윈도우, 소스당 창) + shim v2 다중 핸들 + NSD 자동발견 |
| E6 종단간 | 미달성 | 실제 캡처→표시 미실행. G3/G5 대기 |
| E7 계측 장시간 | 미달성 | H51 대기 |

## 달성한 증거 상세

### E1 — 단위/property 테스트

| 영역 | 위치 | 대표 테스트 | 문서 근거 |
| --- | --- | --- | --- |
| 도메인 | `crates/domain` | `acquire_release_never_negative`(proptest), `rapid_focus_changes_do_not_thrash_encoder`, `allocator_never_exceeds_total_budget`(proptest), `diagnostics_redact_title_path_token_and_ip`, `all_stable_errors_have_user_recovery_mapping` | docs/05 §5.5, docs/07 §16/§18 |
| 리스/스케줄 | `crates/domain/lease.rs` | `last_lease_stops_after_debounce`, `task_removal_releases_exactly_one_lease` | docs/03 §3.3 |
| 미디어 | `crates/media-model` | `delta_before_keyframe_is_dropped`, `old_epoch_is_dropped_after_resize`, `duplicate_fragment_does_not_duplicate_output`, `fragment_flood_stays_within_memory_bound`, `queue_never_exceeds_configured_bytes` | docs/05 §5.4 |
| 프로토콜 | `crates/network-protocol` | `oversized_control_message_allocates_nothing_large`, `version_mismatch_is_fatal`, `no_input_injection_control_kind_exists`(T-06), fuzz smoke `arbitrary_bytes_never_panic_envelope_parser` | docs/07 §13, docs/05 §9.3 |
| 계약 | `crates/control-contract` | `host_add_numbers_20_22_is_42`, `viewer_contract_does_not_expose_input_commands`, `video_payload_type_is_absent_from_generated_typescript`, `generated_contract_hash_is_stable` | docs/08 H02/H09 수용기준 |
| 세션 | `crates/session` | `new_offer_expires_after_two_minutes`, `replayed_offer_is_rejected`, `guessed_source_id_fails_authorization`, `revocation_closes_existing_streams`, `backoff_respects_max_delay_and_budget` | docs/07 §7/§9/§18 |
| 호스트 | `crates/host-core` | `unapproved_source_cannot_start`, `approved_source_starts_once_for_first_lease`, `one_capture_failure_does_not_stop_other_sources`, `stop_all_is_idempotent`, `no_orphan_capture_after_close` | docs/05 §5.2 |
| 뷰어 | `crates/viewer-core` | `unique_source_opens_unique_document_task`, `hub_close_keeps_stream_tasks_alive`, `decoder_output_never_configured_without_surface`, `restored_task_reauthenticates_before_decode`, `attach_once_detaches_at_most_once` | docs/05 §5.3/§6.2/§8.2 |
| C ABI | `native/android-viewer` | `six_abi_symbols_roundtrip`, `double_detach_is_state_error_not_crash`, `invalid_lifecycle_code_rejected` | docs/05 §8.2, docs/07 §14 |
| 진단 | `crates/diagnostics` | `export_contains_no_title_path_token_ip_frame`, `run_scoped_hash_is_stable_within_run` | docs/07 §16 |
| 아키텍처 | `tools/architecture-check` + `ts.ts` | ADR-0002 의존성 방향, domain 순도, Kotlin import allowlist, video-plane-no-contract | docs/03 §4.1 |
| UI(TS) | `apps/*/src` | 상태 그래프, launch-handle 정책, 한글 상태 문구(오류 코드 미노출), Hub open/focus 정책 | docs/01 §6, docs/08 H04 |

실행: `cargo test --workspace && pnpm test && pnpm test:contract`

### E2 — 통합

- **L5 loopback** (`crates/transport-api/tests/loopback.rs`): FakeEncoder→packetize→transport→assembler→FakeDecoder. 1/4-소스 멀티플렉스 교차 없음, 3% loss 전달, outage→IDR 복구(NFR-004 논리), lease 기반 소스 격리(NFR-005 논리). Simulated(5 profile) + InMemory 양쪽.
- **Rustra 실경로**: pin `11ff71f`에서 `addNumbers 20+22=42`가 실제 invoke 경로로 증명(에뮬레이터/문자열 mock 아님).
- **C ABI 왕복**: 6심볼 roundtrip, panic 미통과(catch_unwind), null/double-detach/instance-crossing 거부.

### E3 — 빌드/구조

- `cargo check --workspace` 전 crate green, clippy `-D warnings` 0.
- TS typecheck 2앱 green, `pnpm test:architecture` TS/Kotlin 규칙 green.
- Kotlin shim은 `android/.../shim/` 경로 + manifest(documentLaunchMode=always, PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI)만 존재. Gradle 빌드는 E4+ 단계.
- macOS 파사드(macos-capture/macos-encode): 실API 링크 없이 구조만. 시작 시 `RealBackendNotLinked` 명시 실패(무인 소프트웨어 fallback 금지).

## 대기 중인 증거 (E4–E7) — 판정 진입점

| 증거 | 게이트 | 필요 장치 | 판정 방법 |
| --- | --- | --- | --- |
| 같은 APK 창 4개 동시 표시 | G1/H05 (E5) | Galaxy XR | `adb shell dumpsys activity` task dump + 10분 창 유지 관찰; docs/06 §7.1 W4 |
| 비초점 창 Surface 갱신 지속 | G1/H06/H08 (E5) | Galaxy XR | frame counter 4개 10분 진행 (docs/02 F-02) |
| AMediaCodec 4 디코더 1080p30 | G1/H07-H08 (E5) | Galaxy XR | golden H.264 4개 동시 재생, thermal 기록 (docs/02 F-03) |
| 실제 Mac 창→표시 종단간 | G3/H20 (E6) | Mac+Galaxy XR | S1 10분 + resize/close/outage 시나리오 (docs/08 H20) |
| glass-to-glass p50/p95 | H21 (E7) | 240fps 카메라 | docs/06 §5 절차, 200 sample |
| transport bake-off | G2/H11–H14 (E5) | Galaxy XR | docs/06 §9 동일조건 비교, ADR-0004 갱신 |
| 60분 soak/latency creep | H36/H51 (E7) | Galaxy XR | NFR-002/006/007/008 |

위 항목들은 장치 확보 시 순서대로 실행한다. 이 문서의 표는 그때 갱신한다.

## 구현 범위 참고

- transport-quic: 구현 미포함(의도적). ADR-0004가 bake-off 전 확정 금지. `ProductBuildInfo::new(Undecided)`가 product build를 거부하는 것으로 대체(H14 Red).
- macos-capture/macos-encode: Swift/C ABI shim과 실제 SCK/VT 세션은 H16–H18 장치 단계.
- React Native/Gradle: 소스+스펙+manifest만. RN host 연결은 H05.
- Windows/Linux host: 미착수(P7 단계, 문서대로).


## Expo + Rustra 실기기 경로 (H09, 2026-08-17 추가)

- 스택: Expo 57 / RN 0.86 (검증된 multi-android-viewer 하네스 재사용) + Kotlin shim
  (invoke 전달만) + native/leftcar-rustra (rustra Package::invoke_json 동일 경로)
- 실측: release APK, Metro 없이 번들 내장, TB710FU에서 앱 실행 —
  `addNumbers(20, 22) = 42` PASS, contract hash 16hex PASS를 화면에서 확인
- 이것이 docs/02 §9.1의 기본 아키텍처(TS UI + Rustra + Rust core) 실기기 증거

## RN 뷰어 + Tauri 호스트 재구축 (E10, 2026-08-18 추가)

- **설계 및 계획 문서**: `docs/plans/2026-08-18-rn-tauri-rebuild-design.md`, `docs/plans/2026-08-18-rn-tauri-rebuild.md`
- **구현 영역**:
  1. **호스트 (`apps/host-desktop`)**: Tauri v2 기반 데스크톱 앱. TCP 7777 제어 서버 (pull 방식 `getCatalog`, `startStream`, `stopStream`, `getStatus` + Rustra `addNumbers` 위임) + mDNS `_leftcar._tcp.local.` 자동 광고 + 실시간 세션 상태 UI (fps, kbps, 활성 세션 표).
  2. **캡처 심 (`native/macos-capture-shim`)**: HandleTable 기반 멀티 인스턴스 C ABI (`leftcar_capture_start_v2`, `stop_v2`, `stats_v2`, `list_displays`, `free_string`, `last_error_v2`), ScreenCaptureKit + VideoToolbox H.264 하드웨어 인코딩, 90fps 기본 / 120fps 상한, 비트레이트 dynamic clamp, 연결 끊김 시 자동 세션 정지.
  3. **뷰어 렌더러 (`native/android-viewer`)**: `leftcar_jni_attach_port`로 포트 파라미터화(5000+n), TCP 수신 -> AMediaCodec 하드웨어 디코딩 -> Surface 렌더링.
  4. **RN 뷰어 (`apps/viewer-expo`)**: Expo 57 / RN 0.86, Android OS 멀티윈도우 지원 (`android.window.PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI`, `documentLaunchMode="always"`, `resizeableActivity="true"`), NsdModule (mDNS NSD 자동 호스트 발견), StreamLauncherModule (인스턴스별 독립 OS 창 생성), TCP 제어 클라이언트(`control.ts`) 및 UI(`host.tsx`, `catalog.tsx`).
- **검증**:
  - `cargo test --workspace`: 통과
  - `cargo test` (`apps/host-desktop/src-tauri` - 단위 + e2e): 9 tests 전부 통과
  - `pnpm test` & `pnpm test:architecture` & `pnpm test:contract`: 통과
  - Swift shim dylib 컴파일 & `swift tools/capture_host.swift --list`: 디스플레이 목록 정상 반환
  - 단일 스트림 90fps/1080p 및 다중 스트림 독립 창 수명주기/자동 stop 검증.

## 페어링 + 미디어 출발지 검증 (E11, 2026-08-20 추가)

- **구현 상세**:
  - Viewer에서 `openStream` 호출 시 연결된 control host IP를 네이티브로 전달해 Native에서 사용.
  - Kotlin `StreamLauncher`/`StreamActivity`는 host를 intent로 전달 후 `ViewerNative.attachSurfacePort` 호출 시 넘김.
  - JNI 수신 루프는 `accept()` peer의 IP와 control host를 비교해 불일치 시 드롭.
  - `StartStreamInput.viewer_ips`는 호환성을 위해 `viewer_ips` alias를 유지하되 폐기(deprecated) 처리.

- **검증**:
  - `pnpm test:contract`: 통과
  - `cargo test --workspace`, `cargo test -p control-contract`: 통과
