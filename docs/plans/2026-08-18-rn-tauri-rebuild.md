# RN 멀티윈도우 뷰어 + Tauri 호스트 재구축 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 좌측 아키텍처를 우회한 `apps/host-mac`을 폐기하고, Tauri 호스트(제어 pull + 비디오 push)와 RN 뷰어(OS 멀티윈도우, 소스당 창)로 교체한다. macOS 우선, 90fps 기본/120fps 상한.

**Architecture:** 제어평면 = 뷰어가 호스트 TCP 7777에 접속해 라인 구분 JSON(Rustra 형태)으로 catalog/start/stop/status 요청. 비디오평면 = 기존 증명된 경로 재사용(`native/macos-capture-shim` SCK→VT H.264→TCP push → `native/android-viewer` TCP→AMediaCodec→Surface). Tauri 백엔드가 세션 매니저로 shim dylib을 FFI 다중 인스턴스 구동. 뷰어는 Expo RN에서 네이티브 모듈로 StreamActivity(멀티 인스턴스)를 띄운다.

**Tech Stack:** Tauri 2 (Rust: tokio, libloading, mdns-sd), React 19 + Vite, Expo 57 / RN 0.86, react-native-tcp-socket, Kotlin (NsdManager, Activity), Swift (ScreenCaptureKit/VideoToolbox shim 확장).

**Out of scope(후속 계획):** Windows DXGI 백엔드, 창(SCWindow) 단위 선택 UI, 오디오, 인증/암호화.

**설계 문서:** `docs/plans/2026-08-18-rn-tauri-rebuild-design.md`

---

## 배경 지식 (zero-context 엔지니어용)

