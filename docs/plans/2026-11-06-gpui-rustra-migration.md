# GPUI + Rustra 마이그레이션 계획 (host GPUI 전환 + viewer 코어 Rust 공유)

- 설계 문서: [2026-11-06-gpui-rustra-migration-design.md](./2026-11-06-gpui-rustra-migration-design.md)
- 원 요청: "GPUI와 GPUI-Kit을 이용해서 모두 마이그레이션을 진행해줘. 모바일은 RN 과 함께 rust로 core로직을 공유하게 해주고." — 확정: Flutter 잔여물 삭제, 모바일은 순수 TS 코어 전체 포팅(사용자 답변, 2026-11-06)
- 기준 상태: 커밋되지 않은 작업 트리(panel.rs·leftcar-hostd·linux_backend 등 포함)를 기준 리비전으로 삼는다. 기존 Tauri Host·RN Viewer는 유지하며 `apps/host-desktop/src-tauri/host-gpui`와 `crates/viewer-app-core`를 추가한다. 기준 트리에서 src-tauri 워크스페이스는 컴파일 오류 6건을 갖고 있어(T-0에서 수리, 아래 기준선 참고), 본 계획은 수리된 트리를 기준으로 한다.
- 도구·환경: cargo 1.99.1(rust-toolchain 고정), bun 1.4.3(설치 실측), gpui-kit 0.9.0(Longbridge — gpui-component 0.9 계열을 묶는 배포 크레이트; 빌드 검증 완료 2026-11-06, /tmp 스크래치 크레이트), Linux Wayland(Hyprland/omarchy) 세션, Android SDK `/home/loopy/android-sdk` + x86_64 에뮬레이터 빌드 선례, 평가자: 위임 하위 에이전트(429 재시도 1회 후 self-review 대체를 기록).
- 기준선(2026-11-06 01:53 KST 측정, /tmp/sdd-baseline-js.txt·/tmp/sdd-baseline-rust.txt): `bun run typecheck` PASS, `bun run test:architecture` "TS/Kotlin rules clean", `bun run test` **979/979 통과(80파일)**, `cargo test --workspace --locked`(루트) exit 0. src-tauri 워크스페이스 cargo test는 T-1에서 재측정한다.

## 수락 기준

