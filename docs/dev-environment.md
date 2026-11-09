# 개발 환경 노트 (실측·테스트용)

상태: 상시 갱신 문서. 기기/세션 제약처럼 "문서로 남겨야 다음 사람(또는 에이전트)이
재시도하지 않는 것"을 기록한다. 2026-09-03 작성.

## 머신

- MacBook Pro, Apple M1 Max (32코어 GPU), macOS 26.6.2 (25G83), Xcode 26.2 (Swift 6.2.3, SDK macOS 26.2)
- 테스트 폰: Lenovo TB710FU, Android 16, 시리얼 `HA2D6EMP` (docs/EVIDENCE.md 실기 표본과 동일)

## 창 관리: Aerospace

- 이 머신은 **Aerospace**(타일링 창 관리자)를 쓰며, `option+1..9`로 화면(워크스페이스) 전환을 한다.
- 테스트 시 유의점:
  - 가상 디스플레이 프로브(`tools/cgvd-spark`)가 디스플레이를 잠깐 만들면 Aerospace가
    새 출력을 하나의 화면으로 인식한다. 프로브는 종료 전 스스로 정리하므로 수동 복구는 불필요.
  - UI 자동화(Accessibility/System Events)나 창 위치 가정이 필요한 검증은 Aerospace 단축키·
    워크스페이스 상태와 간섭할 수 있다 — 창 좌표 어설션은 전환 후 기준으로 잡을 것.

## 세션 제약: 자동화 셸은 WindowServer(GUI) 세션 밖이다

- Claude Code 자동화 셸(SSH 계열 세션, `com.apple.access_ssh` 소속)에서는:
  - `CGGetActiveDisplayList` → 0, `CGSessionCopyCurrentDictionary` → nil
  - `open`, Apple Events(System Events), `NSWorkspace.openApplication` → 전부
    `-10827 kLSNoExecutableErr`/`-1728`로 실패 (EVIDENCE.md 검증 6의 -10827 기록과 동일 근원)
- 따라서 **디스플레이 생성·모드 변경·스크린샷·창 조작이 필요한 실측은 사람이
  GUI 터미널(Ghostty 등)에서 직접 실행**해야 한다. 세션 번호(ps SESS)가 0으로 같아도
  SSH 유래 프로세스는 WindowServer 세션에 못 붙는다(launchd gui 도메인 spawn 필요).
- 세션과 무관한 확인은 가능하다: objc 런타임 클래스 존재 확인(`cgvd-exist`),
  컴파일, 단위 테스트, 파일/프로세스 관찰 등.

### 추가 전제(2026-09-03 실측): 활성 화면이 물리적으로 켜져 있어야 한다

- 이 노트북은 **덮개를 닫고 화면 공유(Screen Sharing)로 원격 사용**하는 경우가 있다.
  클램쉘 닫힘(`AppleClamshellState=Yes`)+콘솔 로그인 화면 상태에서는 **GUI 터미널에서
  실행해도** `CGGetActiveDisplayList`가 0을 반환한다 — WindowServer가 활성
  디스플레이를 아예 갖지 않기 때문이다. Aerospace option+1..9 전환은 화면 공유
  세션 안의 워크스페이스 전환일 뿐 활성 출력을 만들지 못한다.
- 디스플레이 관련 실측 전 확인 명령:
  `ioreg -r -k AppleClamshellState | grep ClamshellState` (Yes면 덮개 닫힘),
  `pmset -g systemstate`. **덮개를 열거나 외장 모니터를 켠 뒤 실행할 것.**

## 실측 절차 인덱스

| 대상 | 절차 | 실행 주체 |
|---|---|---|
| USB AOAP 물리 게이트 (T11) | `docs/usb-physical-validation.md` (검증 1~6) | **사람 + 폰 `HA2D6EMP`** |
| 60분 soak·고모션·glass-to-glass | `docs/EVIDENCE.md` E4–E7 표 | 사람 + 기기 |

## Android 디바이스 확인 (자동화 셸에서 가능)

