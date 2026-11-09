# GPUI + Rustra 마이그레이션 설계 (host GPUI 전환 + viewer 코어 Rust 공유)

- 기준일: 2026-11-06
- 기준 상태: 작업 트리에 커밋되지 않은 변경 다수(linux_backend/, panel.rs, leftcar-hostd 등). 본 설계는 현재 작업 트리를 기준 리비전으로 삼는다.
- 원 요청: "GPUI와 GPUI-Kit을 이용해서 모두 마이그레이션을 진행해줘. 모바일은 RN 과 함께 rust로 core로직을 공유하게 해주고." (2026-11-06)
- 확정 범위(사용자 답변, 2026-11-06): ① 어제(11-05) Flutter 마이그레이션 잔여물(apps/host-flutter, apps/viewer-flutter, .fvmrc, theme-dart.ts, Flutter 계획 문서 2건) 삭제. ② 모바일은 순수 TS 코어 **전체**를 Rust로 포팅해 RN과 공유(UI·화면은 RN 유지).
- 제외: 영상·고주파 입력 데이터 경로 변화 없음(기존 규칙 유지), 실기(Galaxy XR) 스트리밍 실측(사용자 협조 필요, 별도 단계), 기존 Tauri Host 앱의 즉시 삭제(마이그레이션 완료까지 병행 유지, 제거는 후속 계획).
- 도구: GPUI-Kit = crates.io `gpui-kit` 0.9.0(Longbridge — gpui-component 0.9 계열을 하나의 의존성으로 묶는 배포 크레이트; 75+ 컴포넌트, macOS/Windows/Linux, stable Rust 2024 edition). 본 머신(cargo 1.99.1, Linux/Wayland)에서 2026-11-06 빌드 검증 완료(dev profile 29.6s, gpui-pre-linux/accesskit_unix 포함).

## 1. 조사 요약 (근거)

### host-desktop (React 약 6.4k + Rust 약 39.1k)

- 윈도우 4종: 대시보드 `main` 780×540(min 640×440, 닫힘→트레이 숨김·dock 숨김), 페어링 `pairing` 420×560(트레이에서 생성, 닫히면 offer 소각), 인디케이터 `indicator` 220×30(장식 없음·최상단·작업표시줄 제외·커서 무시(비 Linux)·전 워크스페이스 표시), 커튼 `curtain-{i}`(모니터별 검정 오버레이, macOS 전용 — 제목 "leftcar-curtain"이 캡처 제외 shim).
- 프론트↔백엔드: `invoke()` 47개 명령(lib.rs:630~1270), **이벤트 0개 — 전부 폴링**. 대시보드 2s, 페어링 기기 2s/대기 요청 1.5s, KM 동의 1s, QR 시계 1s, 가상디스플레이 2s, 인디케이터 2s. 가시성 게이트(document.hidden+focus)는 대시보드만. 지연 응답은 단조 카운터(refreshRevision/operation/panel revision/ToggleGate)로 폐기.
- 순수 로직(React 없음, 이식 대상): `hostState.ts`(26, trayStatus), `streamTermination.ts`(84, 종료 분류·문안), `toggleGate.ts`(28, 낙관 토글 중재), `paired-device-state.ts`(155, revision 게이트 병합 엔진), `encoderDiagnostics.ts`(191, 인코더 진단 뷰), `sessionTypes.ts`(163, SessionRow), vitest 스펙(589줄).
- **panel.rs(948)가 이미 존재**: 47명령 전체를 루프백 관리 프로토콜로 미러링(TCP 127.0.0.1 에페머럴 + 0600 토큰 파일 + 개행 JSON `{"id","command","args"}`/`{"id","ok","result"|"error"}`, ≤1MiB 라인, 테스트 13건). `bin/leftcar-hostd.rs`(172)가 Tauri 없이 `bootstrap::start_host_services()`를 띄우고 panel::serve. `bootstrap.rs`(381)는 Tauri/sidecar 공유 기동. → Flutter 계획의 T-8 산출물을 그대로 재사용한다.
- 트레이: `TrayIconBuilder`(show/pairing/quit, 라벨은 settings.language 따름). 클립보드: `ClipboardBackend` 트레이트(`clipboard.rs:441`) — TauriClipboard(SystemClipboard pbcopy/pbpaste 폴백)를 `server.set_clipboard`로 주입. 대시보드 트레잇 `DashboardWindow`(dashboard.rs).
- i18n/테마: `packages/ui-tokens`(ko/en 번역 스키마 + 변형 헬퍼)와 `theme.css`(`data-theme`=system/light/dark), localStorage 키 `leftcar_lang`/`leftcar_theme`.

