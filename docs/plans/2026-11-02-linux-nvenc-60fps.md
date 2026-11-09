# Linux Host 하드웨어 인코딩(NVENC)과 60fps 계획 (2026-11-02)

요청: "현재 여기에서 linux에서 그래픽 가속을 통해 연결하고 60fps로 진행하는게 좋을꺼 같아" — 2026-11-01 Linux Host MVP(docs/plans/2026-11-01-linux-host.md)의 문서화된 한계(소프트웨어 openh264, 실측 fps=18)를 후속 과제로 해결한다.

## 목표와 범위

- 사용자 결과: 이 리눅스 머신(Ryzen 9 9950X + RTX 4070 Ti SUPER + RTX 4060, nvidia 615.78.08)에서 Host 세션이 하드웨어(NVENC) 인코더를 사용해 1080p60 요청을 소화하고, 대시보드 인코더 진단에 하드웨어 가속이 표시된다.
- 기준 리비전: 49240b6 위에 작업 중인 Linux Host 변경(미커밋) 위 — 기존 변경을 보존하고 그 위에 쌓는다.
- 조사 근거: NVENC만 유효한 이유 — lspci 실물 확인(card0=amdgpu Radeon 610M은 Granite Ridge iGPU로 VCN 부재 → VAAPI 인코딩 불가, card1/2=nvidia 독점 드라이버). `libnvidia-encode.so.1`/`libcuda.so.1`은 nvidia-utils 615.78.08에 포함 설치돼 있고 `nvidia-smi` encoder sessionCount=0. NVENC ABI 참조: ffnvcodec **n12.2.72.0** nvEncodeAPI.h(함수 리스트 42 포인터 순서, NV_ENCODE_API_FUNCTION_LIST_VER=NVENCAPI_STRUCT_VERSION(2), H264/BASELINE/P4 GUID, CBR=0x2, NV12=0x1, FORCEINTRA=0x1, OUTPUT_SPSPPS=0x4, CAVLC=0x2, ULTRA_LOW_LATENCY=3 확인). 병목 위치: `linux_backend/mod.rs` run_capture가 OpenH264Encoder 직접 생성(642행), 인코더 외 경로(스크린카피 shm, wire/FEC/pacer/AEAD)는 Windows의 60fps 경로와 동일해 재사용.
- 범위 안: NVENC H.264 인코더(libcuda + libnvidia-encode dlopen), NV12 변환 공용화, 자동 선택 + 소프트웨어 폴백, stats/진단 표기, LEFTCAR_LINUX_ENCODER 환경변수 오버라이드, 문서.
- 범위 밖 (후속): dmabuf zero-copy 캡처(현재 shm 경로로 예산 내 도달 — 실측 후 필요 판단), VAAPI(하드웨어 부재), 실기기 관찰(사용자 실행), 인코더 선택 UI(EncoderExperiment는 Auto 유지), Windows/macOS 경로 변경.

## 수락 기준

| ID | 관찰할 결과와 적용 조건 | 담당 태스크 | 검증 ID |
|---|---|---|---|
| AC-1 | Linux 세션이 NVENC를 자동 선택하고 stats로 관찰된다: `encoder_id="nvenc"`, `encoder_hardware_accelerated=Some(true)`, 대시보드 인코더 진단 라벨 | T-2, T-3, T-5 | V-2, V-4, V-6 |
| AC-2 | NVENC 초기화 실패 환경에서 소프트웨어 openh264로 자동 폴백하고 사유가 기록된다 | T-3 | V-5 |
| AC-3 | 1080p60 인코딩 파이프라인이 프레임 예산(16.6ms) 내에서 동작한다 — 이동 패턴 마이크로벤치 + 라이브 스모크의 fps/encode_us | T-2, T-4 | V-3, V-4 |
| AC-4 | 와이어·세션 계약 무변경: Annex-B AU 연속성, CFG/IDR 게이팅, AEAD/FEC/pacing, BYE 정리가 기존 스모크로 재확인된다 | T-3, T-4 | V-4 |
| AC-5 | 저장소 게이트 유지: cargo check/clippy/test(기존 실패 2건 외 신규 없음), bun typecheck/test/test:contract, 프론트 변경 시 React Doctor 100 | T-1~T-5 | V-1, V-6 |
| AC-6 | 문서가 실제 상태를 반영한다: dev-environment(NVENC 요구사항·env 오버라이드), EVIDENCE, README/completion 조합표, 본 계획 갱신 | T-5 | V-6 |

