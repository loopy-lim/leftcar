# Viewer-Driven Display Sizing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 태블릿이 자기 화면 메트릭을 호스트에 알려 가상 화면 크기를 자동 매칭하고, 스트리밍 중에 실시간으로 조정하며, XR에서는 창 비율 프리셋을 지정한다.

**Architecture:** 뷰어(태블릿) 주도. 제어 채널 계약(control-contract)에 뷰어 디스플레이 메트릭 전달과 가상 화면 리사이즈 명령을 추가하고, 호스트는 `DisplayManager`에 신규 resize 연산(제거→재생성 기반)과 스트림 시작 시 자동 매칭을 구현한다. 뷰어 UI는 카탈로그에 "가상 화면 크기" 카드를 추가해 프리셋·scale·XR 비율을 선택한다.

**Tech Stack:** Rust (control-contract, host-desktop Tauri), TypeScript (viewer-expo), Kotlin (StreamLauncherModule/StreamActivity), Swift (cgvd-shim).

**Spec:** docs/plans/2026-09-06-viewer-display-sizing-design.md (사용자 승인 2026-09-06)

## Global Constraints

- 기존 legacy 명령 호출 호환성 유지: 새 계약 필드는 `#[serde(default)]`/optional로 추가.
- 스트림 재시작 없이 reconfigureStream 경로로 해상도 전환.
- 세로/가로 동일 픽셀 동등성, 짝수 픽셀 정렬, macOS 논리 범위(폭·높이 최소 640×480 수준, 기존 validate_dimensions 재사용) 유지.
- 실기 검증 없이 실측 통과 주장 금지. 미검증 항목은 문서에 명시.
- React 변경 후 루트 `npx -y react-doctor@latest . --verbose` 100/100, `bun run typecheck`, 관련 vitest 재실행.
- 각 Task는 독립 커밋. 테스트 → 실패 확인 → 구현 → 통과 확인 → 커밋(TDD).

### Task 1: 자동 매칭 순수 함수 (Rust)

Files:
- Create: `apps/host-desktop/src-tauri/src/display_matching.rs`
- Test: `apps/host-desktop/src-tauri/src/display_matching.rs` `#[cfg(test)]`
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (mod 선언)

인터페이스:
```rust
pub struct ViewerDisplayMetrics { pub physical_width: u32, pub physical_height: u32, pub density_dpi: u32 }
pub enum MatchedScale { Two, One }
pub struct MatchedDisplaySize { pub logical_width: u32, pub logical_height: u32, pub scale: MatchedScale }
/// 물리 px ÷ (dpi/160) ÷ 2가 논리 후보. 후보가 1280×720 미만이면 scale 1(물리 px ÷ (dpi/160))로 폴백,
/// 그래도 유효하지 않으면 None. 짝수 픽셀 정렬.
pub fn match_display_size(metrics: &ViewerDisplayMetrics) -> Option<MatchedDisplaySize>
```
- [ ] 세로/가로 동일 픽셀 동등성, 2800×1752/420dpi → 1344×836 scale 2, 저해상도 폴백 scale 1, 무효 입력 None 회귀 테스트 작성.
- [ ] 함수 구현. `cargo test -p leftcar-desktop 2>/dev/null || (cd apps/host-desktop/src-tauri && cargo test display_matching)`.
- [ ] 커밋: `feat(host): 태블릿 화면 메트릭 자동 매칭 순수 함수`

### Task 2: 제어 채널 계약 확장 (Rust)

Files:
- Modify: `crates/control-contract/src/host.rs` (StartStreamInput에 옵션 필드, 새 명령 ResizeVirtualDisplayInput/Output, resize_stream 재사용 확인)
- Test: 같은 파일 `mod stream_control_tests` (serde 라운드트립)

