
## 기존 베이스라인 결함 해소(Linux 최초 전체 녹색) — 2026-11-03

NVENC 작업(위 섹션) 이후 남았던 세 가지 기존 상태를 같은 세션에서 해소했다. 등급: E3(구현·단위) + E4(체커·테스트 실행).

- **km 테스트 리눅스 실패(km::tests::valid_peer_placement_can_confirm_after_six_hundred_milliseconds)**: 원인은 프로덕션 코드가 아니라 테스트 클라이언트 소켓의 Nagle 지연이었다 — 호스트 매니저 쪽은 `set_nodelay(true)`인 반면 테스트 소켓은 아니어서 Heartbeat 프레임이 리눅스 루프백에서 20ms 안에 도달하지 않았다(계측으로 전달 지연 실측, macOS에서는 지연 없어 미발견). 수정: 테스트 소켓에 nodelay 설정(실제 뷰어와 동일) + 고정 20ms 대기를 500ms 마감 폴링으로 교체해 상태 전이 자체를 단정. 3회 연속 통과.
- **control 테스트 리눅스 실패(failed_release_is_retried_from_status_polling)**: `virtual_display.rs`의 `call_destroy`가 macOS/비-macOS 쌍둥이 구현인데 비-macOS가 `test_destroy` 훅 검사 전에 Err를 반환해 테스트 주입 destroy가 무시됐다. 수정: 비-macOS 폴백도 `#[cfg(test)]` 훅을 macOS와 동일하게 우선 검사(프로덕션 경로 무변경).
- **아키텍처 검사 기존 위반 32건**: 커밋된 km 입력 모듈이 체커 스트리밍 규칙(네트워크는 Rust, import 화이트리스트)과 미정합이었고 km 계획 문서가 "별도 후속"으로 남겨둔 상태였다. 프로젝트의 기정 패턴(문서화된 경로 한정 예외 — StreamActivity XR 코루틴, KeyBridge 어댑터, Opus nio 선례)대로 `tools/architecture-check/ts.ts`에 km 디렉터리 한정 예외를 추가: (a) km main/test 소스의 소켓·암호·nio·Shizuku·HiddenApiBypass·org.json import, (b) km 모듈의 자체 TCP 소켓(호스트 Rust km 세션과의 고주파 채널이 설계 경계), (c) KeyBridge AIDL 바인딩 사이트로 KmKeyBridgeLease를 문서와 함께 명시. `architecture-check: TS/Kotlin rules clean` 도달, 체커 자체 테스트 3/3.
- **결과**: `cargo test -- --test-threads=1` = **400/400 전부 통과**(리눅스에서 최초), clippy -D warnings 통과, `bun run test:architecture` 통과.
- **한계**: 이번 체커 변경은 TS 측만이다 — cargo 측 미러 구현이 km을 별도 스캔한다면 후속 대조가 필요하다(현재 `bun run test:architecture`가 실행하는 것은 TS 측).
