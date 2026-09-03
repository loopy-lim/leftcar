# 커서 분리 (LCD1 위치 스트림) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 호스트 커서를 비디오에서 분리해, 옵트인한 뷰어가 제어 채널로 수신한 좌표를 로컬 렌더링한다.

**Architecture:** 호스트는 CGEvent Tap(listen-only)으로 커서 위치를 관측하고, 변경 시만 `LCD1` 상태 패킷을 제어 소켓으로 보낸다(2×FPS coalescing). 뷰어는 atomics에 최신 샘플을 보관하고 Kotlin이 Choreographer 폴링으로 오버레이를 이동한다. 옵트인은 뷰어→호스트 `LCDON`/`LCDOFF` 명령(기존 `IDR`/`BYE`와 같은 토큰 인증 프레임)으로 하며, 스트림 활성 동안만 capture `showsCursor=false`로 전환하고 항상 원복한다.

**Tech Stack:** Swift(macOS shim, ScreenCaptureKit/CoreGraphics), Rust(android-viewer 크레이트), Kotlin(StreamActivity/ViewerNative JNI), TypeScript(Expo 뷰어 프리퍼런스).

**설계 문서:** `docs/plans/2026-09-03-cursor-separation-design.md`

**와이어 포맷 (확정):**

```
LCD1 패킷 (호스트→뷰어, 제어 채널):
오프셋  크기  필드
0      4    magic "LCD1"
4      4    sequence (u32 BE, 전송 시마다 증가)
8      2    x (u16 BE, 정규화 0..=65535)
10     2    y (u16 BE, 정규화 0..=65535)
12     1    visibility (0=화면 밖/숨김, 1=표시)
13     1    shape (예약, 항상 0)
14     N    세션 토큰

LCDON / LCDOFF (뷰어→호스트): 기존 send_viewer_command와 동일 — 명령 바이트 + 토큰
```

**MVP 비목표:** 커서 모양 비트맵(shape 필드만 예약), splitVertical 세션 커서 스트림(호스트가 split에서는 LCD1을 보내지 않음), Galaxy XR 시스템 커서 상호작용.

**규칙:** 각 태스크 끝에 커밋. 커밋 메시지는 저장소 관례대로 한국어 컨벤셔널 커밋. Rust 변경 후 `cargo test -p android-viewer`와 `cargo clippy --workspace --tests -- -D warnings`. TSX/React 변경 후 Task 9에서 React Doctor 100/100 게이트.

---

### Task 1: Rust 커서 프로토콜 모듈 (`cursor_protocol.rs`)

**Files:**
- Create: `native/android-viewer/src/cursor_protocol.rs`
- Modify: `native/android-viewer/src/lib.rs` (모듈 선언 추가)

**Step 1: 실패하는 테스트 작성**

`native/android-viewer/src/cursor_protocol.rs`를 새로 만들고 다음 내용으로 시작한다 (테스트만, 구현은 아직):

```rust
//! Host-to-viewer cursor position plane (LCD1).
//!
//! Mirror of the LCI1 pointer plane: a lossy, newest-wins state stream.
//! The host sends a sample only when the cursor state changes, so a lost
//! datagram is healed by the next change and no reliability layer exists.

/// Wire magic for host→viewer cursor samples.
pub const CURSOR_MAGIC: &[u8; 4] = b"LCD1";
/// Host→viewer command that requests the cursor position stream.
pub const CURSOR_STREAM_ON: &[u8] = b"LCDON";
/// Host→viewer command that stops the cursor position stream.
pub const CURSOR_STREAM_OFF: &[u8] = b"LCDOFF";
/// Fixed payload width between the magic and the session token.
pub const CURSOR_SAMPLE_LEN: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSample {
    pub sequence: u32,
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}

pub fn parse_cursor_sample(packet: &[u8], token: &[u8]) -> Option<CursorSample> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(sequence: u32, x: u16, y: u16, visible: bool, token: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CURSOR_SAMPLE_LEN + token.len());
        bytes.extend_from_slice(CURSOR_MAGIC);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&x.to_be_bytes());
        bytes.extend_from_slice(&y.to_be_bytes());
        bytes.push(u8::from(visible));
        bytes.push(0);
        bytes.extend_from_slice(token);
        bytes
    }

    #[test]
    fn cursor_sample_round_trips_with_token_binding() {
        let token = b"session-token";
        let packet = encode(7, 0x1234, 0xabcd, true, token);
        assert_eq!(packet.len(), CURSOR_SAMPLE_LEN + token.len());
        assert_eq!(
            parse_cursor_sample(&packet, token),
            Some(CursorSample {
                sequence: 7,
                x: 0x1234,
                y: 0xabcd,
                visible: true,
            })
        );
        assert_eq!(parse_cursor_sample(&packet, b"wrong-token"), None);
    }

    #[test]
    fn truncated_or_foreign_packets_are_rejected() {
        let token = b"nonce";
        assert_eq!(parse_cursor_sample(&[], token), None);
        assert_eq!(parse_cursor_sample(&[0u8; 13], token), None);
        let packet = encode(1, 0, 0, false, token);
        assert_eq!(parse_cursor_sample(&packet[..packet.len() - 1], token), None);
        let mut foreign = encode(1, 0, 0, false, token);
        foreign[0] = b'X';
        assert_eq!(parse_cursor_sample(&foreign, token), None);
    }

    #[test]
    fn stream_commands_are_the_documented_bytes() {
        assert_eq!(CURSOR_STREAM_ON, b"LCDON");
        assert_eq!(CURSOR_STREAM_OFF, b"LCDOFF");
    }
}
```

