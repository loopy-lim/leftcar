# Android Input Ownership and Disconnect Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Android와 Mac 사이의 물리 키보드·마우스 소유권을 안전하게 전환하고, 중복 커서와 연결 종료 시 Mac 자동 잠금을 제거한 설치 패키지를 만든다.

**Architecture:** Leftcar의 순수 Kotlin `InputOwnershipController`가 상태와 effect를 결정하고 `StreamActivity`의 Android adapter가 Pointer Capture, LCD1 커서, JNI 입력 해제를 실행한다. KeyBridge는 검증된 Leftcar 바인더 요청 동안만 새 물리 키 시퀀스를 통과시키며, Host에서는 자동 잠금 기능을 interface부터 완전히 삭제한다.

**Tech Stack:** Kotlin/JVM, Android Pointer Capture, Android Binder/AIDL, Rust/UniFFI KeyBridge core, React/TypeScript, Rust/Tauri, Swift/CoreGraphics, Gradle, Bun/Vitest.

**Spec:** `docs/superpowers/specs/2026-09-14-android-input-ownership-and-disconnect-safety-design.md`

## Global Constraints

- 기준 Leftcar 작업 공간은 `/Users/loopy/dev/ll3/leftcar/.worktrees/completion-followup`이며 기존 Dock/HUD/검증 변경을 보존한다.
- KeyBridge의 기존 `desktop/src/app.ts`, `desktop/src/index.html`, `desktop/src/style.css` 수정은 건드리지 않고 별도 worktree에서 Android만 변경한다.
- Power, Android 보안 화면, OEM 예약키를 원격 전달한다고 약속하지 않는다.
- ADB/Shizuku는 일반 입력 소유권 전환에 사용하지 않는다.
- 모든 원격 종료 경로는 `ViewerNative.releaseInput` 뒤 Pointer Capture와 KeyBridge 통과 모드를 해제한다.
- React/React Native/TSX 변경 뒤 저장소 루트의 React Doctor 결과는 반드시 `100 / 100`이어야 한다.
- 실제 기기 검증과 소스 테스트를 구분해 기록한다.
- 커밋과 push는 별도 요청 전에는 수행하지 않는다.

---

### Task 1: Host 자동 잠금 interface 삭제

**Files:**
- Delete: `apps/host-desktop/src-tauri/src/lock.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs`
- Modify: `apps/host-desktop/src-tauri/src/control.rs`
- Modify: `apps/host-desktop/src-tauri/src/settings.rs`
- Modify: `apps/host-desktop/src/Privacy.tsx`
- Modify: `apps/host-desktop/src/App.tsx`
- Modify: `packages/ui-tokens/src/i18n.ts`
- Modify: `tools/ui-regression/entry.jsx`
- Modify: `tools/ui-regression/run.cjs`
- Modify: `docs/07-security-privacy.md`

**Interfaces:**
- Consumes: 기존 `SharedSettings`, `ControlServer` teardown, Host footer toggle 구조.
- Produces: 잠금 field, command, callback, 운영체제 명령이 없는 Host 설정 interface.

- [ ] **Step 1: legacy 설정 키가 동작 계약에 들어오지 않는 실패 테스트 작성**

`settings.rs`의 테스트에서 `{"lockOnDisconnect":true,"privacyCurtain":true}`를 읽고도 새 `HostSettings`가 privacy curtain만 노출하는지 확인한다. 설정을 다시 저장한 JSON에는 `lockOnDisconnect`가 없어야 한다.

```rust
#[test]
fn legacy_lock_setting_is_ignored_and_not_persisted() {
    let path = temp_path("legacy-lock");
    std::fs::write(&path, r#"{"lockOnDisconnect":true,"privacyCurtain":true}"#).unwrap();
    let shared = SharedSettings::load_or_default(Some(path.clone()));
    assert!(shared.privacy_curtain());
    shared.set_privacy_curtain(true).unwrap();
    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(!persisted.contains("lockOnDisconnect"));
}
```

