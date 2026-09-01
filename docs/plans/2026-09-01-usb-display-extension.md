# USB 화면 확장 3종 구현 계획

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 스트림 카드에 USB/Wi-Fi 배지를 추가하고, Host "창이 안 뜸" 원인(panic·숨김 동작)을 개선하며, 문제 해결 가이드를 확장하고, BetterDisplay 가상 디스플레이 옵트인 실험과 USB 물리 검증 문서를 제공한다.

**Architecture:** 배지는 이미 존재하는 `ActiveStream.mediaTransport` 데이터를 UI에 노출하는 것만으로 완성된다. Host 창 개선은 Tauri 시작 경로의 panic을 오류 대화상자(rfd)로 교체하고 launch show를 보강한다. 가상 디스플레이는 `betterdisplaycli` 프로세스 실행을 감싼 옵트인 실험 커맨드로, 기존 캡처 경로를 무수정 재사용한다.

**Tech Stack:** React Native/Expo (viewer-expo), React (host-desktop), Rust/Tauri 2 (host-desktop src-tauri), vitest + cargo test, betterdisplaycli (서드파티, 번들 없음)

**설계 문서:** `docs/plans/2026-09-01-usb-display-extension-design.md`

**검증 명령 요약:**
- RN/TSX 변경 후: 저장소 루트에서 `npx -y react-doctor@latest . --verbose` (100/100 필수) + `bun run typecheck` + `bun run test`
- Host Rust 변경 후: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml` + `cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests -- -D warnings`
- 문서만 변경 시: 검토 후 커밋

---

## Milestone 1: 연결 상태 가시성 (배지 + 문제 해결 안내)

### Task 1: transport 배지 레이블 헬퍼 (TDD)

**Files:**
- Create: `apps/viewer-expo/src/transport-label.ts`
- Test: `apps/viewer-expo/src/transport-label.test.ts`

**Step 1: 실패 테스트 작성**

`apps/viewer-expo/src/transport-label.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { transportBadgeLabel } from "./transport-label";

describe("transportBadgeLabel", () => {
  it("usb이면 USB를 반환한다", () => {
    expect(transportBadgeLabel("usb")).toBe("USB");
  });

  it("udp이면 Wi-Fi를 반환한다", () => {
    expect(transportBadgeLabel("udp")).toBe("Wi-Fi");
  });

  it("tcp이면 Wi-Fi (TCP)를 반환한다", () => {
    expect(transportBadgeLabel("tcp")).toBe("Wi-Fi (TCP)");
  });

  it("adbTcp와 기타 값은 ADB를 반환한다", () => {
    expect(transportBadgeLabel("adbTcp")).toBe("ADB");
    expect(transportBadgeLabel("anything-else")).toBe("ADB");
  });
});
```

**Step 2: 테스트 실패 확인**

Run: `cd /Users/loopy/dev/ll3/leftcar && bun run test -- transport-label`
Expected: FAIL — `Cannot find module './transport-label'`

**Step 3: 최소 구현**

`apps/viewer-expo/src/transport-label.ts`:

```ts
import type { ResolvedTransport } from "./usb";

/**
 * Maps the control contract's media_transport value to a short badge label.
 * Unknown/legacy values fall back to "ADB" because only adbTcp predates the
 * typed transport set.
 */
export function transportBadgeLabel(transport: ResolvedTransport | string): string {
  const normalized = transport.trim().toLowerCase();
  if (normalized === "usb") return "USB";
  if (normalized === "udp") return "Wi-Fi";
  if (normalized === "tcp") return "Wi-Fi (TCP)";
  return "ADB";
}
```

**Step 4: 테스트 통과 확인**

Run: `bun run test -- transport-label`
Expected: PASS (4 tests)

**Step 5: 커밋**

```bash
git add apps/viewer-expo/src/transport-label.ts apps/viewer-expo/src/transport-label.test.ts
git commit -m "feat(viewer): transport 배지 레이블 헬퍼 추가"
```

### Task 2: 스트림 카드 배지 UI + 스타일

**Files:**
- Modify: `apps/viewer-expo/app/catalog.tsx` (ActiveStreamItem, 약 566-590행)
- Modify: `apps/viewer-expo/src/catalog-styles.ts` (streamPort 스타일 뒤)

**Step 1: 배지 스타일 추가**

`catalog-styles.ts`의 `streamPort` 스타일 뒤에 추가:

```ts
    transportBadge: {
      color: colors.textSecondary,
      fontSize: 10,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 4,
      paddingHorizontal: 5,
      paddingVertical: 1,
      overflow: "hidden",
    },
    streamSpecRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