`native/android-viewer/src/lib.rs`에 모듈 선언을 추가한다 (`pub mod input_protocol;` 아래):

```rust
pub mod cursor_protocol;
```

**Step 2: 테스트가 실패하는지 확인**

Run: `cargo test -p android-viewer cursor_protocol`
Expected: FAIL (`unimplemented!()` panic)

**Step 3: 최소 구현**

`parse_cursor_sample`을 구현한다 (`input_protocol.rs`의 `parse_input_status` 스타일):

```rust
pub fn parse_cursor_sample(packet: &[u8], token: &[u8]) -> Option<CursorSample> {
    if token.is_empty()
        || packet.len() != CURSOR_SAMPLE_LEN + token.len()
        || &packet[..4] != CURSOR_MAGIC
        || packet[CURSOR_SAMPLE_LEN..] != *token
    {
        return None;
    }
    Some(CursorSample {
        sequence: u32::from_be_bytes(packet[4..8].try_into().ok()?),
        x: u16::from_be_bytes(packet[8..10].try_into().ok()?),
        y: u16::from_be_bytes(packet[10..12].try_into().ok()?),
        visible: packet[12] != 0,
    })
}
```

**Step 4: 테스트 통과 확인**

Run: `cargo test -p android-viewer cursor_protocol`
Expected: PASS (3 tests)

**Step 5: 커밋**

```bash
git add native/android-viewer/src/cursor_protocol.rs native/android-viewer/src/lib.rs
git commit -m "feat(viewer): LCD1 커서 위치 패킷 파서 추가 — 토큰 인증과 최신값 수용 계약"
```

---

### Task 2: 뷰어 수신 경로 연결 (atomics + LCD1 파싱 + LCDON 전송)

**Files:**
- Modify: `native/android-viewer/src/jni.rs` (`RendererControl`에 커서 상태 추가)
- Modify: `native/android-viewer/src/renderer/single_session/runtime.rs` (초기화)
- Modify: `native/android-viewer/src/renderer/single_session/network.rs` (`consume_viewer_response`에 LCD1 파싱)
- Modify: `native/android-viewer/src/renderer/single_session/runtime/worker.rs` (토큰 수립 시 LCDON 전송)

**Step 1: `RendererControl`에 커서 상태 추가**

`native/android-viewer/src/jni.rs`의 `RendererControl` (`input_enabled: AtomicI8` 근처, 62행 부근)에 추가:

```rust
    // Cursor plane (LCD1). -1 = host has not opted in, 0 = opt-in without a
    // sample yet, 1 = samples flowing. x/y/sequence hold the newest sample.
    pub(crate) cursor_active: AtomicI8,
    pub(crate) cursor_x: AtomicU16,
    pub(crate) cursor_y: AtomicU16,
    pub(crate) cursor_visible: AtomicBool,
    pub(crate) cursor_sequence: AtomicU32,
    // Viewer-side opt-in flag: when set, LCDON is sent once the authenticated
    // control token is established (and re-sent after a same-window rebind).
    pub(crate) cursor_requested: AtomicBool,
```

두 생성자(`new_split` 113행 부근과 `runtime.rs`의 생성자)에 초기화를 추가:

```rust
            cursor_active: AtomicI8::new(-1),
            cursor_x: AtomicU16::new(0),
            cursor_y: AtomicU16::new(0),
            cursor_visible: AtomicBool::new(false),
            cursor_sequence: AtomicU32::new(0),
            cursor_requested: AtomicBool::new(false),
```

(jni.rs 상단 `use std::sync::atomic::{...}`에 `AtomicU16`, `AtomicU32`, `AtomicBool`이 이미 있거나 추가 필요 — 54행의 use 목록 확인)

**Step 2: `consume_viewer_response`에 LCD1 파싱 추가**

`native/android-viewer/src/renderer/single_session/network.rs` 상단 import에 추가:

```rust
    crate::cursor_protocol::{parse_cursor_sample, CursorSample},
```

`consume_viewer_response`의 `parse_input_status` 블록(145행) 뒤에 추가:

```rust
    if let Some(sample) = parse_cursor_sample(packet, token) {
        apply_cursor_sample(control, sample);
        return true;
    }
```

파일 하단에 헬퍼 추가:

```rust
fn apply_cursor_sample(control: &RendererControl, sample: CursorSample) {
    control.cursor_x.store(sample.x, Ordering::SeqCst);
    control.cursor_y.store(sample.y, Ordering::SeqCst);
    control.cursor_visible.store(sample.visible, Ordering::SeqCst);
    control.cursor_sequence.store(sample.sequence, Ordering::SeqCst);
    control.cursor_active.store(1, Ordering::SeqCst);
}
```

(`RendererControl`이 이 모듈에서 접근 가능한지 확인 — `network.rs`는 `single_session` 모듈 안이므로 `super::*` 경유로 이미 접근 가능. `Ordering` import 확인)

**Step 3: 토큰 수립 시 LCDON 전송**

`runtime/worker.rs`에서 LCH1 핸드셰이크 블록(524행 부근, `viewer_control_token.clear()`로 시작)의 `request_idr_debounced(...)` 호출 직후에 추가:

```rust
                if control_clone.cursor_requested.load(Ordering::SeqCst) {
                    send_viewer_command(&control_socket, peer, b"LCDON", &viewer_control_token);
                }
```

suspend 복구 경로(249행 부근 `*input_endpoint.lock().unwrap() = Some((peer, ...))` 직후)에도 동일한 3줄을 추가 — 재바인드 후 새 호스트 세션에 옵트인을 반복 알린다. `send_viewer_command`는 같은 모듈 트리의 `feedback.rs`에 있어 worker.rs에서 접근 가능한지 import로 확인한다.