### viewer-expo (순수 TS 코어 약 5,953줄 / 53파일, RN 의존 약 6,622줄)

- 순수 코어(포팅 대상, RN import 없음): control.ts(732 — 프레이밍은 순수, 전송 계층 `ControlClient` 주입), session.ts(465), launch-stream.ts(779), pairing.ts(684 — SecureStore는 주입점), clipboard-sync.ts(406), decoder-budget.ts(336), adaptive-resolution.ts(309), file-transfer.ts(290), secure-channel.ts(271 — **crates/secure-channel에 이미 동일 구현+벡터 존재**), stream-recovery(215)·streaming-policy(188)·recent-hosts(368)·connect-flow(217) 등 전체 53파일.
- Rustra 파이프라인: `crates/control-contract`(host.rs 1,447 — 권한/소스/페어링/스트림 타입 이미 정의, `add_numbers`만 등록; viewer.rs 204; km.rs; udp_stability.rs) → `packages/control-generated`(TS) + `apps/viewer-expo/generated/*` + `modules/rustra-bridge`(JSI C++ 어댑터 + Kotlin) + `native/leftcar-rustra`(정적 lib, `rustra::native_entry!`). 게이트: `bun run rustra:generate`/`rustra:check`(sha256 소유 매니페스트, schemaHash be754a50…).
- 네이티브: 영상 경로 `libleftcar_viewer.so`(JNI, ViewerNative 39 funs, 풀-폴링, RN 독립 — 무변경). Rustra JSI는 `librustra_bridge.so`(정적 leftcar_rusta.a 포함) — 지금은 proof 전용.
- 검증 기준선: 루트 vitest(apps/viewer-expo/src에 52파일/12,416줄 — secure-channel 벡터, control 프레이밍 등), `test:architecture` clean, react-doctor 100/100.

## 2. 목표 아키텍처

### 2.1 host — `host-gpui`(in-process GPUI 앱)

```
apps/host-desktop/src-tauri/        # 기존 코어+panel.rs+leftcar-hostd (단독 워크스페이스 루트)
apps/host-desktop/src-tauri/host-gpui/  # 신설: src-tauri 워크스페이스 멤버 크레이트(members=["host-gpui"])
  Cargo.toml                        # gpui-kit 0.9 + leftcar-host-desktop(path = "..", 코어·dispatch 재사용)
  src/main.rs                       # gpui_kit::application() 진입, bootstrap::start_host_services, 트레이, 종료 시퀀스
  src/commands.rs                   # panel::dispatch 1:1 위임 타입 래퍼(47명령, 백그라운드 스레드 폴링)
  src/state/                        # 순수 로직 Rust 포팅(termination/toggle_gate/paired_device_state/
                                    #   encoder_diagnostics/host_state) + 단위테스트
  src/ui/                           # 대시보드/페어링/인디케이터/모달/설정 카드(gpui_kit::component)
  src/i18n.rs                       # ko/en 문안(ui-tokens 스키마와 동일 키) + 테마(시스템/라이트/다크)
  src/tray.rs                       # Linux ksni(StatusNotifierItem, GTK 불요), macOS/Windows tray-icon
```

핵심 원칙:

1. **Tauri 앱과 동일한 단일 프로세스 구조를 GPUI로 재현한다(2026-11-06 개정).** 원 설계는 hostd 사이드카+패널 프로토콜 클라이언트였으나, 이는 Dart가 Rust를 직접 호출하지 못하는 Flutter 제약의 산물이다. Tauri 앱은 UI와 코어가 같은 프로세스이므로 parity의 정확한 재현은 in-process다: host-gpui가 `bootstrap::start_host_services()`로 코어를 기동하고, UI 폴러가 `panel::dispatch(&PanelHandles, command, args)` 1:1 위임 표를 백그라운드 스레드에서 호출한다(비동기 명령은 주입된 런타임 핸들로 block_on — panel.rs 내장 방식과 동일). `leftcar-hostd`와 `panel::serve`는 headless/사이드카 경로로 그대로 유지되며 프로토콜 테스트 13건도 유지된다. 토큰·포트 파일·TCP 클라이언트는 GPUI 경로에 필요 없다.
2. **명령 표면은 panel.rs dispatch 표와 1:1로 유지한다.** 뷰 타입(StatusViewPublic, PairedDeviceState 등)은 leftcar-host-desktop 크레이트에서 path 의존으로 재사용하고, 명령 이름·arg(camelCase)·결과 JSON 형태는 dispatch 표와 동일. host-gpui는 panel.rs의 TestHarness 선례(in-memory services)로 래퍼 단위테스트를 둔다.
3. **GPUI 윈도우 매핑**: 대시보드(780×540, min 640×440), 페어링(420×560, 트레이→생성), 인디케이터(220×30, always-on-top, skip taskbar, 커서 무시 — gpui WindowOptions + 플랫폼 보강), 커튼은 현행 유지(macOS 전용, Linux는 토글 no-op). 트레이는 gpui-kit에 없으므로 ksni(Linux DBus)/tray-icon(macOS·Windows)으로 구현하고 메뉴 이벤트를 GPUI executor로 전달한다. 종료 시 `bootstrap::run_exit_shutdown`(UPnP ≤3s·승인·가상디스플레이 ≤2s·감사)을 Tauri의 RunEvent::Exit 경로와 동일하게 수행한다.
4. **상태·오류 경계 이식**: 지연 응답 폐기(단조 revision), 토글 게이트(초기 로드 무시), 종료 배너 dedup(`session:state:error` 키), 페어링 기기 병합(monotonic revision + tombstone), 호스트 오류 분류(원인 문자열→5종), "Measuring" 원칙(측정 전 값 조작 금지) — React vitest 스펙을 Rust 테스트로 포팅해 실행 가능한 명세로 삼는다.
5. **호스트 플랫폼 우선순위**: 이번 사이클은 Linux(개발 머신, Wayland/Hyprland) 우선 검증. macOS/Windows는 소스 수준 정합(조건부 컴파일 최소화)을 유지하고 실행 검증은 후속. 커튼·dock 숨김 등 macOS 전용 경로는 현행 동작 유지.

### 2.2 mobile — RN 유지 + `viewer-app-core`(Rust 코어) + Rustra JSI

```
crates/viewer-app-core/             # 신설: 루트 워크스페이스 멤버 (순수 로직, no_std 아님, tokio 금지)
  src/control/                      # 개행 JSON 프레이밍·한도(16KiB/12MiB)·FIFO 32·바이트 예산 16MiB·
                                    #   명령별 타임아웃·전송 주입(ControlTransport 트레이트)
  src/secure/                       # crates/secure-channel 재사용(벡터 동일) + StreamSealer 래퍼
  src/pairing/                      # QR 페이로드/엔드포인트/자격증명 스키마/pair 흐름(저장소 주입)
  src/session/                      # 세션 상태, connect-flow, auto-reconnect 게이트
  src/prefs/                        # viewer-preferences/preference-persistence/native-settings/
                                    #   udp-stability/recent-hosts(저장 IO 주입, 키 9종 스키마 보존)
  src/stream/                       # streaming-policy·stream-resolution·stream-profile·recovery·
                                    #   termination-policy·lifetime·session-store·reserved-stream·
                                    #   adaptive-resolution·decoder-budget·thermal-policy·
                                    #   encoder-experiment·receiver-telemetry·launch-stream 의사결정
  src/files/                        # file-transfer(768KiB 청크, 512MiB 한도, 오류 매핑)
  src/clipboard/                    # clipboard-sync 게이트·해시·상태(IO 주입)
  src/diag/                         # diagnostics 코어 + recorder 포트 모델
  src/km/                           # km-session·km-entry·km-failure
  src/misc/                         # catalog-helpers·display-size/resize·extension-display·
                                    #   discovery-hint·input-approval·transport-label·defaults 등
  tests/                            # 포팅 스펙(아래 §4)
native/leftcar-rustra/              # 확장: viewer-app-core를 dep로, 명령 핸들러 등록 (스레드+std::net TCP)
crates/control-contract/src/viewer_app.rs  # 신설 패키지 "leftcar.viewer.app" — UI↔코어 명령/상태/오류 계약
apps/viewer-expo/src/               # 포팅된 모듈은 생성된 브리지 위임 래퍼로 교체(공개 API 유지)
```

