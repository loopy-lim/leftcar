# 성능·안전성·외부망 전면 개선

요청: 일부 패킷 수정에 한정하지 않고 유사 프로젝트와 비교해 부족한 점을 찾고, 현재 환경에서 구현·검증 가능한 개선을 수행한다. 기준 소스 `cc50430`와 이전 턴의 미커밋 전송 개선을 함께 검토한다.

## 수락 조건

- AC1: Sunshine/Moonlight, RustDesk, scrcpy, Deskflow, Tailscale 공식 자료와 현재 소스에 근거한 비교. 없는 기능, 플랫폼 격차, 검증 부재를 구분한다.
- AC2: 원격 입력으로 유발되는 메모리·연결 자원 고갈과 잘못된 암호 입력을 제한하고 정상 페어링·철회·재연결을 보존한다.
- AC3: 전송·FEC·디코더의 확인된 불필요한 작업과 프레임 손실 조건을 수정한다. 실제 FPS 개선과 단위/벤치 증거를 구분한다.
- AC4: 실패한 오래된 연결 시도가 새 사용자 선택을 덮어쓰지 않는다. 잘못된 응답은 Viewer를 중단시키지 않고 연결 오류로 끝난다.
- AC5: 관련 Rust/TS/Swift 검사, 변경된 React 동작의 React Doctor 100 및 후속 타입·회귀 검사. 미지원 실기·서명·배포 조건을 명시한다.

## 실행 태스크

| 작업 | 범위 | 검증 | 상태 |
|---|---|---|---|
| T1 비교 및 전체 경계 조사 | `docs/research/2026-10-13-project-comparison.md` | 공식 자료와 현재 호출 경로 대조 | 완료, 제품 격차는 비교 문서에 유지 |
| T2 Host 연결 자원·키 교환 | `control.rs`, `secure-channel`, UPnP, 파일 공유 | IP별 고갈, 인증 전 수명, 잘못된 X25519, 정상 인증, 포트 소유·취소·파일 교체 | 구현 및 독립 리뷰 완료 |
| T3 FEC/디코더 | `fec-core`, `viewer-decoder`, macOS 혼잡 상태 | 3,060개 손실 조합, CPU 벤치, 프레임 유지, NDK 자원 해제 | 로컬 검사 통과, 기기 FPS 미측정 |
| T4 Viewer 제어·재연결 | `control.ts`, `secure-channel.ts`, `session.ts` | 무개행·비정상 응답·연결 종료·선택 경합·FIFO 요청/바이트/와이어 예산 | 완료 |
| T5 이전 전송 수정 재검토 | Windows 송신 크기/예산, Android 재조립, Windows 입력/피드백 | 암호화/FEC MTU, 정상 조각 복원, 분리된 제어 포트·공유 nonce | 이식 가능한 검사 통과, Windows 빌드/실행 미검증 |
| T6 검증 자동화·지원 문서 정합성 | verify/CI, 경로 진단, 벤치 도구, 의존성, 지원 문서 | 순수 정책 실행, 실제 지원과 문서 대조, audit | 로컬 검사와 잔여 경고 아래 기록 |
| T7 Rustra 배포판 동반 업데이트 | Rust core/CLI/types/RN, 생성 코드, Android 빌드 | registry/체크섬, schema 일치, owned FFI, C++ 링크, 실제 APK | 아래 추가 검증 기록 |

검증 결과와 남은 차이는 비교 기록에 통합한다. 새 공개 릴레이·계정 서버 운영, 실사용 장치 변경, 공개 릴리스는 로컬 코드 검증으로 완료 처리하지 않는다.

## 최종 로컬 검증

2026-10-13, macOS arm64, Rust 1.95.0, Bun 1.4.3. 저장소의 Bun 핀은 1.4.1이며 원격 CI 실행 결과는 아니다.

