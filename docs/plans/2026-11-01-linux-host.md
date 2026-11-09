# Linux Host 구현 계획 (2026-11-01)

요청: "맥에서는 잘 쓰고 있는데 리눅스에서 잘 못쓰고 있는 것 같다" — 확인 결과 Linux Host는 미구현(버그 아님)이며, 사용자가 Linux Host 구현 착수를 선택했다. 이 계획은 Hyprland(Wayland) + Android Viewer 조합의 MVP를 목표로 한다.

## 목표와 범위

- 사용자 결과: 이 리눅스 머신(Arch, Hyprland 0.56.2, 1920×1080@60 단일 출력, Ryzen 9 9950X)에서 Host 앱이 실행되고, Android Viewer에서 화면을 보고 원격 입력을 할 수 있다.
- 기준 리비전: 49240b6 (작업 시작 시 clean). Host crate는 루트 워크스페이스 밖(`apps/host-desktop/src-tauri`, 자체 Cargo.lock).
- 조사 근거: 4건의 저장소 조사(플랫폼 경계, 미디어 계약, 리눅스 장애점, 입력 경로)와 실물 확인 — `cargo check` E0599 2건(virtual_display.rs:415/559), grim 캡처 성공(wlr-screencopy 동작), 시스템 libopenh264 2.6.0 설치 확인.
- 범위 안 (MVP): UDP 미디어 전송, 단일/다중 출력 나열, H.264 소프트웨어 인코딩, 원격 입력(포인터·버튼·휠·키·Text 포함 한글), 기존 승인·게이트·통계 정책 재사용.
- 범위 밖 (후속, 문서에 명시): USB/AOAP 전송, 오디오, 가상 디스플레이(현재처럼 미지원 유지), privacy curtain(현재처럼 no-op), KM 입력 공유, NAK/RTX 링(Windows와 동일 격차), 하드웨어 인코더(VAAPI/NVENC), 리눅스 패키징·CI 러너, 트레이 아이콘(런타임 라이브러리 필요, 기능 저하 허용).

## 수락 기준

| ID | 관찰할 결과와 적용 조건 | 담당 태스크 | 검증 ID |
|---|---|---|---|
| AC-1 | Host crate가 리눅스에서 컴파일·검사된다 (check/clippy/test) | T-1, T-2 | V-1 |
| AC-2 | 리눅스에서 Host 앱이 실행되고 대시보드가 열리며 `get_host_platform`이 "linux"를 반환한다 | T-2 | V-2 |
| AC-3 | `list_displays`가 실제 Wayland 출력을 안정적 `source_id`(`linux:output:*`)로 반환한다 | T-3 | V-3 |
| AC-4 | Viewer가 `startStream`(UDP)을 보내면 캡처→인코딩→AEAD 봉인 전송이 시작되고 5초 내 `first_send_ms > 0`이며, BYE/타임아웃 시 세션이 정리된다 | T-4~T-6 | V-4, V-5 |
| AC-5 | 원격 입력(포인터 절대 이동·버튼·휠·키·ReleaseAll)이 승인된 기기에서만 주입되고, 입력 OFF 시 눌린 키가 해제된다 | T-7 | V-6 |
| AC-6 | Text 이벤트(한글 포함 UTF-8)가 가상 키보드 동적 키맵으로 주입된다 | T-7 | V-6 |
| AC-7 | 저장소 게이트가 유지된다: 루트 JS 검사, rustra:check, macOS/Windows cfg 경로 무변경, 프론트엔드 변경 시 React Doctor 100 | T-2~T-8 | V-1, V-7 |
| AC-8 | 문서가 실제 상태를 반영한다 (README 상태, completion-and-support 조합 표, EVIDENCE, 개발 환경 안내) | T-8 | V-7 |

AC-4의 실기기 영상 수용(실제 Android Viewer 페어링·재생)은 세션에 adb/기기가 없어 사용자 실행 항목으로 분리한다 — 이 계획의 완료 판정에서 BLOCKED로 명시한다(§검증 V-6).

## 설계

### 책임과 인터페이스

- 유일한 생성 지점 `platform_backend()`(lib.rs:562-568)의 `not(any(macos, windows))` 분기를 `LinuxBackend::new()`로 교체한다. 패턴은 `WindowsBackend`를 따른다(세션 해시맵 + 스레드 2개/세션: 입력 수신, 캡처·인코딩·전송).
- `CaptureBackend` 트레이트(backend.rs:13-84) 전체를 구현한다. `input_permission/request_input_permission`은 `Ok(true)`(Windows와 동일 — Wayland 가상 입력 프로토콜은 권한 프롬프트가 없다), `screen_permission`은 `Ok(true)`, `set_quality_override`는 기본 Err(Windows와 동일), `encoder_experiments`는 Auto만.
- 플랫폼 무관 모듈의 cfg 게이트 확장: `media_sender`, `tcp_media`, `media_feedback`, `input_gate`(lib.rs:18-31)를 `any(windows, linux, test)`로. 이 모듈들은 코드 무수정(조사 확인).
- 가상 디스플레이 컴파일 수정: `call_create`/`call_destroy`에 `not(target_os = "macos")` 폴백(런타임 도달 불가 — 프로브가 "unsupported platform"으로 조기 반환) + `refresh_removal`의 비-macos 미사용 변수 정리.