핵심 원칙:

1. **경계 규칙 준수(docs/04)**: 영상·고주파 입력은 여전히 `libleftcar_viewer.so` 네이티브 경로. Rustra는 **로컬 UI↔코어 계약**만. 제어 평면의 TCP 소켓 소유를 TS에서 Rust로 이전하고, 네트워크 프로토콜 바이트는 Rustra를 통과시키지 않는다(브리지에는 UI 수준 명령·상태·오류만 오간다).
2. **RN 공개 API 유지**: 포팅된 `*.ts`는 같은 함수/타입을 내보내는 얇은 래퍼가 된다. 훅·화면(use-catalog-model 등 6.6k줄 RN 계층)과 `*.native.ts` 이벤트 shim(Kotlin 발신)은 논리 변경 없이 유지된다. 전송 계층(TCP)만 Rust 내부로 사라진다.
3. **런타임 모델은 풀-폴링 선례**: `librustra_bridge.so`는 스레드 + `std::net::TcpStream` + 채널로 제어 소켓을 운용한다(tokio 정적 lib 금지 — `libleftcar_viewer.so`의 풀-폴링 모델과 동일 방침). Rust→JS 이벤트 대신 상태 조회 명령(기존 React의 폴링 관습과 일치). Kotlin 발신 이벤트(stream-termination 등)는 그대로.
4. **저장 IO는 주입**: SecureStore·파일 IO는 RN 측 어댑터가 유지하고 Rust는 순수 로직+스키마만 소유한다(기존 `*.core.ts`/`*.native.ts` 분할과 같은 형태). 영속 키 9종 이름·스키마 보존.
5. **코드젠 단일 소스**: 명령 표면은 `crates/control-contract`에서 정의하고 `bun run rustra:generate`로 TS·C++ 코덱·브리지를 생성한다. 배너 파일 수동 편집 금지, `rustra:check` 게이트 유지. contract hash 변화는 생성 매니페스트로 추적된다.
6. **테스트 이전 원칙**: 포팅된 모듈의 vitest 스펙(프레이밍 한도, FIFO, 데드라인, 봉인 채널 벡터, 정책 상태머신, 선호 저장, 재접속 게이트 등)은 Rust 테스트로 이전하고 해당 vitest 파일은 제거한다. RN 계층(훅·UI) 스펙은 vitest에 남는다. 기준선 변화를 문서에 기록한다.

### 2.3 잔여물 정리 (사용자 확정)

- 삭제: `apps/host-flutter/`, `apps/viewer-flutter/`, `.fvmrc`, `packages/ui-tokens/src/theme-dart.ts`, `docs/plans/2026-11-05-flutter-migration-design.md`, `docs/plans/2026-11-05-flutter-migration.md`, package.json의 `flutter:*` 스크립트, CI의 flutter 작업(있다면).
- 보존: Flutter 계획이 만든 `panel.rs`·`bootstrap.rs`·`bin/leftcar-hostd.rs`(host 측 자산, 본 계획이 재사용), 기존 앱 전체.

## 3. UI 설계 기준 (review-design 설계 모드)

