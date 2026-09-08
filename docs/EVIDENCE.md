# 구현 증거 문서 (EVIDENCE)

기준일: 2026-09-01
작성 근거: docs/README.md 검증 수준(E0–E7) 규칙. 이 문서는 달성한 증거와 대기 중인 증거를 구분한다. **E5 이상을 달성했다고 표기한 항목은 없다.**

## 요약

| 수준 | 상태 | 비고 |
| --- | --- | --- |
| E0 설계 | 달성 | docs/01–10, ADR-0001..0004 |
| E1 단위/property | 달성 | cargo test --workspace (전 crate) |
| E2 통합 | 달성 | L5 loopback (transport-api tests), C ABI 왕복 |
| E3 빌드 | 부분 달성 | Rust workspace + TS 전부; Android aarch64와 Windows MSVC compile. Windows NSIS CI는 push 후 판정 |
| E4 에뮬레이터 | 미달성 | H05/H08 단계. 에뮬레이터 잡 미연결 |
| E5 Galaxy XR 실기기 | 부분 달성 | Galaxy XR는 없음; **동일 Android 16 실기기(TB710FU)에서** Expo RN 앱 구동, HW 디코더 1/4/6/8개 동시, multi-instance task 분리, 60/90fps 실측 |
| E11(신규) 페어링 + 미디어 출발지 검증 | 달성 | QR 페어링 + 토큰 인증 경로 유지, 미디어 역방향 peer 일치 검사 |
| E12(신규) 네이티브 원격 입력 | 빌드·계약 달성, 실기기 대기 | Host 세션별 opt-in + macOS 접근성 권한 + 인증 UDP 입력 경로. 60fps→120Hz, 90fps→180Hz 목표의 장치 계측은 미달성 |
| E13(신규) Windows 원격 Host | 소스·교차 컴파일 달성, 물리 실행 대기 | WGC monitor capture + Media Foundation hardware H.264 + SendInput + NSIS/Windows CI. 실제 Windows/GPU/Viewer E6는 미달성 |
| E9(신규) Expo+Rustra 실기기 | 부분 달성 | 기존 `11ff71f` 경로는 JS → NativeModules.Rustra → JNI → rustra invoke_json으로 addNumbers(20,22)=42 + contract hash를 앱 화면에서 실측했다. 현재 `0.8.0`/`a9e3ee6` 핀(npm `@rustra/react-native` 0.7.0 / `@rustra/types` 0.8.0)은 코드젠(+`--check` 드리프트 게이트)·계약 테스트·Android native/Kotlin/APK 빌드까지 재검증했고 실기기 재검증은 대기 중이다. |
| E10(신규) RN 뷰어 + Tauri 호스트 재구축 | 달성 | v1 재구축: Tauri 호스트(제어 pull + 비디오 push) + RN 뷰어(OS 멀티윈도우, 소스당 창) + shim v2 다중 핸들 + NSD 자동발견 |
| E6 종단간 | 부분 달성 | Mac CGDisplayStream → VideoToolbox → LAN UDP → TB710FU Qualcomm 저지연 디코더의 실제 화면 갱신 확인. 실제 포인터·키 입력과 4K60은 대기 |
| E7 계측 장시간 | 미달성 | H51 대기 |

## 달성한 증거 상세

### E1 — 단위/property 테스트

| 영역 | 위치 | 대표 테스트 | 문서 근거 |
| --- | --- | --- | --- |
| 도메인 | `crates/domain` | `acquire_release_never_negative`(proptest), `rapid_focus_changes_do_not_thrash_encoder`, `allocator_never_exceeds_total_budget`(proptest), `diagnostics_redact_title_path_token_and_ip`, `all_stable_errors_have_user_recovery_mapping` | docs/05 §5.5, docs/07 §16/§18 |
| 리스/스케줄 | `crates/domain/lease.rs` | `last_lease_stops_after_debounce`, `task_removal_releases_exactly_one_lease` | docs/03 §3.3 |
| 미디어 | `crates/media-model` | `delta_before_keyframe_is_dropped`, `old_epoch_is_dropped_after_resize`, `duplicate_fragment_does_not_duplicate_output`, `fragment_flood_stays_within_memory_bound`, `queue_never_exceeds_configured_bytes` | docs/05 §5.4 |
| 프로토콜 | `crates/network-protocol` | `oversized_control_message_allocates_nothing_large`, `version_mismatch_is_fatal`, `high_rate_input_is_absent_from_json_control_plane`, fuzz smoke `arbitrary_bytes_never_panic_envelope_parser` | docs/07 §13, docs/05 §9.3 |
| 계약 | `crates/control-contract` | `host_add_numbers_20_22_is_42`, `viewer_contract_does_not_expose_high_rate_input_commands`, `video_payload_type_is_absent_from_generated_typescript`, `generated_contract_hash_is_stable` | docs/08 H02/H09 수용기준 |
| 원격 입력 | `native/android-viewer` + Host control | `pointer_rate_is_twice_stream_fps`, `pointer_updates_are_coalesced_at_target_rate`, `reliable_event_retries_until_authenticated_ack`, `release_all_supersedes_pending_input_and_pointer_motion`, `remote_input_is_host_opt_in_per_session` | docs/07, Apple 화면 공유 비교 기준 |
| 세션 | `crates/session` | `new_offer_expires_after_two_minutes`, `replayed_offer_is_rejected`, `guessed_source_id_fails_authorization`, `revocation_closes_existing_streams`, `backoff_respects_max_delay_and_budget` | docs/07 §7/§9/§18 |
| 호스트 | `crates/host-core` | `unapproved_source_cannot_start`, `approved_source_starts_once_for_first_lease`, `one_capture_failure_does_not_stop_other_sources`, `stop_all_is_idempotent`, `no_orphan_capture_after_close` | docs/05 §5.2 |
| 뷰어 | `crates/viewer-core` | `unique_source_opens_unique_document_task`, `hub_close_keeps_stream_tasks_alive`, `decoder_output_never_configured_without_surface`, `restored_task_reauthenticates_before_decode`, `attach_once_detaches_at_most_once` | docs/05 §5.3/§6.2/§8.2 |
| C ABI | `native/android-viewer` | `six_abi_symbols_roundtrip`, `double_detach_is_state_error_not_crash`, `invalid_lifecycle_code_rejected` | docs/05 §8.2, docs/07 §14 |
| 진단 | `crates/diagnostics` | `export_contains_no_title_path_token_ip_frame`, `run_scoped_hash_is_stable_within_run` | docs/07 §16 |
| 아키텍처 | `tools/architecture-check` + `ts.ts` | ADR-0002 의존성 방향, domain 순도, Kotlin import allowlist, video-plane-no-contract | docs/03 §4.1 |
| UI(TS) | `apps/*/src` | 상태 그래프, launch-handle 정책, 한글 상태 문구(오류 코드 미노출), Hub open/focus 정책 | docs/01 §6, docs/08 H04 |