```zsh
adb devices            # 기기 인식 확인
adb shell getprop ro.build.version.release
adb logcat -d -s Leftcar   # 스트리밍 세션 로그 덤프
```

## 리눅스 Host 개발 머신 (2026-11-01)

- Arch Linux, Hyprland 0.56.2 (Wayland), Ryzen 9 9950X, 출력 HDMI-A-3 1920x1080@60 단일.
- 자동화 셸은 Hyprland GUI 세션 밖이다(`XDG_SESSION_TYPE=tty`). GUI가 필요한 실행은
  `WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000`(및 필요 시
  `HYPRLAND_INSTANCE_SIGNATURE=$(ls /run/user/1000/hypr | head -1)`)을 env로 주입해
  세션 소켓에 붙는다. grim 캡처·wlr-screencopy·가상 입력 프로토콜은 이 경로로 동작한다.
- 빌드 요구 패키지: webkit2gtk-4.1, gtk3, libsoup3, pkg-config(빌드),
  libayatana-appindicator(트레이, 런타임 dlopen — 없으면 트레이만 실패),
  openh264(런타임 dlopen, Linux Host 소프트웨어 폴백 인코더),
  nvidia-utils(런타임 dlopen `libnvidia-encode.so.1`+`libcuda.so.1`, Linux Host
  NVENC 하드웨어 인코더 — NVIDIA 독점 드라이버 머신에서 자동 선택, 없으면
  openh264로 폴백), zenity·xdg-desktop-portal(파일 대화상자).
  adb는 미설치 — Android 실기기 검증은 미실행.
- Linux Host 인코더 선택은 세션 시작 시 자동이다: NVENC(H.264 baseline, CBR,
  P4+ULTRA_LOW_LATENCY 튜닝, 무한 GOP·뷰어-IDR 구동, 클래식 시스템 메모리 입력
  버퍼 경로) → 실패 시 OpenH264 소프트웨어. `LEFTCAR_LINUX_ENCODER` 환경변수로
  강제할 수 있다(`auto`(기본) | `hardware` | `software`; 알 수 없는 값은
  세션 시작 오류). 선택 결과는 stats(`encoder_id`/`encoder_hardware_accelerated`)
  와 `linux_encoder_selected` 로그로 관찰된다.
- NVENC 입력 경로는 클래식 `nvEncCreateInputBuffer`+`nvEncLockInputBuffer`를
  쓴다. 드라이버 615.78.08에서 CUDADEVICEPTR register/map 경로는 sync 모드
  encode가 NV_ENC_ERR_EVENT_NOT_REGISTERD로 실패하는 것을 실측했고(FFmpeg
  h264_nvenc가 쓰는 클래식 경로는 동일 드라이버에서 정상), sync 모드의
  `NV_ENC_LOCK_BITSTREAM.hwEncodeStatus`는 성공 인코딩에서도 비-NVENCSTATUS
  내부값(2)을 담으므로 신뢰하지 않는다. NVENC H.264 최소 해상도는 이 드라이버
  기준 145x49(caps WIDTH_MIN/HEIGHT_MIN 실측)다.
- 루트 워크스페이스 `cargo build`/`cargo test --workspace`는 리눅스 호스트에서
  viewer-decoder의 `stream_receiver`/`decode_harness` 바이너리 링크가 Android NDK 심볼
  (AMediaCodec_*)을 요구해 실패한다 — 기존 구조로, Host crate는 별도 매니페스트로
  빌드한다(`cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked`).
- Wayland(tao 0.35) 제약: 숨은 창에 `set_ignore_cursor_events(true)`를 보내면
  tao event_loop의 `window().unwrap()`이 패닉한다. 인디케이터 배지는 리눅스에서 이
  요청을 건너뛴다(lib.rs create_indicator_window 주석). 배지는 클릭을 받는 대신
  항상 위 특성은 컴포지터가 무시할 수 있다.
- Hyprland 0.56의 `hyprctl dispatch`는 Lua 파서를 거치므로 구형 공백 인자 문법이
  실패한다. 스크립트는 IPC 소켓에 Lua 인자 형식(`dispatch name("arg")`)으로 보낸다.