- 대상 사용자·핵심 작업: PC에서 Leftcar Host를 띄워 둔 운영자 — ① 페어링 코드 생성·승인, ② 스트림 상태 감시·입력 공유 토글·품질 조정·정지, ③ 화면/입력 권한 처리, ④ 설정(클립보드 공유·프라이버시 커튼·스트리밍 배지·WAN·확장 디스플레이·파일 공유·실험 값). 진입은 트레이(대시보드 표시/페어링), 이탈은 창 닫기→트레이 숨김.
- 정보 구조는 기존 화면이 표준(parity): 대시보드 = 헤더(제목·언어·테마·도움말·설정) + 본문(스크린 미준비 히어로 또는 스트림 목록+세션 카드) + 푸터(주소 복사·수동 라벨) + 배너(권한·오류·종료). 모달 4종(페어링·문제해결·설정·정지 확인). 페어링 윈도우 = 네트워크 카드·QR/PIN·승인 대기·페어링 기기 목록(그랜트·KM 동의·폐기).
- 디자인 소스: `packages/ui-tokens` 토큰(ko/en 문안·색·변형)과 기존 Tailwind 레이아웃 의미론. gpui-kit 컴포넌트(Button/Toggle/Input/Modal/Notification/Progress/Table 등)로 매핑하며 픽셀 수준 재현은 목표로 하지 않고 계층·밀도·상태 표현 parity를 기준으로 한다. 테마는 system/light/dark를 gpui-kit 테마로 반영.
- 상태 전이: 초기(첫 폴링 전)→준비/오류, 세션(running/stopped/error/unknown), 토글(낙관→확정/오류+재시도), 페어링(유휴→제안→대기→성공/만료/거부), 인코더 진단("Measuring" 원칙). 모든 지연 응답은 revision 폐기.
- 접근성·입력: 키보드 단축키(⌘/Ctrl+P·H·R·,), 포커스 이동 순서, 의미 있는 라벨(ko/en), 44px 상당 히트 타깃, 780×540(min 640×440)에서 스크롤·줄바꿈 유지. 구현 후 Linux에서 실제 창·키보드·트레이를 관찰해 검증(G4).
- 뷰어(RN) UI는 프레임워크·화면 무변경 — 디자인 검토 대상 아님(회귀 관찰만).

## 4. 오류·실패 처리와 상태 경계

- 패널 연결 실패/토큰 불일치: GPUI 앱은 hostd 기동 실패·토큰 파일 부재를 오류 대화상자로 표시하고 종료한다(Tauri 앱의 fatal_startup_error 선례). 폴링 중 연결 끊김은 배너 상태(error 분류)로 표시하고 재연결을 시도한다.
- 제어 평면(RN): transport/timeout/capacity/auth 오류 분류와 문안 매핑(control-error/localized-error)을 Rust로 이식, JSI 오류는 계약의 error 필드로 전달된다.
- 스트림 종료 분류(stream-termination-policy)와 재시도 UI 분기, 호스트 종료 배너 문안 파생 유지.
- 요청 세대 폐기: 대시보드 refreshRevision, 가상디스플레이 operation, 페어링 revision, 토글 seq — Rust 상태 모듈이 동일 규칙을 소유하고 테스트로 고정한다.

## 5. 마이그레이션·롤백

- 병행 기간: Tauri Host·RN Viewer는 삭제하지 않는다. host-gpui는 별도 바이너리, viewer-app-core는 TS 래퍼 뒤로 들어온다.
- 전환: Host는 GPUI 앱이 parity를 충족한 뒤 Tauri 앱 제거·트레이/자동실행 확인을 별도 계획으로. Viewer는 래퍼 교체가 즉시 전환이며(같은 앱 ID), 롤백은 래퍼를 이전 TS 구현으로 되돌리는 것으로 충분하다.
- 위험: 패널 프로토콜 타입 드리프트(완화: panel.rs 테스트 + 클라이언트 fake 테스트), gpui-kit Linux 세부(커서 무시·작업표시줄 제외 속성 — 플랫폼 보강 또는 격차 명시), JSI 스레드 안전성(Rustra 네이티브 엔트리의 스레드 모델 준수), 대규모 포팅의 정책 드리프트(vitest→cargo 스펙 이전), known-red 기준선 혼동(기준선 고정·신규 0 유지).

## 6. 미해결 질문 (구현 태스크에서 확정)

1. Rustra 0.12 패키지 모델에서 "leftcar.viewer.app" 신설 패키지 등록 위치와 `native/leftcar-rustra`의 다중 패키지 지원 방식(현제 host_package 단일 등록).
2. gpui-kit에서 인디케이터 요건(always-on-top+skip-taskbar+커서 무시)의 커버 범위 — 부족 시 Linux 플랫폼 보강 수준과 격차 표기.
3. ksni 트레이가 Hyprland 세션에서 메뉴 표시를 관찰 가능한지(가능하면 V에서 관찰, 불가하면 DBus 등록 확인으로 대체).