실행: `cargo test --workspace && bun run test && bun run test:contract`

### E2 — 통합

- **L5 loopback** (`crates/transport-api/tests/loopback.rs`): FakeEncoder→packetize→transport→assembler→FakeDecoder. 1/4-소스 멀티플렉스 교차 없음, 3% loss 전달, outage→IDR 복구(NFR-004 논리), lease 기반 소스 격리(NFR-005 논리). Simulated(5 profile) + InMemory 양쪽.
- **Rustra 실경로**: 기존 pin `11ff71f`에서 `addNumbers 20+22=42`가 실제 invoke 경로로 증명됐다(에뮬레이터/문자열 mock 아님). 현재 pin `f8bab299`(Rustra `0.4.0`)은 생성 계약 해시 갱신, Rust/TypeScript 계약 테스트, Android native/Kotlin/APK 빌드를 통과했지만 같은 실기기 경로는 다시 확인해야 한다.
- **C ABI 왕복**: 6심볼 roundtrip, panic 미통과(catch_unwind), null/double-detach/instance-crossing 거부.

### E3 — 빌드/구조

- `cargo check --workspace` 전 crate green, clippy `-D warnings` 0.
- `apps/host-desktop/src-tauri`는 `x86_64-pc-windows-msvc` 교차 `cargo check --lib`를 통과했다. 이 결과는 Windows API 타입/cfg의 E3 소스 검증이며 Windows 실행 또는 installer 생성 증거가 아니다.
- TS typecheck 2앱 green, `bun run test:architecture` TS/Kotlin 규칙 green.
- Kotlin shim은 `android/.../shim/` 경로 + manifest(documentLaunchMode=always, PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI)에 있고 `:app:compileDebugKotlin`과 `:app:assembleDebug`를 통과했다. Debug APK에는 arm64 `libleftcar_viewer.so`와 `libleftcar_rustra.so`가 포함되고 v2 서명이 유효하다. 이는 내부 빌드 증거이며 설치·실행 증거는 아니다.
- macOS 파사드(macos-capture/macos-encode): 실API 링크 없이 구조만. 시작 시 `RealBackendNotLinked` 명시 실패(무인 소프트웨어 fallback 금지).

## 대기 중인 증거 (E4–E7) — 판정 진입점

| 증거 | 게이트 | 필요 장치 | 판정 방법 |
| --- | --- | --- | --- |
| 같은 APK 창 4개 동시 표시 | G1/H05 (E5) | Galaxy XR | `adb shell dumpsys activity` task dump + 10분 창 유지 관찰; docs/06 §7.1 W4 |
| 비초점 창 Surface 갱신 지속 | G1/H06/H08 (E5) | Galaxy XR | frame counter 4개 10분 진행 (docs/02 F-02) |
| AMediaCodec 4 디코더 1080p30 | G1/H07-H08 (E5) | Galaxy XR | golden H.264 4개 동시 재생, thermal 기록 (docs/02 F-03) |
| 실제 Mac 창→표시 장시간 종단간 | G3/H20 (E6) | Mac+Galaxy XR | TB710FU 단기 실시간 표시는 확인. S1 10분 + resize/close/outage 시나리오는 대기 (docs/08 H20) |
| glass-to-glass p50/p95 | H21 (E7) | 240fps 카메라 | docs/06 §5 절차, 200 sample |
| transport bake-off | G2/H11–H14 (E5) | Galaxy XR | docs/06 §9 동일조건 비교, ADR-0004 갱신 |
| 60분 soak/latency creep | H36/H51 (E7) | Galaxy XR | NFR-002/006/007/008 |

위 항목들은 장치 확보 시 순서대로 실행한다. 이 문서의 표는 그때 갱신한다.

## 구현 범위 참고

- transport-quic: 구현 미포함(의도적). ADR-0004가 bake-off 전 확정 금지. `ProductBuildInfo::new(Undecided)`가 product build를 거부하는 것으로 대체(H14 Red).
- macos-capture/macos-encode: Swift/C ABI shim과 실제 SCK/VT 세션은 H16–H18 장치 단계.
- React Native/Gradle: 소스+스펙+manifest만. RN host 연결은 H05.
- Windows Host: P7 source 구현과 MSVC 교차 compile까지 진행. Windows CI NSIS artifact와 물리 E6/E7은 대기. Linux Host는 미착수.


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
  3. **뷰어 렌더러 (`native/android-viewer`)**: `leftcar_jni_attach_port`로 포트 파라미터화(5000+n), MTU 단위 UDP H.264 AU 재조립 -> AMediaCodec 하드웨어 디코딩 -> Surface 렌더링. 유실 감지 시 즉시 IDR를 요청한다.
  4. **RN 뷰어 (`apps/viewer-expo`)**: Expo 57 / RN 0.86, Android OS 멀티윈도우 지원 (`android.window.PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI`, `documentLaunchMode="intoExisting"`, `resizeableActivity="true"`), NsdModule (mDNS NSD 자동 호스트 발견), StreamLauncherModule (서로 다른 `host:port`는 독립 OS 창, 동일 스트림 재접속은 기존 task 재사용), TCP 제어 클라이언트(`control.ts`) 및 UI(`host.tsx`, `catalog.tsx`).
