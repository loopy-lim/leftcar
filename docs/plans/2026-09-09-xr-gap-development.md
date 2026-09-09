# XR 갭 리뷰 개발 계획 — 2026-09-09

`docs/2026-09-09-xr-support-gap-review.md`의 개선 옵션 중 이번 패스에 실제로 구현하는 범위와 설계를 기록한다.

## 범위 확정

| 리뷰 항목 | 이번 패스 | 근거 |
| --- | --- | --- |
| §D 텍스트 입력 프로토콜 | **구현** (최우선) | "최대 기능 공백", 백로그 명시 항목. 프로토콜 자체가 없어 IME 텍스트가 Mac에 도달하지 않는다. |
| §B UI 밀도 상향 | **구현** | 폰 기준 9~13px 폰트·48dp 미만 타깃. 패널 폭 기반 스케일로 해소. |
| §A uses-feature 선언 누락 | **구현** | 1줄 수정. 설치 시점 XR 분류 보강. |
| §C XR 컨트롤러 제스처 | 후속 (실기 측정 선행) | 컨트롤러 발화 여부가 추측 상태라 리뷰 §3 체크리스트 2번이 먼저다. |
| §E 풀스페이스 극장 모드 | 후속 | androidx.xr.compose 신규 도입으로 단일 패스 범위 초과. |
| §F 2디스플레이 랑데부 | 후속 | 별도 연구 트랙 (`2026-09-08-two-display-lag-research.md`). |
| §A 비율 프리셋 실기 검증 | 후속 (사용자 수행) | 리뷰 §3 체크리스트 측정 작업. |

## §D 텍스트 입력 설계

기존 입력 평판(LCI1, kind 1~5) 위에 **kind 6 = Text**를 추가한다. 신뢰(ack 재전송) 경로를 그대로 재사용하므로 유실이 없고, 순서는 reliable 큐가 보장한다.

- **프로토콜** (`native/android-viewer/src/input_protocol.rs`): `InputEvent::Text { text: String }`. 페이로드는 길이 접두사 없는 UTF-8 바이트(호스트는 message 길이 − 10바이트 헤더로 계산). reliable. UDP 컨트롤 버퍼 512B 안에 들도록 뷰어가 200B 단위로 청크 전송.
- **JNI**: `sendText(instanceId: String, payload: ByteArray)` — JNI modified-UTF-8이 보문 문자(이모지)를 CESU-8로 깨뜨리는 문제를 피하려고 String이 아닌 바이트 배열로 전달.
- **뷰어 네이티브**:
  - `TextInputRelay`(JVM 순수): IME InputConnection 의미를 전송 명령으로 변환. commitText는 `\n`을 Enter로 분리, deleteSurroundingText는 백스페이스 키 다운/업, forward delete 지원, sendKeyEvent의 DEL/ENTER도 동일 매핑. 200바이트 그래핌 경계 청크.
  - `TextInputLensView`: 1×1px 투명 뷰. `onCreateInputConnection`이 릴레이로 포워딩하는 BaseInputConnection을 돌려준다. 스트림 중 조합 중 텍스트는 전송하지 않고(commitText 시점에만 확정 텍스트 전송) 한글 조합 중간 값이 Mac에 쓰이는 것을 막는다.
  - `StreamActivity`: 렌즈를 decorView에 1회 부착, HUD 토글 콜백으로 IME open/close. 전송은 기존 `remoteInputLocked()` 게이트를 통과. 하드웨어 키 경로(dispatchKeyEvent)와 공존 — IME 텍스트는 InputConnection으로만 들어온다.
  - `StreamHudController`: "?"칩 아래 "ABC" 토글 칩 추가. IME 표시 상태는 WindowInsets(API 30+)로 추적해 칩 강조.
- **호스트** (`CaptureSession+Input.swift`): kind 6 → UTF-8 복원 후 그래핌별 `CGEvent(keyboardEventSource:, virtualKey: 0, keyDown:)` + `keyboardSetUnicodeString` 다운/업 페어를 기존 키보드 경로와 같은 `.cghidEventTap`에 게이트(`enabled`) 뒤에서 주입.

## §B UI 밀도 설계