인터페이스:
```rust
// StartStreamInput에 추가 (legacy 호환: default, skip_serializing_if)
#[serde(default, skip_serializing_if = "Option::is_none")]
pub viewer_display: Option<ViewerDisplayMetricsMsg>, // physicalWidth/physicalHeight/densityDpi camelCase
#[serde(default, skip_serializing_if = "Option::is_none")]
pub virtual_display_id: Option<String>, // 이미 존재하는 관리 화면 재사용 요청

// 새 명령: 가상 화면 리사이즈 (뷰어 → 호스트)
pub struct ResizeVirtualDisplayInput { pub id: String, pub width: u32, pub height: u32, pub scale: u8 }
pub struct ResizeVirtualDisplayOutput { pub id: String, pub logical_width: u32, pub logical_height: u32, pub scale: u8, pub backing_width: u32, pub backing_height: u32 }
```
- [ ] camelCase 라운드트립·legacy JSON(필드 없음) 파싱 회귀 테스트 작성.
- [ ] 구조체 추가. `cargo test -p control-contract`.
- [ ] 커밋: `feat(contract): 뷰어 화면 메트릭 전달과 가상 화면 리사이즈 명령`

### Task 3: 호스트 DisplayManager.resize (Rust)

Files:
- Modify: `apps/host-desktop/src-tauri/src/display_management.rs` (resize 메서드), `apps/host-desktop/src-tauri/src/provider.rs` (CGVD·BetterDisplay 모드 재적용), `apps/host-desktop/src-tauri/src/lib.rs` (새 Tauri 명령 + 제어 채널 핸들러 연결), `apps/host-desktop/src-tauri/src/backend.rs`(Fake 관련이 있으면 정합)
- Test: `display_management.rs` `#[cfg(test)]`

동작: resize(id, w, h, scale) → 제공자별 모드 전환 시도 → 관측 논리/픽셀 검증 → record 갱신. BetterDisplay는 CLI 재생성(제거→create_hidpi_args)이 안전한 경로면 그렇게 하고, 화면 ID 유지가 가능하면 유지. CGVD는 stdin `RESIZE <w> <h> <scale>` 명령으로 apply(settings) 재적용(신규 프로토콜 메시지).
- [ ] 배치 계산 재사용(rect 유지)과 제거 실패 시 기존 화면 보존 회귀 테스트 작성.
- [ ] 구현 후 `cd apps/host-desktop/src-tauri && cargo check && cargo test display_management`.
- [ ] 커밋: `feat(host): 관리 가상 화면 리사이즈 — 모드 재적용과 소유권 유지`

### Task 4: cgvd-shim RESIZE 프로토콜 (Swift)

Files:
- Modify: `tools/cgvd-shim/Sources/cgvd-shim/main.swift` (stdin 루프에 RESIZE 명령), `tools/cgvd-shim/README.md`
- Test: 실기 필요 — 자동 검사는 빌드 성공으로 대체

동작: `RESIZE <width> <height> <scale>` 수신 → CGVirtualDisplaySettings 재적용 → pollRequestedMode → `RESIZED <w> <h> <pw> <ph>\n` 응답. 실패 시 `FAILED ...` 기존 규약 유지. provider.rs의 CGVD stdin 파서에 RESIZE 전송·응답 대기 추가(Task 3과 같은 커밋 범위로 병행 가능).
- [ ] `cd tools/cgvd-shim && swift build` 성공.
- [ ] 커밋: `feat(cgvd): RESIZE stdin 명령으로 가상 화면 모드 재적용`

### Task 5: 뷰어 메트릭 획득 + 시작 요청 전달 (Kotlin/TS)