실기기(Galaxy XR Viewer)에서의 60fps 관찰은 세션에 기기가 없어 사용자 실행 항목으로 유지한다(§검증 V-7, BLOCKED 명시).

## 설계

### 책임과 인터페이스

- `linux_backend/encoder.rs`에 `VideoEncoder` 트레이트 도입: `encode(bgra, src_w, src_h, src_stride) -> EncodedAu`, `force_keyframe()`, `meta() -> EncoderMeta{id, hardware, preset, profile}`. `OpenH264Encoder`와 새 `NvencEncoder`가 모두 구현. `EncodedAu{data, keyframe}` 계약 유지.
- 새 모듈 `linux_backend/nvenc.rs`: libloading으로 `libcuda.so.1` + `libnvidia-encode.so.1` dlopen(openh264 dylib 패턴과 동일, 새 crate 없음). ABI는 ffnvcodec n12.2.72.0 `nvEncodeAPI.h`에서 손복사(출처 태그를 주석으로 기록) — openh264 구조체 복사 방식과 동일.
- CUDA 경로: `cuInit(0)` → `cuDeviceGet(0)` → `cuCtxCreate` → `cuMemAllocHost`(pinned CPU NV12) + `cuMemAlloc`(device NV12, pitch×height×3/2) → 프레임마다 `cuMemcpyHtoD`.
- NVENC 경로: `NvEncodeAPICreateInstance` → `nvEncOpenEncodeSessionEx(deviceType=CUDA, device=CUcontext)` → `nvEncGetEncodePresetConfigEx(H264, P4, ULTRA_LOW_LATENCY)`로 기본 NV_ENC_CONFIG 복사 후 오버라이드(profile baseline, CAVLC, CBR, frameIntervalP=1(B프레임 없음), gopLength/idrPeriod 무한(뷰어-IDR 구동), repeatSPSPPS=1(IDR마다 SPS/PPS — CFG 게이팅 재사용), vbvBuffer=bitrate/fps(낮은 지연), lookahead 끔) → `nvEncInitializeEncoder` → `nvEncRegisterResource(CUDADEVICERPTR)` + `nvEncMapInputResource` → `nvEncCreateBitstreamBuffer`(동기 모드 1개).
- 프레임 처리: BGRA→NV12(pinned) → H2D → `nvEncEncodeFrame`(sync, IDR 요구 시 `NV_ENC_PIC_FLAG_FORCEINTRA|OUTPUT_SPSPPS`) → `nvEncLockBitstream(doWait)` → Annex-B 복사 → `EncodedAu{data, keyframe=pictureType==IDR}`. Destroy 순서: unmap→deregister→destroy bitstream→destroy encoder→cuMemFree→cuCtxDestroy.
- 공용 변환 `linux_backend/convert.rs`: 기존 `bgra_to_i420`(행 분할 2스레드, BT.601 limited)을 encoder.rs에서 이동하고 `bgra_to_nv12`(동일 Y 패스 + interleaved UV) 추가. openh264 출력은 동작 무변경(기존 테스트로 보호).
- 팩토리 `create_video_encoder`: `LEFTCAR_LINUX_ENCODER` ∈ {auto(기본), hardware, software}. auto는 NVENC init을 시도하고 실패 시 사유를 eprintln으로 남긴 뒤 openh264 선택. 런타임 encode 실패는 폴백 없이 기존 세션 error 경로(스트림 도중 코덱 전환은 디코더 참조를 끊는다).
- stats 반영: run_capture에서 선택 결과를 세션 stats에 기록 — NVENC: mode `nvenc`, id `nvenc`, hardware Some(true), preset `p4-ultraLowLatency`, profile `baseline`. 소프트웨어는 기존 값 유지. `capture_backends()` 힌트 텍스트도 실제 상태("NVENC 하드웨어 H.264 + 소프트웨어 폴백")로 갱신.
- 프론트: `encoderDiagnostics.ts`에 `nvenc` 모드 라벨 추가(기존 ave/unknown 라벨 컨벤션 따름) + 테스트 보강. 세션/와이어 계약 변경 없음.

### 데이터 흐름 (변경점만)

캡처 스레드: `copier.capture()`(shm BGRA) → **BGRA→NV12(pinned CPU)** → **cuMemcpyHtoD** → **NVENC sync encode** → Annex-B AU → 기존 그대로(normalize→CFG 게이팅→fragment+FEC→pacer→AEAD 송신). 입력 스레드·제어 평면 무수정.

