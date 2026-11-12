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
패치에 의해 오염돼 있었다.

**2차 정정(아래 라운드 참고):** 위 복원은 web.tsx에만 해당했다. `native.tsx`는
같은 날짜(2026-11-10 02:35)의 손패치(`type PressableInstance = any;` 수제 별칭 +
`ref as any`) 상태로 남아 1차 라운드의 typecheck·react-doctor "로컬 통과"를 만들
먼 것으로 밝혀졌다. 1차 검증 기록 중 ui 패키지 소스에 의존한 항목은 오염
노드베이스 기반이다.

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

---

# 2차 라운드: ts·android 잔여 실패의 진짜 근원과 bun 패치 (같은 날 저녁)

e186264 push 후 CI: **rust-fast·windows-host·device-evidence 회복**, ts(Typecheck)·
android(ninja) 잔여 실패.

## 규명: "CI가 맞았고 로컬이 틀렸다"

- 신규 다운로드한 npm `@unikit-bridge/ui@0.1.0` 타르볼(sha512 == lock K3FO…)의
  `native.tsx`는 **18–21줄 `type PressableInstance, …` react-native import** 버전 —
  CI TS2305×4·(8bbd08b 시점의) 98줄 TS2322와 정확히 일치. **0.1.0은 출간 결함이
  있는 그 자체**였고 0.2.0(latest)도 동일 결함(라인만 이동).
- 로컬이 초록이었던 이유: node_modules의 native.tsx가 2026-11-10 02:35 손패치
  (수제 별칭 + `ref as any`)였다. bun은 lock 불변 시 기존 폴더를 다시 풀지
  않아 어떤 클린 재설치에서도 손패치가 살아남았고, 1차 라운드에서 `bun patch`
  1차 시도가 이 오염 폴더를 diff 기반으로 삼으면서 `_patch_hash` 캐시
  항목까지 오염됐다(2차 시도의 재오염 원인). 재현 시 `~/.bun/install/cache`
  전량 파기가 필수였다.
- android 잡도 같은 결함군: 순정 `@unikit-bridge/react-native@0.11.0`의
  `UnikitJSIBridge.hpp`는 `namespace unikit`로 개명됐으나 `unikit-jsi-jni.cpp`
  호출부 5곳은 `unikit_bridge::` 잔존(CI `use of undeclared identifier` 그대로).

## 수정: 소비측 bun 패치 2건(저장소 기록·CI 결정론적)

업스트림(UniKit 별도 저장소) 재출간은 불가(ENEEDAUTH) → 이 저장소 선례 방식
(`react-native-tcp-socket@6.4.2`)대로 `patches/` + `patchedDependencies`.

| 패치 | 내용 |
| --- | --- |
| `patches/@unikit-bridge%2Fui@0.1.0.patch` | `native.tsx`: 존재하지 않는 `*Instance` import 4개 제거 → `ElementRef<typeof RN*>` 로컬 별칭(`PressableInstance`만 `any` — RN Pressable 타입에서 안전 유도 불가). 98줄 캐스트는 순정 유지(`Ref<PressableInstance>`로 충분) |
| `patches/@unikit-bridge%2Freact-native@0.11.0.patch` | ① `unikit-jsi-jni.cpp`: `namespace unikit_bridge = unikit;` 별칭. ② `UnikitJSIBridge.cpp`: `make_array_buffer` 정의를 `unikit` 블록 밖 전역으로 이동(C++ 규칙: 형제 네임스페이스 멤버를 블록 안 정의 불가) + `facebook::jsi::` 완전 한정자(블록 안 `using namespace` 사라짐) |

②는 CI 로그에 없던 것 — CI는 jni.cpp에서 먼저 죽어 도달 못 했고, **로컬에서
CI와 동일한 태스크(`:unikit-bridge_generated-react-native:externalNativeBuildDebug`)를
실컴파일해야** 발견됐다(1차: `generated::` 미선언, 2차: 전역 스코프 `Value` 미선언,
3차 통과).