| ID | 관찰할 결과와 적용 조건 | 담당 태스크 | 검증 ID |
|---|---|---|---|
| AC-H1 | host-gpui가 in-process로 `bootstrap::start_host_services`를 기동하고 `panel::dispatch` 1:1 위임 표(47명령)를 타입 래퍼로 호출한다. panel.rs 13테스트·leftcar-hostd headless 경로는 유지되며, host-gpui는 panel.rs TestHarness 선례의 in-memory fake로 래퍼 테스트를 갖는다(2026-11-06 개정: Tauri와 동일한 단일 프로세스 구조가 parity의 정확한 재현) | T-1 | V-1, V-6 |
| AC-H2 | GPUI 대시보드(780×540, min 640×440)가 폴링으로 상태·세션 카드(입력 토글·품질 슬라이더·정지)·권한/종료 배너·설정 모달(토글 4종·확장 디스플레이·파일 공유·실험 섹션)·문제해결 모달·인스펙터(파이프라인/인코더 진단)를 표시·제어한다 | T-3 | V-5, V-8 |
| AC-H3 | 페어링 윈도우·모달이 QR/PIN·대기 승인(허가/거부)·페어링 기기 목록(그랜트·KM 동의·폐기 확인)을 동작시키고, 페어링 기기 병합 엔진(revision 게이트·tombstone)이 Rust 테스트로 고정된다 | T-2, T-4 | V-3, V-5, V-8 |
| AC-H4 | 인디케이터 윈도우(220×30, 항상 위·작업표시줄 제외)가 스트리밍 중 표시/숨김되고 커튼 토글은 현행 호환(macOS 전용, Linux no-op)이다 | T-5 | V-5, V-8 |
| AC-H5 | 트레이 메뉴(대시보드 표시/페어링/종료, 언어 반영)와 대시보드 닫힘→트레이 숨김이 Linux에서 동작한다 | T-5 | V-5 |
| AC-H6 | ko/en 전환·테마(system/light/dark)·단축키(⌘/Ctrl+P·H·R·,)가 동작한다 | T-3 | V-5 |
| AC-V1 | 뷰어 순수 TS 코어 전체가 `crates/viewer-app-core`로 포팅되고 cargo 테스트가 통과한다 — 프레이밍 한도·FIFO·데드라인·봉인 채널 벡터(crates/secure-channel 재사용)·정책 상태머신·선호 저장·재접속 게이트 포함. "전체"의 계산 규칙: RN/react import가 없는 `apps/viewer-expo/src/*.ts` 순수 모듈(조사 기준 53파일·약 6.0k줄, 테스트 제외) — `*.native.ts` 플랫폼 shim과 use-*/화면 RN 계층은 제외하며 T-6·T-7 경계에 파일별로 열거한다 | T-6, T-7 | V-2 |
| AC-V2 | Rustra 계약에 뷰어 앱 명령 표면이 등록되고 `rustra:generate`·`rustra:check`가 통과한다(생성물 무결성·hash 기록) | T-8 | V-4 |
| AC-V3 | RN 앱의 포팅 모듈이 JSI 브리지 위임 래퍼로 교체되고 공개 API·훅·화면·`*.native.ts` 이벤트가 유지된다 | T-9 | V-7, V-9 |
| AC-V4 | Android debug APK(x86_64 에뮬레이터)가 빌드되고 로컬 leftcar-hostd와 제어 평면 연결(페어링→카탈로그)이 성립한다 | T-10 | V-10 |
| AC-C1 | 기존 게이트 유지 — 루트 vitest(이동된 스펙 제외 후 전부 통과)·typecheck·test:architecture(clean)·TSX/RN 변경에 react-doctor 100/100 | T-0, T-9, T-11 | V-7, V-9, V-11 |
| AC-C2 | Flutter 잔여물이 삭제된다 — apps/host-flutter, apps/viewer-flutter, .fvmrc, theme-dart.ts, flutter 스크립트·문서·CI 잔여 제거, flutter 재사용 자산(panel.rs 등)은 보존 | T-0 | V-11 |
| AC-C3 | Rust 게이트 — 루트 워크스페이스 cargo test(+src-tauri 워크스페이스 cargo test) 통과(host-gpui·viewer-app-core 포함) | T-1, T-7, T-11 | V-1, V-2, V-6 |
| AC-C4 | 문서 동기화 — 신설 계획·설계 문서, docs/README 인덱스, EVIDENCE.md, README 현재 상태 갱신 | T-11 | V-12 |

명시적 한계: 실기(Galaxy XR) 설치·스트리밍 실측은 사용자 협조가 필요해 본 계획에서 제외하고 별도 BLOCKED 표기한다. macOS/Windows는 소스 수준 정합까지만(실행 검증 후속). 인디케이터·트레이의 플랫폼 속성이 gpui-kit/ksni에서 관찰 불가하면 V-5에서 격차로 명시한다.

## 태스크

