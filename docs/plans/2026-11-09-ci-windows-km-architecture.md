# CI 차단 해제: Windows virtual_display cfg + KM architecture 32

## 목표와 범위

- **결과:** `main`과 동일한 두 CI 차단을 실제 수정으로 해소한다.
  1. Windows Host `virtual_display` 컴파일/테스트
  2. `bun run test:architecture` KM Kotlin 위반 32건 → 0건
- **제외:** allowlist 확대·ignore·검사 제외로 통과 위장. PR #8 React Doctor 워크플로 자체는 범위 밖(이미 별도 PR).
- **보존:** 로컬 미커밋 `rust-toolchain.toml`(stable/minimal) 변경은 손대지 않는다.
- **기준 리비전:** `origin/main` @ `49240b6` (+ 본 작업 브랜치).
- **편성:** 백엔드·코어·인프라(③). 위임: Cursor native Task / 동일 제공자. UI `G4` N/A.

## 수락 기준

| ID | 관찰할 결과 | 담당 | 검증 |
|---|---|---|---|
| AC-1 | Windows Host CI와 동일하게 `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked`가 `call_create`/`call_destroy`/`set_test_find_stale` E0599 없이 컴파일·실행된다(로컬은 macOS에서 virtual_display 회귀 + 가능하면 Windows 크로스 `--tests` 보조). macOS zombie 테스트 유지. | T-1 | V-1, V-2 |
| AC-2 | `bun run test:architecture`가 `architecture-check: TS/Kotlin rules clean`으로 종료 0. allowlist/ignore로 숨기지 않음. 32건 전부 해소(A≈17 network/crypto/tests + B2 + C1 + D4 + E7 + F1). | T-2…T-5 | V-3 |
| AC-3 | Expo Android JVM unit test가 KM 관련 회귀 없이 통과. | T-2…T-5 | V-4 |
| AC-4 | Host↔viewer KM 암호/프레임 계약은 `secure_channel`/`control_contract::km` 소유를 유지(Kotlin `javax.crypto`/`Socket` 재구현 제거). | T-2 | V-5 |
| AC-5 | 이 작업은 stashed/로컬 `rust-toolchain.toml`을 수정·커밋하지 않는다(`git status`로 확인). | 전 태스크 | V-6 |

## 설계

### Track A — Windows `virtual_display`

- `create_inner`/`remove_inner`가 macOS 전용 `call_create`/`call_destroy`를 호출해 Windows에서 E0599.
- **수정:** `#[cfg(not(target_os = "macos"))]` 스텁. `call_destroy`는 `cfg(test)`의 `test_destroy` 훅을 유지(Windows `control.rs` 테스트 의존).
- zombie 복구 테스트 2개는 `#[cfg(target_os = "macos")]`.
- 프로덕션 Windows는 기존처럼 probe `supported: false`.

### Track B — KM architecture

위반 32 = import-allowlist 29 + kotlin-no-policy(network) 3. 전부 `apps/viewer-expo/android/.../km/`.

| Slice | 내용 | 대략 해소 |
|---|---|---|
| A | Android `android-viewer`에 KM TCP+`StreamSealer`/`km_keys` JNI. Kotlin `KmDuplex`/`KmCipher` 소켓·crypto 제거. 테스트의 `ServerSocket` 제거. | ~16 |
| B | `JSONObject` → 수동/바이트 경계(`KmEnvelope` 또는 기존 패턴) | 2 |
| C | `KmKeyBridgeLease` → 허용된 `stream/KeyBridgeInputAdapter` 경로 재사용 | 1 |
| D | Shizuku/`HiddenApiBypass` → `dev.leftcar.viewer.*` 어댑터. **금지 import 라인 0**(`Class.forName`/리플렉션만). `tools/architecture-check/ts.ts`의 `KOTLIN_ALLOW`·예외 목록은 수정하지 않음 | 4 |
| E | uinput/HID의 `java.nio`/`java.io.File*`/`java.math` → `android.*`/`Os`/byte[] 또는 Rust 패킹 | 7 |
| F | 테스트 `java.time.Duration` 등 잔여 | 1 |

Slice A 건수 ≈17(network/crypto/socket tests). A+B+C+D+E+F = 32.