**Step 4: 기존 회귀 확인**

Run: `cargo test -p android-viewer && cargo clippy -p android-viewer --tests -- -D warnings`
Expected: PASS, clippy 경고 0건

**Step 5: 커밋**

```bash
git add native/android-viewer/src
git commit -m "feat(viewer): LCD1 수신 경로와 LCDON 옵트인 전송 연결 — atomics 최신값 보관"
```

---

### Task 3: JNI exports (cursorState 폴링 + setCursorStream)

**Files:**
- Modify: `native/android-viewer/src/jni_exports/session_io.rs`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt`

**Step 1: `session_io.rs`에 두 export 추가**

파일 하단에 추가 (기존 `pack_stream_stats`의 i64 패킹 패턴 따름):

```rust
/// Pack the newest cursor sample for Choreographer-rate polling.
/// bits 0..15 x, 16..31 y, 32..61 sequence (low 30 bits), 62 reserved,
/// 63 visible. Returns -1 while the host has not opted in.
#[no_mangle]
pub extern "C" fn leftcar_jni_cursor_state(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1L as i64,
        };
        if control.cursor_active.load(Ordering::SeqCst) <= 0 {
            return -1L as i64;
        }
        let x = u32::from(control.cursor_x.load(Ordering::SeqCst)) as i64;
        let y = u32::from(control.cursor_y.load(Ordering::SeqCst)) as i64;
        let sequence = u64::from(control.cursor_sequence.load(Ordering::SeqCst)) & 0x3fff_ffff;
        let visible = i64::from(control.cursor_visible.load(Ordering::SeqCst));
        x | (y << 16) | (sequence << 32) | (visible << 63)
    });
    guard.unwrap_or(-1)
}

/// Record the viewer-side cursor stream opt-in. The flag is applied at the
/// next control-token establishment (attach or same-window rebind).
#[no_mangle]
pub extern "C" fn leftcar_jni_set_cursor_stream(instance_c: *const c_char, enabled: bool) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        control.cursor_requested.store(enabled, Ordering::SeqCst);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}
```

**Step 2: `ViewerNative.kt`에 선언 추가**

`inputStatus` 선언(91행) 근처에 추가:

```kotlin
    external fun cursorState(instanceId: String): Long
    external fun setCursorStream(instanceId: String, enabled: Boolean): Int
```

**Step 3: 빌드·회귀 확인**

Run: `cargo test -p android-viewer && cargo clippy -p android-viewer --tests -- -D warnings`
Expected: PASS

**Step 4: 커밋**

```bash
git add native/android-viewer/src/jni_exports/session_io.rs apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/shim/ViewerNative.kt
git commit -m "feat(viewer): 커서 상태 JNI 폴링과 옵트인 설정 export 추가"
```

---

### Task 4: Kotlin 커서 오버레이 (Choreographer 로컬 렌더)

**Files:**
- Create: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/CursorOverlayView.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt`
- Modify: `apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt` (intent extra)

**Step 1: `CursorOverlayView` 작성**

```kotlin
package dev.leftcar.viewer.stream

import android.app.Activity
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.view.Choreographer
import android.view.View
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Local cursor overlay fed by the host LCD1 position stream. Polls the
 * packed native sample at frame rate and moves itself; a -1 state means the
 * host never opted in, so the overlay stays fully hidden (old-host fallback).
 */
internal class CursorOverlayView(
    activity: Activity,
    private val instanceId: String,
) : View(activity) {
    private val choreographer = Choreographer.getInstance()
    private var running = false
    private var lastSequence = Long.MIN_VALUE
    private var lastVisible = false

    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xF0FFFFFF.toInt()
        style = Paint.Style.FILL
    }
    private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xFF0F172A.toInt()
        style = Paint.Style.STROKE
        strokeWidth = resources.displayMetrics.density * 1.5f
    }
    private val arrow = Path()

    init {
        isClickable = false
        isFocusable = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        elevation = resources.displayMetrics.density * 4f
        visibility = GONE
    }

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!running) return
            applyState(ViewerNative.cursorState(instanceId))
            choreographer.postFrameCallback(this)
        }
    }

    fun start() {
        if (running) return
        running = true
        choreographer.postFrameCallback(frameCallback)
    }

    fun stop() {
        running = false
        choreographer.removeFrameCallback(frameCallback)
    }

    private fun applyState(packed: Long) {
        if (packed == -1L) {
            if (lastVisible) {
                visibility = GONE
                lastVisible = false
            }
            return
        }
        val visible = packed < 0 // sign bit 63
        val x = (packed and 0xffffL).toInt()
        val y = ((packed ushr 16) and 0xffffL).toInt()
        val sequence = (packed ushr 32) and 0x3fff_ffffL
        if (!visible) {
            if (lastVisible) {
                visibility = GONE
                lastVisible = false
            }
            return
        }
        if (!lastVisible || sequence != lastSequence) {
            lastSequence = sequence
            lastVisible = true
            visibility = VISIBLE
            val parent = parent as? android.view.ViewGroup ?: return
            translationX = x / 65535f * (parent.width - width)
            translationY = y / 65535f * (parent.height - height)
        }
    }

    override fun onDraw(canvas: Canvas) {
        val density = resources.displayMetrics.density
        arrow.rewind()
        arrow.moveTo(2f * density, 2f * density)
        arrow.lineTo(2f * density, 18f * density)
        arrow.lineTo(6.5f * density, 14f * density)
        arrow.lineTo(9.5f * density, 20.5f * density)
        arrow.lineTo(12f * density, 19.2f * density)
        arrow.lineTo(9f * density, 12.8f * density)
        arrow.lineTo(14.5f * density, 12.5f * density)
        arrow.close()
        canvas.drawPath(arrow, fillPaint)
        canvas.drawPath(arrow, strokePaint)
    }
}
```