- **검증**:
  - `cargo test --workspace`: 통과
  - `cargo test` (`apps/host-desktop/src-tauri` - 단위 + e2e): 9 tests 전부 통과
  - `bun run test` & `bun run test:architecture` & `bun run test:contract`: 통과
  - Swift shim dylib 컴파일 & `swift tools/capture_host.swift --list`: 디스플레이 목록 정상 반환
  - 단일 스트림 90fps/1080p 및 다중 스트림 독립 창 수명주기/자동 stop 검증.

## 페어링 + 미디어 출발지 검증 (E11, 2026-08-20 추가)

- **구현 상세**:
  - Viewer에서 `openStream` 호출 시 연결된 control host IP를 네이티브로 전달해 Native에서 사용.
  - Kotlin `StreamLauncher`/`StreamActivity`는 host를 intent로 전달 후 `ViewerNative.attachSurfacePort` 호출 시 넘김.
  - JNI 수신 루프는 UDP 발신자의 IP와 control host를 비교해 불일치 시 미디어 데이터그램을 드롭한다.
  - 비우회 VPN이 제어 연결을 서브넷 라우터로 보낼 수 있으므로 Viewer는 물리 Wi-Fi IPv4를 `StartStreamInput.viewer_ips` 후보로 전달한다. Host는 제어 peer와 같은 사설 `/24` 후보만 고려하고, 캡처 전에 예측 불가능한 난수의 UDP 왕복을 증명한 주소만 사용한다.
  - Viewer의 IDR/종료 데이터그램도 같은 난수를 포함해야 하므로 경로만 위조한 비인증 패킷은 제어로 처리하지 않는다.

- **검증**:
  - `cargo test --workspace`, `cargo test -p control-contract`: 통과 (net_guard 단위 테스트 포함).
  - `cargo test` (`apps/host-desktop/src-tauri`): 관련 없는 `viewer_ips`를 거부하고 제어 peer로 폴백하는 e2e 통과.
  - JNI 입장 허용 경로는 `cargo check --target aarch64-linux-android -p android-viewer`로 컴파일 검증만 수행(기기 실행 검증 아님).
- `bun run test` / `bun run typecheck` / `bun run test:contract` / `bun run test:architecture`: 통과.

## 네이티브 원격 입력 (E12, 2026-08-22 추가)

- **구현 경로**: Android `MotionEvent`/`KeyEvent` → Kotlin shim → JNI → Rust 입력 스케줄러 → 기존 스트림 UDP 소켓의 세션 난수 인증 데이터그램 → macOS `CGEvent`.
- **빈도 정책**: 포인터 목표 전송률은 스트림 FPS의 2배이며 30–240Hz로 제한한다. 포인터 이동은 최신 좌표만 유지하고, 키·버튼·휠·전체 해제는 sequence/ACK/20ms 재시도를 사용한다.
- **권한·안전**: 모든 스트림은 Observe로 시작한다. macOS 손쉬운 사용 권한과 Host의 세션별 Control 토글을 모두 만족해야 입력을 주입한다. 포커스 이탈, Surface 종료, 연결 재수립, Host 토글 OFF에서 눌린 키와 버튼을 해제한다.
- **검증 완료**:
  - `cargo test --workspace`: 입력 스케줄러·wire 계약 8개를 포함한 Rust 전체 테스트 통과.
  - Host Tauri Rust 단위 23개 + e2e 8개 통과, `cargo clippy --workspace --tests -- -D warnings` 통과.
  - Android arm64 Rust release 빌드와 `:app:assembleDebug` 통과. 완성 APK의 dynamic symbol table에서 `sendPointer`, `sendKey`, `releaseInput` JNI 3개를 확인했다.
  - macOS Swift shim을 ScreenCaptureKit, VideoToolbox, CoreGraphics에 링크하여 dylib 컴파일 통과.
  - `bun run typecheck`, TS/계약/아키텍처 테스트, React Doctor 통과.
- **2026-08-23 실기기 확인**: Lenovo TB710FU에 release APK를 기존 `leftcar.ll3.kr` 패키지로 덮어 설치했다. 1080p60 세션에서 네이티브 디코더가 지속 렌더링했고 `inputDrops=0`을 확인했다. 인증된 `LCP1/LCP2` 프로브로 `NET` 왕복 네트워크 지연과 `M→A` Mac 전송→Android 수신 지연을 분리했으며, 실제 화면에서 `NET 11 ms · M→A 7 ms · 54 FPS · DEC 4 ms · SKIP 59 LOSS 2`를 관찰했다. 상단 HUD와 작은 잠금 벡터 아이콘은 입력 시 나타나고 easing으로 사라졌으며, 약 7초 뒤 두 표시가 모두 제거된 화면도 확인했다. APK 업데이트 뒤 `firstInstallTime=2026-08-22 11:27:19`가 유지돼 별도 앱 설치가 아닌 기존 패키지 갱신임을 확인했다.
- **아직 증명하지 않은 항목**: Host 입력 잠금 해제 후 Mac에 대한 실제 포인터·키보드 적용, 120/180Hz wire rate 측정, 키보드 레이아웃·수식키·멀티모니터 좌표, 장시간 유실/재연결 복구, 실제 4K 소스의 4K60 스트림, Apple 화면 공유와의 동일망 비교는 E6/E7 실기기 게이트로 유지한다.