```

**Step 2: ActiveStreamItem에 배지 렌더링**

`catalog.tsx`의 ActiveStreamItem을 수정. 임포트에 추가:

```ts
import { transportBadgeLabel } from "../src/transport-label";
```

렌더링 변경:

```tsx
        <View style={styles.streamSpecRow}>
          <Text style={styles.streamPort} numberOfLines={1}>
            {stream.width} × {stream.height} · {stream.fps} FPS
          </Text>
          <Text style={styles.transportBadge}>{transportBadgeLabel(stream.mediaTransport)}</Text>
        </View>
```

**Step 3: 타입 체크 + 테스트**

Run: `bun run typecheck && bun run test`
Expected: 모두 PASS

**Step 4: React Doctor**

Run: `npx -y react-doctor@latest . --verbose`
Expected: 100/100

**Step 5: 커밋**

```bash
git add apps/viewer-expo/app/catalog.tsx apps/viewer-expo/src/catalog-styles.ts
git commit -m "feat(viewer): 활성 스트림 카드에 USB/Wi-Fi transport 배지 표시"
```

### Task 3: 문제 해결 가이드에 "창이 안 뜸" 항목 추가 (i18n)

**Files:**
- Modify: `packages/ui-tokens/src/i18n.ts` (ko 157행 근처, en 370행 근처)

**Step 1: 한국어 host 섹션에 키 추가**

`troubleshootPermDesc`(157행) 다음에 추가:

```ts
      troubleshootHiddenWindow: "컴퓨터 앱 창이 안 떠요",
      troubleshootHiddenWindowDesc: "창 닫기는 종료가 아니라 숨기기입니다. macOS 메뉴 막대 우측 상단의 Leftcar 아이콘을 클릭한 뒤 'Leftcar Host 열기'를 누르세요.",
```

**Step 2: 영어 host 섹션에 키 추가**

`troubleshootPermDesc`(370행) 다음에 추가:

```ts
      troubleshootHiddenWindow: "Computer app window does not appear",
      troubleshootHiddenWindowDesc: "Closing the window hides it; the app keeps running. Click the Leftcar icon at the right end of the macOS menu bar, then choose 'Open Leftcar Host'.",
```

**Step 3: TranslationSchema 타입 자동 반영 확인**

i18n.ts가 스키마 유추형이라면 별도 수정 불필요. `bun run typecheck`으로 확인.

Run: `bun run typecheck`
Expected: PASS

**Step 4: 커밋**

```bash
git add packages/ui-tokens/src/i18n.ts
git commit -m "feat(i18n): 창 숨김 동작 문제 해결 항목 한/영 추가"
```

### Task 4: Host troubleshoot 모달 + Viewer 문제 해결 팁에 항목 렌더링

**Files:**
- Modify: `apps/host-desktop/src/App.tsx` (troubleshoot 모달, 약 423-430행 — troubleshootPerm 카드 뒤)
- Modify: `apps/viewer-expo/app/host.tsx` (troubleshoot 팁 목록)

**Step 1: Host 모달에 카드 추가**

App.tsx troubleshoot 모달의 troubleshootPerm 카드 뒤에 추가. lucide-react 임포트에 `AppWindow` 추가:

```tsx
          <div className="troubleshoot-card">
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13, color: "var(--text-primary)" }}>
              <AppWindow size={16} />
              <span>{t.host.troubleshootHiddenWindow}</span>
            </div>
            <p style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4, lineHeight: 1.5 }}>
              {t.host.troubleshootHiddenWindowDesc}
            </p>
          </div>