**Step 2: `StreamActivity`에 연결**

`StreamActivity.kt`:

1. 필드 추가 (`hud` 선언 근처):

```kotlin
    private var cursorOverlay: CursorOverlayView? = null
    private var localCursorEnabled: Boolean = false
```

2. `onCreate`의 `setContentView(surfaces.root)` 직후에 추가:

```kotlin
        localCursorEnabled = intent?.getBooleanExtra("localCursor", false) ?: false
```

3. `attachStableSurfaces`에서 `surfaceAttached = res == 0` 뒤 `if (surfaceAttached) {` 블록 안에 추가:

```kotlin
            enableCursorOverlay()
```

파일 하단(private 헬퍼 영역)에 추가:

```kotlin
    private fun enableCursorOverlay() {
        if (!localCursorEnabled) return
        val root = streamSurfaces?.root as? android.view.ViewGroup ?: return
        ViewerNative.setCursorStream(instanceId, true)
        val overlay = cursorOverlay ?: CursorOverlayView(this, instanceId).also { view ->
            root.addView(
                view,
                android.widget.FrameLayout.LayoutParams(
                    (18 * resources.displayMetrics.density).toInt(),
                    (22 * resources.displayMetrics.density).toInt(),
                ),
            )
            cursorOverlay = view
        }
        overlay.start()
    }
```

4. `rebindOnSameSurface`의 `if (result == 0)` 블록 안에 `enableCursorOverlay()` 추가 — 재바인드에서 새 호스트 세션에 LCDON 재전송.

5. `onDestroy`의 `hud?.stop()` 직전에 추가:

```kotlin
        cursorOverlay?.stop()
        cursorOverlay = null
```

(호스트는 세션 종료에서 showsCursor를 원복하므로 LCDOFF 전송은 불필요 — BYE가 세션을 끝냄)

**Step 3: `StreamLauncherModule.openStream`에 extra 전달**

`openStream` 시그니처에 `localCursor: Boolean?` 파라미터를 `showFps` 뒤에 추가하고, intent에 추가:

```kotlin
                putExtra("localCursor", localCursor ?: false),
```

**Step 4: 커밋** (Task 6에서 TS 호출부와 함께 컴파일 정합 확인 — Kotlin만으로는 빌드 불가하므로 Task 6와 묶지 말고 여기서 커밋, Task 6가 호출부를 완성)

```bash
git add apps/viewer-expo/android
git commit -m "feat(viewer): LCD1 좌표 로컬 렌더 오버레이와 Choreographer 폴링 추가"
```

---

### Task 5: TypeScript 프리퍼런스 + 토글 UI

**Files:**
- Modify: `apps/viewer-expo/src/viewer-preferences.ts`
- Modify: `apps/viewer-expo/src/viewer-preferences.test.ts`
- Modify: `apps/viewer-expo/src/use-catalog-model.ts`
- Modify: `apps/viewer-expo/src/launch-stream.ts`
- Modify: `apps/viewer-expo/app/catalog.tsx`

**Step 1: 실패하는 테스트 추가**

`viewer-preferences.test.ts`에 기존 showFps 테스트 스타일대로 추가:

```typescript
  it("defaults localCursor to false and persists toggles", async () => {
    expect(DEFAULT_VIEWER_PREFERENCES.localCursor).toBe(false);
    expect(parseViewerPreferences(null).localCursor).toBe(false);
    expect(parseViewerPreferences('{"showFps":true}').localCursor).toBe(false);
    expect(
      parseViewerPreferences('{"localCursor":true,"showFps":true}').localCursor,
    ).toBe(true);
    expect(
      parseViewerPreferences('{"localCursor":"yes"}').localCursor,
    ).toBe(false);
  });
```

Run: `cd apps/viewer-expo && npx vitest run src/viewer-preferences.test.ts`
Expected: FAIL (`localCursor` 속성 없음)

**Step 2: `viewer-preferences.ts` 구현**

- `ViewerPreferences` 인터페이스에 `localCursor: boolean;` 추가
- `DEFAULT_VIEWER_PREFERENCES`에 `localCursor: false,` 추가
- `parseViewerPreferences`에 추가:

```typescript
      localCursor: typeof parsed.localCursor === "boolean"
        ? parsed.localCursor
        : DEFAULT_VIEWER_PREFERENCES.localCursor,
```

- `writeViewerPreferences`의 JSON에 `localCursor: preferences.localCursor,` 추가

**Step 3: `use-catalog-model.ts` 토글 핸들러**

`handleToggleFps`(276행) 옆에 추가하고 카탈로그 모델 반환 객체에 `showFps: preferences.showFps`(437행 근처)와 함께 `localCursor: preferences.localCursor`와 `handleToggleCursor`를 노출:

```typescript
  const handleToggleCursor = useCallback((localCursor: boolean) => {
    setPreferences((current) => ({ ...current, localCursor }));
  }, []);
```

**Step 4: `launch-stream.ts` 전달**

`openStream` 인터페이스(35행)와 구현부(262행, 336행)에 `localCursor?: boolean` 파라미터를 `showFps` 뒤에 추가하고 전달:

```typescript
      args.localCursor ?? false,
```

(두 호출부 모두 — `StartStreamArgs`에도 `localCursor?: boolean;` 추가. `use-catalog-model.ts`의 `openStream` 호출 3곳 — 346, 370, 390행 부근 — 에 `preferences.localCursor` 전달. 정확한 호출 시그니처는 편집 시 파일에서 확인)

