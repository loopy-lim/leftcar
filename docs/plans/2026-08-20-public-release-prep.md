# Leftcar Public 전환 준비 구현 계획

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** MIT 라이선스 부여, QR 페어링 인증 도입, 위생 정리 후 git 히스토리에서 31GB 빌드 산출물을 제거해 저장소를 public 전환 가능하게 만든다.

**Architecture:** `crates/session`의 `PairingService`(단일 사용 OfferSecret + 6자리 확인 코드 + TTL 120s)를 Tauri 호스트의 TCP 컨트롤 서버에 연결한다. 모든 제어 명령은 32B 토큰을 요구하고, 토큰은 호스트 파일(0600)에 저장돼 재시작 후에도 유효하다. 미디어 평면은 뷰어가 기대하는 호스트 IP의 연결만 수락한다. 히스토리 재작성은 모든 코드 작업이 끝난 뒤 마지막에 한 번 수행한다.

**Tech Stack:** Rust (tokio, tauri 2, session crate), TypeScript/React (viewer-expo, expo-camera, expo-secure-store), Kotlin (shim만), git-filter-repo

**설계 문서:** `docs/plans/2026-08-20-public-release-prep-design.md`

---

## 검증 명령 (전 작업 공통)

```bash
cargo test --workspace                                # Rust 전체
cargo clippy --workspace --tests -- -D warnings
cd apps/host-desktop/src-tauri && cargo test          # 호스트 별도 lockfile
pnpm test && pnpm test:contract && pnpm test:architecture && pnpm typecheck
```

---

## Phase A — 라이선스 + 위생 정리

### Task 1: MIT LICENSE + 라이선스 필드

**Files:**
- Create: `LICENSE`
- Modify: `Cargo.toml` (workspace.package), `apps/host-desktop/src-tauri/Cargo.toml`, `package.json`, `apps/viewer-expo/package.json`, `apps/host-desktop/package.json`, `apps/viewer-android/package.json` (있으면)

**Step 1:** `LICENSE` 생성 (표준 MIT 전문, `Copyright (c) 2026 loopy-lim`)

**Step 2:** 루트 `Cargo.toml` `[workspace.package]`의 `license = "Proprietary"` → `"MIT"`. host-desktop `Cargo.toml` `[package]`에 `license = "MIT"` 추가

**Step 3:** 모든 `package.json`에 `"license": "MIT"` 추가

**Step 4:** `git ls-files | grep -i license`로 LICENSE 추적 확인 후 커밋: `chore: add MIT license`

### Task 2: 추적 파일 정리 + 패키지 매니저 단일화

**Files:**
- Delete from git: `apps/viewer-expo/android/app/debug.keystore`, `build/apk/classlist.txt`, `build/apk/sources.txt`, `apps/host-desktop/package-lock.json`, `apps/viewer-expo/package-lock.json`, `apps/viewer-expo/package.json.orig`
- Modify: `.gitignore`, `apps/host-desktop/src-tauri/tauri.conf.json`

**Step 1:** `git rm --cached`로 위 파일 untrack (debug.keystore는 `--cached`만 — 로컬 빌드에 필요)

**Step 2:** `.gitignore`에 추가: `*.keystore`, `**/package-lock.json`, `*.orig`

**Step 3:** `tauri.conf.json`의 `beforeDevCommand`/`beforeBuildCommand`를 `npm run dev`/`npm run build` → `pnpm dev`/`pnpm build`로 변경 (host-desktop package.json scripts에 dev/build 있는지 확인, 없으면 추가)

**Step 4:** `git ls-files | grep -E 'keystore|package-lock|classlist|sources.txt'` 빈 출력 확인. 커밋: `chore: untrack build artifacts, unify on pnpm`

### Task 3: .cargo/config.toml 로컬 경로 제거

**Files:**
- Modify: `.cargo/config.toml`, `.github/workflows/ci.yml` (android job), `README.md` (빌드 문서 — Task 19에서 본문 작성, 여기선 PATH 안내만)

**Step 1:** linker를 절대경로 → `aarch64-linux-android35-clang` (PATH lookup). rustflags 유지:

```toml
[target.aarch64-linux-android]
linker = "aarch64-linux-android35-clang"
rustflags = ["-C", "link-arg=-lmediandk", "-C", "link-arg=-llog", "-C", "link-arg=-landroid"]
```

**Step 2:** ci.yml의 android-compile job에 NDK toolchain bin 경로를 PATH에 추가 (`$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin` 또는 brew NDK 경로 — 기존 job 설정 확인 후 조정)

