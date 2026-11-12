# Windows Host exe 산출과 빌드 회복 계획 (2026-11-12)

원 요청: "맥은 잘 되기도 하는데(30fps로 떨어지기도 한다). 윈도우와 리눅스에서도 잘 되도록 해줄 수 있어? 일단 윈도우부터 진행하자. 내일 아침에 다시 확인할게. exe파일을 만들어줘."

## 배경 사실 (조사·감사 완료)

- Windows 백엔드는 이미 구현돼 있다: `apps/host-desktop/src-tauri/src/windows_backend/`(WGC 캡처 + Media Foundation HW H.264 + SendInput + UDP/USB-AOAP). 신규 구현이 아니라 빌드·검증·산출이 과제.
- main 8bbd08b 이후 CI 전 cargo job 실패: 루트 `Cargo.toml`의 `unikit-bridge = { path = "../rustra/crates/unikit-bridge" }`가 CI 체크아웃에 없음. crates.io 미게시(404 확인). 형제 로컬 디렉터리 `/home/loopy/dev/ll3/rustra`의 git origin은 **github.com/loopy-lim/unikit-bridge(공개)**, HEAD `aea88a8f`(clean, origin/main에 존재). 마지막 green run 2026-11-09(`Leftcar Host_0.1.11_x64-setup.exe` 산출).
- 이 머신(Arch Linux): clang/clang-cl 23.1.3, llvm-rc, rust-lld 있음. cargo-xwin 0.23.1 + 타깃 `x86_64-pc-windows-msvc` 설치 완료. mingw/makensis 없음(NSIS는 tauri CLI가 자체 다운로드).
- SDD 규칙상 커밋·푸시는 사용자 요청 시에만 → 오늘 밤 exe는 로컬 크로스컴파일(cargo-xwin)로 산출. CI는 다음 push에서 즉시 재가동되도록만 수정.
- 버전 정책(docs/versioning.md): 버전은 함부로 올리지 않는다. 0.1.11 유지, 산출물은 경로로 구분.
- **G1 감사 반영(F-1)**: `actions/checkout`은 워크스페이스 밖 배치(`path: ../rustra`)를 거부하므로, 워크스페이스 내 `path: unikit-bridge-sibling` 체크아웃 후 심볼릭링크(ubuntu/macos)·정션 `mklink /J`(windows)로 `../rustra` 위치를 만든다.

## 수용 조건 (AC)

- **AC-1 Windows 소스 수준 검증**: 현재 작업 트리에서 호스트 크레이트가 `x86_64-pc-windows-msvc` 타깃으로 `cargo check --tests` 및 `clippy --tests -D warnings`를 통과한다(cargo-xwin 교차, 이 머신). **1차 check exit 0 확보(2026-11-12)**.
- **AC-2 exe 산출**: 사용자가 Windows PC에서 바로 설치·실행할 산출물이 이 머신 디스크에 존재한다. 우선 NSIS `*-setup.exe`(산출 경로 `apps/host-desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`), 차선 portable `leftcar-host-desktop.exe`(+`leftcar-hostd.exe`). `file`로 PE32/NSIS 확인. 미서명 internal 후보(공개 배포 아님)이며 SmartScreen 경고·"추가 정보→실행" 절차가 예상됨을 인수인계에 명시.
- **AC-3 CI 빌드 회복**: `../rustra` 형제 저장소 의존을 CI가 해결하도록 고쳐 다음 push에서 rust-fast/ts/android-compile/windows-host가 재실행될 수 있다. 실제 CI 실행은 사용자 push 후(이번 검증은 구문·구조 수준, 한계 명시).
- **AC-4 기존 회귀 없음**: 이 머신에서 호스트 크레이트 Linux `cargo test --locked` green 유지, `cargo fmt --all -- --check`(루트+호스트) clean. 워크스페이스 cargo 검증은 viewer-decoder의 NDK 링크 제약으로 원래 불가(docs/dev-environment.md).
- **AC-5 인수인계**: exe 경로·설치 방법·Windows 실기기 체크리스트(E6)·알려진 한계(오디오 없음, 가상 디스플레이 미지원, curtain no-op, UIPI 제약, 미서명)를 담은 보고. Windows 실기기 동작 확인 자체는 사용자 차례(BLOCKED 조건 명시).