- 저장소 루트가 Cargo workspace (`Cargo.toml` members: crates/*, native/*). **Tauri 크레이트는 workspace에서 분리**해야 의존성 충돌을 피한다 (`src-tauri/Cargo.toml`에 빈 `[workspace]` 테이블).
- 비디오 송신 프로토콜 (변경 금지): TCP, 패킷 = `[u32 BE len][payload]`. payload 3종: `"CFG"`(SPS/PPS), `"AU"`+pts, `0x46` 단일 프레임. 수신측 `native/android-viewer/src/jni.rs`가 재조립→AMediaCodec.
- shim은 현재 **글로벌 싱글턴** (`Shim.shared`), 720p30 고정. 다중 세션을 위해 핸들 테이블로 확장한다.
- `crates/control-contract`은 Rustra `Package`(명령 디스패치) 정의. 현재 구현 명령은 `addNumbers`(H02 증명용)뿐. 상태를 필요로 하는 신규 명령(get_catalog 등)은 rustra 패키지가 순수 함수만 지원하므로 **Tauri 백엔드의 TCP 서버가 serde 타입을 공유해 직접 디스패치**한다(타입은 control-contract에 두고 계약을 하나로 유지).
- 뷰어 포트: 각 StreamActivity 인스턴스가 `5000+n`에 TCP 리스너를 만들고(루스트), 호스트가 그 포트로 push. 호스트는 **제어 연결의 peer 주소**를 viewer IP로 사용(수동 IP 불필요, 단 UI 수동 시작 시 IP 입력).
- Android 멀티윈도우: manifest에 `launchMode="standard"` + `documentLaunchMode="always"` + `taskAffinity=""` + `PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI` (viewer-android shim에서 H05로 실증).
- viewer-expo의 `android/`는 prebuild 결과물이지만 저장소에 커밋되어 있으므로 직접 수정한다 (`expo prebuild` 재실행 시 덮어씀 주의).
- fps: shim이 `minimumFrameInterval=1/fps`, `ExpectedFrameRate=fps`, 비트레이트 `clamp(w*h*fps*0.07, 4Mbps, 24Mbps)`.

---

### Task 1: Tauri 2 스캐폴드 (`apps/host-desktop`)

**Files:**
- Create: `apps/host-desktop/src-tauri/Cargo.toml`, `apps/host-desktop/src-tauri/tauri.conf.json`, `apps/host-desktop/src-tauri/build.rs`, `apps/host-desktop/src-tauri/src/main.rs`, `apps/host-desktop/src-tauri/src/lib.rs`, `apps/host-desktop/src-tauri/icons/icon.png` (임의 1장), `apps/host-desktop/index.html`, `apps/host-desktop/src/main.tsx`, `apps/host-desktop/src/App.tsx`, `apps/host-desktop/vite.config.ts`
- Modify: `apps/host-desktop/package.json`, `apps/host-desktop/tsconfig.json`

**Step 1: package.json 교체**

```json
{
  "name": "@leftcar/host-desktop",
  "private": true,
  "version": "0.1.0",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "typecheck": "tsc --noEmit",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "react": "^19.1.0",
    "react-dom": "^19.1.0"
  },
  "devDependencies": {
    "@types/react": "~19.1.0",
    "@types/react-dom": "~19.1.0",
    "@vitejs/plugin-react": "^4",
    "typescript": "^5.7.0",
    "vite": "^6",
    "@tauri-apps/cli": "^2"
  }
}
```

**Step 2: `npm install` 실행** — Expected: lockfile 생성, exit 0.

**Step 3: src-tauri/Cargo.toml** (빈 `[workspace]`로 루트 workspace에서 분리)

```toml
[package]
name = "leftcar-host-desktop"
version = "0.1.0"
edition = "2021"

[workspace]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync"] }
libloading = "0.8"
mdns-sd = "0.11"
```

`tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Leftcar Host",
  "version": "0.1.0",
  "identifier": "dev.leftcar.host",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{ "title": "Leftcar Host", "width": 720, "height": 480 }],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "app", "icon": ["icons/icon.png"] }
}
```

`build.rs`: `fn main() { tauri_build::build() }`
`src/lib.rs`: `#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run() { tauri::Builder::default().run(tauri::generate_context!()).expect("tauri run"); }`
`src/main.rs`: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] fn main() { leftcar_host_desktop::run() }`

**Step 4: Vite/React UI 최소본** — `index.html`(`#root` + `/src/main.tsx`), `main.tsx`(createRoot), `App.tsx`(`Leftcar Host` 제목 + `invoke('ping')` 결과 표시), `vite.config.ts`(react 플러그인, `server.port: 1420, strictPort: true`, `clearScreen: false`, envPrefix `TAURI_`).

**Step 5: icons** — `mkdir -p src-tauri/icons && sips -z 128 128 <기존 스크린샷이나 임의 png> src-tauri/icons/icon.png` (아무 128px PNG면 됨)

**Step 6: 빌드 검증** — `cd apps/host-desktop && npm run build` (vite build) exit 0, `cargo check` in src-tauri exit 0.

**Step 7: Commit** — `git add apps/host-desktop && git commit -m "feat(host): Tauri 2 scaffold for desktop host"`

---

### Task 2: 제어 계약 타입 (control-contract)

**Files:**
- Modify: `crates/control-contract/src/host.rs` (신규 섹션 추가)
- Test: `crates/control-contract/src/host.rs` 내 `#[cfg(test)]`

**Step 1: 실패 테스트 작성** (host.rs 하단 tests 모듈에 추가)

```rust
#[cfg(test)]
mod stream_control_tests {
    use super::*;

    #[test]
    fn start_stream_input_roundtrips_camel_case() {
        let json = r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#;
        let v: StartStreamInput = serde_json::from_str(json).unwrap();
        assert_eq!(v.source_index, 0);
        assert_eq!(v.viewer_port, 5001);
        assert_eq!(v.fps, 90);
        let back = serde_json::to_string(&v).unwrap();
        assert!(back.contains("\"sourceIndex\""));
    }

    #[test]
    fn status_view_serializes() {
        let v = StatusView {
            sessions: vec![SessionView {
                session: 1,
                source_index: 0,
                source_name: "Main Display".into(),
                viewer_addr: "192.168.0.18:5001".into(),
                state: "running".into(),
                fps: 90,
                kbps: 12000,
            }],
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"sourceName\""));
    }
}
```

**Step 2: 실행해 실패 확인** — `cargo test -p control-contract stream_control` → FAIL (`StartStreamInput` 없음).

**Step 3: 타입 구현** (host.rs의 `ExportDiagnosticsOutput` 뒤에 추가. `#[command]` 없음 — Tauri 백엔드가 직접 디스패치, 이유는 계약 문서화)

```rust
// -- v1 stream control (docs/plans/2026-08-18-rn-tauri-rebuild-design.md) -----
// 상태를 갖는 명령이라 rustra Package(순수 함수) 대신 Tauri 제어 서버가
// 디스패치한다. 타입은 이 크레이트에 두어 계약을 하나로 유지.

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub displays: Vec<DisplayInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamInput {
    pub source_index: u32,
    pub viewer_port: u16,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamOutput {
    pub session: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopStreamInput {
    pub session: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub sessions: Vec<SessionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session: u32,
    pub source_index: u32,
    pub source_name: String,
    pub viewer_addr: String,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
}
```

**Step 4: 통과 확인** — `cargo test -p control-contract` → PASS 전체.
**Step 5: Commit** — `feat(contract): v1 stream control types (catalog/start/stop/status)`

---

### Task 3: 제어 서버 (TCP JSON 디스패치, FakeBackend TDD)

**Files:**
- Create: `apps/host-desktop/src-tauri/src/control.rs`, `apps/host-desktop/src-tauri/src/backend.rs`

**Step 1: backend.rs — 캡처 백엔드 트레잇 + Fake**

```rust
use control_contract::host::{DisplayInfo, StatsInfo};
use std::sync::Arc;

/// Capture backend trait: macOS shim FFI 구현과 테스트용 Fake이 공유.
pub trait CaptureBackend: Send + Sync {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String>;
    fn start(&self, source_index: u32, ip: &str, port: u16, w: u32, h: u32, fps: u32) -> Result<u32, String>;
    fn stop(&self, handle: u32) -> Result<(), String>;
    fn stats(&self, handle: u32) -> Result<StatsInfo, String>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatsInfoRaw { pub frames: i64, pub bytes: i64, pub state: String, pub fps: u32, pub kbps: u32 }

/// 테스트용 인메모리 백엔드.
pub struct FakeBackend {
    pub displays: Vec<DisplayInfo>,
}
impl CaptureBackend for FakeBackend {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, String> { Ok(self.displays.clone()) }
    fn start(&self, _s: u32, _ip: &str, _p: u16, _w: u32, _h: u32, _f: u32) -> Result<u32, String> { Ok(7) }
    fn stop(&self, handle: u32) -> Result<(), String> { if handle == 7 { Ok(()) } else { Err("no such handle".into()) } }
    fn stats(&self, handle: u32) -> Result<StatsInfo, String> {
        if handle != 7 { return Err("no such handle".into()); }
        Ok(StatsInfo { frames: 100, bytes: 1_000_000, state: "running".into(), fps: 90, kbps: 12000 })
    }
}

pub type SharedBackend = Arc<dyn CaptureBackend>;
```

(Task 2에 `StatsInfo`도 추가한다 — `DisplayInfo` 뒤에:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatsInfo {
    pub frames: i64,
    pub bytes: i64,
    pub state: String,
    pub fps: u32,
    pub kbps: u32,
}
```

테스트: `StatsInfo` JSON이 `"frames"` 키를 갖는지 확인 1개.)

**Step 2: control.rs — 실패 테스트 먼저**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FakeBackend, SharedBackend};
    use control_contract::host::DisplayInfo;
    use std::sync::Arc;

    async fn setup() -> (ControlServer, std::net::SocketAddr) {
        let b: SharedBackend = Arc::new(FakeBackend { displays: vec![DisplayInfo { index: 0, name: "Main".into(), width: 1920, height: 1080 }] });
        let s = ControlServer::new(b);
        let addr = s.bind("127.0.0.1:0").await.unwrap();
        tokio::spawn(async move { s.run().await; });
        (ControlServer::new(b.clone()), addr) // 첫 반환값은 버림 — run이 소유
    }

    #[tokio::test]
    async fn catalog_start_status_stop_roundtrip() {
        let (_, addr) = setup().await;
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();

        // viewer 주소는 제어 연결 peer에서 온다
        let line = request(&mut sock, "getCatalog", "{}").await;
        assert!(line.contains("\"displays\""), "{line}");

        let line = request(&mut sock, "startStream", r#"{"sourceIndex":0,"viewerPort":5001,"width":1920,"height":1080,"fps":90}"#).await;
        assert!(line.contains("\"session\":1"), "{line}");

        let line = request(&mut sock, "getStatus", "{}").await;
        assert!(line.contains("\"state\":\"running\""), "{line}");

        let line = request(&mut sock, "stopStream", r#"{"session":1}"#).await;
        assert!(line.contains("\"ok\":true"), "{line}");
    }

    async fn request(sock: &mut tokio::net::TcpStream, cmd: &str, args: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        sock.write_all(format!("{{\"command\":\"{cmd}\",\"args\":{args}}}\n").as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            sock.read_exact(&mut byte).await.unwrap();
            if byte[0] == b'\n' { break; }
            buf.push(byte[0]);
        }
        String::from_utf8(buf).unwrap()
    }
}
```

**Step 3: 실패 확인** — `cargo test` (src-tauri) → 컴파일 에러.
**Step 4: 구현** — `ControlServer`: `SharedBackend` + `Mutex<HashMap<u32, Session>>` + 다음 세션 번호 counter. `bind(addr)` → TcpListener. `run()` → accept 루프, 각 연결마다: `viewer_addr = peer_addr()`, 라인 단위 JSON 파싱 → 디스패치:
- `getCatalog` → backend.list_displays → `CatalogView`
- `startStream` → `StartStreamInput` 파싱, viewer_addr의 IP와 `viewer_port`로 backend.start → 세션 등록 → `StartStreamOutput`
- `stopStream` → backend.stop + 세션 제거
- `getStatus` → 활성 세션들의 backend.stats → `StatusView`
- 응답: `{"ok":true,"result":<json>}` 또는 `{"ok":false,"error":"..."}` + `\n`
- `addNumbers`는 `control_contract`의 rustra `host_package().invoke_json`으로 위임 (H02 경로 보존)

`lib.rs`의 `run()`에 백엔드 주입 지점만 마련 (실제 FFI 백엔드는 Task 5):

```rust
pub fn run() {
    let backend: SharedBackend = Arc::new(FakeBackend { displays: vec![] }); // Task 5에서 FFI로 교체
    tauri::Builder::default()
        .setup(move |app| {
            let server = ControlServer::new(backend.clone());
            let addr = tauri::async_runtime::block_on(server.bind("0.0.0.0:7777"))?;
            println!("control server on {addr}");
            tauri::async_runtime::spawn(async move { server.run().await; });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run");
}
```

**Step 5: 통과 확인** — `cargo test` → PASS.
**Step 6: Commit** — `feat(host): TCP JSON control server with pluggable capture backend`

---

### Task 4: shim 다중 인스턴스 + fps 90/120 (CaptureShim.swift)

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift` (전면 재구성)
- Modify: `tools/capture_host.swift` (v2 심볼로 갱신)

**Step 1: Swift 재구성** — 핵심 변경 (기존 프로토콜·인코딩 로직은 그대로 재사용):

1. `Shim` 싱글턴 → `final class CaptureSession` (기존 Shim의 필드/메서드 이동, `targetPort` 등 인스턴스화).
2. 정적 레지스트리 + C ABI:

```swift
private let registryLock = NSLock()
private var registry: [UInt32: CaptureSession] = [:]
private var nextHandle: UInt32 = 1

@_cdecl("leftcar_capture_start_v2")
public func leftcarCaptureStartV2(ip: UnsafePointer<CChar>, port: UInt16, displayIndex: UInt32,
                                   width: UInt32, height: UInt32, fps: UInt32) -> UInt32 {
    // ip/port/display 선택 → CaptureSession 생성 → registry 등록 → 핸들 반환 (실패 0)
}
@_cdecl("leftcar_capture_stop_v2")
public func leftcarCaptureStopV2(handle: UInt32) -> Int32
@_cdecl("leftcar_capture_stats_v2")
public func leftcarCaptureStatsV2(handle: UInt32) -> UnsafeMutablePointer<CChar> // JSON {frames,bytes,state,fps,kbps}
@_cdecl("leftcar_capture_list_displays")
public func leftcarCaptureListDisplays() -> UnsafeMutablePointer<CChar> // JSON [{index,name,width,height}]
@_cdecl("leftcar_capture_free_string")
public func leftcarCaptureFreeString(s: UnsafeMutablePointer<CChar>)
@_cdecl("leftcar_capture_last_error_v2")
public func leftcarCaptureLastErrorV2() -> UnsafePointer<CChar>
```

3. fps 파라미터화: `minimumFrameInterval = 1/fps`, `ExpectedFrameRate = fps`, `MaxKeyFrameInterval = max(1, fps/2)`,
   `AverageBitRate = clamp(w*h*fps*7/100, 4_000_000, 24_000_000)`, `DataRateLimits = [1.5×, 1]`.
4. 전송 실패(`send() <= 0`) 시 세션 자동 stop + `state="stopped"` (뷰어 창关闭→호스트 세션 정리).
5. stats의 fps/kbps: 최근 1초 윈도우 프레임/바이트 카운터로 계산 (tick 배열 대신 마지막 측정 시점 기준 단순 delta).
6. 기존 싱글턴 심볼(`leftcar_capture_start` 등)은 **삭제** — 사용처(capture_host.swift)를 v2로 갱신.

**Step 2: 빌드 검증**

```bash
cd native/macos-capture-shim
swiftc -O -shared Sources/CaptureShim.swift -o libleftcar_capture.dylib \
  -framework ScreenCaptureKit -framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework Foundation
nm -gU libleftcar_capture.dylib | grep _v2
```
Expected: `leftcar_capture_start_v2`, `stop_v2`, `stats_v2`, `list_displays`, `free_string`, `last_error_v2` 심볼 존재.

**Step 3: capture_host.swift 갱신** — dlsym으로 v2 심볼 로드, `--ip --port --fps --size` 인자, list-displays 모드 출력. `swiftc tools/capture_host.swift -o /tmp/capture_host` 로 컴파일 확인.

**Step 4: Commit** — `feat(shim): multi-instance handle table, fps/bitrate params, auto-stop on disconnect`

---

### Task 5: Tauri FFI 백엔드 연결

**Files:**
- Create: `apps/host-desktop/src-tauri/src/ffi.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (FakeBackend → FfiBackend), `apps/host-desktop/src-tauri/Cargo.toml` (`control-contract` path 의존성)

**Step 1: Cargo.toml에 추가** — `control-contract = { path = "../../../crates/control-contract" }` (src-tauri는 workspace 밖이지만 path 의존은 가능; 이 크레이트가 rustra git 의존을 당겨온다 — 최초 빌드 시간 주의)

**Step 2: ffi.rs 구현**

```rust
pub struct FfiBackend { lib: libloading::Library, path: std::path::PathBuf }
impl FfiBackend {
    pub fn new(dylib: &std::path::Path) -> Result<Self, String> {
        // dylib 젹대경로: LEFTCAR_CAPTURE_DYLIB env 또는 ../../..(repo root)/native/macos-capture-shim/libleftcar_capture.dylib
        unsafe { libloading::Library::new(dylib) }.map_err(|e| format!("dlopen {dylib:?}: {e}"))
        // 심볼 6개를 미리 로드해 존재 검증 (없으면 Err)
    }
}
// unsafe extern "C" 시그니처 6개 대응하여 CaptureBackend 구현.
// list_displays: JSON 파싱 serde_json::from_str
// stats: JSON → StatsInfo
```

테스트: dylib이 없는 경로 → `Err`에 `dlopen` 포함되는지 1개 (실 dylib 테스트는 Task 12 수동).

**Step 3: lib.rs 교체** — `FfiBackend::new(...)` 실패 시 앱 창에 오류 표시하며 FakeBackend로 폴백(개발 편의), 성공 시 실제 사용.

**Step 4: 검증** — `cargo check && cargo test` PASS. `cargo tauri dev`로 앱 기동, 콘솔에 `control server on 0.0.0.0:7777` 확인.

**Step 5: Commit** — `feat(host): FFI backend driving capture shim dylib`

---

### Task 6: mDNS 광고 + Tauri UI 상태판

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (mdns-sd 등록), `apps/host-desktop/src-tauri/src/control.rs` (상태 스냅샷 공유)
- Modify: `apps/host-desktop/src/App.tsx`, Create: `apps/host-desktop/src/control.ts`

**Step 1: lib.rs setup에** — `let mdns = mdns_sd::ServiceDaemon::new()?; mdns.register(mdns_sd::ServiceInfo::new("_leftcar._tcp.local.", "leftcar-host", host_local_ip()?, 7777, None))?;` (host_local_ip: UDP 소켓 8.8.8.8 connect 트릭으로 인터페이스 IP 획득)

**Step 2: UI 상태판** — 1초 폴링으로 활성 세션 테이블(소스, 뷰어 주소, fps, kbps, 상태) 표시. `control.rs`에 `pub fn snapshot(&self) -> StatusView` 추가(응답 재사용). Tauri command `get_status` + 프론트 `invoke`. `hostState.ts`의 `trayStatus/canStopAll` 재사용해 상단 배너 텍스트 구성.

**Step 3: 검증** — `cargo tauri dev`: 창에 "Leftcar" 배너 + 빈 세션 테이블. `dns-sd -B _leftcar._tcp local.` 실행 시 서비스 발견 출력.

**Step 4: Commit** — `feat(host): mDNS advertise + live session status UI`

---

### Task 7: 안드로이드 jni.rs — 포트 파라미터화

**Files:**
- Modify: `native/android-viewer/src/jni.rs`

**Step 1:** `spawn_live_stream_renderer(instance_str, surface_window, port: u16)` — 포트를 인스턴스 문자열 해킹(`src-1`→5001) 대신 인자로. 기존 `leftcar_jni_attach`는 `5000`으로 위임(하위호환).

```rust
#[no_mangle]
pub extern "C" fn leftcar_jni_attach_port(
    state: StatePtr, instance_c: *const c_char, surface: *mut c_void, port: u16,
) -> i32 {
    // leftcar_jni_attach와 동일 골격 + spawn_live_stream_renderer(instance_str, surface, port)
}
```

**Step 2: 크로스 컴파일 검증** — `cargo build -p android-viewer --target aarch64-linux-android --release` exit 0 (실기기 검증은 Task 10).

**Step 3: Commit** — `feat(viewer): explicit port for stream renderer attach`

---

### Task 8: viewer-expo — StreamActivity + 시작 모듈 + manifest

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`, `StreamNative.kt`, `StreamLauncherModule.kt`, `StreamLauncherPackage.kt`
- Modify: `apps/viewer-expo/android/app/src/main/AndroidManifest.xml`, `MainApplication.kt`, `android/app/build.gradle` (jniLibs에 libleftcar_viewer.so 복사 태스크)

**Step 1: StreamNative.kt** — viewer-android의 `ViewerNative.kt`를 그대로 복사하되 `attachSurface`에 port 오버로드 추가:

```kotlin
object StreamNative {
    init { System.loadLibrary("leftcar_viewer") }
    external fun start(): Long
    external fun attachSurfacePort(state: Long, instanceId: String, surface: Surface, port: Int): Int
    external fun surfaceChanged(state: Long, instanceId: String, width: Int, height: Int): Int
    external fun detachSurface(state: Long, instanceId: String): Int
    external fun release(state: Long, instanceId: String): Int
}
```

(jni.rs의 `leftcar_jni_attach_port`가 kotlinx 없이 C ABI로 노출되므로, ViewerNative 스타일 `external fun`이 JNI 네이밍(`Java_...`)과 맞도록 **주의**: 기존 shim은 `System.loadLibrary`+`external`이 아니라 Kotlin에서 `external fun` 선언이 JNI 규약을 탄다 — viewer-android의 ViewerNative가 이미 이 방식으로 동작했으므로 동일 구조 유지. 함수명은 JNI 시그니처에 맞춰 `Java_dev_leftcar_viewer_stream_StreamNative_attachSurfacePort`로 jni.rs에 등록하거나 기존 C-ABI 진입점을 Kotlin `external`에서 부르는 래퍼 구조를 그대로 재사용한다 — viewer-android shim 구조를 참조해 일치시킬 것.)

**Step 2: StreamActivity.kt** — `apps/viewer-android/.../StreamActivity.kt`를 복사해 다음 변경:
- `instanceId`, `port`(Int, 기본 5000), `title`(String)을 intent **extras**에서 수신
- 라벨/타이틀 = `title`
- `surfaceCreated`에서 `attachSurfacePort(nativeState, instanceId, holder.surface, port)`
- Wi-Fi lock 로직 그대로 유지

**Step 3: manifest** (viewer-expo의 AndroidManifest application 태그에 추가)

```xml
<property android:name="android.window.PROPERTY_SUPPORTS_MULTI_INSTANCE_SYSTEM_UI" android:value="true" />
<activity android:name=".stream.StreamActivity" android:exported="false"
    android:launchMode="standard" android:documentLaunchMode="always"
    android:taskAffinity="" android:resizeableActivity="true"
    android:excludeFromRecents="false" />
```
application에 `android:resizeableActivity="true"` 추가.

**Step 4: StreamLauncherModule.kt** — `@ReactMethod fun openStream(port: Int, title: String, promise: Promise)`: `instanceId = "src-${port}"`, Intent extras + `FLAG_ACTIVITY_NEW_TASK`, `currentActivity ?: reactApplicationContext`로 startActivity → promise.resolve(instanceId). `MainApplication.getPackages()`에 `StreamLauncherPackage()` 등록.

**Step 5: build.gradle jniLibs** — `android { sourceSets["main"].jniLibs.srcDir("src/main/jniLibs") }` + 태스크:

```gradle
task copyViewerSo(type: Copy) {
    from rootProject.file("../../../target/aarch64-linux-android/release/libleftcar_viewer.so")
    into "src/main/jniLibs/arm64-v8a"
}
preBuild.dependsOn copyViewerSo
```

**Step 6: 빌드 검증** — `cargo build -p android-viewer --target aarch64-linux-android --release` 후 `cd apps/viewer-expo && npx expo run:android` (기기 연결 필요 — 없으면 `./gradlew :app:assembleDebug`로 컴파일만 검증).

**Step 7: Commit** — `feat(viewer-expo): multi-instance StreamActivity + launcher module`

---

### Task 9: RN UI — 연결/카탈로그/열기 + 제어 클라이언트

**Files:**
- Create: `apps/viewer-expo/src/control.ts`, `apps/viewer-expo/app/host.tsx`, `apps/viewer-expo/app/catalog.tsx`
- Modify: `apps/viewer-expo/package.json` (`react-native-tcp-socket`), `apps/viewer-expo/app/index.tsx` (네비게이션 진입)

**Step 1: 의존성** — `npm install react-native-tcp-socket` 후 `npx expo run:android`로 네이티브 빌드에 포함.

**Step 2: control.ts** — 라인 JSON 프로토콜 클라이언트:

```ts
import TcpSocket from "react-native-tcp-socket";
export type ControlClient = {
  request<T>(command: string, args: unknown): Promise<T>;
  close(): void;
};
export function connect(host: string, port = 7777): Promise<ControlClient> {
  // connect → 요청마다 {"command","args"}\n 송신, 라인 버퍼로 {"ok":true,"result"} 수신 resolve
  // ok:false → reject(error), 소켓 에러 → 모든 pending reject
}
```

단위 테스트는 생략(디바이스 I/O) — Task 12 e2e에서 검증. `npm run typecheck`는 통과해야 함.

**Step 3: host.tsx** — IP 입력 + 연결 버튼 (v1 수동 입력; Task 10에서 NSD 목록 추가). 연결 성공 시 catalog.tsx로 이동하며 클라이언트를 전역 컨텍스트/모듈 싱글턴에 보관.

**Step 4: catalog.tsx** — `getCatalog` 결과 목록. 각 항목 "이 창으로 열기" 버튼:
1. 다음 포트 할당(모듈 카운터 5000+, AsyncStorage 저장)
2. `NativeModules.StreamLauncher.openStream(port, sourceName)` (창 생성 → 리스너 기동)
3. 300ms 대기 후 `startStream {sourceIndex, viewerPort: port, width:1920, height:1080, fps:90}`
4. 세션 목록에 추가, 닫기 버튼 → `stopStream {session}`

`app/index.tsx`는 기존 Hub 증명 화면을 유지하되 "호스트 연결" 버튼 → host.tsx (expo-router `Stack`).

**Step 5: typecheck** — `cd apps/viewer-expo && npm run typecheck` exit 0.

**Step 6: Commit** — `feat(viewer-expo): control-plane UI (connect, catalog, open window, start/stop stream)`

---

### Task 10: NSD 자동 발견

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/nsd/NsdModule.kt`, `NsdPackage.kt`
- Modify: `MainApplication.kt`, `apps/viewer-expo/app/host.tsx`

**Step 1: NsdModule.kt** — `NsdManager.discoverServices("_leftcar._tcp.", PROTOCOL_DNS_SD)` + `resolveService` → `DeviceEventEmitter.emit("leftcar:host-found", {name, host, port})`. `@ReactMethod startDiscovery/stopDiscovery`. `MainApplication`에 패키지 등록.

**Step 2: host.tsx** — 마운트 시 startDiscovery, 이벤트로 발견 목록 렌더, 탭 → connect. 언마운트 stopDiscovery. 권한: manifest에 이미 `ACCESS_WIFI_STATE`/`CHANGE_WIFI_MULTICAST_STATE` 있는지 확인 후 추가.

**Step 3: 빌드** — `npx expo run:android` 성공.

**Step 4: Commit** — `feat(viewer-expo): NSD host discovery`

---

### Task 11: 루스트 e2e (랩탑) — 제어 서버 ↔ fake 비디오

**Files:**
- Create: `apps/host-desktop/src-tauri/tests/control_e2e.rs`

**Step 1:** Task 3의 테스트를 확장 — `stopStream` 후 재`startStream` 세션 번호 증가, `addNumbers` 위임 응답 `42` 검증, 잘못된 커맨드 → `ok:false`.

**Step 2: `cargo test` PASS. Commit** — `test(host): control server e2e coverage`

---

### Task 12: 실기기 검증 + EVIDENCE

**Steps (수동 절차, EVIDENCE.md에 기록):**
1. `cd native/macos-capture-shim && swiftc ...` (Task 4 명령) → dylib 최신화
2. `cd apps/host-desktop && cargo tauri dev` — 화면 녹화 권한 TCC 승인
3. `cd apps/viewer-expo && npx expo run:android` — 실기기(Galaxy) 설치
4. 뷰어에서 NSD 또는 IP로 연결 → 카탈로그 → "열기" × 2 (두 OS 창)
5. 측정: 호스트 UI fps/kbps, `adb logcat -s LeftcarNative` 디코더 렌더 카운트
6. **E10 항목 추가** (`docs/EVIDENCE.md`): 단일 1080p90 안정성, 2스트림 동시, 창 닫기→세션 자동 stop 확인
7. Commit — `docs: E10 evidence for RN viewer + Tauri host`

**수용 기준 (설계 문서):** 단일 스트림 90fps 안정(120 시도), 2스트림 동시 안정 재생, 발견→카탈로그→창 2개 흐름 완주.

---

### Task 13: host-mac 폐기 + 정리

**Steps:**
1. `git rm -r apps/host-mac` (untracked이므로 `rm -rf` 후 커밋 목록에서 제외 확인) — 삭제 전 파일 스캔해서 참조 없는지 확인: `grep -rn "host-mac" --include="*.md" --include="*.sh" --include="*.json" .`
2. `native/macos-capture-shim/libleftcar_capture.dylib` `.gitignore`에 추가 (빌드 산출물)
3. `tools/capture_host.swift`가 여전히 동작하는지 재확인
4. Commit — `chore: remove apps/host-mac prototype (superseded by Tauri host)`

---

## 실행 순서 의존성

- Task 1→2→3→5→6 순차 (호스트 코어)
- Task 4는 2/3과 병렬 가능, 5 이전에 완료 필요
- Task 7→8→9→10 순차 (뷰어), 8은 7에 의존
- Task 11은 3 직후 가능, Task 12는 전부 완료 후, Task 13 마지막