- [ ] **Step 2: 테스트를 실행해 기존 구현이 실패하는지 확인**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml legacy_lock_setting_is_ignored_and_not_persisted -- --nocapture`

Expected: 기존 `persist`가 `lockOnDisconnect`를 다시 쓰므로 FAIL.

- [ ] **Step 3: 자동 잠금 코드와 UI를 최소 범위로 제거**

`HostSettings.lock_on_disconnect`, getter/setter, JSON 저장 field를 삭제한다. `ControlServer.lock_screen`, `set_lock_screen`, `maybe_lock_after_teardown`와 모든 호출 및 관련 테스트를 삭제한다. Tauri의 `set_lock_on_disconnect`, lock module 등록, startup callback 주입을 삭제한다. Host footer와 `Privacy.tsx`의 lock state를 제거하고 i18n/UI regression fixture를 새 shape로 맞춘다.

- [ ] **Step 4: Host 단위 테스트와 정적 부재 검사를 실행**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --lib`

Run: `rg -n "lockOnDisconnect|lock_on_disconnect|CGSession|LockWorkStation|displaysleepnow|screen_locked" apps/host-desktop packages/ui-tokens tools/ui-regression docs/07-security-privacy.md`

Expected: 테스트 PASS, 검색 결과 0건.

### Task 2: 자동 단일 커서 정책과 순수 입력 소유권 module

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/InputOwnershipController.kt`
- Create: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/InputOwnershipControllerTest.kt`
- Modify: `apps/viewer-expo/src/viewer-preferences.ts`
- Modify: `apps/viewer-expo/src/viewer-preferences.test.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`
- Modify: `apps/viewer-expo/src/use-catalog-model.ts`
- Modify: `apps/viewer-expo/src/catalog-model-types.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `packages/ui-tokens/src/i18n.ts`

**Interfaces:**
- Consumes: `InputOwnershipEvent`, 현재 `InputOwner`, 정규화 시작 좌표.
- Produces: `InputOwnershipController.on(event): List<InputOwnershipEffect>`, `routePhysicalKey(keyCode, down): KeyRoute`, `advancePointer(dx, dy, width, height): PointerAdvance`.

- [ ] **Step 1: 상태 전이와 좌표 누적 실패 테스트 작성**

테스트는 첫 클릭이 `ACQUIRING_REMOTE`로 가되 Mac 클릭을 만들지 않는지, capture 확인 뒤 `REMOTE_MAC`이 되는지, Esc/포커스 상실/Host 잠금/경계 바깥 이동이 항상 `RELEASE_REMOTE_INPUT`을 첫 effect로 내는지 확인한다.

```kotlin
@Test fun `first click acquires without forwarding and capture confirms remote`() {
    val owner = InputOwnershipController()
    assertEquals(
        listOf(InputOwnershipEffect.ACQUIRE_KEYBRIDGE, InputOwnershipEffect.REQUEST_POINTER_CAPTURE),
        owner.on(InputOwnershipEvent.MouseActivation(0.5f, 0.5f)),
    )
    assertEquals(InputOwner.ACQUIRING_REMOTE, owner.owner)
    owner.on(InputOwnershipEvent.PointerCaptureChanged(true))
    assertEquals(InputOwner.REMOTE_MAC, owner.owner)
}