Host 참조: `apps/host-desktop/src-tauri/src/km.rs`, `crates/secure-channel`, `crates/control-contract/src/km.rs`. Android에는 현재 KM duplex JNI 없음.

## 태스크

| ID | 목적·AC | 담당 | 수정 경계 | 선행 | 검증 | 상태 |
|---|---|---|---|---|---|---|
| T-1 | Windows cfg 스텁 + macOS 테스트 게이트 | `Backend/Core Engineer` | `apps/host-desktop/src-tauri/src/virtual_display.rs`만 | 없음 | V-1,V-2 | done |
| T-2 | KM duplex/crypto → Rust JNI; Kotlin production thin | `Backend/Core Engineer` | `native/android-viewer/**`, `.../km/KmDuplex.kt`, `KmCipher.kt`, 관련 JNI 노출(테스트 파일 제외) | 없음(파일∅ T-1) | V-3,V-4,V-5 | done |
| T-3 | Wire JSON + KeyBridge lease 경로 | `Backend/Core Engineer` | `KmWire.kt`, `KmRuntime.kt`(JSON), `KmKeyBridgeLease.kt`, 필요 시 `stream/KeyBridgeInputAdapter.kt` | T-2 계약 확정 후 | V-3,V-4 | done |
| T-4 | Shizuku/HiddenApi/uinput/HID import 위생 | `Platform/Tooling Engineer` | `KmModule.kt`, `KmRuntime.kt`(Shizuku만; JSON은 T-3), `KmOwnObservation.kt`, `KmInputAuthority.kt`, `KmHidReports.kt`, 신규 얇은 어댑터(`dev.leftcar.viewer.*`, 금지 import 0) | `KmRuntime` 공유 → T-3 뒤 | V-3,V-4 | done |
| T-5 | 모든 `km/*Test.kt` 정리(ServerSocket/`java.time` 등) + architecture 0 | `Verification/Test Engineer` | `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/km/**`만 | T-2…T-4 | V-3,V-4 | done |

## 검증 계획

| ID | 방법 | cwd | 기대 | 판정 |
|---|---|---|---|---|
| V-1 | CI 동일: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked` (`.github/workflows/ci.yml` windows-host). 보조(선택): docs의 `cargo check … --target x86_64-pc-windows-msvc --tests`(+RC) 또는 가용 시 gnu | repo root / Windows runner | 종료 0, E0599 없음 | local: macOS V-2 + stubs; Windows runner deferred to CI |
| V-2 | `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked virtual_display` | repo root (macOS) | virtual_display 테스트 통과(zombie 포함) | pass (23) |
| V-3 | `bun run test:architecture` (`package.json`) | repo root | `TS/Kotlin rules clean`, exit 0 | pass |
| V-4 | `./gradlew :app:testDebugUnitTest --no-daemon` (`ci.yml` android-compile cwd) | `apps/viewer-expo/android` | 종료 0 | pass |
| V-5 | `cargo test -p secure-channel --locked` | repo root | KM/seal 관련 테스트 통과 | pass (16) |
| V-6 | `git status -- rust-toolchain.toml` + stash 목록 | repo root | 작업 트리/커밋에 toolchain 변경 없음 | pass (`stash@{0}: wip rust-toolchain`) |

## 평가와 인계

- G0/G1: 본 문서. 조사 근거 = Windows/KM 병렬 research + `tools/architecture-check/ts.ts`, CONTRIBUTING 32건 정책.
- G2: T-1…T-5 implemented on `fix/ci-windows-vdisp-km-architecture` (uncommitted).
- G3 round 1 FAIL → fixed: JNI handle Arc registry (UAF), `KM_ERR_INTERNAL=5` ≠ timeout; KmShizuku test hooks.
- G3 re-review (independent Staff + Security): **PASS** (잔여 P2 테스트 보강 옵션, P3 `exception_pending`→send `3` 위생).
- G4: N/A (non-UI).
- G5 (independent Release Gatekeeper): **PASS** — AC-1…AC-5 현행 스냅샷 증거 충족. V-1 Windows runner는 PR CI에서 최종 확인.
- 다음: 사용자 요청 시 commit → PR → Windows Host CI로 V-1 closeout. `rust-toolchain.toml`/stash 손대지 않음.
