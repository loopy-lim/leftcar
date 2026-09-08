# Contributing

## 개발 환경

- Rust: stable toolchain
- Bun (>= 1.3)
- Android Studio/SDK + NDK (`ANDROID_HOME` 설정)
- Rusttauri/host 빌드가 필요하면 Tauri CLI (`cargo install tauri-cli`)

## 로컬 셋업

```bash
bun install
```

## 기본 검증 명령

```bash
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
bun run typecheck
bun run test
bun run test:contract
bun run test:architecture
```

- 빌드 산출이 필요할 때: `cargo check --workspace`, `cargo run -p control-contract --bin generate`
- `docs/` 변경 시: `docs/README.md`의 상태 표/근거 수준과 동기화

## Rustra 코드젠

계약(Rust `control-contract`)을 고치면 두 단계로 재생성한다:

```bash
bun run rustra:generate                                   # packages/control-generated (+ 스키마)
cd apps/viewer-expo && bunx --package @rustra/cli@0.8.0 rustra codegen --config rustra.json
```

- CLI 버전은 Rust crate 핀(docs/10-references.md)과 같은 라인으로 맞춘다.
- CLI 실행 직후 `apps/viewer-expo/package.json`에 `workspaces: ["modules/rustra-bridge"]`가 다시 생기면 제거한다 — 루트 workspace가 `apps/*/modules/*`를 이미 커버하며, 이 키가 남으면 react-doctor가 viewer-expo를 모노레포 루트로 오판한다.
- 브리지 실기기 증명(E9): metro 띄우고(`bunx expo start --dev-client --port 8082` + `adb reverse tcp:8081 tcp:8082`) 앱 설치·실행 뒤 딥링크 `leftcar://rustra-proof`로 진입하면 `addNumbers(20,22)=42`와 계약 해시가 화면에 렌더링된다.

## PR 규칙

- 최소 변경 단위로 구현하고, 각 변경은 plan task와 연동해 요약
- 아키텍처 위반(입력 주입, Kotlin shim policy 침범)은 PR 전에 `bun run test:architecture`를 통과해야 함
- 보안·증거가 바뀌면 `docs/EVIDENCE.md`에 상태 반영
- 라이선스/저작권 header 변경이 있을 경우 `LICENSE` 및 매니페스트의 SPDX 정책 일치 확인