## Windows 원격 Host (E13, 2026-08-22 추가)

- **구현 경로**: Tauri platform factory → Windows display catalog → `CreateForMonitor` WGC free-threaded frame pool → D3D11 texture → `MFCreateDXGISurfaceBuffer`/`IMFDXGIDeviceManager` → `MFT_ENUM_FLAG_HARDWARE` Media Foundation H.264 MFT → 기존 CFG/Annex-B/fragment → Android MediaCodec. CPU staging readback과 CPU BGRA→NV12 변환은 제거했다.
- **입력 경로**: 기존 인증 UDP `LCI1/LCA1` 계약 또는 USB AOAP framed 양방향 경로 → capture와 독립된 Windows input worker → session별 Control 승인 → `SendInput`. 포인터 2× FPS와 reliable ACK/retry 정책은 macOS와 동일하다.
- **실패·권한 정책**: Windows backend 또는 hardware H.264 MFT가 없으면 Fake/software backend로 조용히 전환하지 않는다. SendInput은 Windows UIPI 경계를 따르며 관리자 프로세스 제어를 우회하지 않는다.
- **패키징**: `tauri.windows.conf.json`은 current-user NSIS를 지정하고, `windows-host` CI job은 Windows unit/clippy 뒤 unsigned installer artifact를 생성한다.
- **현재 검증**: macOS에서 `x86_64-pc-windows-msvc` target `cargo check --lib` 통과. platform-neutral wire/input sequence unit test 통과.
- **아직 증명하지 않은 항목**: Windows runner의 NSIS CI 결과, 물리 Windows WGC frame, 실제 GPU encoder identity, Windows→Android E6, 120/180Hz input 측정, DPI/회전/다중 monitor, sleep/wake/device-loss, 60분 soak. 상세 수용 기준은 `docs/windows-remote-host.md`에 유지한다.

## 배포 크기 최적화 (E14, 2026-08-23 추가)

- **Android 기준선과 결과**: arm64 release APK를 `43,473,166` bytes에서 `19,733,916` bytes로 줄였다(약 54.6%). R8 코드 최적화에 resource shrink, 한국어/영어 resource configuration, Metro bundle 압축을 결합하고, 직접 사용하지 않는 Gesture Handler/Reanimated/Worklets 자동 링크 및 RN GIF/WebP 디코더를 제외했다. 직접 APK 배포를 위해 native `.so`는 legacy packaging으로 압축한다.
- **기능 보존 경계**: QR 페어링에 필요한 `expo-camera`, `libbarhopper_v3.so`, CAMERA 권한과 Leftcar의 `libleftcar_viewer.so`/`libleftcar_rustra.so`는 APK에 유지했다. 패키지는 `leftcar.ll3.kr`, ABI는 `arm64-v8a` 한 개다.
- **Desktop 기준선과 결과**: 서명된 macOS `Leftcar Host.app`을 11MB에서 5.0MB로 줄였다. standalone Tauri crate의 release profile에 size 최적화, full LTO, 단일 codegen unit, symbol strip을 적용했다. 설치 앱은 기존 identifier와 TeamIdentifier를 유지하며 deep/strict codesign 검증을 통과했다.
- **회귀 검증**: Android release assemble, Desktop release bundle, Rust workspace, Host 30 unit + 8 E2E, TypeScript, UI/제어 테스트, 4 contract 테스트, architecture check를 통과했다. UI 접근성·시맨틱 요소·React Native shadow 스타일을 수정한 뒤 React Doctor `100 / 100`과 0 issues를 확인했다. 최종 APK를 기존 `leftcar.ll3.kr` 앱에 덮어 설치해 최초 설치 시각이 유지되는 것을 확인했고, 실기기에서 4K60 프로필 표시와 1080p H.264 하드웨어 디코더의 지속 프레임 렌더링(`inputDrops=0`)을 확인했다.

## 저지연 계측과 실기기 재검증 (E15, 2026-08-23 추가)