**Step 3:** `cargo check --target aarch64-linux-android -p viewer-core` 로컬 검증 (NDK PATH 넣고). 실패 시 PATH 문제이므로 README에 `export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin:$PATH"` 안내. 커밋: `chore: drop hardcoded NDK path from cargo config`

### Task 4: control.ts 죽은 코드 제거

**Files:**
- Modify: `apps/viewer-expo/src/control.ts:83`

**Step 1:** `const queue: Array<...> = [];` (선언 후 미사용) 제거. `pnpm typecheck` 통과 확인. 커밋: `refactor(viewer): remove dead queue var`

---

## Phase B — QR 페어링 (호스트 Rust)

### Task 5: 페어링 서버 모듈 (TDD)

**Files:**
- Create: `apps/host-desktop/src-tauri/src/pairing.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (mod 선언), `apps/host-desktop/src-tauri/Cargo.toml` (session, base64, dirs 의존성)

**Step 1 (의존성):** host-desktop Cargo.toml에 추가:

```toml
session = { path = "../../../crates/session" }
domain = { path = "../../../crates/domain" }
base64 = "0.22"
dirs = "5"
serde = { version = "1", features = ["derive"] }
```

**Step 2 (실패하는 테스트):** `pairing.rs`에 단위 테스트부터 작성 — 실제 `session::PairingService`를 구동하는 `PairingServer` 래퍼:

```rust
/// 실패 테스트 목록 (전부 먼저 작성):
/// - begin_pairing_creates_qr_payload_and_code: payload에 v/id/s/h/p 필드, code는 6자리
/// - pair_with_correct_secret_and_code_issues_token: approve 성공 → 토큰 반환, 기기 목록 추가
/// - pair_with_wrong_code_fails: CodeMismatch → Err, 기기 추가 없음
/// - three_failed_attempts_burn_offer: 3회 실패 후 정확한 시크릿/코드로도 Expired
/// - authorize_accepts_issued_token_and_rejects_others
/// - persisted_devices_survive_restart: 파일 저장소 → 새 PairingServer 로드 후 토큰 유효
/// - revoke_removes_token_and_persists
```

**Step 3:** 구현. 핵심 구조:

```rust
pub struct PairingServer {
    clock: std::sync::Mutex<Box<dyn session::Clock>>,  // 실제로는 PairingService 재생성 방지 위해 단일 Mutex<Inner>
    inner: std::sync::Mutex<Inner>,
    store_path: Option<std::path::PathBuf>,  // None = 메모리(테스트)
    fail_counts: HashMap<String, u32>,       // offer_id -> 실패 횟수
}

struct Inner {
    service: session::PairingService,
    paired: Vec<PairedDevice>,
    fingerprint: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    pub token_hex: String,   // 32B 토큰 hex
    pub paired_at: String,   // RFC3339
}
```

- `begin_pairing(host_ip, port) -> PairingSession { qr_payload: String, code: String, expires_at }` — `PairingService::begin_offer` + `take_secret_for_qr`로 payload 생성: `{"v":1,"id":"offer-…","s":"<base64url 32B>","h":"<ip>","p":7777}`
- `pair(offer_id, secret_b64, code, device_id, name) -> Result<String /*token_hex*/>` — `service.approve()` (DeviceId::from_raw(device_id)) 성공 시 `OfferSecret::from_random()`으로 32B 토큰 생성, `paired`에 push + persist. 실패 시 `fail_counts` 증가, ≥3이면 `service.cancel(offer_id)`
- `authorize(token_hex: &str) -> bool` — `session::constant_time_eq` (hex 디코딩 후) 또는 문자열 상수시간 비교
- `revoke(device_id)` / `list_devices()`
- persist: `store_path`가 있으면 JSON 직렬화 후 0600으로 저장 (`std::fs::write` + `std::os::unix::fs::PermissionsExt`). 앱 데이터 경로는 `dirs::data_dir()/leftcar-host/paired_devices.json`
- 로드: 새 생성자 `PairingServer::load_or_new(fingerprint, store_path)` — 파일 있으면 `paired` 복원

**Step 4:** `cargo test -p leftcar-host-desktop pairing` 통과 확인

**Step 5:** 커밋: `feat(host): pairing server wrapping session PairingService`

### Task 6: 컨트롤 서버 인증 게이트 (TDD)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/control.rs`, `apps/host-desktop/src-tauri/src/lib.rs`
- Test: `apps/host-desktop/src-tauri/tests/control_e2e.rs`

**Step 1 (기존 테스트가 깨지는 것부터):** `ControlServer::new(backend)` → `ControlServer::new(backend, pairing: Arc<PairingServer>)`. `control_e2e.rs`의 `spawn_server()` 헬퍼가 컴파일 실패하는 것으로 red 상태 확인