| 검사 | 결과 |
|---|---|
| `npx -y react-doctor@latest . --verbose` → `bun run typecheck` → `bunx vitest run --maxWorkers=4` | **100/100**, 전체 타입 검사, 82개 파일 **1,072개 테스트 통과** |
| `bun run test:contract` | 4개 통과 |
| `bun run test:ui` | 16개 suite 통과. 새 의존성 설치 배치에서도 동작하도록 fixture 빌드의 하드코딩된 Bun 내부 경로 제거 |
| `cargo test --workspace --locked` | 전체 통과. Android viewer 292, decoder 34, FEC 12 포함. 이후 replay-window 변경은 secure-channel 16개 별도 통과(벡터 출력용 기존 2개 ignored) |
| Host 독립 Rust workspace 전체 검사 | 단위 398개 + 제어 통합 13개 통과 |
| Swift 전체 shim 연결 정책 시험 | UDP 크기 60개, 혼잡 기억, 혼잡 감속, 프레임 예산 72개, pacing clock 통과. 화면 캡처 없음 |
| `cargo fmt` / Clippy `-D warnings` | root/Host 모두 통과. 기존 domain re-export 순서도 formatter에 맞게 정리 |
| Host UI production build | 타입 검사·Vite 빌드 통과. 네이티브 패키지 설치 증거는 아님 |
| TS/Kotlin architecture | **실패 32건**, 변경 전 검사에서도 동일. KM Kotlin 네트워크·암호 구현과 Rust 소유 규칙의 불일치. 허용목록 확대나 검사 제외로 숨기지 않음 |
| `bun audit --json` | 12개 advisory → **6개**, critical `shell-quote` 포함 호환 패치 반영. 경고 0 상태 아님 |
| Cargo audit 전체 잠금 파일 | root: 취약점 0·경고 0. Host: 취약점 분류 0, **unmaintained 6·unsound 1** 경고 |

UI 시험의 첫 실행은 Chromium 설치 부재, 다음 실행은 샌드박스의 macOS 프로세스 통신 제한으로 중단됐다. 임시 폴더에 격리한 테스트 Chromium을 허용된 실행 환경에서 사용한 최종 검사는 통과했다. 초기 무제한 Vitest worker 실행은 큰 파일 비교 1개가 5초 timeout을 넘었으며, worker 4개로 제한한 전체 재실행이 통과했다. 테스트 상한·검사 규칙을 완화하지 않았다.

### Rustra 추가 업데이트

추가 요청에 따라 공식 registry의 최신 stable 조합을 적용했다. Rust core/macros/naming은 Git 0.10.2에서 crates.io **0.12.0**, RN 어댑터는 **0.9.0 → 0.10.0**, types는 **0.10.0 → 0.12.1**, CLI는 기존 생성 버전 0.10.0에서 **0.12.0 고정 설치**로 전환했다. 배포 출처와 재생성 방법은 [Rustra 계약 문서](../04-rustra-control-contracts.md)에 기록했다.

- 두 Cargo 잠금 파일과 Bun 잠금 파일을 갱신하고 TS/C++/Kotlin 모듈을 공식 CLI로 재생성했다. 기존 계약 schema/hash와 네트워크 프로토콜은 그대로다.
- 새 owned-response FFI의 호출자 버퍼/할당 응답 두 경로를 실제 Rust 명령 호출로 검사했다. JS만 바꾸는 업데이트가 아니다.
- 생성 파일은 그대로 두고, 앱 소유 Gradle에서 Bun hoisting 경로를 해결한다. 공유 Cargo.lock 변경이 이전 staticlib 캐시에 가려지지 않도록 Cargo 자체 의존성 검사를 매 빌드 실행한다.
- 사용하지 않는 과거 `libleftcar_rustra.so`가 수동 빌드 폴더에 남아 있어도 APK에 들어가지 않게 했다. 현재 로드하는 `librustra_bridge.so`에는 새 Rust core를 정적으로 링크한다.
- CI에 생성 코드 검사와 APK 네이티브 링크 검사를 연결했다. 로컬 증거이며 원격 CI 결과는 아니다.

| 업데이트 후 검사 | 결과 |
|---|---|
| React Doctor → 전체 타입 검사 → TS 테스트 | **100/100**, **82개 파일·1,072개 통과** |
| 계약·생성 코드·Rustra doctor | 계약 4개, `rustra:check`, 필수 doctor 항목 통과. 기기 실행 증거를 대신하지 않음 |
| Rust root/Host | 전체 테스트 및 Clippy 통과. 새 owned FFI 포함 native entry 3개, Host 단위 398개·통합 13개 |
| UI 회귀 | **16개 suite 통과** |
| Android `:app:assembleDebug :app:testDebugUnitTest` | **빌드 성공, JVM 43개 suite·252개 통과**. arm64/x86_64 브리지 링크 성공, 앱 APK는 기존 arm64 구성 |
| 증분 빌드 | 후속 빌드에서도 `buildRustAndroid` 실행, Cargo x86_64 0.06초·arm64 0.05초에 변경 없음 확인. 전체 후속 빌드 18초 |
| APK 내부 검사 | `librustra_bridge.so`의 `rustra_ffi_invoke_frame_owned`·`rustra_ffi_free_owned_bytes` 정의 확인, 구형 `libleftcar_rustra.so` 부재 확인 |
| 의존성/구조 재검사 | JS advisory 6개, Host unmaintained 6·unsound 1, 기존 architecture 실패 32개 유지. 이번 업데이트로 경고를 숨기지 않음 |