```

**Step 2: Viewer host.tsx에 팁 항목 추가**

`app/host.tsx`의 troubleshoot 항목 목록(troubleshootWifi/Ap/Manual 형태) 끝에 동일 항목 추가. `t.viewer.troubleshootHiddenWindow` 키도 i18n에 병행 추가(Step 3).

**Step 3: viewer 섹션 i18n 키 추가 (ko/en)**

viewer 섹션의 `troubleshootManualDesc` 다음에:

```ts
      troubleshootHiddenWindow: "컴퓨터 앱 창이 안 떠요",
      troubleshootHiddenWindowDesc: "창 닫기는 종료가 아니라 숨기기입니다. macOS 메뉴 막대 우측 상단의 Leftcar 아이콘 → 'Leftcar Host 열기'.",
```

```ts
      troubleshootHiddenWindow: "Computer app window does not appear",
      troubleshootHiddenWindowDesc: "Closing the window hides it. Use the Leftcar icon at the right end of the macOS menu bar → 'Open Leftcar Host'.",
```

**Step 4: 검증**

Run: `bun run typecheck && bun run test && npx -y react-doctor@latest . --verbose`
Expected: PASS + 100/100

**Step 5: 커밋**

```bash
git add apps/host-desktop/src/App.tsx apps/viewer-expo/app/host.tsx packages/ui-tokens/src/i18n.ts
git commit -m "feat(ui): Host/Viewer 문제 해결 가이드에 창 숨김 항목 렌더링"
```

### Task 5: Host 창 표시 보강 (launch show)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (setup hook, 약 82-118행)
- Modify: `apps/host-desktop/src-tauri/tauri.conf.json`

**Step 1: tauri.conf.json에 visible 명시**

windows[0]에 추가:

```json
        "visible": true
```

**Step 2: setup hook에서 show 보강**

`lib.rs` setup hook의 tray build 뒤(`.build(app)?;` 다음)에 추가:

```rust
            // The dashboard window can lose its first-show race on slow
            // AppKit startups. Show+focus defensively; users reported the
            // app appearing to "not launch" when only the tray existed.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
```

**Step 3: 테스트 + clippy**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml && cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests -- -D warnings`
Expected: PASS

**Step 4: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/lib.rs apps/host-desktop/src-tauri/tauri.conf.json
git commit -m "fix(host): 시작 시 메인 창 표시 보강 (창이 안 뜸 보고 대응)"
```

### Task 6: panic 제거 — 오류 대화상자 + 정상 종료

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (run() 35-50행)
- Modify: `apps/host-desktop/src-tauri/Cargo.toml` (rfd 의존성 추가)

**Step 1: rfd 의존성 추가**

`apps/host-desktop/src-tauri/Cargo.toml` `[dependencies]`에:

```toml
rfd = "0.15"
```

**Step 2: 오류 대화상자 헬퍼 추가**

`lib.rs`에 추가:

```rust
/// Report a fatal startup failure without panicking. A message dialog is the
/// only way to reach users when no window exists yet; without it the app
/// exits silently and users report "the app does not launch".
fn fatal_startup_error(message: String) -> ! {
    eprintln!("Leftcar Host startup failed: {message}");
    let _ = rfd::MessageDialog::new()
        .set_title("Leftcar Host")
        .set_level(rfd::MessageLevel::Error)
        .set_description(&message)
        .show();
    std::process::exit(1);
}
```

**Step 3: panic 2곳 교체**

`run()`의 unwrap_or_else 교체:

```rust
    let backend = platform_backend().unwrap_or_else(fatal_startup_error);
```

```rust
    let (control_listener, control_port) =
        bind_control_listener().unwrap_or_else(fatal_startup_error);