## 태스크 (T ↔ 담당 직군 · 수정 경계)

- **T-1 로컬 Windows 교차 검증 환경** · `Platform/Tooling Engineer` · 도구 설치만. **완료**(rustup 타깃, cargo-xwin 0.23.1).
- **T-2 CI 형제 저장소 체크아웃** · `Platform/Tooling Engineer` · 수정 경계: `.github/workflows/ci.yml`(안내 주석도 ci.yml 내부에 배치 — 루트 `Cargo.toml`은 미수정, G2 감사 지적 반영). 워크스페이스 내 `unikit-bridge-sibling` 체크아웃(repository `loopy-lim/unikit-bridge`, ref `aea88a8f`) 후 링크 스텝. rust-fast/ts/android-compile/windows-host 4개 job.
- **T-3 Windows 교차 검증 실행** · `Platform/Tooling Engineer` · 읽기·빌드만. `RC=/usr/bin/llvm-rc cargo xwin check|clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests --locked`(clippy `-D warnings`). 발견 결함은 T-5로 이관.
- **T-4 exe 크로스 빌드** · `Platform/Tooling Engineer` · 빌드 산출만. `RC=/usr/bin/llvm-rc PATH=$HOME/.cargo/bin:$PATH bun run --cwd apps/host-desktop tauri build --target x86_64-pc-windows-msvc --runner cargo-xwin --bundles nsis --ci -- --locked`(실패 시 `cargo xwin build --release` portable 폴백).
- **T-5 Windows 코드 결함 수정(조건부)** · `Backend/Core Engineer` · T-3/G3에서 결함 발견 시 `windows_backend/`·공용 모듈 파일만. 수정 후 T-3/V 재실행.
- **T-6 Linux 회귀 검증** · `Verification/Test Engineer` 관점 · 읽기·검증만. 호스트 크레이트 `cargo test --locked`(Linux) + fmt(루트+호스트).
- **T-7 인수인계 보고** · `Product Manager` · 이 문서 + 완료 보고(대화). docs/EVIDENCE.md 갱신은 커밋 시점으로 보류.

## 검증 (V)

- V-1 (AC-1): 교차 check/clippy 종료 코드 0. **완료** — check EXIT=0, clippy CLIPPY=0(단, 1차 배경 파이프라인 클리피 결과는 tail 종속 코드를 cargo 종료 코드로 오기록했던 사실을 발견·정정하고 재실행으로 확보). panel.rs:747 `write_with_newline`은 main 8bbd08b 시점 선행 결함으로 T-5에서 수정.
- V-2 (AC-2): **완료** — 최종 산출 3종(`file`·sha256 검증, 아래 "최종 산출물" 참조).
- V-3 (AC-3): ci.yml yaml 파싱 + 스텝 구조 수검. **완료** — G3 재심사에서 ref 4곡 전체 SHA·경화 확인. 실CI 실행은 BLOCKED(push 권한 없음) 한계 기록.
- V-4 (AC-4): Linux 테스트·fmt 종료 코드 0. **완료** — 호스트 `cargo test --locked` TEST=0(413+13 등 전 스위트), 루트+호스트 fmt clean.
- V-5 (AC-1/AC-4 재검증): T-5 수정 발생 시 영향받은 검증 재실행. **완료**(위 V-1/V-4가 수정 이후 재실행 분).
- V-6 (AC-5): 완료 보고가 경로·설치·E6 체크리스트·한계(미서명 포함)·Linux 후속 계획을 포함하는지 검사. **완료** — 최종 보고(대화)에 5요소 전부 포함.

## 최종 산출물 (T-4, 2026-11-12)

`apps/host-desktop/src-tauri/target/x86_64-pc-windows-msvc/release/` 기준:

| 파일 | 형태 | 크기 | sha256 |
| --- | --- | --- | --- |
| `bundle/nsis/Leftcar Host_0.1.11_x64-setup.exe` | PE32 Nullsoft 인스톨러(GUI), 메인 바이너리=leftcar-host-desktop | 2.6MB | `155d5e0d…7fa56f` |
| `leftcar-host-desktop.exe` | PE32+ **GUI 서브시스템**, 프런트엔드 자산 삽입(`index-BttaOEk2.js`) | 8.1MB | `271d064e…86c958` |
| `leftcar-hostd.exe` | PE32+ console(사이드카, 의도된 값) | 3.0MB | `f1291bf3…61622c4` |

### tauri CLI 교차 번들 특이사항 (재현 레시피)

1. **`mainBinaryName` 필수**: `tauri.conf.json` 최상위에 `"mainBinaryName": "leftcar-host-desktop"` 없으면 CLI가 `src/bin/leftcar-hostd.rs`를 메인으로 골라 인스톨러에 hostd만 싣는다(추가함).
2. **CLI가 빌드한 메인 exe는 무효 후보였음**: 이 exe는 console 서브시스템 + 프런트엔드 자산 미삽입(devUrl 문자열만 존재). 최종 산출은 `cargo xwin build --release --target x86_64-pc-windows-msvc --bin leftcar-host-desktop --features tauri/custom-protocol --locked`로 직접 빌드해 GUI+자산 삽입을 확인한 뒤, 이미 생성된 `installer.nsi`(staging: `release/nsis/x64/`)에 makensis만 수동 재실행했다.
3. **makensis 확보 방법**: sudo 불가·wine 부재 환경이라 Debian `nsis_3.12-1_amd64.deb` + `nsis-common_3.12-1_all.deb`를 `~/tmp/nsis-pkg/ext`에 풀고(usr/bin/makensis + usr/share/nsis), `NSISDIR=<ext>/usr/share/nsis`를 export하는 `~/tmp/nsis-bin/makensis` 래퍼를 PATH에 두었다. tauri-bundler는 Linux에서 `Command::new("makensis")`(PATH 조회)를 실행하며 에러 메시지만 `makensis.exe`로 표기한다(소스 tauri-v2.11.4 확인). NSISDIR은 래퍼 내부에서 재설정하므로 bundler의 `env_remove("NSISDIR")`와 무관.
4. WebView2: 기본 downloadBootstrapper 모드 — 대상 PC에 Evergreen 런타임 없으면 설치 시 다운로드(인터넷 필요), 있으면 생략.

## 위험과 폴백

- cargo-xwin×tauri bundler 조합 실패 → portable exe 폴백(AC-2 차선), 인스톨러는 아침 CI 경로로.
- WebView2Loader 정적 링크 문제 → webview2-com 결합 확인 후 대처(문서화 우선). 교차 check에서 webview2-com 0.39.1 컴파일 확인.
- xwin SDK 다운로드 실패/지연 → 재시도, 캐시 경로 지정.
- Windows 런타임 동작은 오늘 밤 증명 불가(실기기 없음) — 산출물 성격을 internal cross-build 후보로 명시.
- 미서명 exe SmartScreen 경고(설치 시 "추가 정보 → 실행" 필요 가능).

## 후속 (이번 범위 밖, 사용자 아침 결정)

- Linux 호스트 "잘 되게" 개선은 이번 Windows 우선 범위 밖(사용자 지시). Linux 백엔드는 이 머신에서 구동·테스트 가능 상태 유지(T-6로 회귀만 확인).
- Mac 30fps 저하 원인 분리·개선은 별도 작업.
- **G3 P3 후속(비차단, 다음 Windows 작업 시)**: ① `GpuScaler` lazy 초기화 — 동일-크기 fast path 세션이 불필요하게 D3DCompile에 묶이지 않도록 첫 불일치 프레임에서 생성(Staff F-2 / Security F-3). ② `scale()`에서 rasterizer/blend/depth 기본 상태 명시 바인딩(Staff F-3). ③ 셰이더 사전 컴파일 임베드로 d3dcompiler_47 런타임 의존 제거 검토(Security 잔여위험 3).
- **릴리스 서명**: 미서명 internal 산출물. 배포 시 번들 windows sign_command 또는 인증서 도입 필요.