- **단계별 계측**: macOS 미디어 데이터그램 `L2` 헤더에 capture/encode/send wall-clock을 싣고 Android에서 `NET RTT`, `CAP→DEC`, `ENC→DEC`, `WIRE→DEC`를 10초 rolling p50/p95로 표시한다. `DEC`는 MediaCodec 입력 제출 시점이며 물리 패널의 photon 시점이 아니다.
- **저지연 경로**: Android는 전용 control/probe UDP socket과 EF DSCP, 미디어는 AF41 DSCP를 사용한다. 미디어 수신·송신 버퍼를 줄이고, ScreenCaptureKit queue depth를 2로 제한했으며, 80ms를 넘은 의존 체인은 버리고 IDR를 요청한다.
- **실제 디코더**: TB710FU 로그에서 `actualCodec=c2.qti.avc.decoder.low_latency`와 codec low-latency 값 활성화를 확인했다. 범용 MIME 선택이 아니라 장치의 Qualcomm 저지연 AVC decoder가 선택됐다.
- **실제 화면 표본**: Android HUD에서 `NET 9/10 ms`, `CAP→DEC 25/29 ms`, `ENC→DEC 8/11 ms`, `WIRE 8/11 ms` 표본을 확인했다. 이후 native 로그 표본의 `captureAgeMs`는 주로 21–33ms, `encodeAgeMs`와 `wireAgeMs`는 주로 3–15ms였다.
- **Host 5회 반복 표본**: 실제 running 세션은 59–61 FPS, 1,071–2,034 kbps였다. capture→encode p95는 18.883–19.096ms, queue wait p95는 0.460–0.702ms, send block p95는 1.721–2.061ms였고 `pendingFrame=0`, `error=null`이었다.
- **90 FPS 회귀와 수정**: 약 60Hz인 현재 Mac 캡처 소스에 90 FPS operating-rate를 요청한 표본은 Android HUD가 39 FPS, SKIP 174까지 악화됐다. 동일 빌드의 60 FPS 표본은 55–56 FPS로 회복했다. 기본 저지연 프로필을 1080p60으로 복구하고 입력 폴링은 2배인 120Hz로 유지했다. 실제 90Hz 이상 소스·Surface를 런타임에서 함께 확인하기 전에는 90 FPS를 기본값으로 다시 노출하지 않는다.
- **입력 검증 경계**: wire/스케줄러 테스트와 180Hz 목표 설정은 통과했지만 이 표본의 Host 세션은 `inputEnabled=false`였다. 따라서 실제 Mac 포인터·키 주입과 click-to-photon은 아직 달성으로 표기하지 않는다.
- **배포 산출물**: arm64 release APK는 v2 개발 서명으로, macOS app/DMG는 기존 개발 서명 ID로 검증했다. DMG checksum 검증은 통과했지만 Apple notarization은 수행하지 않았다.
- **2026-08-24 현재 빌드 실기기 재검증**: 서명된 macOS Host를 다시 빌드·설치하고 최신 debug APK를 Lenovo TB710FU에 덮어 설치한 뒤, `Display 0`의 Mac 화면이 `c2.qti.avc.decoder.low_latency`로 실제 갱신되는 것을 확인했다. HUD 표본은 `SRC 60 / XR 60 Hz`, `NET 13/21 ms`, `CAP→SCR 40/43 ms`, `55 FPS`, `FEED 5 ms`, `SKIP 461`, `LOSS 32`였다. 같은 세션의 Host 표본은 `60 FPS`, `4,125 kbps`, `pendingFrame=0`, `udpSendFailures=0`, capture→encode p95 `18.969 ms`, send block p95 `2.621 ms`였다. 이 값은 패널 photon 측정이 아닌 Surface render 직전 시점이다.
- **종료·고장 감지 실기기 재검증**: 인증된 `stopStream`에 종료 사유 `2`를 전달했을 때 Viewer가 `host terminated stream: reason=2 (forced stop)`을 기록하고 스트림 Activity를 닫아 메인 Activity로 복귀했으며, BYE에 의한 세션 재생성은 없었다. 별도 세션에서 Viewer 프로세스를 강제 종료하자 Host가 약 6초 뒤 `viewer connection lost (feedback timeout)` 오류로 전환했고, 오류 상태를 약 5초 보존한 다음 세션을 제거했다.
- **현재 판정 경계**: 이번 실시간 표본은 약 3분이며 장시간 soak, outage 후 자동 재연결, glass-to-glass 카메라 측정은 수행하지 않았다. `SKIP`/`LOSS`와 Host drop이 누적됐으므로 E7과 무손실 판정은 계속 미달성으로 유지한다.

## 회복 정책 재설계 (2026-08-26 구현 기록)

- **소스 구현**: UDP VideoToolbox GOP를 `3600` 프레임으로 전환해 정기 IDR 대신 인증된 IDR 요청 경로를 회복 경계로 사용하고, Viewer IDR 요청 debounce를 `750ms`에서 `250ms`로 줄였다.
- **히스테리시스**: 단일 stale delta를 즉시 폐기하지 않고 연속 3개 초과 프레임에서만 재동기화하는 순수 결정 함수와 단위 테스트를 추가했다. 1~2개 초과 프레임은 늦게라도 디코더 경로로 진행한다.
- **계측 경계**: `staleInputDrops`와 `outputBurst`를 별도 Atomic 카운터 및 `leftcar_jni_skip_breakdown`으로 노출했다. 기존 HUD 합산 필드는 하위 호환을 위해 유지한다.
- **실기기 판정**: 기존 E15 표본의 `SKIP 461`을 새 빌드에서 재수집하지 않았으므로 SKIP 감소, 복구 p95 약 50ms, 목표 `SKIP < 50`은 측정 대기로 남긴다. E15와 같은 수집 방식으로 비교해야 한다.

## FEC·ABR·Windows 제로카피 (2026-08-26 구현 기록)

- **FEC 코어**: 계획의 `nanors` URL은 Cargo crate가 아닌 C 저장소로 확인되어, 별도 GPL 코드를 vendoring하지 않고 `crates/fec-core`에 순수 Rust GF(256) Reed-Solomon 코어를 구현했다. Host macOS/Windows 송신 경로와 Android 수신 복구 경로가 같은 코어를 사용하며, 8+2 손실 복구·3손실 실패·짧은 tail 그룹 테스트를 통과했다.
- **wire/receiver**: 기존 `G` 데이터그램은 유지하고 `P` 패리티 마커와 bounded FEC group을 추가했다. 복구 실패는 기존 frame-gap/IDR 경로로 되돌아가며, FEC 자체가 제어 패킷을 복구한다고 주장하지 않는다.
- **ABR**: `LCF1` 후위 확장으로 `staleInputDrops`와 `outputBurstDiscards`를 전달하고, Swift ABR 혼잡 신호는 분리된 stale 입력 드랍을 사용한다. 구형 피드백 길이는 기존 6개 필드로 계속 파싱한다.
- **Windows GPU source path**: WGC D3D11 texture를 staging `Map(CPU_READ)` 또는 `bgra_to_nv12` CPU 변환 없이 `MFCreateDXGISurfaceBuffer`와 `IMFDXGIDeviceManager`가 연결된 ARGB32 hardware H.264 MFT에 직접 전달하도록 바꿨다. Windows USB transport에서는 동일한 bounded TCP framing을 AOAP loopback proxy에 연결하고, input/ACK도 같은 framed 양방향 경로를 사용한다.