```

메시지 품질을 위해 `platform_backend` 오류 문자열과 `bind_control_listener` 오류에 컨텍스트 포함(기존 문자열 유지 + "다른 Leftcar Host가 실행 중일 수 있습니다" 힌트는 bind 실패 메시지에 추가).

**Step 4: 테스트 + clippy**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml && cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests -- -D warnings`
Expected: PASS

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/lib.rs apps/host-desktop/src-tauri/Cargo.toml
git commit -m "fix(host): 시작 실패 시 panic 대신 오류 대화상자 표시"
```

---

## Milestone 2: BetterDisplay 가상 디스플레이 (옵트인 실험)

### Task 7: ADR-0005 작성

**Files:**
- Create: `docs/decisions/0005-virtual-display-via-betterdisplay-cli.md`

**Step 1: ADR 작성**

```markdown
# ADR-0005: 가상 디스플레이는 BetterDisplay CLI 옵트인 실험으로 제공

날짜: 2026-09-01
상태: Accepted
관련: ADR-0003 (창 스트림 우선), R-015 (가상 디스플레이 scope 팽창 위험)

## 상태 컨텍스트

R-015는 가상 디스플레이를 v1 논골로 지정했다. 자체 구현은 DriverKit 기반 신규
프로젝트로 "큰 새 프로젝트"다. 사용자 요구로 "창을 드래그해 옮길 수 있는 진짜
확장 모니터" 경험이 필요해졌다.

## 결정

- 가상 디스플레이는 BetterDisplay(서드파티, 유료 Pro 기능 포함)의
  `betterdisplaycli`를 프로세스 실행으로 감싸는 **옵트인 실험**으로 제공한다.
- BetterDisplay를 번들하지 않는다. 설치 감지 후 미설치 시 안내+링크만 제공.
- macOS 전용. Windows는 범위 밖.
- 기본값은 꺼짐. 정식 승격은 실기기 검증 후 별도 결정.

## 결과

- 긍정: 기존 list_displays/캡처/스트리밍 경로를 무수정 재사용. 구현 소규모.
- 부정: 서드파티 앱/라이선스 의존. CLI 계약 변경 시 대응 필요.
- 중립: ADR-0003(창 스트림 우선)은 유지된다 — 가상 디스플레이는 옵트인 보조
  경로다.
```

**Step 2: 커밋**

```bash
git add docs/decisions/0005-virtual-display-via-betterdisplay-cli.md
git commit -m "docs(adr): 가상 디스플레이 BetterDisplay CLI 옵트인 실험 결정"
```

### Task 8: createVirtualDisplay/removeVirtualDisplay Tauri 커맨드 (TDD)

> ADR-0005 참조: 아래 CLI 계약은 BetterDisplay 4.3.6 CLI 헬프 기준이다. 정밀 해상도 지정(aspectWidth/aspectHeight 대비 resolutionList)과 연결 계약은 T11 실기기 검증 대상이며, 검증 결과에 따라 인자 구성이 조정될 수 있다.

**Files:**
- Create: `apps/host-desktop/src-tauri/src/virtual_display.rs`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (mod 선언 + invoke_handler 등록)
- Modify: `apps/host-desktop/src-tauri/Cargo.toml`

**Step 1: 실패 테스트와 함께 모듈 작성**

`apps/host-desktop/src-tauri/src/virtual_display.rs`:

```rust
//! Opt-in BetterDisplay virtual display experiments (macOS only).
//!
//! Leftcar never bundles BetterDisplay. We only shell out to
//! `betterdisplaycli` when the user explicitly enables the experiment.

#[cfg(target_os = "macos")]
pub const CLI: &str = "betterdisplaycli";

/// Builds the argv for creating a virtual display. Unit-tested; the actual
/// process spawn is exercised only on a machine with BetterDisplay installed.
pub fn create_args(name: &str, aspect_w: u32, aspect_h: u32) -> Vec<String> {
    vec![
        "create".into(),
        "-devicetype=virtualscreen".into(),
        format!("-virtualscreenname={name}"),
        format!("-aspectWidth={aspect_w}"),
        format!("-aspectHeight={aspect_h}"),
    ]
}

