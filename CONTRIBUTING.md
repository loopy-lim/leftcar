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

## PR 규칙

- 최소 변경 단위로 구현하고, 각 변경은 plan task와 연동해 요약
- 아키텍처 위반(입력 주입, Kotlin shim policy 침범)은 PR 전에 `bun run test:architecture`를 통과해야 함
- 보안·증거가 바뀌면 `docs/EVIDENCE.md`에 상태 반영
- 라이선스/저작권 header 변경이 있을 경우 `LICENSE` 및 매니페스트의 SPDX 정책 일치 확인