## USB AOAP 전송 (2026-08-26 구현 기록)

- **Host**: `nusb 0.1` AOAP GET PROTOCOL/SEND STRING/START 협상, accessory bulk endpoint 탐색, 1바이트 채널 mux(ch0 제어/ch1 미디어), bounded queue와 nusb hotplug stream을 구현했다. 일반 USB attach에서는 AOAP를 시작하지 않고, 인증된 `requestUsb` 스트림 요청 때만 협상한다. hotplug API가 실패하는 환경은 제한된 enumeration polling으로 폴백한다. Host control relay는 기존 `ControlServer::dispatch`와 token authorization을 재사용한다.
- **Viewer**: Android `UsbManager`/`UsbAccessory` 모듈이 fd를 Rust bridge로 넘기고, Rust bridge가 channel mux·loopback control TCP·renderer UDP side-channel을 제공한다. attach/detach intent와 dynamic receiver를 처리하며 RN은 USB 연결 시 USB를 우선 선택하고 없으면 기존 Wi-Fi auto 경로를 사용한다.
- **검증 완료**: `usb-mux` 단위 테스트, Host unit/E2E, Android native unit 및 aarch64 cross check, Kotlin compile, TypeScript/contract/architecture, React Doctor `100 / 100`을 통과했다.
- **물리 판정 대기**: 실제 AOAP GET PROTOCOL 응답·재열거 VID/PID, 케이블 제거 후 Wi-Fi failover, USB stream frame continuity, 60분 soak는 폰·케이블을 연결한 T11 물리 게이트에서만 달성으로 변경한다. 현재 구현·컴파일 증거는 E3 수준이며 E6/E7을 대신하지 않는다.

## 4K HEVC 저지연 경로 (2026-08-26 구현·실기기 재검증)

- **wire 호환 경계**: 기존 H.264 `CFG`와 `G` fragmentation은 유지하고, codec id와 VPS/SPS/PPS를 담는 `CF2`를 추가했다. Android는 `video/hevc`의 `csd-0/1/2`와 `video/avc`의 기존 `csd-0/1`을 각각 사용한다.
- **Host 선택 정책**: 3840x2160 이상 video profile에서 VideoToolbox HEVC를 먼저 준비하고, 준비·생성·키프레임 설정에 실패하면 같은 시작 시도 안에서 H.264로 폴백한다. interactive/1080p 경로는 H.264를 유지한다.
- **실기기 확인**: Lenovo TB710FU에 release APK를 덮어 설치했다. 4K video profile에서 `Received CF2 datagram (95 bytes)`, `vps=28B sps=40B pps=11B`, `codec=Hevc actualCodec=c2.qti.hevc.decoder.low_latency`를 확인했고, 장시간 관찰 중 `Rendered` 카운터가 계속 증가하며 종료·recovery gate timeout은 발생하지 않았다.
- **현재 성능 판정**: Host UI와 native 로그의 실제 출력은 약 34–37 FPS였고, video processing은 약 65–81ms, UDP send failure는 0이었다. 고정 0 FPS로 멈추던 recovery gate는 timeout 해제로 회복됐지만, 4K 55–60 FPS는 아직 달성하지 못했다. 고속 화면에서 capture/recovery drop과 burst가 늘어 남은 병목은 macOS 4K capture/encode path로 판정한다.
- **현재 APK 증거**: 재빌드·재설치한 APK SHA-256은 `8767f4ebfd268fbaec0e1a8cf017fb30eca7105f91f9e1158c1a791d9cf10748`이다. 이 표본의 미디어 전송은 `Wi-Fi UDP`였으며 USB/AOAP 물리 스트림 증거로 해석하지 않는다.

## H-03/H-04 원인 분리 구현 및 정적 smoke (2026-08-26)

- **구현**: Android `frame-id` 점프를 실제 `networkLoss`, 의도적인
  `liveEdgeDiscard`, 이미 IDR을 기다리며 건너뛴 `recoverySkip`으로 분리했다.
  live-edge로 중간 delta를 버린 뒤 선택된 delta는 참조 체인 손상을 피하기
  위해 다음 keyframe까지 버리며, 같은 recovery episode에서 IDR을 반복
  요청하지 않는다. Host에는 userspace media queue의
  `pendingFrameBytes`와 `pendingFrameOldestAgeUs`를 추가했다.
- **검증**: 관련 Android native 순수 로직 회귀 6개와 전체 native 51개,
  `viewer-decoder`, Rust workspace/Host clippy, Swift policy, TypeScript와
  React Doctor `100 / 100`을 통과했다. arm64 native release build와
  release APK assemble/install도 통과했다.
- **짧은 실기기 smoke**: `HA2D6EMP`에 APK SHA-256
  `9333620c1cf9a79b19f5cc857e5b3630a90a97482a8ecc5f54bf0c6d1f6f0858`을
  설치했다. `2560×1440@60`, H.264, `CgDisplayStream`, Wi-Fi UDP에서
  `actualCodec=c2.qti.avc.decoder.low_latency`, `Rendered 150`,
  `decoderInputDrops=0`, `outputDrops=0`, `frameGaps=0`,
  `intentionalLiveEdgeGaps=0`을 확인했다. 이는 정적 장면 smoke이며
  고모션 7~15fps 해결 또는 USB/AOAP 경로 증거가 아니다.

## 복구 경계·고모션 재검증 (2026-08-26)