/// Builds the argv for connecting a virtual display. Unit-tested; the actual
/// process spawn is exercised only on a machine with BetterDisplay installed.
pub fn connect_args(name: &str) -> Vec<String> {
    vec![
        "set".into(),
        format!("-namelike={name}"),
        "-connected=on".into(),
    ]
}

#[cfg(target_os = "macos")]
pub fn create_virtual_display(name: &str, aspect_w: u32, aspect_h: u32) -> Result<String, String> {
    run_cli(create_args(name, aspect_w, aspect_h))?;
    run_cli(connect_args(name))
}

#[cfg(target_os = "macos")]
fn run_cli(args: Vec<String>) -> Result<String, String> {
    let output = std::process::Command::new(CLI)
        .args(&args)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "BetterDisplay CLI를 찾을 수 없습니다. BetterDisplay를 설치하고 설정에서 CLI 접근을 허용하세요.".to_string()
            } else {
                format!("betterdisplaycli 실행 실패: {error}")
            }
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "betterdisplaycli 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_matches_cli_contract() {
        assert_eq!(
            create_args("Leftcar Virtual", 16, 9),
            vec![
                "create".to_string(),
                "-devicetype=virtualscreen".to_string(),
                "-virtualscreenname=Leftcar Virtual".to_string(),
                "-aspectWidth=16".to_string(),
                "-aspectHeight=9".to_string(),
            ]
        );
    }

    #[test]
    fn connect_args_matches_cli_contract() {
        assert_eq!(
            connect_args("Leftcar Virtual"),
            vec![
                "set".to_string(),
                "-namelike=Leftcar Virtual".to_string(),
                "-connected=on".to_string(),
            ]
        );
    }
}
```

**Step 2: lib.rs에 모듈 등록**

`lib.rs` 상단 mod 선언에 추가:

```rust
#[cfg(target_os = "macos")]
pub mod virtual_display;
```

**Step 3: 테스트**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml virtual_display`
Expected: PASS

**Step 4: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/virtual_display.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): betterdisplaycli 가상 디스플레이 커맨드 코어 (macOS)"
```

### Task 9: Tauri 커맨드 노출 + 설정 UI 토글

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (invoke_handler + commands)
- Modify: `apps/host-desktop/src/App.tsx` (실험 섹션 토글)
- Modify: `packages/ui-tokens/src/i18n.ts` (토글 레이블 ko/en)

**Step 1: Tauri 커맨드 추가**

`lib.rs`:

```rust
#[tauri::command]
fn create_virtual_display(
    name: String,
    aspect_width: u32,
    aspect_height: u32,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        virtual_display::create_virtual_display(&name, aspect_width, aspect_height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (name, aspect_width, aspect_height);
        Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
    }
}
```

invoke_handler 목록에 `create_virtual_display` 추가.

**Step 2: i18n 토글 레이블 추가 (ko/en host 섹션)**

```ts
      virtualDisplayExperiment: "가상 디스플레이 (실험)",
      virtualDisplayHint: "BetterDisplay가 필요합니다. 설치 후 CLI 접근을 허용하세요.",
      virtualDisplayCreate: "16:9 가상 디스플레이 생성",
      virtualDisplayCreated: "가상 디스플레이 생성됨",
      virtualDisplayFailed: "생성 실패: {error}",
```

(en 버전 병행)

```ts
      virtualDisplayExperiment: "Virtual Display (Experiment)",
      virtualDisplayHint: "Requires BetterDisplay. Install it and allow CLI access in its settings.",
      virtualDisplayCreate: "Create 16:9 virtual display",
      virtualDisplayCreated: "Virtual display created",
      virtualDisplayFailed: "Failed: {error}",