## 2차 검증 기록 (완전 클린: `~/.bun/install/cache` 파기 + 전 node_modules 삭제 + bun 1.4.1 frozen)

- 클린 재설치 후 두 패치 적용 내용 byte 검증(별칭·splice·한정자 전부).
- `:unikit-bridge_generated-react-native:externalNativeBuildDebug` **BUILD
  SUCCESSFUL**(arm64-v8a, NDK 27.1) — CI 실패 지점 그대로 재현 통과.
- typecheck 3종(루트 tsc -b·viewer-expo·host-desktop) 0 — 레지스트리 순정+패치
  내용 기반 최초의 진짜 통과.
- vitest 979/979, `test:contract` 0, react-doctor **100/100**,
  `test:architecture` clean, `rustra:check`+`git diff --exit-code` clean.
- cargo 경로: `PATH="$HOME/.cargo/bin:$PATH"` 필요(tools 빌드 테스트 spawnSync).

## 2차 커밋 경계

`package.json`·`bun.lock`·`patches/@unikit-bridge%2F{ui@0.1.0,react-native@0.11.0}.patch`·
이 문서. 사용자 진행 WIP(`apps/host-desktop/src-tauri/src/panel{,_client}.rs`)은 미포함.

---

# 3차 라운드: 93ed3e3 CI — ts·android 회복, rust·windows 새 실패 2건

ts(Typecheck)·android(ninja)·device-evidence **초록 — 2차 패치 효과 확인.**
rust·windows 실패는 모두 내 커밋이 아닌 원인:

- **windows-host** `missing_token_flag_needs_xdg_runtime_dir`(leftcar-hostd):
  사용자 커밋 904d478·faf7b3b는 그때까지 푸시되지 않아 93ed3e3 push가 CI에
  처음 노출. 테스트는 "XDG 없으면 오류"만 가정했으나 Windows 러너는
  LOCALAPPDATA 폴백(`panel_dir_from_env` 문서화 규칙)으로 항상 성공한다.
  → 테스트가 우선순위 전체(XDG → 플랫폼 폴백 → 오류)를 반영하도록 수정.
  Windows 규칙 자체는 panel.rs의 순수 근사 테스트가 이미 결정론적으로 커버.
- **rust** `a_failed_accessory_owner_must_retire_before_another_reader_is_admitted`
  (android-viewer, e186264 이후 불변 영역): stop 플래그 관측과 워커 스레드
  종료(`is_retired`) 사이 창에 의존하는 경합 — CI 병렬 부하에서 창이 닫혀
  같은 fd로 새 브리지가 입장(OK 반환). 로컬 10회 재현 불가(0.03s).
  → `UsbBridge`가 최초 fd의 fstat 정체(dev, ino)를 기억하고, 정지된 브리지로의
  **같은 물리 소켓 재입장을 타이밍과 무관하게 거부**(`owns_socket`). 새
  액세서리는 언제나 새 fd/inode로 오므로 합법적 재연결 경로 보존(실행 중
  브리지 재사용·retired+다른 fd 새 시작 그대로).

검증: android-viewer fmt/clippy `-D warnings`/296 테스트 0, usb_ownership
20회 연속 그린, 워크스페이스 전체 0 실패, hostd 테스트 XDG 있음/없음 양쪽 그린.

## 3차 커밋 경계

`native/android-viewer/src/{usb_bridge.rs,jni_exports.rs}`,
`apps/host-desktop/src-tauri/src/bin/leftcar-hostd.rs`(테스트만), 이 문서.
사용자 진행 WIP(`panel.rs`·`panel_client.rs`)은 미포함.

---

# 4차 라운드: ef9bfad CI — 4잡 중 rust만 남은 타이밍 예산 부족

ts·android·windows·device-evidence 초록(USB 소켓 정체 게이트 효과 확인).
rust는 새 지점에서 마지막 1건:

- **`tcp_media::tests::fragmented_header_survives_an_idle_timeout_inside_a_frame`**
  (43196980786): writer 스레드의 fragment 간 sleep(50ms)이 CI 병렬 부하에서
  수백 ms로 늘어나며 whole-frame 마감(200ms)을 넘겨 `Err(TimedOut)` 패닉.
  마감은 리더 측 `Instant`라 부하에 비례해 늘어나지 않고 절대 지연만 끼는데,
  반대로 writer의 wall-clock sleep은 부하에 그대로 늘어나는 비대칭이 원인.
  → **테스트 예산만 확대**: idle 20→100ms, 갭 50→250ms, whole 200→2_000ms
  (불변식 idle < 갭 < whole 유지 — 갭이 idle을 넘겨야 조각 경계에서 Idle이
  아니라 계속 읽히고, whole가 갭을 넘겨야 프레임 완결이 가능하다).
  trickling 테스트의 elapsed 상한도 250→500ms로 동일 사유 확대.
  프로덕션 경로(`run_frames`의 idle 200ms·frame 2s)는 불변.

검증: CI 동일 명령 그대로 — `cargo test --manifest-path
apps/host-desktop/src-tauri/Cargo.toml --locked` 422 passed / 0 failed / 2
ignored(해당 테스트 포함), `cargo clippy --manifest-path … --tests --locked
-- -D warnings` 0, 대상 테스트 단독 8회 연속 그린, `rustfmt --edition 2021
--check src/tcp_media.rs` clean. /simplify: 자명한 수치 확대라 무변경 종료.

## 4차 커밋 경계

`apps/host-desktop/src-tauri/src/tcp_media.rs`(테스트만)와 이 문서.

---

# 5차 라운드: CI 5잡 전량 초록(9f4d87a) + unikit 의존 최신화(ui 0.2.0)

4차 커밋 9f4d87a에서 **rust 잡까지 통과해 5개 잡 전원 초록** — 마이그레이션
부채로 시작한 CI 실패는 여기서 전량 해소됐다.

이어 "unikit 업데이트 다 안되어 있어?" 점검 결과:

- `@unikit-bridge/react-native@0.11.0`·`cli@0.13.0`·`types@0.13.0` —
  **이미 레지스트리 latest**. 갱신 없음(패치 1건은 계속 필요).
- `@unikit-bridge/ui` — 0.1.0(패치 사용) → **0.2.0(latest)** 이 유일한 차등.
  0.2.0은 shadcn 계열 23종 신규 컴포넌트 + 브리지 접근성 prop·동적 style
  패스스루가 추가된 기능 릴리스다.

## ui 0.2.0의 출간 결함 2건(소비측 패치 연장)

1. **0.1.0과 동일 결함이 5개로 확대**: `native.tsx`가 react-native에서
   수출하지 않는 `type *Instance` import — 0.1.0의 4개에서 `ImageInstance`가
   추가됐다. 패치 재작성(별칭 5개, 기존 방식 동일).
2. **신규 결함 — `BridgeRole` × RN 0.86**: 브리지가 웹·네이티브 공통 역할
   유니언(`BridgeRole`: heading·tabpanel·option·separator·listitem·status
   포함)을 그대로 `accessibilityRole`에 전달한다. 업스트림 개발 의존 RN
   0.88.1의 `AccessibilityRole`은 `| string`으로 열려 있어 통과하지만, 우리
   RN 0.86.2는 닫힌 리터럴 유니언이라 TS2322. → 패치에서
   `NativeAccessibilityRole`(`ComponentProps<typeof RNView>["accessibilityRole"]`)
   로 좁혀 3개 할당 지점(View·Text·Pressable)에서 캐스팅. 알 수 없는 역할
   문자열은 런타임에서 OS가 무시하므로 안전하다.

uniwind peer 요구(>=1.13.0, 설치는 1.11.0)는 0.1.0부터 동일한 상태이며
브리지가 쓰는 uniwind API는 `Uniwind` 하나뿐이라 영향 없음 — 이번 범위에서
유지(불일치는 업스트림이 요구조건을 올린 것으로, 실사용 API엔 무관).