- **복구 경계 수정**: Host가 이미 복구 키프레임을 전송 중이거나 네트워크 큐에 보유한 상태에서 반복 IDR 요청을 받아도, 대기 중인 복구 경계를 다시 지우지 않도록 `shouldStartNetworkRecovery` 정책을 추가했다. 큐 overflow에서도 이미 queued keyframe을 보존하고, 같은 recovery episode의 delta만 버린다.
- **1440p 고모션 표본**: `HA2D6EMP`에서 `2560×1440@60`, H.264, `CgDisplayStream`, Wi-Fi UDP를 60초 유지했다. 캡처는 60fps였지만 후반 Host 표본은 submit 58fps, output 57fps, Android render 53fps였고 `decoderInputDrops=0`이었다. Android 누적 `frameGaps=72`, `outputDrops=320`, `fecRecovered=249`까지 증가해 복구·고모션 구간은 아직 60 unique fps로 판정하지 않는다.
- **복구 경계 A/B**: queued recovery boundary 보존과 1440p 이상 encoder in-flight 5개 변경 후 별도 30초 표본에서는 Android 로그가 약 56fps 수준으로 유지됐고 Host 상세 표본은 `capture/submit/output/rendered = 60/60/60/60`이었다. 그러나 `frameGaps=9`, `outputDrops=57`, `recoveryFramesDropped=126`이 남아 장시간 무손실·60fps 달성으로 해석할 수 없다.
- **4K 영상 프로필 비교**: 같은 기기에서 `동영상 우선` 프로필은 실제 `3840×2160@60` HEVC 경로로 시작되었다. Host 표본은 capture 약 39–46fps, output 약 30–35fps, Android render 약 1–37fps, video processing 약 74–80ms였고 UDP send failure는 0이었다. 따라서 이 환경에서 영상 프로필 전환만으로 4K 고모션 병목이 해결되지 않으며, 1440p를 안정적인 fallback으로 유지한다.

## USB 화면 확장 3종 (2026-09-01 구현 기록)

feature/usb-display-extension 브랜치(커밋 `2aaa319`..`441e94f`, 14개 커밋)의 구현 증거다. **모든 항목은 E3(컴파일/단위 테스트) 수준이며 E6/E7의 증거가 아니다.** 물리 게이트(실기기 USB 검증, 60분 soak, BetterDisplay 실기기 동작)는 미수행이며 `docs/usb-physical-validation.md`의 절차로 대기 중이다. `cargo test`와 APK 설치는 이 항목들의 E6/E7 증거가 아니다.

### 1. 전송 방식 배지 (Viewer)

- **구현**: `apps/viewer-expo/src/transport-label.ts`의 `transportBadgeLabel`이 control contract의 `media_transport` 값(`usb`/`udp`/`tcp`/legacy)을 `USB`/`Wi-Fi`/`Wi-Fi (TCP)`/`ADB` 배지 문자열로 매핑한다. `app/catalog.tsx`의 ActiveStream 카드가 `stream.mediaTransport` 기반으로 렌더링한다.
- **검증 (E3)**: `apps/viewer-expo/src/transport-label.test.ts` 단위 테스트 4개가 매핑 계약을 고정한다. `bun run test`, `bun run typecheck` 통과.
- **미증명**: 실기기에서 실제 USB/Wi-Fi 전환 시 배지 갱신이 화면에 표시되는 것(검증 2·3의 관찰 항목).

### 2. Host 창 표시 개선 + panic 제거

- **구현**: Host `lib.rs`가 시작 시 main 창 `show()`+`set_focus()`를 보강했고(`tauri.conf.json` `visible: true` 포함), 시작 실패 시 panic 대신 `fatal_startup_error` rfd 오류 대화상자를 표시한다. bind 실패 메시지에 "another Leftcar Host instance may be running" 힌트를 포함한다.
- **검증 (E3)**: 기존 bind 관련 단위 테스트는 유지되어 통과한다. `fatal_startup_error`는 rfd 대화상자 호출을 수행하므로 자동 검증 범위 밖이며, 대화상자 표시 자체는 자동 테스트로 증명하지 않는다.
- **미증명**: "창이 안 뜬다"는 실제 사용자 환경에서의 재현·해소 확인.

### 3. 문제 해결 가이드 확장

- **구현**: `packages/ui-tokens/src/i18n.ts`에 "컴퓨터 앱 창이 안 떠요" 항목(`troubleshootHiddenWindow`)의 ko/en 쌍을 추가했다. Host `App.tsx`와 Viewer `host.tsx`가 렌더링한다.
- **검증 (E3)**: i18n 키는 타입 시스템이 ko/en 쌍 존재를 강제한다. 렌더링은 E3 컴파일·타입 수준까지다. `bun run typecheck` 통과.
- **미증명**: 실제 화면 렌더링 관찰(E6 수준).

### 4. 가상 디스플레이 실험 (ADR-0005)