로컬 결과물: `apps/viewer-expo/android/app/build/outputs/apk/debug/app-debug.apk`, 55,237,838 bytes, SHA256 `a66907e92a575aace7d664499d876c6066e80e0d3ab4a92688946009f9455786`. Metro 연결이 필요한 개발용 debug APK이며 서명된 배포판으로 표시하지 않는다. 설치하지 않았다.

첫 Android 시도는 생성 템플릿의 app-local 어댑터 경로로 실패했고, 앱 소유 Gradle에서 실제 설치 경로를 해결했다. 이후 arm64만 빌드하는 임시 환경값과 생성 모듈의 두 ABI 설정이 달라 오래된 x86_64 정적 라이브러리가 노출됐으며, 최종 검사는 생성 모듈 기본값대로 두 ABI를 모두 재빌드해 통과했다. 생성 파일을 수동 수정하거나 이전 바이너리로 검사를 우회하지 않았다.

영상 경로는 Rustra를 통과하지 않는다. 이 업데이트를 영상 FPS 향상으로 표현하지 않으며, 실제 Android 실행·장시간 스트리밍·iOS 네이티브 실행은 별도 미검증이다.

### 성능 재현

`bun tools/perf-matrix/fec-benchmark.mjs`는 기준 `cc504306dee40b6e83570f36aec3af23d445e686`과 작업 트리의 FEC를 같은 최적화 바이너리에 링크한다. 8 × 1,124바이트 payload, 회당 20,000회, 실행 순서를 교차하며 각 3회 측정한다. 임시 파일만 사용하며 기기·캡처·네트워크를 건드리지 않는다.

| 손실 data shard | 기존 | 개선 후 |
|---|---|---|
| 1개 | 62.8–63.3µs | 2.19–2.27µs |
| 2개 | 60.8µs | 8.90–8.99µs |
| 4개 | 61.3–61.7µs | 16.0–18.4µs |

CPU microbenchmark이며 실제 Android FPS/입력 지연·경쟁 제품 대비 우위의 증거가 아니다. 무손실 경로도 행렬 복원을 생략하지만 이 측정의 첫 baseline 샘플은 동시 빌드 영향을 받았으므로 대표 성능치로 쓰지 않는다. 추가로 암호 채널 replay-window의 순방향 패킷마다 발생하던 1KiB 임시 할당을 제자리 비트 이동으로 제거했고, 64비트 word 및 8,192패킷 window 경계를 기준 이력 모델과 대조했다.

### 남은 의존성 및 구조 문제

- JS: `braces@3.0.3`, `decode-uri-component@0.2.2`, `image-size@1.2.1`(2건), `node-forge@1.4.0`, `uuid@7.0.3`. 조회 당시 braces/node-forge의 수정 배포판이 없고, 나머지 수정판은 상위 Metro/query-string/xcode가 요구하는 major 범위를 벗어난다. 무조건 override하지 않았으며 실제 도달성은 별도 조사다.
- Host Rust: `glib@0.18.5`의 `RUSTSEC-2024-0429` unsound 경고 및 proc-macro-error/UNIC 계열 6개 유지보수 중단. 전체 타깃 lock 검사이며 macOS 앱에서 해당 함수에 도달한다는 결론은 아니다. 상위 Tauri/GTK 의존성 전환과 해당 플랫폼 검증이 필요하다.
- 구조: KM의 Kotlin 소켓·암호 정책을 Rust 소유 경계로 옮기는 작업은 완료하지 않았다. 기존 32개 architecture 실패를 이번 보안 수정의 통과 증거로 바꾸지 않는다.
- 제품: Windows 동적 bitrate/NACK·오디오·커서 동등성, 앱 내 경로 진단, 별도 bulk 전송, 자체 릴레이와 지속 fuzz는 남는다.
- 수용: Windows 타깃 빌드·실행, 실제 공유기/WAN/기기, 새 Host/APK 설치, 30분 장시간·동일 조건 타 제품 비교는 미실행. 기존 설치·장치·네트워크 설정을 변경하지 않았다.

## 커밋 구성 — 사용자 승인 반영

2026-10-13 사용자가 아래 네 커밋 생성을 승인했다. 검증을 마친 변경을 아래 구성으로 커밋한다. push/PR/배포는 이번 승인 범위에 포함하지 않는다. 파일은 저장소 상대 경로이며 모든 변경 67개를 한 번씩 배정한다.