Files:
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt` (getDisplayMetrics 신규 @ReactMethod: 실제 디스플레이 물리 px + densityDpi), `apps/viewer-expo/src/launch-stream.ts` (StartStreamArgs에 viewerDisplay 전달), `apps/viewer-expo/src/use-catalog-model.ts` (openDisplay에서 launcher로 메트릭 획득 후 전달), `apps/viewer-expo/src/control.ts` (타입)
- Test: `apps/viewer-expo/src/launch-stream.test.ts`(있으면 확장, 없으면 새로), `apps/viewer-expo/src/use-catalog-model.test.ts`(있으면 확장)

- [ ] TS 레벨: viewerDisplay가 있으면 startStream 요청에 포함되는지 vitest 회귀 테스트 작성.
- [ ] Kotlin: `WindowManager.maximumWindowMetrics`(API 30+) → `realDisplaySize`, 하위는 `DisplayMetrics`. densityDpi 포함.
- [ ] `bun run test`, `bun run typecheck`. Android: `cd apps/viewer-expo/android && ./gradlew :app:assembleDebug -q` (테스트는 이후 Task에서 통합).
- [ ] 커밋: `feat(viewer): 연결 시 태블릿 화면 메트릭을 호스트에 전달`

### Task 6: 호스트 자동 매칭 연결 — 스트림 시작 시 가상 화면 생성/재사용 (Rust)

Files:
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (제어 채널 start_stream 핸들러가 viewer_display를 받아 match_display_size 실행 → 확장 모드 소스(display index가 관리 화면인 경우)면 DisplayManager.add 재사용/생성), `apps/host-desktop/src-tauri/src/display_matching.rs` (Task 1 재사용)
- Test: `display_matching.rs` 통합 케이스, `cd apps/host-desktop/src-tauri && cargo test`

- [ ] 세션 종료 시 정리 정책: 자동 생성 화면도 기존 소유 ID 추적 유지(즉시 제거하지 않음)를 문서+테스트로 고정.
- [ ] `cargo check && cargo test`. 
- [ ] 커밋: `feat(host): 스트림 시작 시 뷰어 메트릭 자동 매칭으로 가상 화면 준비`

### Task 7: 태블릿 "가상 화면 크기" 카드 UI (TS/React)

Files:
- Create: `apps/viewer-expo/src/DisplaySizeCard.tsx`
- Modify: `apps/viewer-expo/app/catalog.tsx` (카드 삽입), `apps/viewer-expo/src/use-catalog-model.ts` (handleResizeVirtualDisplay: resizeVirtualDisplay 요청 → 스트림 target 갱신 → reconfigure), `apps/viewer-expo/src/control.ts` (명령 타입)
- Test: `apps/viewer-expo/src/DisplaySizeCard.test.tsx` 또는 모델 테스트 확장

프리셋: 태블릿 매칭(시작 시 저장된 메트릭)/1080p/1440p/4K/직접 입력(px) + scale(1x/2x). 적용 = `resizeVirtualDisplay` 성공 후 세션 target 갱신 → 기존 reconfigure 경로 재사용. 실패 시 오류 표시, 기존 크기 유지.
- [ ] 프리셋 계산 순수 함수(현재 크기→다음 후보)와 실패 유지 로직 vitest 테스트 작성.
- [ ] UI 구현. 루트에서 `npx -y react-doctor@latest . --verbose` 100/100 확인 후 `bun run typecheck && bun run test`.
- [ ] 커밋: `feat(viewer): 가상 화면 크기 카드 — 프리셋·scale·실시간 적용`

### Task 8: XR 창 비율 프리셋 (TS/Kotlin)

Files:
- Modify: `apps/viewer-expo/src/DisplaySizeCard.tsx` (XR 감지 시 비율 프리셋 UI로 전환), `apps/viewer-expo/src/launch-stream.ts` (setWindowAspectRatio optional 전파), `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt` (setWindowAspectRatio 신규 @ReactMethod: 활성 StreamActivity에 비율 전달), `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt` (ratio 저장·SpatialWindow 재적용, config-change 안전), `tools/architecture-check/ts.ts` (필요 시 import 예외)
- Test: `apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/stream/` (ratio→bounds 순수 함수), 카드 모델 vitest

비율 프리셋: 16:10 / 16:9 / 4:3 / 9:16(세로). Mac 가상 화면 해상도는 변경하지 않는다.
- [ ] Kotlin 순수 함수 테스트 + `./gradlew :app:testDebugUnitTest`.
- [ ] 카드 XR 분기 vitest. React Doctor 100/100.
- [ ] 커밋: `feat(viewer): XR 창 비율 프리셋 — SpatialWindow 재적용`

### Task 9: 통합 검사·빌드·문서

Files:
- Modify: `docs/sidecar-quality-validation.md` 또는 신규 `docs/viewer-display-sizing-validation.md`

- [ ] 전체: `bun run typecheck && bun run test && bun run test:architecture && npx -y react-doctor@latest . --verbose`, `cargo check/test`(host+contract), Swift shim 빌드, `./gradlew :app:assembleDebug`.
- [ ] 실기 항목(연결 필요): 자동 매칭 크기 확인, 스트리밍 중 프리셋 전환 지연 측정, XR 비율 전환, 장시간 안정. 미수행 시 미검증으로 명시.
- [ ] 커밋: `docs: 뷰어 주도 화면 크기 검증 기록 — 자동 검사 결과와 미검증 항목`