- **구현**: `apps/host-desktop/src-tauri/src/virtual_display.rs`가 `betterdisplaycli` 프로세스 실행(create/set/discard, `-namelike` 식별자)을 감싸고, Tauri 커맨드(비동기, 메인 스레드 블로킹 회피)와 Host UI로 노출한다. Host App.tsx의 "가상 디스플레이 (실험)" 옵트인 토글(기본 꺼짐, `leftcar_virtual_display_experiment` 설정 영속화)이 꺼져 있으면 VirtualDisplayCard를 렌더링하지 않아 커맨드 호출 자체가 발생하지 않는다. CLI 계약은 `docs/decisions/0005-virtual-display-via-betterdisplay-cli.md`(ADR-0005)에 고정했다.
- **2026-09-02 실기기 정정**: ADR-0005의 `aspectWidth/aspectHeight=16/9`(비율 숫자) 계약은 잘못된 것이었다. CLI는 이 값을 픽셀로 해석하고, HiDPI 기본·자유 multiplier 조합에서 16x9 요청이 6400x4000 백킹 스토어(UI 논리 3200x2000)를 만들어 스트리밍 시 UI가 지나치게 작아졌다. 실기 검증으로 확정한 올바른 계약은 픽셀 지정 + HiDPI off + multiplier 1x 고정이며, 이를 `create_args`와 `validate_dimensions`(800px 미만 비율 숫자 입력 거부)로 코드에 반영했다. 앱 기본값도 태블릿 16:10에 맞춰 1920x1200(WUXGA)으로 바꿨다.
- **검증 (E3)**: CLI 인자 생성 단위 테스트 6개가 정정된 CLI 계약을 고정한다. Host Tauri crate 전체 테스트 통과(65 lib + 10 e2e).
- **검증 (E6, 실기기)**: 정정된 계약과 동일한 argv로 `betterdisplaycli create`→`set -connected=on`을 실행해 `system_profiler`에서 `1920 x 1200 (WUXGA)`, `UI Looks like: 1920 x 1200 @ 60.00Hz`를 확인했고, `discard -namelike`로 제거 확인. 다만 이는 CLI 직접 실행이며 앱 UI(토글→카드→버튼) 경유의 최종 확인은 여전히 사용자 수동 확인 대상이다. 앱 UI 자동 클릭 검증은 시도했으나 이 자동화 셸이 GUI 세션(System Events)과 통신할 수 없어(-10827) 불가능했고, 대신 WebKit localStorage DB에 실험 토글 플래그(`leftcar_virtual_display_experiment=on`)를 주입해 카드 표시 조건만 준비했다.
- **미증명**: 앱 UI 경유 생성→Leftcar 스트리밍 캡처→태블릿 렌더링까지의 전 경로(사용자가 모달에서 카드 확인 → 생성 버튼 → 태블릿에서 스트림 열어 UI 크기 확인). 위험 대장 R-015의 v1 논골 유지는 변함없으며 이 구현이 정식 기능 승격이 아니다.

### 5. USB 물리 검증 게이트 문서

- **구현**: `docs/usb-physical-validation.md`가 AOAP 핸드셰이크, Wi-Fi failover, USB 자동 복귀, 60분 soak, 인텐트 경로, 가상 디스플레이 CLI의 6개 검증 절차와 합격 기준, 실패 시 진단 가이드, 결과 기록 템플릿을 확정했다.
- **상태**: 절차 확정, 수행 대기. AOAP CONTROL 시퀀스 표기를 정정했다(START=53, 54 아님). 이 문서의 수행이 완료되기 전까지 AOAP 전송과 가상 디스플레이는 E3 실험 상태로 유지된다.

## 커서 분리 (LCD1 위치 스트림) — 2026-09-03~04 구현 기록

feat/cursor-separation 브랜치(커밋 `df24d7a`..`922953d`, 13개 커밋)의 구현 증거다. **모든 항목은 E3(구현·단위 테스트·컴파일) 수준이며 E6/E7의 증거가 아니다.**

- **등급**: E3 — 구현·단위 테스트 완료, 실기 미검증.
- **계약**: 뷰어 LCDON 옵트인 → 호스트 CGEvent Tap(listen-only) 관측 → 2×FPS coalescing 상태 스트림. 커서가 LCD1 위에 있고 변경될 때만 `LCD1` 패킷을 제어 경로로 보낸다.
- **층별 구현**:
  - Rust(뷰어): `native/android-viewer`에 LCD1 커서 위치 패킷 파서(토큰 인증, 최신값 수용)와 인증 수립 후 LCDON 옵트인 전송을 연결하고, 수신 atomics에 최신 샘플만 보관한다.
  - JNI: 커서 상태 폴링과 옵트인 설정 export를 추가했다.
  - Kotlin: `CursorOverlayView` 로컬 렌더 오버레이와 Choreographer 폴링을 추가했다.
  - TypeScript: `viewer-preferences`에 원격 커서 로컬 표시 프리퍼런스와 카탈로그 설정 토글을 추가하고 `openStream localCursor` 파이프로 네이티브까지 전달한다.
  - Swift(호스트): `CaptureSession+Cursor` LCD1 코디네이터(2×FPS coalescing, 변경 시만 전송), CGEvent Tap 관측, LCDON/LCDOFF 파싱, capture `showsCursor` 전환을 연결했다.
- **안전장치**: 세션 종료·피드백 타임아웃·shim 해제 시 capture `showsCursor`를 원복해, 뷰어가 LCDOFF 없이 사라져도 비디오 커서를 되돌린다(커서 소실 방지).
- **검증 (E3)**: `cargo test --workspace`와 `cargo clippy --workspace --tests -- -D warnings` 통과. `npx tsc --noEmit`와 vitest 15개 파일 122개 테스트 통과. React Doctor 85개 파일 스캔에서 finding 0(점수 숫자는 Score API 도달 실패로 미산출). `tools/build_apk.sh` assemble 통과, `cargo check -p android-viewer --target aarch64-linux-android` 교차 컴파일 통과.
- **미해결 (와이어 간극)**: 뷰어 토글-off가 Rust `cursor_requested` 플래그만 해제하고 와이어 LCDOFF 프레임을 실제로 전송하지 않는다. 호스트 측 원복 경로는 닫혀 있어 커서 소실 위험은 없으나, 토글 off 직후 비디오 커서가 즉시 복귀하지 않을 수 있다.
- **미검증**: Wi-Fi 실기 지연 체감, 태블릿 오버레이 렌더, TCP 폴백 동작, 위 LCDOFF 간극의 실기 체감 — 다음 실기 세션에서 확인한다.