### 1. `fix(native): 외부 연결 안전성과 미디어 복구 성능 개선`

- `apps/host-desktop/src-tauri/src/control.rs`
- `apps/host-desktop/src-tauri/src/file_transfer.rs`
- `apps/host-desktop/src-tauri/src/lib.rs`
- `apps/host-desktop/src-tauri/src/media_feedback.rs`
- `apps/host-desktop/src-tauri/src/media_pacing.rs`
- `apps/host-desktop/src-tauri/src/media_sender.rs`
- `apps/host-desktop/src-tauri/src/upnp.rs`
- `apps/host-desktop/src-tauri/src/windows_backend/capture.rs`
- `apps/host-desktop/src-tauri/src/windows_backend/mod.rs`
- `apps/host-desktop/src-tauri/src/wire.rs`
- `crates/domain/src/lib.rs`
- `crates/fec-core/src/lib.rs`
- `crates/secure-channel/src/lib.rs`
- `crates/viewer-decoder/src/android/boundary_tests.rs`
- `crates/viewer-decoder/src/android/decoder.rs`
- `crates/viewer-decoder/src/android/ffi.rs`
- `native/android-viewer/src/media_datagram.rs`
- `native/macos-capture-shim/Sources/Encoder/PeerCongestionMemory.swift`
- `native/macos-capture-shim/Tests/PeerCongestionMemoryTests.swift`
- `native/macos-capture-shim/Tests/UdpPacketSizeTests.swift`

### 2. `fix(viewer): 제어 요청 자원 제한과 재연결 경합 수정`

- `apps/viewer-expo/src/control-error.ts`
- `apps/viewer-expo/src/control.test.ts`
- `apps/viewer-expo/src/control.ts`
- `apps/viewer-expo/src/secure-channel.test.ts`
- `apps/viewer-expo/src/secure-channel.ts`
- `apps/viewer-expo/src/session.test.ts`
- `apps/viewer-expo/src/session.ts`

### 3. `fix(deps): Rustra 업데이트와 보안 수정 버전 적용`

- `Cargo.lock`
- `Cargo.toml`
- `apps/host-desktop/src-tauri/Cargo.lock`
- `apps/viewer-expo/android/app/build.gradle`
- `apps/viewer-expo/android/build.gradle`
- `apps/viewer-expo/generated/.rustra-generated.json`
- `apps/viewer-expo/generated/commands.ts`
- `apps/viewer-expo/generated/react-native.ts`
- `apps/viewer-expo/modules/rustra-bridge/RustraBridge.podspec`
- `apps/viewer-expo/modules/rustra-bridge/android/CMakeLists.txt`
- `apps/viewer-expo/modules/rustra-bridge/android/build.gradle`
- `apps/viewer-expo/modules/rustra-bridge/android/src/main/java/dev/rustra/bridge/RustraBridgeModule.kt`
- `apps/viewer-expo/modules/rustra-bridge/generated/rustra-generated-codecs.cpp`
- `apps/viewer-expo/modules/rustra-bridge/generated/rustra-generated-codecs.hpp`
- `apps/viewer-expo/modules/rustra-bridge/package.json`
- `apps/viewer-expo/modules/rustra-bridge/scripts/prepare-native.cjs`
- `apps/viewer-expo/modules/rustra-bridge/src/index.ts`
- `apps/viewer-expo/package.json`
- `apps/viewer-expo/src/rustra.ts`
- `bun.lock`
- `crates/control-contract/Cargo.toml`
- `docs/04-rustra-control-contracts.md`
- `native/leftcar-rustra/Cargo.toml`
- `native/leftcar-rustra/src/lib.rs`
- `package.json`
- `packages/control-generated/host/commands.ts`
- `packages/control-generated/viewer/commands.ts`

### 4. `test(tooling): 네트워크 진단과 성능 회귀 검증 보강`

- `.github/workflows/ci.yml`
- `README.md`
- `docs/07-security-privacy.md`
- `docs/completion-and-support.md`
- `docs/plans/2026-10-13-performance-security.md`
- `docs/research/2026-10-13-project-comparison.md`
- `tools/network-diagnostics.mjs`
- `tools/network-diagnostics.test.mjs`
- `tools/perf-matrix/fec-benchmark.mjs`
- `tools/perf-matrix/fec-benchmark.rs`
- `tools/ui-regression/build.mjs`
- `tools/verify.mjs`
- `tools/verify.platform.test.mjs`