| ID | 목적·AC | 수정할 파일/경계 | 선행 | 검증 | 상태 |
|---|---|---|---|---|---|
| T-0 | 기준선 측정(typecheck/test/architecture/cargo test 기록), src-tauri 컴파일 오류 6건 수리(bootstrap 추출 잔재 중복 정의 제거·panel Vec<u8> trim·async `?`·never 코어션·테스트 클로저 캡처), Flutter 잔여물 삭제·스크립트 정리(AC-C2) | apps/host-flutter/**, apps/viewer-flutter/**, .fvmrc, packages/ui-tokens/src/theme-dart.ts, docs/plans/2026-11-05-flutter-*.md(삭제), package.json, apps/host-desktop/src-tauri/src/{lib.rs,panel.rs,bin/leftcar-hostd.rs}(수리) | 없음 | V-11, V-1 | done |
| T-1 | host-gpui 크레이트 스캐폴드(src-tauri 워크스페이스 멤버) + in-process 코어 기동 + panel::dispatch 1:1 타입 래퍼(47명령, 백그라운드 폴러) + fake-harness 테스트(AC-H1, 2026-11-06 개정) | apps/host-desktop/src-tauri/host-gpui/**(신설), apps/host-desktop/src-tauri/Cargo.toml(members 추가) | T-0 | V-1, V-6 | done |
| T-2 | 호스트 순수 로직 Rust 포팅: streamTermination·toggleGate·paired-device-state·encoderDiagnostics·hostState + vitest 스펙 이전(AC-H3 지원) | apps/host-desktop/src-tauri/host-gpui/src/state/** | T-1 | V-3 | done |
| T-3 | GPUI 대시보드: 헤더/푸터/히어로/스트림 목록/세션 카드/배너/모달 4종/설정 카드(확장 디스플레이·파일 공유·실험)/인스펙터/폴링·가시성 게이트/i18n·테마/단축키(AC-H2, H6) | apps/host-desktop/src-tauri/host-gpui/src/ui/**, src/i18n.rs | T-1, T-2 | V-5, V-8 | pending |
| T-4 | GPUI 페어링: QR 카드·승인 대기·페어링 기기 행·KM 동의 패널·폐기 확인·윈도우 생성(AC-H3) | apps/host-desktop/src-tauri/host-gpui/src/ui/pairing/** | T-3 | V-5, V-8 | pending |
| T-5 | 인디케이터 윈도우·커튼 토글 호환·트레이(ksni)·닫힘→숨김(AC-H4, H5) | apps/host-desktop/src-tauri/host-gpui/src/tray.rs, src/ui/indicator.rs, src/main.rs | T-3 | V-5 | pending |
| T-6 | viewer-app-core: 제어 클라이언트 엔진(프레이밍·한도·FIFO·데드라인) + secure-channel 연동 + 페어링/자격증명(AC-V1 핵심) | crates/viewer-app-core/**(신설), Cargo.toml(루트 워크스페이스 members에 추가 — T-1·T-6가 공유하지 않도록 T-6가 단독 소유) | T-0 | V-2 | pending |
| T-7 | viewer-app-core 나머지: 세션·정책·예산·적응·선호·호스트·파일·클립보드·진단·KM·기타 + 스펙 이전(AC-V1) | crates/viewer-app-core/src/{session,stream,prefs,files,clipboard,diag,km,misc}/** | T-6 | V-2 | pending |
| T-8 | Rustra 계약 확장: viewer_app.rs 명령 정의, 코드젠 재생성, leftcar-rustra 핸들러 등록(스레드+std::net TCP 소유)(AC-V2) | crates/control-contract/src/**, native/leftcar-rustra/**, packages/control-generated/**, apps/viewer-expo/{generated,modules}/** | T-7 | V-4 | pending |
| T-9 | RN 래퍼 교체: 포팅 모듈 → 브리지 위임(공개 API 유지), 훅·화면 최소 수정, 이동된 vitest 제거(AC-V3, C1) | apps/viewer-expo/src/** | T-8 | V-7, V-9 | pending |
| T-10 | Android 빌드(x86_64) + 에뮬레이터 제어 평면 스모크(페어링→카탈로그)(AC-V4) | apps/viewer-expo/android/**(빌드 설정 필요 시), 빌드 산출물 | T-9 | V-10 | pending |
| T-11 | 게이트 전수(vitest/typecheck/architecture/react-doctor/cargo 전체) + 문서(인덱스·EVIDENCE·README)(AC-C1, C3, C4) | docs/**, README.md | T-5, T-10 | V-7, V-9, V-11, V-12 | pending |

병렬화: T-0 직후 T-1(host)과 T-6(mobile core)은 파일 경계가 완전히 분리되어 동시 진행. T-2→T-3→T-4→T-5는 host-gpui 내부 순차. T-7→T-8→T-9→T-10은 viewer 경로 순차. 두 경로는 T-11에서 수렴한다. 위임 구현은 경계별 하위 에이전트(최대 2동시)로 하고 rate-limit 시 순차 self-구현으로 전환한다.

## 검증 계획과 결과

| ID | 방법·정의 근거 | cwd·환경 | 기대 결과 | 실제 결과·증거 | 판정 |
|---|---|---|---|---|---|
| V-1 | 클라이언트 fake 서버 테스트 + `cargo test -p host-gpui` 및 기존 panel 테스트(`cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked`) | apps/host-desktop/src-tauri(host-gpui 멤버) | 신규+기존 테스트 통과 | `cargo test -p host-gpui` 84/84, 워크스페이스 `cargo test --locked` 413 lib + 13 bin 전부 통과, clippy 0 경고(2026-11-06, T-1+T-2 위임 보고). 실런타임 스모크: Wayland에서 창 매핑·제어 서버 7777·SIGTERM 정상 종료(hyprctl clients 관찰) | pending |
| V-2 | `cargo test -p viewer-app-core`(+워크스페이스 전체) — 포팅 스펙 실행 | 저장소 루트(cargo PATH) | 전부 통과, 한도·벡터·정책 고정 | 미실행 | pending |
| V-3 | T-2 스펙 이전 테스트(`cargo test -p host-gpui state`) — vitest 원본과 동등 케이스 | apps/host-desktop/src-tauri | 전부 통과 | termination 10·toggle_gate 3·paired_device_state 20·encoder_diagnostics 15+2(React 렌더 2건은 뷰 데이터+라벨 표 테스트로 치환)·host_state 6·error_view 6·commands 17 — 84/84 통과(T-1+T-2 위임 보고). vitest 2건 React 렌더 단정은 T-3 UI 범위로 이동 | pending |
| V-4 | `bun run rustra:generate && bun run rustra:check` + `cargo test -p control-contract` | 저장소 루트 | 생성 청결, check 통과, hash 기록 | 미실행 | pending |
| V-5 | Hyprland 세션 실관찰: hostd+host-gpui 기동 → 대시보드 렌더(스크린샷), 트레이 메뉴, 페어링 윈도우·모달, 인디케이터 표시 조건, 단축키, ko/en·테마 전환, 닫힘→숨김 | 개발 머신(Wayland) | 나열 동작 관찰(스크린샷·스크립트 출력) | 미실행 | pending |
| V-6 | hostd 스모크: `--panel-token-file`로 기동 → 클라이언트로 get_host_platform/get_status 호출 성공 | 저장소 루트 | 47명령 표면 응답 확인 | 미실행 | pending |
| V-7 | `bun run typecheck` + `bun run test`(유지 스펙) | 저장소 루트 | 기준선 유지(이동분 감소 명시) | 미실행 | pending |
| V-8 | GPUI UI 단위·렌더 관찰 보조: state 모듈 테스트와 V-5 관찰의 조합 | — | AC-H2~H4 상태 전이 증명 | 미실행 | pending |
| V-9 | `npx -y react-doctor@latest . --verbose`(저장소 루트) 100/100 + RN 유지 스펙 | 저장소 루트 | 100/100 | 미실행 | pending |
| V-10 | 에뮬레이터 x86_64 debug APK 빌드(Gradle cargo 태스크로 libleftcar_rusta.a 재빌드 확인) → AVD 설치 → leftcar-hostd 페어링·카탈로그 성공 | ANDROID_HOME=/home/loopy/android-sdk, 에뮬레이터 | 페어링→카탈로그 성립 + 화면 관찰 | 미실행 | pending |
| V-11 | `bun run test:architecture`(clean) + 삭제 대상 부재 확인(git status/ls) | 저장소 루트 | 총 위반 0, 잔여물 없음 | 미실행 | pending |
| V-12 | docs/README 인덱스·EVIDENCE.md·README 상태 대조 | 저장소 루트 | 기록 존재·동기화 | 미실행 | pending |
| V-13 | 실기(Galaxy XR) 설치·스트리밍 실측 | 사용자 기기 | 스트림 렌더·이벤트 반영 | 미실행 — 사용자 협조 필요, 별도 단계 | BLOCKED(예고) |

## 평가와 인계

- G1 관문: validate-plan plan 모드 + review-design 설계 모드로 본 문서·설계 문서를 평가한다(위임 평가자; 실패 시 429 재시도 1회 후 self-review 기록). 통과 후 G2로 즉시 진행(전체 SDD 요청).
- G2 평가: 태스크별 validate-plan task 모드, 독립 하위 에이전트에 diff·증거 전달. G3은 review-changes로 최종 diff 재검토. G4는 V-5 관찰로 review-design 구현 모드 적용(host UI만, RN은 회귀 관찰). G5는 completion 모드로 AC별 증거 대조.
- 완료 판정: V-13(실기)·macOS/Windows 실행 검증은 계획 시점 BLOCKED 예고로 분리. 나머지 필수 검증이 전부 PASS일 때 "Linux 에뮬레이터·빌드·윈도우 관찰 범위에서 완료, 실기·타 OS 실행 검증은 보류"로 보고한다. 부분 성공을 전체 완료로 표기하지 않는다.