@Test fun `outward motion at host edge releases remote first`() {
    val owner = remoteOwnerAt(x = 1f, y = 0.5f)
    val effects = owner.on(InputOwnershipEvent.CapturedMove(dx = 3f, dy = 0f, width = 1000, height = 1000))
    assertEquals(InputOwnershipEffect.RELEASE_REMOTE_INPUT, effects.first())
    assertEquals(InputOwner.LOCAL_ANDROID, owner.owner)
}
```

- [ ] **Step 2: 새 JVM 테스트가 class 부재로 실패하는지 확인**

Run: `cd apps/viewer-expo/android && ./gradlew :app:testReleaseUnitTest --tests 'dev.leftcar.viewer.stream.InputOwnershipControllerTest'`

Expected: `InputOwnershipController` 미정의로 FAIL.

- [ ] **Step 3: 순수 Controller를 구현**

세 상태 `LOCAL_ANDROID`, `ACQUIRING_REMOTE`, `REMOTE_MAC`과 effect enum을 만든다. 상대 좌표는 첫 클릭의 정규화 좌표에서 시작해 `[0, 1]`로 clamp하고, 이미 경계인데 같은 방향으로 더 움직일 때만 해제한다. 모든 종료 이벤트는 같은 idempotent `releaseEffects()`를 사용한다.

- [ ] **Step 4: 수동 커서 설정을 자동 정책으로 마이그레이션**

`ViewerPreferences.localCursor`, 카탈로그 switch, toggle callback과 오류 문구를 삭제한다. 예전 JSON의 `localCursor`는 무시하고 쓰지 않는다. 네이티브 launch에는 내부 자동 정책을 나타내는 `true`를 전달해 LCD1 capability는 유지한다.

- [ ] **Step 5: Viewer TypeScript와 Controller 테스트 실행**

Run: `bun run test -- apps/viewer-expo/src/viewer-preferences.test.ts`

Run: `cd apps/viewer-expo/android && ./gradlew :app:testReleaseUnitTest --tests 'dev.leftcar.viewer.stream.InputOwnershipControllerTest'`

Expected: 모두 PASS.

### Task 3: KeyBridge 원격 통과 gate와 검증된 바인더 service

**Files:**
- Worktree: `/Users/loopy/dev/ll3/KeyBridge/.worktrees/leftcar-input-handoff`
- Create: `android/app/src/main/java/dev/loopy/keybridge/RemotePassthroughGate.kt`
- Create: `android/app/src/main/java/dev/loopy/keybridge/RemoteInputOwnershipService.kt`
- Create: `android/app/src/main/aidl/dev/loopy/keybridge/remote/IRemoteInputOwnership.aidl`
- Create: `android/app/src/test/java/dev/loopy/keybridge/RemotePassthroughGateTest.kt`
- Modify: `android/app/src/main/java/dev/loopy/keybridge/RemapService.kt`
- Modify: `android/app/src/main/AndroidManifest.xml`
- Modify: `android/app/build.gradle.kts`

**Interfaces:**
- Consumes: `remoteOwned: Boolean`, physical key identity `(deviceId, scanCode, keyCode)`, down/up phase.
- Produces: `RemotePassthroughGate.route(identity, down): KeyBridgeRoute` where route is `REMAP`, `PASSTHROUGH`, or `CONSUME_LOCAL_RELEASE`; AIDL `boolean acquire()` and `void release()`.

- [ ] **Step 1: down/up 소유권 보존 실패 테스트 작성**

```kotlin
@Test fun `local down remains consumed through up after remote acquire`() {
    val gate = RemotePassthroughGate()
    val key = PhysicalKey(16, 30, 29)
    assertEquals(KeyBridgeRoute.REMAP, gate.route(key, down = true))
    gate.noteRemapDecision(key, consumed = true)
    gate.setRemoteOwned(true)
    assertEquals(KeyBridgeRoute.CONSUME_LOCAL_RELEASE, gate.route(key, down = false))
}