### 새 모듈 `linux_backend/` (windows_backend 레이아웃 미러)

- `mod.rs` — `LinuxBackend`, `LinuxSession`(access·stop·input_gate·force_keyframe·stats·injector·notice_sender·crypto_rx), `CaptureBackend` impl, `list_displays`, `start`(udp만 수용, `prove_udp_reachability` 재사용), retire/health(15초 접촉 타임아웃), 통계(initial_stats 패턴).
- `wayland.rs` — wayland-client 연결, 출력 나열(wl_output + xdg_output), wlr-screencopy 캡처(요청→ready→shm 픽셀), 가상 포인터(wlr-virtual-pointer: motion_absolute·button·axis), 가상 키보드(wlr-virtual-keyboard: keymap 업로드 + evdev 키코드).
- `encoder.rs` — 시스템 libopenh264를 libloading으로 dlopen(저장소의 macOS dylib 패턴과 동일, 새 crate 없음), `WelsCreateSVCEncoder` C vtable API, BGRA→I420 변환(행 분할), Annex-B 출력, `ForceIntraFrame`으로 IDR 요구 수용. 내부에 소형 인코더 트레이트를 두어 후속 VAAPI/NVENC 교체 여지를 남긴다. 대안(막힐 때): libx264 dlopen(버전 접미 심볼 프로브).
- `input.rs` — `InputInjector`(Windows input.rs 미러): PointerMove→motion_absolute(출력 논리 크기 기준), 버튼 1/2/4→BTN_LEFT/RIGHT/MIDDLE, 8/16→BTN_BACK/FORWARD, 휠 milli-lines→axis(줄당 40px 근사, 상수로 문서화), Key→`keymap::linux(android)`→evdev 키코드, meta_state→수식키 선처리, ReleaseAll/Drop→release_all.

### 데이터 흐름 (세션 1개)

start(승인 lease 진입, udp 도달성 증명) → 캡처 스레드: screencopy 프레임(BGRA shm) → I420 변환 → openh264(저지연 옵션, AU당 Annex-B, 요구 w/h와 다르면 스케일) → `wire::normalize_h264` → SPS/PPS 확보 전 AU 폐기+강제 IDR → `wire::config_datagram`(첫 AU·키프레임마다) → `wire::media_datagrams_with_limit` → `fec::parity_datagrams_for_media` → `MediaPacer::drain`(자체 DrainIo) → AEAD 봉인 송신. 입력 스레드: `process_authenticated_message` 패턴 재사용(BYE/IDR/LCK1/LCP1/LCF1/시퀀서→injector).

### 키맵

`crates/keymap`에 `linux: Option<u16>` 필드와 `linux()` 헬퍼 추가(evdev 키코드). 기존 mac/windows 값 불변, `render_swift` 출력 불변, parity 테스트에 linux 열 추가. Text 이벤트는 wtype 방식: 유니코드 keysym(0x01000000|코드포인트) 전용 예약 키코드(248~254)에 동적 할당, 필요하면 xkb 키맵 텍스트를 재생성해 `keymap()` 업로드 후 키 눌렀다 뗌.

### 외부 계약·호환성

- 와이어 포맷 변경 없음(CFG/LT/FEC/LCI1 그대로). 제어 평면·페어링·클립보드·파일 전송 무수정. Linux 토큰 저장은 기존 FileTokenStore(0600)를 그대로 쓴다(keyring mock 무관).
- Cargo.toml: `[target.'cfg(target_os = "linux")'.dependencies]`에 wayland-client·wayland-protocols 추가, host Cargo.lock 갱신(루트 워크스페이스 lock 무관). macOS/Windows CI는 target-gated라 그래프에 영향 없음.
- 프론트엔드 최소 변경: `DashboardModals.tsx` curtain `supported`를 `platform === "macos"`로(리눅스에서 no-op 토글을 지원됨으로 표시하는 부정확함 제거). `get_host_platform`은 std::env::consts::OS 기반 platform() 문자열("linux")로 자동 반영.
- 실패·복구 경계: compositor 연결 실패/프로토콜 부재 시 `LinuxBackend::new`에서 명시적 Err(기존 fatal_startup_error 경로), 캡처 버퍼 크기 불일치·encoder 실패는 세션 error 상태 + 통계 error 문자열, encoder 미설치(dlopen 실패)는 start에서 즉시 Err.