- 곡선: `width ≤ 600dp → 1.0`, `그 외 min(width / 600, 1.5)`. 폰(≤600dp)은 오늘 승인된 다이어트 UI 그대로, 태블릿·XR 패널만 확대.
- TS (`apps/viewer-expo/src/panel-density.ts`): 순수 함수 `panelDensityScale(width)` + 스타일 후처리 스케일러(크기 속성 명시 세트만 곱함 — flex/opacity/transform 제외, 문자열 값 skip). catalog/host/index/pairing의 `createStyles` 결과에 적용. 단위 테스트 동반.
- Kotlin (`StreamPanelDensity.kt`): 동일 곡선을 미러링하는 JVM 순수 함수 + 테스트. `StreamHudController`·`CursorOverlayView`·`GestureHintOverlay`의 dp 계산에 곱해 XR 대형 패널에서 배지·칩·커서가 커진다.

## §A manifest

`apps/viewer-expo/android/app/src/main/AndroidManifest.xml`에 `<uses-feature android:name="android.software.xr.api.spatial" android:required="false"/>` 추가 — 런타임 프로브는 유지(비 XR 기기 설치 가능성 보존).

## 검증 계획

- Rust: `cargo test -p android-viewer` (호스트 타깃) — Text 인코딩/와이어 길이/스케줄러 반영.
- Kotlin: `gradlew testReleaseUnitTest` — 릴레이 매핑·청크·문자열.
- TS: vitest(panel-density) + `bun run typecheck` + react-doctor 100/100 (게이트).
- Swift: `tools/build-macos-capture-shim.zsh library` 컴파일 확인.
- APK: cargo ndk → `assembleRelease` → 도달 가능한 기기에 `adb install -r` 후 기동 스모크. IME commitText는 adb `input text`(키 이벤트 경로)로는 재현 불가 → 실기 키보드 검증은 사용자 체크리스트로 이관.

## 결과 (같은 날 구현 완료)

### 변경 파일

- **프로토콜·JNI (Rust)**: `input_protocol.rs`(Text kind 6 + 와이어/스케줄러 테스트), `jni_exports/session_io.rs`(`leftcar_jni_input_text` + 큐 반영 테스트), `jni_wrappers.rs`/`jni_wrappers/input.rs`(바이트 배열 경유 `sendText`).
- **뷰어 네이티브 (Kotlin)**: `TextInputRelay.kt`(신설, 조합 버퍼·개행→Enter·200B 코드포인트 청크), `TextInputLensView.kt`(신설, 더미 BaseInputConnection 포워딩 + IME inset 콜백), `StreamActivity.kt`(렌즈 부착·토글·잠금 게이트), `StreamHudController.kt`("ABC" 칩 + panelScale), `ViewerStrings.kt`, `CursorOverlayView.kt`/`GestureHintOverlay.kt`(panelScale), `StreamPanelDensity.kt`(신설), `AndroidManifest.xml`(uses-feature).
- **호스트 (Swift)**: `CaptureSession+Input.swift` — kind 6 → 그래핌별 CGEvent 유니코드 문자열 주입(HID 탭, enabled 게이트 준수).
- **RN (TS)**: `panel-density.ts`(신설) + catalog/host/index/pairing 4개 화면 적용.

### 검증 결과

| 게이트 | 결과 |
| --- | --- |
| `cargo test -p android-viewer` | 185 passed (구현 직후 02:53 기준) |
| `gradlew :app:testReleaseUnitTest` | 72 passed, 0 failed (릴레이 10 + 밀도 4 포함) |
| `bun run typecheck` / vitest 전체 / contract | tsc clean, 506 + 4 passed |
| `react-doctor . --verbose` | 100 / 100 |
| capture shim Swift 컴파일 | 성공 (dylib 생성) |
| `assembleRelease` + 태블릿 `adb install -r` | 성공, 기동 후 치명 크래시 0 |

### 미검증 / 후속

- **Rust 게이트 이중 재확인 (최종)**: (a) 구현 직후 전체 스위트 185 passed, (b) 타 세션 재구조화가 일시적으로 `viewer-core` 컴파일을 깨뜨리던 동안 `.worktrees` 격리 체크아웃(HEAD f961615 + 본 변경 4파일)에서 185 passed, (c) 재구조화 안정 후 **현재 트리에서 `cargo test -p android-viewer` 185 passed, 0 failed (exit 0)** — 커밋 전 조건은 해소됐다. 격리 작업 트리는 정리 완료(`git worktree list` = main만).
- **Galaxy XR 실기**: 오프라인(192.168.0.249 연결 거부) — 미설치. 리뷰 §3 체크리스트(비율 프리셋·컨트롤러·키보드)를 사용자 실행 필요. IME commitText는 adb로 재현 불가해 실기 소프트키보드 타이핑 검증이 남아 있다.
- **후속 과제**: 풀스페이스 극장 모드(§E), 컨트롤러 제스처 재매핑(§C), 2디스플레이 랑데부 실험(§F).