@Test fun `remote down keeps passing through after release until paired up`() {
    val gate = RemotePassthroughGate().apply { setRemoteOwned(true) }
    val key = PhysicalKey(16, 30, 29)
    assertEquals(KeyBridgeRoute.PASSTHROUGH, gate.route(key, down = true))
    gate.setRemoteOwned(false)
    assertEquals(KeyBridgeRoute.PASSTHROUGH, gate.route(key, down = false))
}
```

- [ ] **Step 2: KeyBridge worktree를 만들고 기존 desktop 수정이 따라오지 않는지 확인**

Run from `/Users/loopy/dev/ll3/KeyBridge`: `git worktree add .worktrees/leftcar-input-handoff -b codex/leftcar-input-handoff 4bf8ce0`

Expected: 새 worktree `git status --short`가 비어 있고 원본의 desktop 수정은 그대로 남는다.

- [ ] **Step 3: gate 테스트를 실행해 class 부재로 실패하는지 확인**

Run: `cd android && ./gradlew :app:testReleaseUnitTest --tests 'dev.loopy.keybridge.RemotePassthroughGateTest'`

Expected: 새 class 미정의로 FAIL.

- [ ] **Step 4: gate와 service를 구현**

`RemoteInputOwnershipService`의 AIDL transaction은 `Binder.getCallingUid()`를 `PackageManager.getPackagesForUid()`로 확인해 `leftcar.ll3.kr`만 허용한다. acquire 시 KeyBridge의 합성 출력 해제 listener를 호출하고 remote mode를 켠다. release, unbind, service destroy에서 remote mode를 끈다. `RemapService`는 gate route가 `PASSTHROUGH`면 false, `CONSUME_LOCAL_RELEASE`면 true, `REMAP`이면 기존 engine 결과를 사용하고 consumed 여부를 gate에 기록한다.

- [ ] **Step 5: manifest service와 버전을 갱신하고 KeyBridge 검증 실행**

Service는 explicit bind만 허용하고 `exported=true`로 등록한다. versionCode는 `7`, versionName은 `0.3.4`로 올린다.

Run: `bun run test:android`

Run: `bun run check:android`

Expected: Rust core, Kotlin JVM, binding 검사가 모두 PASS.

### Task 4: Leftcar Pointer Capture와 KeyBridge adapter 연결

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/aidl/dev/loopy/keybridge/remote/IRemoteInputOwnership.aidl`
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/KeyBridgeInputAdapter.kt`
- Create: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/KeyBridgeInputAdapterTest.kt`
- Modify: `apps/viewer-expo/android/app/src/main/AndroidManifest.xml`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamSurfaceLayout.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/viewer-expo/android/app/build.gradle`

**Interfaces:**
- Consumes: Task 2 Controller effects, Task 3 AIDL `acquire/release`.
- Produces: `KeyBridgeInputAdapter.prepare()`, `acquire(onResult)`, `release()`, `close()`; captured pointer listener; one cursor selected from ownership state.

- [ ] **Step 1: adapter 상태와 구버전 판별 실패 테스트 작성**

Robolectric test에서 KeyBridge 미설치면 `AVAILABLE_WITHOUT_KEYBRIDGE`, 새 service resolve면 `READY`, 패키지는 있으나 service가 없으면 `UPDATE_REQUIRED`가 되는지 검증한다. release/close를 반복해도 unbind가 한 번만 호출되는지 확인한다.

- [ ] **Step 2: adapter 테스트가 새 class 부재로 실패하는지 확인**

Run: `cd apps/viewer-expo/android && ./gradlew :app:testReleaseUnitTest --tests 'dev.leftcar.viewer.stream.KeyBridgeInputAdapterTest'`

Expected: `KeyBridgeInputAdapter` 미정의로 FAIL.

- [ ] **Step 3: AIDL adapter와 package visibility를 구현**

Manifest `<queries>`에 `dev.loopy.keybridge`를 추가한다. Activity 시작 시 service를 미리 bind하되 acquire하지 않는다. 첫 클릭에서 동기 acquire 성공 뒤 Pointer Capture를 요청하고, service 사망/구버전/실패는 Controller release event로 돌린다.

- [ ] **Step 4: StreamActivity 입력 경로를 Controller 하나로 통합**

물리 마우스의 첫 button press는 전환에만 소비한다. `setOnCapturedPointerListener`에서 상대 이동, 버튼, 스크롤을 기존 `sendPointerUnlocked` 절대 좌표로 변환한다. `dispatchKeyEvent`는 `REMOTE_MAC`에서만 일반 키를 보내고 Esc는 해제에 사용한다. Host input status 0, focus loss, surface destroy, pause, destroy는 공통 release를 호출한다.

로컬 상태는 LCD1 수신을 유지하되 overlay를 숨기고 Android arrow만 표시한다. 원격 상태는 pointer icon을 `TYPE_NULL`로 하고 overlay만 표시한다. 터치 입력 시 overlay를 보여 주며 다음 로컬 마우스 이벤트에서 숨긴다. 기존 1.5초 Android cursor timer는 삭제한다.

- [ ] **Step 5: Leftcar Android 버전과 단위 테스트를 갱신**

versionCode는 `5`, versionName과 Expo version은 `0.1.6`으로 맞춘다.

Run: `cd apps/viewer-expo/android && ./gradlew :app:testReleaseUnitTest`

Run: `bun run test:gradle-policy`

Expected: 모두 PASS.

### Task 5: 전체 품질 게이트와 패키지 생성

**Files:**
- Preserve: `apps/host-desktop/src-tauri/src/lib.rs`의 Dock visibility 변경
- Preserve: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamHudController.kt`의 불필요한 칩 제거
- Output: Leftcar Host app, Leftcar APK, KeyBridge APK, build manifests.