```

**Step 3: App.tsx 설정 섹션에 실험 카드 추가**

기존 encoder 실험 선택 UI 패턴을 따라 카드 추가: 토글 + "생성" 버튼 + 결과/오류 표시. `invoke("create_virtual_display", { name: "Leftcar Virtual", aspectWidth: 16, aspectHeight: 9 })` 호출.

**Step 4: 검증**

Run: `bun run typecheck && bun run test && cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml && npx -y react-doctor@latest . --verbose`
Expected: PASS + 100/100

**Step 5: 커밋**

```bash
git add apps/host-desktop/src-tauri/src/lib.rs apps/host-desktop/src/App.tsx packages/ui-tokens/src/i18n.ts
git commit -m "feat(host): 가상 디스플레이 실험 토글과 Tauri 커맨드 노출"
```

### Task 10: removeVirtualDisplay + 물리 검증 기록

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/virtual_display.rs` (remove 지원)

**Step 1: remove_args 추가 + 테스트**

```rust
pub fn remove_args(name: &str) -> Vec<String> {
    vec!["discard".into(), format!("-virtualscreenname={name}")]
}
```

테스트 추가:

```rust
    #[test]
    fn remove_args_matches_cli_contract() {
        assert_eq!(
            remove_args("Leftcar Virtual"),
            vec!["discard".to_string(), "-virtualscreenname=Leftcar Virtual".to_string()]
        );
    }
```

**Step 2: 실행부 + Tauri 커맨드 등록**

create와 동일 패턴의 `remove_virtual_display(name)` + `run_cli` 재사용. lib.rs 커맨드 등록.

**Step 3: 검증 + 커밋**

Run: `cargo test --manifest-path apps/host-desktop/src-tauri/Cargo.toml && cargo clippy --manifest-path apps/host-desktop/src-tauri/Cargo.toml --tests -- -D warnings`
Expected: PASS

```bash
git add apps/host-desktop/src-tauri/src/virtual_display.rs apps/host-desktop/src-tauri/src/lib.rs
git commit -m "feat(host): 가상 디스플레이 제거 커맨드 추가"
```

---

## Milestone 3: USB 물리 검증 문서

### Task 11: usb-physical-validation.md 작성

**Files:**
- Create: `docs/usb-physical-validation.md`

**Step 1: 문서 작성**

설계 문서의 "설계 3" 내용을 실행 가능한 절차로 전개: 사전 조건 체크리스트(데이터 케이블/허브 금지/폰 USB 구성/APK 버전), 검증 절차 5단계(AOAP 핸드셰이크 기록 → failover 시간 측정 → 자동 복귀 → 60분 soak → 인텐트 경로), 결과 기록 템플릿(EVIDENCE.md 스타일), 실패 시 진단 가이드(증상→확인 순서 4종).

**Step 2: README 문서 인덱스에 추가**

README 문서 섹션에 한 줄 추가:

```markdown
- [USB 물리 검증 절차](docs/usb-physical-validation.md)
```

**Step 3: 커밋**

```bash
git add docs/usb-physical-validation.md README.md
git commit -m "docs: USB AOAP 물리 검증 게이트(T11) 절차와 진단 가이드"
```

### Task 12: EVIDENCE.md 갱신 + 최종 검증

**Files:**
- Modify: `docs/EVIDENCE.md`

**Step 1: EVIDENCE.md에 구현 기록 추가**

"USB AOAP 전송" 섹션 뒤에 구현 기록 섹션 추가: 배지·창 표시 개선·문제해결 항목·가상 디스플레이 실험·검증 문서가 무엇을 증명하고 무엇을 증명하지 않는지(E3 대비 물리 게이트 미수행) 기록.

**Step 2: 전체 검증 스위트**

Run:
```bash
bun install
bun run typecheck
bun run test
bun run test:contract
bun run test:architecture
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
npx -y react-doctor@latest . --verbose
```
Expected: 모두 PASS + React Doctor 100/100

**Step 3: 커밋**

```bash
git add docs/EVIDENCE.md
git commit -m "docs: USB 화면 확장 3종 구현 증거 기록"
```