### 외부 계약·호환성

- 와이어 포맷·제어 평면 무변경. bitrate 공식 무변경(1080p60≈12.4Mbps, clamp 이내). Cargo.toml 의존성 무변경(libloading은 이미 사용 중).
- NVENC 런타임 요구사항은 nvidia-utils(이미 설치) — 문서에 명시. NVENC 부재 머신(비 NVIDIA/누보)은 자동 소프트웨어 폴백으로 기존 동작 유지.
- 실패 경계: nvenc 초기화 전체가 `new()` 안에서 Err → 팩토리 폴백(Auto 기준). encode 중 Err → 기존 세션 error 표기.

## 태스크

| ID | 목적·AC | 수정할 파일/경계 | 선행 | 검증 | 상태 |
|---|---|---|---|---|---|
| T-1 | 공용 변환 추출(동작 무변경) AC-4·5 | linux_backend/convert.rs 신설, encoder.rs 위임 | 없음 | V-1 | done |
| T-2 | NVENC FFI+인코더 AC-1·3 | linux_backend/nvenc.rs 신설, encoder.rs(트레이트) | T-1 | V-2, V-3 | done |
| T-3 | 팩토리·세션 통합·stats AC-1·2·4 | encoder.rs(팩토리), linux_backend/mod.rs(run_capture/stats/hint) | T-2 | V-4, V-5 | done |
| T-4 | 라이브 스모크 HW/SW/auto AC-3·4 | 없음(실행·기록만) | T-3 | V-4 | done |
| T-5 | 프론트 라벨·문서 AC-1·5·6 | encoderDiagnostics.ts(+test), dev-environment, EVIDENCE, README, completion-and-support, 본 계획 | T-3 | V-6 | done |

설계 확정 과정의 실측 변경(계획 대비): 입력 경로가 CUDADEVICEPTR register/map에서
**클래식 nvEncCreateInputBuffer+nvEncLockInputBuffer**로 바뀌었다. 드라이버
615.78.08의 CUDADEVICEPTR sync encode는 NV_ENC_ERR_EVENT_NOT_REGISTERD로
실패하고(바이트 동일 C 클라이언트 재현, SDK 12.2·13.1 모두, gdb로 ffmpeg 파라미터
덤프 대조), FFmpeg h264_nvenc가 쓰는 클래식 경로만 동일 드라이버에서 정상이다.
sync 모드 `hwEncodeStatus`는 성공 인코딩에서도 내부값 2를 담아 신뢰할 수 없고,
강제 키프레임은 IDR이 아니라 I(전용)로 보고되며, NVENC H.264 최소 해상도는
caps 실측 145x49, async 인코딩은 미지원("Async mode not supported")이다.
NV12 변환은 31ms(2스레드) → 행 스트라이프 병렬화로 ~5ms(코어 수, 상한 32)로
축소해 프레임 예산의 병목을 해소했다.

## 검증 계획과 결과