## 관문 기록

| 관문 | 평가 직군 | 판정 | 비고 |
| --- | --- | --- | --- |
| G0 요구 | independent `Requirements Auditor`(결합: Product & Plan Auditor) | **PASS** | P3 3건(F-3/5/6) 계획에 반영 완료 |
| G1 설계·계획 | independent `Architecture & Plan Reviewer`(결합 동일) | **PASS** | 1차 FAIL(F-1 P1 외 5건) → T-2 재설계·반영 후 재심사 PASS |
| G2 구현 | independent `Task QA Validator` | **PASS** | AC-1/AC-2/AC-4 전부 — clippy CLIPPY=0·테스트 TEST=0·fmt 0/0 독립 재실행, 산출물 해시·서브시스템·installer.nsi·타임라인 대조 일치. 사소 기록 정정 1건(T-2 경계) 반영 |
| G3 코드 | independent `Staff Code Reviewer` + `Security & Reliability Reviewer` | **PASS** | 1차 Staff FAIL(ci.yml 단축 SHA 4곳 checkout 불가, P1 fetch 실증) → 전체 40자 SHA화+persist-credentials 4곡+`permissions: contents: read` 적용 → 재심사 PASS(경화 2종 구조 무해 확인). Security PASS(Medium 1·Low 1 CI 경화 처방 반영 완료). P3 후속 2건(lazy 스케일러 초기화, 파이프라인 상태 명시)·주석 정정(PR #112 출처) 반영 |
| G4 사용성 | N/A | — | UI 변경 없음(exe/CI/빌드 경로 변경) |
| G5 완료 | independent `Release Gatekeeper` | **PASS** | AC-1~AC-5 전부 달성 — 산출물 해시 3종·형제 저장소 상태·ci.yml ref 4곳·커밋 경계(HEAD 8bbd08b 유지·트리 보존) 실측 대조 일치. BLOCKED 항목(실기기·실CI)은 완료로 표시되지 않고 한계로 기록됨 확인. 완료 보고 발행 승인 |

## 구현 기록 (T-3/T-5/T-6 요약)

- F-1(P1): `capture.rs` 콘텐츠 크기 ≠ 협상 크기인즉 HW MFT가 E_FAIL로 죽음 → `GpuScaler`(풀스크린 트라이앵글 HLSL blit, D3D11 RTV/SRV, CPU 리드백 없음 — 설계 정책 준수) 추가, 동일 크기 fast-path 유지.
- F-2(P1): `input.rs` X버튼(BACK/FORWARD) release 경로가 per-button flag를 요구해 abort → `send_button` 공용 경로(XDOWN/XUP + mouseData id)로 통합, `release_all` 동일 경로 사용.
- F-3(P2): `capture.rs` WGC `item.Closed` 미구독 → 모니터 제거/캡처 철회 시 무한 대기 → `TypedEventHandler` 구독 + 루프 상단 AtomicBool 검사. (프레임 타임아웃은 정적 화면 오탐 때문에 기각.)
- F-4(P2): `leftcar-hostd.rs` DPI 인지 부재 → PER_MONITOR_AWARE_V2 설정(best effort).
- F-6(P2): `capture.rs` 노란 테두리 → `SetIsBorderRequired(false)`(Win11+).
- panel.rs:747 선행 clippy 결함 `write_with_newline` 수정.
- F-7(fmt 감사): 8bbd08b가 포맷 안 된 채 커밋한 33개 .rs `cargo fmt`(rustfmt 출력과 byte 동일 검증), `gen/schemas/windows-schema.json` 재생성 반영.
- T-6: Linux 호스트 테스트 전수 green, fmt clean.