**Step 5: `catalog.tsx` 토글 UI**

`showFps` 스위치 섹션(303~325행)을 복제해 커서 스위치를 추가 — props에 `localCursor: boolean; onToggleCursor: (localCursor: boolean) => void;` 추가, 스위치 라벨:

- 제목: `원격 커서 로컬 표시`
- 설명: `Mac 커서를 화면 속 영상 대신 오버레이로 그려 입력 반응 속도를 높입니다.`
- accessibilityLabel: `원격 커서 로컬 표시`

호출부(669행 근처)에 `localCursor={model.localCursor} onToggleCursor={model.handleToggleCursor}` 전달.

**Step 6: 테스트·타입 체크**

Run: `cd apps/viewer-expo && npx vitest run src/viewer-preferences.test.ts src/launch-stream.test.ts && npx tsc --noEmit`
Expected: PASS (launch-stream 테스트가 시그니처 변경으로 깨지면 호출부 정합으로 수정)

**Step 7: 커밋**

```bash
git add apps/viewer-expo/src apps/viewer-expo/app
git commit -m "feat(viewer): 원격 커서 로컬 표시 프리퍼런스와 설정 토글 추가"
```

---

### Task 6: Swift 커서 코디네이터 + 패킷 인코더 (TDD)

**Files:**
- Create: `native/macos-capture-shim/Sources/Transport/CursorStreamCoordinator.swift`
- Create: `native/macos-capture-shim/Tests/CursorStreamTests.swift`
- Modify: `tools/build-macos-capture-shim.zsh` (cursor-test 모드)

**Step 1: 실패하는 테스트 작성**

`Tests/CursorStreamTests.swift`:

```swift
import Foundation

@main
struct CursorStreamTests {
    static func main() {
        let token = Data("session-token".utf8)

        // Wire format: LCD1 | seq u32 BE | x u16 BE | y u16 BE | vis u8 | shape u8 | token
        var coordinator = CursorStreamCoordinator(fps: 60)
        coordinator.setEnabled(true)
        coordinator.note(position: CGPoint(x: 960, y: 600), visible: true)
        // First packet after enable is immediate even without a tick.
        guard let packet = coordinator.packetDue(nowUs: 1) else {
            fatalError("expected immediate first cursor packet")
        }
        let expectedX = UInt16((960 / 1920.0 * 65535.0).rounded())
        let expectedY = UInt16((600 / 1200.0 * 65535.0).rounded())
        precondition(packet.count == 14 + token.count)
        precondition(packet.prefix(4) == Data("LCD1".utf8))
        precondition(packet[12] == 1, "visibility must be 1 for an on-screen cursor")
        precondition(packet[13] == 0, "shape is reserved and always 0")
        precondition(packet.suffix(token.count) == token)

        // Coalesced duplicate state produces no packet.
        precondition(coordinator.packetDue(nowUs: 1_000) == nil)
        precondition(coordinator.packetDue(nowUs: 1_000_000) == nil)

        // Movement emits at most one packet per polling tick (2x fps = 120Hz).
        coordinator.note(position: CGPoint(x: 970, y: 600), visible: true)
        precondition(coordinator.packetDue(nowUs: 1_001) == nil, "must respect the 2xFPS interval")
        let second = coordinator.packetDue(nowUs: 9_000)
        precondition(second != nil, "due packet after one 120Hz interval")
        let firstSequence = UInt32(packet[4]) << 24 | UInt32(packet[5]) << 16
            | UInt32(packet[6]) << 8 | UInt32(packet[7])
        let secondSequence = UInt32(second![4]) << 24 | UInt32(second![5]) << 16
            | UInt32(second![6]) << 8 | UInt32(second![7])
        precondition(secondSequence == firstSequence &+ 1, "sequence must increment")

        // Off-screen cursor reports invisibility.
        coordinator.note(position: CGPoint(x: -50, y: 300), visible: false)
        let hidden = coordinator.packetDue(nowUs: 17_000)
        precondition(hidden?[12] == 0)

        // Disable stops packets; re-enable starts a fresh stream.
        coordinator.setEnabled(false)
        precondition(coordinator.packetDue(nowUs: 18_000) == nil)
        coordinator.setEnabled(true)
        precondition(coordinator.packetDue(nowUs: 18_001) != nil, "re-enable sends an immediate sample")

        // Polling rate mirrors the LCI1 pointer policy.
        precondition(cursorPollingHz(fps: 0) == 30)
        precondition(cursorPollingHz(fps: 60) == 120)
        precondition(cursorPollingHz(fps: 90) == 180)
        precondition(cursorPollingHz(fps: 240) == 240)

        print("cursor stream tests passed")
    }
}
```

**Step 2: 테스트가 실패하는지 확인**

Run: `zsh tools/build-macos-capture-shim.zsh library /tmp/cursor-probe-dummy 2>&1 | head -3` — 빌드가 아직 Coordinator를 모르므로, 테스트 파일만 단독 컴파일로 실패 확인:

Run: `/usr/bin/xcrun swiftc -O native/macos-capture-shim/Tests/CursorStreamTests.swift -o /tmp/cursor-test-fail 2>&1 | head -5`
Expected: FAIL (`cannot find 'CursorStreamCoordinator' in scope`)

**Step 3: 구현**

`Sources/Transport/CursorStreamCoordinator.swift`:

```swift
import Foundation
import CoreGraphics

/// Polling rate for the LCD1 cursor stream — the same 2x-stream-FPS policy
/// the LCI1 pointer plane uses, clamped to a bounded datagram budget.
func cursorPollingHz(fps: UInt32) -> UInt32 {
    (fps &* 2).clamped(to: 30...240)
}

/// Newest-wins cursor state with change detection. `packetDue` coalesces
/// CGEvent-tap observations into at most one LCD1 datagram per polling tick,
/// mirroring the viewer's InputScheduler pointer path in reverse.
struct CursorStreamCoordinator {
    private let pollingIntervalUs: UInt64
    private var enabled = false
    private var sequence: UInt32 = 0
    private var lastSentUs: UInt64 = 0
    private var dirty = false
    private var position = CGPoint.zero
    private var visible = false

    init(fps: UInt32) {
        pollingIntervalUs = UInt64(1_000_000 / Int64(cursorPollingHz(fps: fps)))
    }

    mutating func setEnabled(_ newValue: Bool) {
        guard enabled != newValue else { return }
        enabled = newValue
        if newValue {
            // Force an immediate sample so the viewer paints the cursor as
            // soon as it opts in.
            dirty = true
            lastSentUs = 0
        }
    }

    mutating func note(position newPosition: CGPoint, visible newVisible: Bool) {
        guard enabled else { return }
        if position != newPosition || visible != newVisible {
            dirty = true
        }
        position = newPosition
        visible = newVisible
    }

    /// Returns the encoded LCD1 datagram when a fresh state is due.
    mutating func packetDue(nowUs: UInt64) -> Data? {
        guard enabled, dirty,
              lastSentUs == 0 || nowUs >= lastSentUs + pollingIntervalUs
        else { return nil }
        dirty = false
        lastSentUs = nowUs
        sequence = sequence &+ 1
        return encodeCursorPacket(
            sequence: sequence,
            position: position,
            visible: visible
        )
    }
}

/// LCD1 | sequence u32 BE | x u16 BE | y u16 BE | visibility u8 | shape u8 | token
/// Coordinates are normalized to the captured content rect; see
/// CaptureSession+Cursor.swift for the mapping.
func encodeCursorPacket(sequence: UInt32, position: CGPoint, visible: Bool) -> Data {
    Data([
        UInt8(sequence & 0xff00_0000 >> 24),
        UInt8(sequence & 0x00ff_0000 >> 16),
        UInt8(sequence & 0x0000_ff00 >> 8),
        UInt8(sequence & 0x0000_00ff),
        UInt8(position.x),
        UInt8(position.x & 0xff00 >> 8),
        UInt8(position.y),
        UInt8(position.y & 0xff00 >> 8),
        visible ? 1 : 0,
        0,
    ].with ...
}
```

(주의: 위 `encodeCursorPacket` 초안은 정확하지 않다 — 최종 구현은 아래 계약으로 작성한다. 마지막 인자 token은 전송 계층에서 append한다:

```swift
func encodeCursorPacket(sequence: UInt32, x: UInt16, y: UInt16, visible: Bool, token: Data) -> Data {
    var bytes = Data(capacity: 14 + token.count)
    bytes.append(Data("LCD1".utf8))
    var sequenceBE = sequence.bigEndian
    withUnsafeBytes(of: &sequenceBE) { bytes.append(contentsOf: $0) }
    var xBE = x.bigEndian
    withUnsafeBytes(of: &xBE) { bytes.append(contentsOf: $0) }
    var yBE = y.bigEndian
    withUnsafeBytes(of: &yBE) { bytes.append(contentsOf: $0) }
    bytes.append(visible ? 1 : 0)
    bytes.append(0)
    bytes.append(token)
    return bytes
}
```

코디네이터는 x/y를 u16 정규화로 보관한다: `note(position:visible:)`는 CGPoint를 받아 CGEventTap 좌표계 그대로 보관하고, `packetDue`에서 contentRect 사각형을 알아야 정규화가 가능하다. 따라서 `init(fps:bounds:)`로 `CGRect`(inputBounds)를 주입받고, 패킷 직전에 `normalizedAxis` 동등 물로 변환한다:

```swift
private func normalized(_ value: CGFloat, origin: CGFloat, extent: CGFloat) -> UInt16 {
    guard extent > 0 else { return 0 }
    let fraction = (value - origin) / extent
    return UInt16((min(max(fraction, 0), 1) * 65535).rounded())
}
```

test의 expectedX/expectedY는 bounds 1920x1200 origin 0 전제 — 테스트의 `CursorStreamCoordinator(fps: 60)` 호출을 `CursorStreamCoordinator(fps: 60, bounds: CGRect(x: 0, y: 0, width: 1920, height: 1200))`로 맞춘다.)

**Step 4: 빌드 스크립트에 cursor-test 모드 추가**

`tools/build-macos-capture-shim.zsh`의 `split-test` 케이스 뒤에:

```zsh
  cursor-test)
    /usr/bin/xcrun swiftc -O \
      "${shim_sources[@]}" \
      "$shim_root/Tests/CursorStreamTests.swift" \
      -o "$output" \
      "${framework_args[@]}"
    ;;
```

**Step 5: 테스트 통과 확인**

Run: `zsh tools/build-macos-capture-shim.zsh cursor-test /tmp/cursor-test && /tmp/cursor-test`
Expected: `cursor stream tests passed`

**Step 6: 커밋**

```bash
git add native/macos-capture-shim tools/build-macos-capture-shim.zsh
git commit -m "feat(host): LCD1 커서 패킷 코디네이터 추가 — 2xFPS coalescing과 변경 시만 전송"
```

---

### Task 7: 호스트 수신·전송 경로 (CGEvent Tap + LCDON/LCDOFF + showsCursor 전환)

