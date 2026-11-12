# unikit-bridge 마이그레이션 부채 정리 (2026-11-12)

원 요청: "그거 모두 마이그레이션 잘 할 수 있도록 해줄 수 있나요?? 정리해주세요."
— 8bbd08b(rustra→unikit-bridge 전환)가 남긴 CI 4잡 실패의 전량 해소.

## 원인 → 수정 대응표

| CI 잡 | 증상 | 근원 | 수정 |
| --- | --- | --- | --- |
| windows-host | `tsc` TS2322 `Ref<HTMLDivElement>` ×4 (`@unikit-bridge/ui/src/bridge/web.tsx`) | 워크스페이스별 `@types/react` 분열 — 루트 `^19.3.0`(호이스트 19.3.0) vs host-desktop/viewer-expo `~19.2.4`(중첩 19.2.18). `web.tsx`의 명시적 `React.Ref`는 루트 사본, JSX ref 속성은 앱 사본을 봐 서로 다른 타입이 됨 | 루트 `@types/react`를 `~19.2.4`로 단일화(`package.json` + `bun.lock` 재해석 — 신규 설치 시 물리 사본 1개만 남음) |
| ts | vitest가 `unikit-bridge-sibling/` 체크아웃의 node:test 파일(`scripts/api-surface.test.ts`)을 수집·실행해 실패 | 847f99b의 CI 형제 저장소 체크아웃이 vitest 디폴트 수집 범위 안에 들어옴 | `vitest.config.mts` exclude에 `**/unikit-bridge-sibling/**` 추가 |
| android | gradle 설정 중 `providers.exec node` exit 1 — `import.meta.resolve('@rustra/react-native')` 모듈 부재 + 주입 변수명 구식 | `apps/viewer-expo/android/build.gradle`의 어댑터 resolve/검증/주입이 마이그레이션 전 이름(`@rustra/react-native`, `RustraJSIBridge.cpp`, `RUSTRA_ADAPTER_ROOT`) 잔존. 생성 모듈은 이미 `UNIKIT_ADAPTER_ROOT`를 소비 | `@unikit-bridge/react-native` + `cpp/UnikitJSIBridge.cpp` + `UNIKIT_ADAPTER_ROOT`로 갱신 |
| rust-fast | `control_secure_spec.rs` `rejects_an_oversized_sealed_request...` — `ControlError{Timeout,"control request timeout: getStatus"}` | `getStatus` 기본 데드라인 5s가 부하 찬 macOS 러너의 10MiB 거부 경로 처리 시간을 못 견딤(코드 주석 스스로 "오탐이면 안 된다" 경고) | 해당 테스트만 `ConnectOptions::timeout_overrides`로 `getStatus` 60s |

### 도중 발견한 추가 부채(CI가 도달 못 해 미검출이던 것)

- **viewer-expo·루트 typecheck TS2769**(`native.tsx` ×3): `@types/react` 사본 분열의 RN 측 발현(VoidOrUndefinedOnly 브랜드 타입 불일치). 위 단일화로 동시 해소.
- **workspace clippy `-D warnings` 9+4건**(viewer-app-core lib+tests): `new_without_default`·`redundant_closure_call`·`never_loop`·`manual_is_multiple_of`·`chunks_exact_to_as_chunks`·`manual !RangeInclusive::contains`·`needless_borrow` ×2·`type_complexity`, 테스트 쪽 unused import/variable·`bool_assert_comparison`·dead helper. 전부 코드 수준 수정(억제 없음). semantic 동치 검증: 핸드셰이크 IIFE→블록, connect `loop`→단일 분기(전 분기 return/break라 1회 반복과 동치), `chunks_exact(2)`→`as_chunks::<2>()`(짝수 길이 가드 후 잔여 없음).
- **`control_fifo_spec` 병렬 경합 3건**: ① 요청 채널 확정 직후 `server.destroyed()` 즉시 단언 2곳(병렬 부하 시 재현: 수정 전 소스 101 0 0 101) — `wait_until` 5s로 경화. ② `terminates_the_fifo_channel_when_a_later_socket_write_throws_synchronously`의 **순서 경합**(독립 진단으로 심층 규명): `issue(second)`가 쓰기 실패를 내기 전 메인 스레드의 `server.send`(유효 응답)가 리더에 도달하면 선두 요청 `first`가 Ok로 확정되어 `.expect_err("transport")` 패닉(~1/10 재현). teardown 순서(보류 싱크 정리→writer 파괴) 근거로 `destroyed()` 관찰 후 응답 발송하도록 재배열해 경합 원천 제거 — 이후 **12/12 연속 green**.
- **/simplify 독립 진단 채택 3건**: `retain_rollback` 파라미터의 `TokenRollback` 별칭 완결, `session_spec` 죽은 클론 2쌍 제거, 위 ② 순서 경합. 기각: gradle 지역변수 개명(모듈명이 여전히 rustra-bridge라 거짓 아님), `site/package.json`(독립 루트), `to_hex` 할당(콜드 패스), `is_some_and` 치환(취향).

## 공개 사항: 릴리스 빌드 당시의 node_modules 패치

v0.1.11 산출 과정에서 로컬 `node_modules/@unikit-bridge/ui/src/bridge/web.tsx`가
`ref as any`로 수 패치돼 있었고, 이것이 로컬 typecheck 통과·CI 실패의 "불일치"로
보였다. 진품 npm 타르볼(0.1.0은 `ref as React.Ref<...>` 캐스트)로 복원하자
로컬에서도 CI와 동일한 TS2322가 재현됐다 — 불일치는 존재하지 않았고 로컬 검증이
패치에 의해 오염돼 있었다. 현재 node_modules은 lock 기준 전면 재구성(진품).

## 검증 기록 (2026-11-12, 이 머신 — CI push 후 4잡 green으로 최종 확인 예정)

- `bun install --frozen-lockfile`(완전 재설치) 후 물리 `@types/react` 사본 = 루트 19.2.18 단일(viewer-android 18.3.31은 격리 프로그램 전용).
- typecheck 3종: host-desktop 0 / viewer-expo 0 / 루트 0.
- vitest `bun run test` 979/979, `test:contract` 0 (sibling 더미 실패 테스트로 수집 제외 실측: 실행 0건).
- react-doctor **100 / 100**.
- `rustra:generate`→`rustra:check`→`git diff --exit-code`(control-generated·generated·rustra-bridge) clean. `test:architecture` clean.
- gradle 설정 단계(`./gradlew help`) 통과 — CI 실패 지점(어댑터 resolve·UnikitJSIBridge.cpp 검증·UNIKIT_ADAPTER_ROOT 주입)과 동일 경로.
- `cargo fmt --all -- --check` clean. `cargo test --workspace --locked` 0. `cargo clippy --workspace --tests --locked -- -D warnings` 0. 호스트 크레이트 `cargo test --locked` 0. viewer-app-core 전체 스위트 5회 연속 0.
- playwright `test:ui`(크로미움, `--with-deps` 없이 로컬 설치): 전 시나리오 PASS, exit 0.

## 커밋 경계

마이그레이션 수정 14파일만 커밋. 사용자 미커밋 WIP(`apps/host-desktop/src-tauri/*` 6파일, `docs/dev-environment.md`)은 보존·미포함.