## 태스크

| ID | 목적·AC | 수정할 파일/경계 | 선행 | 검증 | 상태 |
|---|---|---|---|---|---|
| T-1 | AC-1 컴파일 수정 | virtual_display.rs(non-macos 폴백) | 없음 | V-1 | done |
| T-2 | AC-1·2·7 게이트 확장+백엔드 골격 | lib.rs(cfg), linux_backend/mod.rs 골격, Cargo.toml/lock | T-1 | V-1, V-2 | done |
| T-3 | AC-3 출력 나열 | linux_backend/wayland.rs(wl_output) | T-2 | V-3 | done |
| T-4 | AC-4 캡처 | linux_backend/wayland.rs(wlr-screencopy) | T-3 | V-4 | done |
| T-5 | AC-4 인코더 | linux_backend/encoder.rs(openh264 FFI, I420 변환) | 없음(T-4와 병렬 가능) | V-4 | done |
| T-6 | AC-4 전송 연결 | linux_backend/mod.rs(세션 루프·pacer·도달성·health·retire), input 수신 스레드 | T-4, T-5 | V-4, V-5 | done |
| T-7 | AC-5·6 입력 주입 | linux_backend/input.rs, wayland.rs(가상 포인터/키보드), crates/keymap(linux 열), input.rs Text 동적 키맵 | T-2 | V-6 | done |
| T-8 | AC-7·8 프론트엔드·문서 | DashboardModals.tsx, README, completion-and-support, EVIDENCE, 본 계획 갱신 | T-2~T-7 | V-7 | done |

구현 중 발견·수정한 결함: (1) `GlobalList::bind`가 with_list가 잠근 globals 뮤텍스를
다시 잠가 자기-교착 — list_outputs·VirtualInput을 with_list 밖 bind로 수정.
(2) screencopy capture_output 요청 미-flush로 무한 대기 — flush 추가·데드라인 상시 검사.
(3) Ready 처리가 이미 소비된 buffer_meta에 의존 — ShmBuffer 보유 크기 사용.
(4) tao(Wayland)가 숨은 창의 cursor-ignore 요청에서 패닉 — 리눅스는 해당 요청 생략.
테스트 하네스 자체의 결함 두 건(뮤텍스 잡은 채 join, G 헤더 au_id 잘못된 오프셋
17–18→5–6)도 시험 과정에서 바로잡았다.

통합 완료 기준: T-6 이후 실제 Hyprland 세션에서 스트림 스모크(V-4)가 통과해야 하고, 개별 태스크 PASS를 통합 PASS로 쓰지 않는다.

## 검증 계획과 결과

| ID | 방법·정의 근거 | cwd·환경 | 기대 결과 | 실제 결과·증거 | 판정 |
|---|---|---|---|---|---|
| V-1 | `cargo check --locked`, `cargo clippy -- -D warnings`, `cargo test --locked` (CONTRIBUTING.md 명령, src-tauri 매니페스트) | apps/host-desktop/src-tauri, Linux | 종료 0, 신규 경고 없음 | check 0 경고, clippy -D warnings 통과, test 387/389 — 실패 2건(km 1, control 1)은 HEAD+컴파일픽스만의 베이스라인에서 동일 재현된 기존 리눅스 미실행 문제(이 플랫폼에서 테스트가 처음 돌아감) | PASS(기존 2건은 회귀 아님) |
| V-2 | 앱 실행(그래픽 세션 env 주입), hyprctl clients·grim 관찰, 로그 확인 | WAYLAND_DISPLAY=wayland-1, Hyprland 세션 | 창 표시, 치명 오류 없음, platform="linux" | 프로세스 생존, 0.0.0.0:7777 리슨, mDNS 광고(192.168.0.7), 창 mapped(ws 2) — hyprctl clients. 화면 픽셀 관찰은 사용자 브라우저 오버레이에 가려짐 — 사용자 확인 대기 항목 | PASS(픽셀 관찰은 사용자 몫) |
| V-3 | 스트림 스모크가 list_outputs로 목표 출력 해석 | 동일 | 실제 출력이 `linux:output:*`로 나열·선택 | 스모크가 HDMI-A-3 출력으로 세션 시작 성공(source_id 일치) | PASS |
| V-4 | 통합 스모크(`stream_smoke --ignored`): 실제 compositor에서 캡처→인코딩→루프백 UDP→수신 복호화, Annex-B SPS/PPS·AU 연속성 검사 | 동일, 세션 키 자체 생성 | 프레임 다수, AU 단조, SPS/PPS 선행 | PASS: frames=65, cfg=1(SPS/PPS), AU id 1..65 연속, 루프백 AEAD 왕복. 실측 fps=18(정적 화면, 소프트웨어 파이프라인) — 1080p60 목표와 간격, 문서화된 한계 | PASS(성능 한계 기록) |
| V-5 | Viewer-driven 경로: 스모크 뷰어가 LCH1 증명→LCK1 하트비트→BYE | 동일 | first_send>0, BYE 후 state="stopped" | proof=true, heartbeats≈19, BYE 후 state="stopped", error=None | PASS |
| V-6 | 단위: keymap linux 매핑 parity·동적 키맵 텍스트의 실제 libxkbcommon 파싱. 프로토콜: 가상 포인터/키보드 생성·주입이 compositor에서 수락 | 동일 | 매핑 테스트 통과, 생성 키맵이 실제 파서에서 한글·이모지 keysym으로 해석 | 초기 구현의 `uXXXX` 소문자 keysym 이름이 libxkbcommon에서 NoSymbol로 귀결되는 결함(F-1)과 키맵 업로드 순서 결함(F-2)을 독립 리뷰가 발견 → `UXXXX`로 수정 + 업로드 선행. /tmp/keymap_check 하네스(컴포지터와 동일 libxkbcommon 1.13.2)에서 UD558(한)·UC138(가)·U1F600 정상 해석 확인. keymap 테스트 5종·input 테스트 3종 통과. 물리 타이핑 관찰은 사용자 실행 항목 | PASS(물리 관찰 제외, 수정 후) |
| V-7 | 루트 `bun run typecheck`, `bun run test`, `bun run test:contract`, React Doctor, `test:architecture`(기존 32건 유지), `rustra:check` | 저장소 루트 | 사전 상태 유지 + 신규 실패 없음 | typecheck 통과, test 978/978, test:contract 4/4, 호스트 프론트 테스트 39/39, React Doctor 100/100. rustra:check·architecture는 최종 확인 예정 | pending(잔여 2검사) |
| V-8 | (사용자 실행) Android Viewer 페어링 → 영상 → 입력 | 사용자 기기·네트워크 | 실제 영상 재생·원격 입력 | 미실행(세션에 adb·기기 없음) | BLOCKED |