**Files:**
- Create: `native/macos-capture-shim/Sources/Transport/CaptureSession+Cursor.swift`
- Modify: `native/macos-capture-shim/Sources/Capture/CaptureSession.swift` (커서 상태 필드)
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+ViewerControl.swift` (LCDON/LCDOFF 파싱)
- Modify: `native/macos-capture-shim/Sources/Transport/CaptureSession+Input.swift` 또는 Backend (showsCursor 상태 참조)

**Step 1: `CaptureSession.swift`에 상태 필드 추가**

`inputEnabled`(52행) 근처에 추가:

```swift
    // Cursor plane (LCD1). Protected by cursorLock; the CGEvent tap callback
    // writes observations and the polling timer drains them.
     let cursorLock = NSLock()
     var cursorCoordinator: CursorStreamCoordinator?
     var cursorStreamEnabled = false
     var cursorEventTap: CFMachPort?
     var cursorPollingTimer: DispatchSourceTimer?
    // Viewer's ephemeral control port learned from the LCDON datagram; UDP
    // LCD1 responses must go back there, not the media target.
     var cursorStreamDestination: sockaddr_in?
     var cursorStreamFD: Int32 = -1
```

**Step 2: `CaptureSession+Cursor.swift` 작성**

```swift
import Foundation
import CoreGraphics
import Darwin

extension CaptureSession {
    /// Viewer opt-in. UDP keeps the requester's ephemeral control port as the
    /// LCD1 destination; TCP responses reuse the control framing.
     func handleCursorStreamCommand(
        _ command: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        let enable = command == Data("LCDON".utf8)
        guard enable || command == Data("LCDOFF".utf8) else { return }
        // splitVertical tiles stream half-frame video; the cursor plane is a
        // single-session feature for now.
        guard requestedEncoderExperiment != .splitVertical else { return }
        if mediaTransport.usesTCP {
            cursorStreamDestination = nil
        } else {
            guard let destination else { return }
            cursorStreamDestination = destination
        }
        cursorStreamFD = fd
        setCursorStreamEnabled(enable)
    }

     func setCursorStreamEnabled(_ enable: Bool) {
        cursorLock.lock()
        let changed = cursorStreamEnabled != enable
        cursorStreamEnabled = enable
        if enable {
            var coordinator = cursorCoordinator ?? CursorStreamCoordinator(
                fps: fps,
                bounds: capturedContentRect()
            )
            coordinator.setEnabled(true)
            cursorCoordinator = coordinator
        } else {
            cursorCoordinator?.setEnabled(false)
        }
        cursorLock.unlock()
        guard changed else { return }
        if enable {
            installCursorEventTap()
            startCursorPollingTimer()
        } else {
            removeCursorEventTap()
            stopCursorPollingTimer()
        }
        applyCaptureCursorVisibility()
    }

    /// Cursor separation only makes sense while the stream is live; capture
    /// hides the cursor exactly while LCD1 samples are flowing and restores
    /// the embedded cursor the moment the stream stops.
     func applyCaptureCursorVisibility() {
        cursorLock.lock()
        let hideFromVideo = cursorStreamEnabled
        cursorLock.unlock()
        guard let stream else { return }
        let config = SCStreamConfiguration()
        config.width = Int(outWidth)
        config.height = Int(outHeight)
        config.showsCursor = !hideFromVideo
        stream.updateConfiguration(config)
    }

    /// A listen-only HID tap observes physical and injected mouse movement
    /// without requiring accessibility permission and without swallowing
    /// events.
     func installCursorEventTap() {
        guard cursorEventTap == nil else { return }
        let mask = CGEventMask(
            (1 << CGEventType.mouseMoved.rawValue)
                | (1 << CGEventType.leftMouseDragged.rawValue)
                | (1 << CGEventType.rightMouseDragged.rawValue)
                | (1 << CGEventType.otherMouseDragged.rawValue)
        )
        guard let tap = CGEvent.tapCreate(
            tap: .cghidEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: mask,
            callback: { _, type, event, _ in
                CursorEventTapBridge.handle(type: type, event: event)
            },
            userInfo: nil
        ) else { return }
        cursorEventTap = tap
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
    }

     func removeCursorEventTap() {
        guard let tap = cursorEventTap else { return }
        CGEvent.tapEnable(tap: tap, enable: false)
        cursorEventTap = nil
        // The run loop source is released with the tap port by ARC of the
        // CFMachPort; an explicit invalidation keeps teardown deterministic.
    }

     func startCursorPollingTimer() {
        guard cursorPollingTimer == nil else { return }
        let hz = cursorPollingHz(fps: fps)
        let timer = DispatchSource.makeTimerSource(queue: inputQueue)
        timer.schedule(
            deadline: .now(),
            repeating: .milliseconds(max(1, 1_000 / Int(hz)))
        )
        timer.setEventHandler { [weak self] in
            self?.sendCursorPacketIfDue()
        }
        timer.resume()
        cursorPollingTimer = timer
    }

     func stopCursorPollingTimer() {
        cursorPollingTimer?.cancel()
        cursorPollingTimer = nil
    }

     func sendCursorPacketIfDue() {
        cursorLock.lock()
        let packet = cursorCoordinator?.packetDue(nowUs: DispatchTime.now().uptimeNanoseconds / 1_000)
        cursorLock.unlock()
        guard let packet else { return }
        packet.withUnsafeRepresentation { ... }
        var payload = packet
        cursorLock.lock()
        let token = viewerControlToken
        let fd = cursorStreamFD
        let destination = cursorStreamDestination
        cursorLock.unlock()
        payload.append(token)
        _ = sendControlPayload(payload, fd: fd, destination: destination)
    }