## 5차 검증 (완전 클린: 캐시·node_modules 파기 + frozen 재설치로 패치 결정론 확인)

typecheck 체인(ui-theme·react-runtime·루트 tsc -b·viewer-expo·host-desktop)
0, vitest 979/979, contract 0, architecture clean, react-doctor **100/100**.
패치 적용은 풀 파기+frozen 재설치로 바이트 검증(별칭·캐스팅 전부).

## 5차 커밋 경계

`package.json`·`bun.lock`·`apps/{host-desktop,viewer-expo}/package.json`
(버전 0.1.0→0.2.0), `patches/@unikit-bridge%2Fui@0.2.0.patch`(신규,
0.1.0 패치 삭제), 이 문서.

**최종 확정:** 9f4d87a·940d2f8 연속 두 커밋 모두 CI 5잡 전원 초록
(43199389965·43202008524). unikit-bridge 마이그레이션 부채·의존 최신화
전부 종결.

---

# 6차 라운드: c964228(docs-only)에서 control_secure_spec 부하 플레이크

문서만 바뀐 커밋의 CI(43204212572)에서 rust 잡만 실패 —
`rejects_an_oversized_sealed_request_before_writing_and_preserves_earlier_fifo_work`
가 `first.recv_timeout(5s).expect("settles")`에서 Timeout. 직전 두 런에서
같은 바이너리가 통과했으므로 순수 러너 부하 오탐: 1차 라운드에서 클라이언트
측 `getStatus` 데드라인만 60s 오버라이드했고, 응답 라우팅을 기다리는 테스트
측 정착 대기는 5s wall-clock 그대로였다. → 같은 테스트의 정착 대기 2곳
(`first`·`next`)을 60s로 확대. FIFO·용량 거부 의미는 타임아웃과 무관하다.

검증: `cargo test -p viewer-app-core --test control_secure_spec` 9회 연속
4/4 그린, rustfmt clean, clippy `-D warnings` 0. /simplify: 수치 확대라
무변경 종료.

---

# 7차 라운드: 4f67e8e CI — panel 과다 요청 응답이 RST로 파기되는 실제 계약 경합

6차 커밋(코드 수정 포함)의 CI(43205819803)에서 rust 잡이 다른 지점에서
실패: host 크레이트 `panel::tests::oversized_request_line_is_rejected_and_
closes_the_connection`이 첫 응답 읽기에서 `ConnectionReset(code 54)`.

- **근원(코드 결함, 테스트 아님)**: `handle_connection`의 `TooLong` 처리가
  byte-limit 오류 응답을 flush하고 즉시 `return`(close)한다. 1MiB 초과
  페이로드가 수신 큐에 남은 채 close하면 커널은 FIN 대신 **RST**를 보내고,
  RST는 클라이언트 수신 큐에 이미 도착한 오류 응답까지 파기한다. 느린
  독자(부하 찬 러너의 테스트 스레드 포함, 정직한 실제 클라이언트도)는
  byte-limit 응답을 못 받고 reset을 본다 — 직전 두 런에서 통과한 것은
  클라이언트가 RST보다 빨리 읽었을 뿐인 타이밍이었다.
- **수정**: `drain_remaining_input` — 닫기 전 잔여 수신을 버린다(상한
  4×MAX_REQUEST_BYTES, 읽기 제한 250ms). 정직한 발신자는 유한하게 보내고
  끝내므로 큐가 비워져 FIN으로 닫히고, 계속 붓는 발신자는 상한에서 끊는다.

검증: host 크레이트 clippy `-D warnings` 0, oversized 테스트 10/10 그린,
전체 447 passed / 0 failed(CI 동일 명령). /simplify: 단일 목적 헬퍼로 이미
최소 — 무변경 종료.

## 7차 커밋 경계

`apps/host-desktop/src-tauri/src/panel.rs`(TooLong arm + 헬퍼), 이 문서.