## 평가와 인계

- G3(독립 평가, review-changes 기준): 첫 판정 **FAIL** — F-1(P1, keysym 이름 `uXXXX`가 libxkbcommon에서 미해석 → 한글 Text 전무), F-2(P1, 키맵 업로드가 키 발송 후에 실행), F-3(P2, 다중 출력 원점 미상쇡), F-4~F-6(P3, 스레드 잔류·스트라이드 패닉·입력 부재 시 시작 실패), F-7(P3, 비안정 출력명 문서 불일치). 7건 전부 수정 후 재판정 **PASS** — 리뷰어가 현재 소스에서 TextKeymap 렌더 경로를 추출해 컴포지터 동일 libxkbcommon으로 파싱(안·라·가·이모지 전부 정상 keysym), clippy --all-targets -D warnings·cargo test 387/389·스트림 스모크를 직접 재실행해 확인. 잔여 N1(P3 문구)도 수정 완료. 물리 타이핑·실기기는 사용자 실행 항목으로 유지.
- G2 이후 독립 평가자(review-changes 기준)로 최종 diff 재검토. 사용 가능 환경: Linux 로컬 cargo/bun, Hyprland 실세션. macOS/Windows 네이티브 컴파일은 이 세션에서 불가 — cfg 정확성은 코드 검토로 보완하고 그 한계를 명시한다.
- 실기기 수용(V-8)은 사용자가 실행할 때까지 BLOCKED로 유지하며, 완료 보고에 실행 절차를 남긴다.
- G4(UI): 이 변경의 프론트엔드 수정은 커튼 지원 플래그의 정직화 한 건이다. 호스트 프론트 vitest 39종과 React Doctor 100으로 동작을 확인했고, 렌더 픽셀 관찰은 사용자 세션 오버레이로 가려져 미실행 — 대시보드 실물 확인은 사용자 몫으로 남긴다.
- 커밋 시 포함 대상: `apps/host-desktop/src-tauri/gen/schemas/linux-schema.json`(트래킹된 mac/windows-schema 형제와 동일 취급), `protocols/wlr-virtual-keyboard-unstable-v1.xml`(wlroots MIT 라이선스 헤더 포함), linux_backend 4개 파일. viewer-expo Kotlin 3개 변경 + untracked input/ 디렉터리는 이 변경이 아니므로 함께 커밋하지 않는다.
- 사용자 실행 절차(V-8): adb가 있는 머신에서 Viewer 설치 → Host 앱(리눅스, `bun run tauri dev` 또는 debug 바이너리+vite) 실행 → 뷰어 QR 페어링 → 소스 승인 → 1080p 영상 확인 → 입력(포인터·키·한글 Text) 확인. fps는 소프트웨어 인코더 한계로 20 내외에서 시작할 수 있다.