**Interfaces:**
- Consumes: Tasks 1-4의 소스와 기존 completion-followup 변경.
- Produces: 검증된 로컬 설치본과 복사본·해시.

- [ ] **Step 1: React 및 TypeScript 게이트 실행**

Run: `npx -y react-doctor@latest . --verbose`

Expected: `100 / 100`.

Run: `bun run typecheck`

Run: `bun run test`

Run: `bun run test:contract`

Run: `bun run test:architecture`

- [ ] **Step 2: Rust/Swift/Host 게이트 실행**

Run: `cargo test --workspace --locked`

Run: `cargo clippy --workspace --tests --locked -- -D warnings`

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml --locked`

Run: `swift test --package-path native/macos-capture-shim`

Expected: 모두 PASS.

- [ ] **Step 3: 내부 패키지를 빌드하고 서명·동일성 검사**

Run: `bun run build android-internal`

Run: `bun run build host-macos-internal`

Run in KeyBridge worktree: `bun run build:android:release`

각 산출물에서 package id, version, ABI, 인증서와 SHA-256을 기록한다.

- [ ] **Step 4: 안정된 경로에 설치하고 복사**

Leftcar Host는 `bun run dev:host:macos`의 서명 동일성/rollback 보호 경로로 `/Applications/Leftcar Host.app`에 교체한다. `adb install -r`로 `dev.loopy.keybridge`와 `leftcar.ll3.kr`을 업데이트하며 앱 데이터와 접근성 설정을 지우지 않는다. 두 APK를 `/Users/loopy/Downloads`에 버전과 commit 식별자가 포함된 이름으로 복사하고 원본과 `cmp` 및 SHA-256을 확인한다.

- [ ] **Step 5: 설치본 정적·런타임 검증**

Host 창을 닫은 뒤 같은 PID가 살아 있고 Dock activation policy가 accessory로 바뀌는지 확인한다. Host 설정 파일에서 `lockOnDisconnect`를 삭제하고 스트림 종료 전후 감사 로그에 새 `screen_locked`가 없는지 확인한다. Android `dumpsys input`으로 첫 클릭 뒤 Pointer Capture enabled, Esc/경계/끊김 뒤 disabled를 확인한다. KeyBridge 접근성 service와 사용자 keymap이 업데이트 후에도 유지되는지 확인한다.

- [ ] **Step 6: 물리 입력 확인의 증거 경계를 기록**

외부 Lenovo 키보드/마우스로 로컬 KeyBridge 동작, 원격 키/수정키/버튼/휠, 단일 커서, Esc/경계 복귀를 직접 확인한다. 자동화로 대체할 수 없는 항목은 미검증으로 숨기지 않고 사용자에게 짧은 확인 절차를 제공한다.