    /// Teardown hook called from the session stop path — restores the
    /// embedded cursor even if the viewer vanished without LCDOFF.
     func teardownCursorStream() {
        setCursorStreamEnabled(false)
        cursorLock.lock()
        cursorCoordinator = nil
        cursorStreamDestination = nil
        cursorStreamFD = -1
        cursorLock.unlock()
    }
}

/// CGEvent taps receive C function pointers, so the callback routes through
/// an unowned bridge to the active capture session.
enum CursorEventTapBridge {
    nonisolated(unsafe) static weak var session: CaptureSession?

    static func handle(type: CGEventType, event: CGEvent) {
        guard type == .mouseMoved || type == .leftMouseDragged
            || type == .rightMouseDragged || type == .otherMouseDragged,
            type != .tapDisabledByTimeout || type != .tapDisabledByUserInput
        else { return }
        session?.observeCursorPosition(CGEventGetLocation(event))
    }
}

extension CaptureSession {
     func observeCursorPosition(_ position: CGPoint) {
        let onScreen = CGCursorIsVisible()
        cursorLock.lock()
        cursorCoordinator?.note(position: position, visible: onScreen)
        cursorLock.unlock()
    }
}
```

(구현 노트: (a) `CGEventGetLocation`은 CGEvent의 전역 좌표를 준다. (b) `capturedContentRect()`는 `inputBounds`(inputLock 보호)를 읽어 돌려주는 헬퍼 — `CaptureSession+Cursor.swift` 안에 `inputLock`으로 읽는 private 헬퍼로 작성. (c) `Data.withUnsafeRepresentation`가 없으므로 `sendCursorPacketIfDue`는 단순히 `payload.append(token)` 후 전송. (d) 브릿지 `session`은 캡처 시작 시(`startInputReceiver` 호출부 근처) 설정하고 teardown에서 nil.)

**Step 3: LCDON/LCDOFF 파싱 연결**

`CaptureSession+ViewerControl.swift`:

- UDP 경로 `consumeViewerControl`: `if message == Data("IDR".utf8)` 블록(86행) 뒤에:

```swift
            if message == Data("LCDON".utf8) || message == Data("LCDOFF".utf8) {
                handleCursorStreamCommand(message, fd: fd, destination: source)
                continue
            }
```

- TCP 경로 `consumeViewerTCPControl`: IDR 블록(163행) 뒤에 동일 추가 (`destination: nil`).

**Step 4: teardown 연결**

`stopInputReceiver`의 호출부를 찾아(grep `stopInputReceiver(`) 그 앞에 `teardownCursorStream()`을 추가한다. 세션 중단 경로 어디서든 커서 원복이 한 번은 실행되도록 한다. 또한 `CursorEventTapBridge.session = self`를 `startInputReceiver`에서 설정하고 `teardownCursorStream`에서 nil로 한다.

**Step 5: 빌드·테스트 확인**

Run: `zsh tools/build-macos-capture-shim.zsh library /tmp/leftcar-cursor-check.dylib && zsh tools/build-macos-capture-shim.zsh cursor-test /tmp/cursor-test && /tmp/cursor-test`
Expected: 빌드 성공, `cursor stream tests passed`

**Step 6: 커밋**

```bash
git add native/macos-capture-shim
git commit -m "feat(host): CGEvent Tap 관측과 LCDON/LCDOFF 옵트인, showsCursor 전환 원복 연결"
```

---

### Task 8: 전체 검증 (Rust clippy/test, Swift 테스트, React Doctor 게이트)

**Step 1: Rust 전체**

Run: `cargo test --workspace && cargo clippy --workspace --tests -- -D warnings`
Expected: PASS, 경고 0

**Step 2: TypeScript**

Run: `cd apps/viewer-expo && npx tsc --noEmit && npx vitest run`
Expected: PASS

**Step 3: React Doctor 게이트 (CLAUDE.md 필수)**

Run: `npx -y react-doctor@latest . --verbose` (저장소 루트에서)
Expected: `100 / 100`. 미달 시 소스에서 수정 — ignore 규칙·점수 조정으로 숨기지 않는다.

**Step 4: Android 컴파일 확인 (가능한 경우)**

Run: `zsh tools/build_apk.sh` (ANDROID_HOME 필요 — 환경에 없으면 스킵하고 미검증 사항으로 명시)

**Step 5: EVIDENCE 기록**

`docs/EVIDENCE.md`의 USB 화면 확장 3종 기록 패턴대로 커서 분리 구현을 **E3(구현+단위 테스트, 실기 미검증)** 등급으로 추가:

```markdown
### 커서 분리 (LCD1 위치 스트림) — 2026-09-03

- 등급: E3 — 구현·단위 테스트 완료, 실기 미검증
- 계약: 뷰어 LCDON 옵트인 → 호스트 CGEvent Tap 관측 → 2×FPS coalescing 상태 스트림
- 안전장치: 세션 종료·LCDOFF 시 capture showsCursor 원복 (커서 소실 방지)
- 미검증: Wi-Fi 실기 지연 체감, 태블릿 오버레이 렌더, TCP 폴백 동작 — 다음 실기 세션에서 확인
```

**Step 6: 커밋**

```bash
git add docs/EVIDENCE.md
git commit -m "docs(evidence): 커서 분리 구현 기록 — E3 등급과 미검증 항목 명시"
```

---

## 실행 순서 요약

Task 1→2→3 (Rust 수신 평면) → Task 4→5 (Android+TS 표현 평면) → Task 6→7 (호스트 전송 평면) → Task 8 (게이트·기록).

의존성: Task 4의 Kotlin은 Task 3의 JNI가, Task 5의 TS는 Task 4의 intent extra가 필요하다. Task 6·7은 Rust와 독립이라 병렬 가능하나, 단일 세션에선 순서대로.