**Step 2 (인증 시나리오 테스트 추가):**

```
- unauthenticated_getcatalog_is_rejected: 토큰 없는 요청 → {"ok":false,"error":"unauthorized"} + 연결 종료
- pair_then_catalog_works: begin_pairing → pair(정확한 secret+code) → 토큰으로 getCatalog 성공
- wrong_token_is_rejected
- startstream_ignores_viewer_ips_uses_peer: viewer_ips에 임의 IP를 넣어도 peer IP로만 push (FakeBackend가 받은 ip 인자 검증)
```

**Step 3 (구현):**

- Envelope에 `#[serde(default)] token: Option<String>` 추가
- `handle_conn`: 파싱 후 `command != "pair"`이고 `!server.pairing.authorize(token)`면 `{"ok":false,"error":"unauthorized"}` 응답 후 `break` (연결 종료)
- `dispatch`에 `"pair"` 커맨드 추가: `{offerId, secret, code, deviceId, deviceName}` → `PairingServer::pair` → `{"token": "..."}`. 이떄 `viewer_ip` 기록
- `startStream`에서 `input.viewer_ips` 완전 무시 — `candidates = vec![viewer_ip.to_owned()]` 고정
- `pair` 실패 응답에 에러 문자열만 (서버 상태 누출 없음)

**Step 4:** `cargo test -p leftcar-host-desktop` 전체 통과 (기존 4개 e2e는 헬퍼에서 토큰 발급받도록 수정)

**Step 5:** 커밋: `feat(host): token-gate control server, wire pair command, ignore viewer_ips`

### Task 7: Tauri 페어링 UI (창 + QR + 기기 목록)

**Files:**
- Create: `apps/host-desktop/src/PairingPanel.tsx`
- Modify: `apps/host-desktop/src/App.tsx`, `apps/host-desktop/src-tauri/src/lib.rs`, `apps/host-desktop/package.json` (qrcode 의존성)

**Step 1:** host-desktop에 `qrcode` (+`@types/qrcode`) 의존성 추가

**Step 2:** lib.rs에 Tauri 명령 4개 추가 — `begin_pairing` (PairingServer에 접근; `app.manage(Arc<PairingServer>)`), `cancel_pairing`, `list_paired_devices`, `revoke_device`. 트레이 메뉴에 "기기 페어링…" 항목 추가 → `pairing` 레이블 웹뷰 창 (`WebviewUrl::App("index.html#/pairing".into())`, 420x560, show/quit 사이)

**Step 3:** `App.tsx`에서 `window.location.hash === "#/pairing"`이면 `<PairingPanel/>` 렌더. PairingPanel:
- 시작 버튼 → `invoke("begin_pairing")` → `{qrPayload, code, expiresAt}` → `QRCode.toDataURL(qrPayload)` 표시 + 6자리 코드 크게 + 남은 초 카운트다운 (2분)
- "페어링된 기기" 목록 (`invoke("list_paired_devices")`, 5초 폴링 — 페어링 성공 즉시 반영) + 기기별 "제거" 버튼 → `invoke("revoke_device")`
- 만료 시 "다시 생성" 버튼

**Step 4:** `pnpm --filter` 또는 apps/host-desktop에서 `pnpm typecheck`/`pnpm build` 통과. 커밋: `feat(host): pairing window with QR, verification code, device list`

---

## Phase C — 뷰어 페어링

### Task 8: 페어링 로직 + 토큰 저장 (TDD)

**Files:**
- Create: `apps/viewer-expo/src/pairing.ts`, `apps/viewer-expo/src/pairing.test.ts`
- Modify: `apps/viewer-expo/src/control.ts`, `apps/viewer-expo/src/session.ts`, `apps/viewer-expo/package.json`

**Step 1 (의존성):** viewer-expo에 `expo-secure-store`, `expo-camera` 추가 (pnpm --filter @leftcar/viewer-expo add)

**Step 2 (실패하는 테스트):** `pairing.test.ts` — `expo-secure-store`는 `vi.mock`:

```
- parseQrPayload: 유효 JSON → {id, secret, host, port}; v!=1 / 필드 누락 → null
- pairWithHost: 성공 시 토큰+deviceId 저장 후 반환
- pairWithHost: codeMismatch 에러 → 그대로 throw (UI가 안내)
- getStoredToken: 저장 없으면 null
- clearToken: 삭제 후 getStoredToken null
```

**Step 3 (구현):** `pairing.ts`:

```ts
export interface QrPayload { id: string; secret: string; host: string; port: number }
export function parseQrPayload(text: string): QrPayload | null
export function getDeviceId(): Promise<string>        // secure-store에 없으면 생성해 저장
export function getStoredToken(): Promise<string | null>
export async function pairWithHost(p: QrPayload, code: string): Promise<string>  // connect → {"command":"pair",args:{offerId,secret,code,deviceId,deviceName}} → 토큰 저장
export function clearToken(): Promise<void>
export function deviceName(): string  // expo-constants의 deviceName 또는 "Android 뷰어"
```

**Step 4:** `control.ts` — `connect()`가 token provider를 받도록: 모든 요청 엔벨로프에 `token` 포함 (`{"command","args","token"}`), 토큰 없으면 생략. 응답 `error === "unauthorized"`면 `ControlRequestError` kind `"unauthorized"` 추가 — 호출부가 재페어링 안내로 전환 가능. `session.ts`의 `connectHost`/`reconnectHost`가 `getStoredToken()`을 조회해 주입

**Step 5:** `pnpm test` 통과. 커밋: `feat(viewer): QR pairing logic with secure token storage`

### Task 9: 페어링 UI 화면 + 인증 실패 흐름

**Files:**
- Create: `apps/viewer-expo/app/pairing.tsx`
- Modify: `apps/viewer-expo/app/host.tsx`, `apps/viewer-expo/app/catalog.tsx`, `apps/viewer-expo/app/index.tsx`

**Step 1:** `app/pairing.tsx` — 두 단계 화면:
1. QR 스캔: `expo-camera`의 `CameraView` (barcodeScannerSettings qr). 스캔 성공 → payload 저장 후 2단계로
2. 확인 코드 입력: 6자리 입력 + "페어링" 버튼 → `pairWithHost` → 성공 시 이전 화면으로 복귀, 실패(코드 불일치) 재입력 안내

**Step 2:** `host.tsx` — 연결 후 최초 요청(getCatalog)이 `unauthorized`면 `/pairing`으로 push (QR에 host/port가 있으므로 자동으로 스캔만 하면 됨)

**Step 3:** `catalog.tsx` — `unauthorized` 감지 시 `/pairing` 안내. 토큰 무효화(호스트 revoke 후) 대응: `clearToken()` 후 안내

**Step 4:** typecheck 통과. 커밋: `feat(viewer): pairing screen with QR scan and code entry`

---

## Phase D — 미디어 평면 출발지 검사

### Task 10: JNI 피어 IP 검사 (TDD)

**Files:**
- Modify: `native/android-viewer/src/jni.rs`, `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt`, `.../stream/StreamLauncherModule.kt`, `.../stream/StreamActivity.kt`, `apps/viewer-expo/app/catalog.tsx`

**Step 1 (실패하는 테스트):** jni.rs에 순수 헬퍼 + 테스트:

```rust
/// Returns true when the peer address matches the expected host (strict IP
/// equality; the viewer dials the host for control, so the media connection
/// must come back from that exact address).
fn peer_allowed(peer: Option<std::net::SocketAddr>, expected_host: &str) -> bool {
    match (peer, expected_host.parse::<std::net::IpAddr>()) {
        (Some(addr), Ok(expected)) => addr.ip() == expected,
        _ => false,
    }
}

#[test] fn peer_allowed_matches_exact_ip_only() { /* 일치 허용, 불일치 거부, parse 실패 거부 */ }
```

**Step 2:** `spawn_live_stream_renderer`에 `expected_host: String` 파라미터 추가. accept 후 `if !peer_allowed(s.peer_addr().ok(), &expected_host) { log; drop; continue; }`

**Step 3:** JNI `attachSurfacePort` 시그니처에 `host: String` 추가 (port 뒤). `ViewerNative.kt` external 선언 갱신, `StreamLauncherModule.openStream(port, host, width, height, fps)` + intent extra `"host"`, `StreamActivity`에서 읽어 전달, `catalog.tsx`의 `openStream` 호출에 제어 연결 호스트 IP(`controlHost()`에서 `:port` 제거한 값) 전달

**Step 4:** `cargo test --workspace` (jni.rs 유닛테스트 포함) + `pnpm typecheck`. 커밋: `feat(viewer): media listener accepts only the paired host IP`

### Task 11: viewer_ips 제거 마무리

**Files:**
- Modify: `apps/viewer-expo/app/catalog.tsx`, `.../StreamLauncherModule.kt`, `crates/control-contract/src/host.rs`

**Step 1:** catalog.tsx에서 `getLocalAddresses` 호출/`viewer_ips` 전송 제거

**Step 2:** `StreamLauncherModule.kt`에서 `getLocalAddresses` 메서드와 `java.net.*` import 전부 삭제 (architecture allowlist 위반 해소)