## 후속 실기 검증·정리 (2026-09-09 저녁)

### 커밋(12:57) 이후 발견·수정한 블로커

- **안드로이드 타깃 컴파일 깨짐**: 2086b9e가 `TerminationReason::code()`를 u8화하면서 android 전용 호출부 2곳(`renderer/single_session/network.rs`, `renderer/split_session/tile_worker.rs`)이 i8 저장소에 u8을 그대로 넘겨 `cargo ndk` 빌드가 실패했다. `i8::try_from(code).unwrap_or(-1)` 변환으로 수정. 이 경로는 `cfg(target_os = "android")` 뒤라 호스트 타깃 `cargo test`에서는 발각되지 않았다 — **android-viewer 변경 후 `cargo ndk` 조합 빌드도 게이트로 돌려야 한다.**
- **워크스페이스 전체 Windows 타깃 체크 통과**: `viewer-decoder`의 `stream_receiver` 빈은 Unix 전용(SO_RCVBUF raw setup)이라 `#[cfg(unix)]` 게이트를, `android-viewer`의 `usb_bridge` 모듈(AOAP raw fd)은 `#[cfg(unix)]`를 걸었다. 수정 후 `cargo check --workspace --target x86_64-pc-windows-msvc` 클린. 단, tauri 앱 자체(src-tauri)는 tauri-winres가 llvm-rc를 요구(본 기기 미설치)해 여전히 환경 블로커다. 수정 전에는 호스트 의존 크레이트 6종(control-contract/session/domain/fec-core/usb-mux/media-model)이 Windows 타깃에서 이미 클린이었다.

### 태블릿(Lenovo TB710FU) 재빌드·E2E — 전부 통과

`cargo ndk`(rustra+android-viewer, 185 tests) → `assembleRelease` → `adb install -r` 후:

- 페어링 토큰 재설치 보존 + 무음 자동 재접속 확인(재시작 없이 "컴퓨터 연결됨").
- stream-stats 수치: 1920x1080 @ 56~58fps, cap/enc 정렬, `inputEnabled=true`.
- 탭→커서: 태블릿 스트림 (50%, 50%) 탭이 Mac (960, 540) 정중앙에 도달.
- 숫자 키(adb key-event 경로): TextEdit에 그대로 도달.
- **remoteKeyRemap 훅**: `defaults write -g dev.leftcar.remoteKeyRemap -dict 15 25` 상태에서 뷰어 "8"(Android 15)이 Mac "9"로 치환 기록, 비리맵 "7"은 통과 — injectKey 직전 치환 실증. 검증 후 default 삭제.
- **ABC 칩·렌즈**: 칩 탭으로 IME 오픈(`mInputShown=true`), `dumpsys input_method`의 `mServedView`가 1×1 렌즈, 칩 하이라이트 전환 확인.
- 밀도: 카탈로그·HUD 스모크에서 레이아웃 파손 없음(수치 곡선은 TS/Kotlin 단위 테스트로 담보).

### 여전히 미검증

- ~~실기 IME commitText → kind 6 주입까지~~ → **같은 날 밤 검증 완료(아래)**. ZUI 소프트키보드가 adb 주입 탭을 받지 않는(레이아웃 전환키도 무반응, 앱 윈도우는 정상 — 기기 입력 쿼크) 탓에 직접 타이핑 재현은 불가했고, 위조 LCI1 패킷도 컨트롤 토큰(LCH1 논스) 검증에 막혀 전송 불가했다. 대신 **commitText를 브로드캐스트로 호출하는 최소 테스트 IME**(aapt2+kotlinc+d8 핸드빌드, 설치 후 `ime set`, 검증 후 제거)로 렌즈 InputConnection에 한글 커밋 → **Mac TextEdit에 "안녕 마카" 도달 확인**. 릴레이 조합 버퍼 플러시(finishComposingText)도 함께 관찰됐다. 주의: ABC 오프 토글은 포커스를 스트림 Surface로 돌리므로, 그 직후의 commitText는 의도대로 폐기된다(fieldId=0/inputType=0 리바인드).
- Galaxy XR 항목(상동). XR은 이날도 오프라인.