| ID | 방법·정의 근거 | cwd·환경 | 기대 결과 | 실제 결과·증거 | 판정 |
|---|---|---|---|---|---|
| V-1 | `cargo clippy -- -D warnings`, `cargo test`(CONTRIBUTING 명령, Host crate) | apps/host-desktop/src-tauri | 기존 실패 2건 외 신규 없음 | clippy 통과, test 397/399 — 실패 2건(km 1, control 1)은 2026-11-01 계획 베이스라인과 동일 → 같은 세션 후속에서 원인 규명·수정, 이후 전체 400/400(EVIDENCE 기존 베이스라인 결함 해소 섹션) | PASS |
| V-2 | NVENC 단위 5종: 합성 프레임→Annex-B·첫 AU SPS/PPS·강제 키프레임→전용, NV12 레이아웃 값 테스트, 팩토리, env 파싱 | 동일, GPU 있음 | 단정 통과 | 5종 전부 통과(최소 해상도 145x49 실측 반영해 256x256 사용) | PASS |
| V-3 | 스루풋 마이크로벤치(`#[ignore]`): 1080p 이동 패턴 120프레임 | 동일 | 평균 ≤16.6ms 단정 | avg 5.41ms / p95 6.09ms / max 6.41ms — 예산 1/3. 변환 31ms→~5ms 병렬화 효과 포함 | PASS |
| V-4 | 라이브 스모크(`LEFTCAR_STREAM_SMOKE=1 ... --ignored`, WAYLAND_DISPLAY=wayland-1): hardware + software + auto | 그래픽 세션(화면 on) | 60프레임·CFG·AU 연속·BYE→stopped | hardware: NVENC 선택, 64프레임·fps=34·CFG 1·AU 연속·BYE 정리. software: openh264, fps=20(기존 한계, 회귀 없음). auto: NVENC 자동, fps=48. 첫 시도는 화면 DPMS 오프로 프레임 미생성 → 출력 전원 on 후 통과(DPMS 오프 출력은 스트림 원천 불가 실측) | PASS |
| V-5 | 팩토리 단위: env=software→openh264, auto→NVENC(이 머신), 프로브 실패 주입→openh264+사유 | 동일 | 단정 통과 | `auto_selection_falls_back_to_openh264_when_nvenc_probe_fails`·`hardware_selection_propagates_nvenc_failure`·`software_selection_never_probes_nvenc`·`preference_parsing_covers_env_values` 통과 | PASS |
| V-6 | 루트 `bun run typecheck`, `test`, `test:contract`, React Doctor, `rustra:check`, `test:architecture` | 저장소 루트 | 기존 상태 유지 + 신규 실패 없음, React Doctor 100 | typecheck 0, vitest 979/979, contract 통과, React Doctor 100/100, rustra 통과, architecture 32건 = viewer-expo Kotlin 기존 베이스라인(이번 변경 무관) | PASS |
| V-7 | (사용자 실행) Galaxy XR Viewer 페어링 → 1080p60 영상·입력 관찰 | 사용자 기기 | 실측 fps·지연 관찰 | 미실행(세션에 기기 없음) | BLOCKED |

## 평가와 인계

- G3(독립 평가, review-changes 기준): **PASS** — P0~P2 없음. 평가자가 ffnvcodec 헤더를 include한 C 프로그램과 nvenc.rs 정의를 추출한 Rust 프로그램을 각각 컴파일해 sizeof/offsetof를 대조(구조체 10종·GUID 3종·상수·버전 매크로 전부 일치), clippy·cargo test 398/400·NVENC 단위·스루풋(avg 6.36ms)·대상 vitest·React Doctor를 직접 재실행, 라이브 스모크 로그 3건을 대조. 접수된 P3 결함 5건(F-1 입력버퍼 버전 상수·레이아웃, F-2 codec_pic_params 플레이스홀더 크기, F-3 세션 오픈 전 실패 시 ctx 누수, F-4 teardown 주석, F-5 문서 오탈자)은 **전부 수정 후 재검증** — 수정 후 NVENC 단위 6종(신규 ABI 크기 가드 포함)·스루풋(avg 6.40ms)·HW 라이브 스모크(fps=47)·전체 test 398/400(기존 베이스라인 2건만 실패)·typecheck·contract·React Doctor 재통과. ABI 크기는 `abi_struct_sizes_match_ffnvcodec_12_2` 테스트로 C 실측값을 고정해 회귀를 방지한다.
- 한계(평가자 명시 분): 라이브 스모크는 저장 로그 대조로 수행, 드라이버 행동 실측(CUDADEVICEPTR 실패·hwEncodeStatus 내부값·최소 145x49·async 미지원)은 독립 재현하지 않고 이를 반영한 클래식 경로의 실동작만 직접 확인. macOS/Windows는 cfg(linux) 게이트로 교차 빌드 영향 없음(lib.rs 확인).
- 실기기 수용(V-7)은 사용자 실행까지 BLOCKED 유지, 실행 절차: Host 앱 실행(리눅스, 화면 on 상태) → 뷰어 QR 페어링 → 소스 승인 → 1080p 영상 확인(stats encoder_id=nvenc·hardware) → 입력 확인. 세션 시작 시 `linux_encoder_selected` 로그로 선택 결과 관찰 가능.
- G4(UI): 프론트 수정은 인코더 진단 라벨(nvenc) 한 건. vitest·typecheck·React Doctor 100으로 동작 확인, 대시보드 실물 관찰은 사용자 세션 오버레이로 가려질 수 있어 사용자 몫으로 남긴다.
- 커밋 포함 대상: linux_backend/{nvenc,convert,encoder,mod}.rs, encoderDiagnostics.ts(+test), 문서 5종(계획·EVIDENCE·dev-environment·README·completion-and-support).