**Step 3:** `control-contract/src/host.rs`의 `StartStreamInput.viewer_ips` 필드에 `#[serde(default, alias = "viewer_ips")]` + `#[deprecated]` 주석 — 서버는 이미 무시하므로 제거가 아닌 폐기 표기로 계약 호환 유지

**Step 4:** `pnpm test:contract && cargo test -p control-contract` 통과. 커밋: `refactor: drop viewer_ips redirection path`

---

## Phase E — CI + 문서

### Task 12: CI 강화

**Files:**
- Modify: `.github/workflows/ci.yml`, `tools/architecture-check/ts.ts`

**Step 1:** ci.yml ts 잡에 `pnpm typecheck` 추가

**Step 2:** `tools/architecture-check/ts.ts` — 규칙 2(viewer-no-input-commands), 규칙 3(Kotlin allowlist/정책)의 스코프를 `apps/viewer-android` → viewer-android + viewer-expo 양쪽으로 확장. viewer-expo의 `java.net` import는 Task 11에서 제거됐으므로 통과해야 함

**Step 3:** `pnpm test:architecture` 통과. 커밋: `ci: typecheck job + architecture rules cover viewer-expo`

### Task 13: 문서 갱신

**Files:**
- Modify: `README.md`, `docs/README.md`, `docs/07-security-privacy.md`, `docs/EVIDENCE.md`, `CONTRIBUTING.md`(Create)

**Step 1:** README: 상단에 MIT 배지/라이선스 언급, "빌드 방법" 섹션 (Rust/pnpm/Android SDK+NDK PATH/Tauri CLI, `pnpm i`, 검증 명령), "보안" 섹션 (QR 페어링 필수, 미디어 출발지 검사, **평문 TCP 한계와 TLS/PAKE 후속 계획 명시**), DESIGN.md 링크 추가

**Step 2:** docs/README.md "구현 상태: 시작 전" → "v0.1.0 구현, QR 페어링 인증 포함" 갱신. docs/07에 구현 현황 반영 (페어링 게이트 적용됨, TLS 미구현 명시)

**Step 3:** EVIDENCE.md에 E11 항목 추가: "QR 페어링 + 토큰 인증 — 구현/CI 검증, 실기기 페어링 흐름 확인은 대기"

**Step 4:** CONTRIBUTING.md (간결하게: 개발 환경, 검증 명령, PR 규칙). 커밋: `docs: security posture, build instructions, evidence E11`

---

## Phase F — 히스토리 재작성 + 공개

### Task 14: 전체 검증

**Step 1:** 전체 검증 명령 실행 — `cargo test --workspace`, clippy, host-desktop cargo test, `pnpm test`, `test:contract`, `test:architecture`, `typecheck`. 전부 green인지 출력으로 확인

### Task 15: 히스토리 재작성

**Step 1:** 백업: `git bundle create ../leftcar-backup-$(date +%Y%m%d).bundle --all`

**Step 2:** `git filter-repo` 설치 확인 (`which git-filter-repo` 없으면 `brew install git-filter-repo` — 사용자 승인 필요하면 요청)

**Step 3:** `git filter-repo --path apps/host-desktop/src-tauri/target --invert-paths` 실행

**Step 4:** 검증: `git count-objects -vH` (size가 MB 단위로 감소), `git log --oneline | wc -l` (커밋 수 유지 확인), `git rev-list --objects --all | grep src-tauri/target | wc -l` → 0

**Step 5:** 원격 재설정 후 force push: `git remote add origin …`, `git push --force origin main feat/rn-tauri-rebuild --tags`

**Step 6:** 기존 release 정리: `gh release delete host-v0.1.0 viewer-v0.1.0 --yes` + 태그 삭제 (재작성된 히스토리와 불일치)

**Step 7:** 커밋: (재작성 자체는 커밋 아님 — 완료 보고만)

### Task 16: public 전환 (사용자 확인 필요)

GitHub Settings → Danger Zone → Change visibility. 사용자에게 수행하거나 `gh api repos/loopy-lim/leftcar -X PATCH -f private=false` 실행 여부를 확인받는다. 새 릴리스(v0.1.1, 인증 포함)는 아티팩트 빌드 후 별도 진행.

---

## 완료 기준

- [ ] 모든 검증 명령 green
- [ ] 토큰 없는 컨트롤 요청 거부 (e2e로 증명)
- [ ] `.git` 크기 MB 단위
- [ ] LICENSE 존재, 모든 manifest에 MIT
- [ ] README에 보안 범위 명시
- [ ] force push 완료, stale release 제거
