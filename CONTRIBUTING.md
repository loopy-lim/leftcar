# Contributing

## 개발 환경

- Rust **1.95.0**: 저장소의 `rust-toolchain.toml` 사용, rustfmt·clippy 포함
- Bun **1.4.1**: 설치와 JS/TS 스크립트의 기본 진입점
- Node **22.21.1**: CI 기준. Expo/React Native와 Gradle 플러그인의 요구 버전은 `bun run doctor`로 확인
- Android Studio의 JDK(`JAVA_HOME`), Android SDK(`ANDROID_HOME` 또는 `ANDROID_SDK_ROOT`), NDK **27.1.12297006**, Rust 대상 `aarch64-linux-android`
- macOS Host: Xcode 명령줄 도구의 Swift와 저장소에 선언된 Tauri CLI

## 로컬 셋업

```bash
bun install --frozen-lockfile
bun run doctor js
bun run setup:browser           # 최초 실제 브라우저 검사 준비
```

## 기본 검증 명령

```bash
bun run doctor                 # 전체 개발 환경 확인
bun run verify                 # JS + Rust workspace + 별도 Host + Android JVM
bun run verify js              # JS/React/타입/계약/구조/실제 화면 검사
bun run verify rust            # Rust와 해당 플랫폼의 네이티브 컴파일·정책 검사
```

전체 검증은 설치·기기 실행·화면 전송을 수행하지 않는다. macOS 전체 캡처 어댑터는 컴파일하고, 실제로 실행하는 Swift 검사는 합성 데이터의 순수 정책 경계다. React/React Native/스타일 변경은 루트의 `npx -y react-doctor@latest . --verbose`에서 **100 / 100**을 받아야 하며 이후 타입 검사와 관련 테스트를 다시 통과해야 한다. 경고를 숨기는 ignore나 점수 덮어쓰기는 사용하지 않는다.

개별 검사:

```bash
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
bun run typecheck                # 루트 tsc -b + viewer-expo/host-desktop 앱 typecheck 포함
bun run --cwd apps/viewer-expo typecheck  # Expo app/ 화면 포함(루트 typecheck에도 포함됨)
bun run test
bun run test:contract
bun run test:architecture
cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked  # Host crate(루트 워크스페이스 밖)
```

- 빌드 산출이 필요할 때: `cargo check --workspace`, `cargo run -p control-contract --bin generate`
- `docs/` 변경 시: `docs/README.md`의 상태 표/근거 수준과 동기화

## 내부 빌드와 산출물 확인

```bash
bun run build android-internal
bun run build host-macos-internal
bun run release:manifest -- verify /absolute/path/build-manifest.json
```

내부 Android 빌드는 기존 `apps/viewer-expo/android/app/debug.keystore`를 사용한다. 다른 위치의 기존 키를 사용할 때는 `LEFTCAR_INTERNAL_DEBUG_KEYSTORE`에 절대 경로를 지정한다. 키가 없으면 빌드를 시작하기 전에 실패하며 자동 생성하거나 복사하지 않는다. 공개 배포용 키는 `android-release`의 별도 설정이다. macOS 내부 빌드는 로컬 서명과 공증 여부를 manifest에 구분해 기록하며 공개 배포를 수행하지 않는다.

빌드는 저장소 밖에 manifest와 실제 산출물을 보관한다. 소스 해시와 패키지 안의 네이티브 일치를 확인해도 설치·OS 권한·실기기 성능까지 확인된 것은 아니다. 비교 시험은 별도 `LEFTCAR_BENCHMARK_PROFILE`과 전용 절대 `LEFTCAR_BENCHMARK_ROOT`를 사용한다. 일반 설치본의 앱 데이터나 자격 저장소를 시험 상태로 재사용하지 않는다.

## 실제 Host와 모델 테스트의 범위

실제 세션 승인·취소·정리 정책은 `apps/host-desktop/src-tauri`의 ControlServer/PairingServer와 네이티브 캡처 경계에 연결돼 있다. `crates/host-core`의 FakeCapture/FakeEncoder 경로는 모델 테스트용이다. 이 crate의 테스트 결과를 실제 ScreenCaptureKit·Windows 캡처 또는 기기 전송 증거로 취급하지 않는다.

## Rustra 코드젠

계약(Rust `control-contract`)을 고치면 두 단계로 재생성한다:

```bash
bun run rustra:generate                                   # packages/control-generated (+ 스키마)
cd apps/viewer-expo && bunx --package @rustra/cli@0.9.0 rustra codegen --config rustra.json
```

- CLI 버전은 Rust crate 핀(docs/10-references.md)과 같은 라인으로 맞춘다.
- CLI 실행 직후 `apps/viewer-expo/package.json`에 `workspaces: ["modules/rustra-bridge"]`가 다시 생기면 제거한다 — 루트 workspace가 `apps/*/modules/*`를 이미 커버하며, 이 키가 남으면 react-doctor가 viewer-expo를 모노레포 루트로 오판한다.
- 브리지 실기기 증명(E9): metro 띄우고(`bunx expo start --dev-client --port 8082` + `adb reverse tcp:8081 tcp:8082`) 앱 설치·실행 뒤 딥링크 `leftcar://rustra-proof`로 진입하면 `addNumbers(20,22)=42`와 계약 해시가 화면에 렌더링된다.

## PR 규칙

- 최소 변경 단위로 구현하고, 각 변경은 plan task와 연동해 요약
- 아키텍처 위반(입력 주입, Kotlin shim policy 침범)은 PR 전에 `bun run test:architecture`를 통과해야 함
- 보안·증거가 바뀌면 `docs/EVIDENCE.md`에 상태 반영
- 라이선스/저작권 header 변경이 있을 경우 `LICENSE` 및 매니페스트의 SPDX 정책 일치 확인
