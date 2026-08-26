# 회복 정책 재설계·FEC·ABR·Windows 제로카피 구현 계획

> **For Claude:** REQUIRED SUB-KILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 꼬리 지연 1프레임이 프레임 폭포로 증폭되는 회복 정책을 고치고(무한 GOP·히스테리시스·debounce 단축), Reed-Solomon FEC와 피드백 ABR을 얹으며, Windows 캡처의 CPU readback을 GPU 상주 경로로 교체한다.

**Architecture:** Viewer의 stale 재동기화 정책을 "연속 3프레임 초과 시에만"으로 바꾸고 Host GOP를 사실상 무한(3600)으로 올려 IDR on-request 회복만 남긴다. FEC는 기존 `G` 미디어 데이터그램 뒤에 `P` 패리티 데이터그램을 추가하는 방식(새 wire 마커, 구형 혼합 시 기존 무시 관례 유지)이며 ABR은 이미 존재하는 `adaptBitrateIfNeeded` 창에 receiver 신호가 이미 들어와 있으므로 SKIP 분리 카운터만 피드백에 추가한다. Windows는 WGC D3D11 텍스처를 GPU 컴퓨트셰이더 없이 CPU 매핑 없이 NV12 텍스처로 변환해 MF 인코더에 `IMFMediaBuffer` 대신 DXGI 텍스처 샘플로 투입한다.

**Tech Stack:** Rust(host Tauri crate, android-viewer), Swift(CaptureShim), Reed-Solomon(`nanors` MIT vendored), D3D11/Media Foundation(windows crate)

**설계 문서:** `docs/plans/2026-08-26-recovery-fec-abr-zerocopy-design.md` (승인됨)
**주의:** `docs/plans/2026-08-25-v0.2-hardening.md`(HMAC 작업)과 같은 파일을 수정한다. 이 계획은 HMAC Task 이후를 가정하지 않는다. 충돌 시 wire 포맷 변경은 별도 커밋 순서로 조정한다.

---

## 검증 명령 (전 작업 공통)

```bash
cargo test --workspace && cargo clippy --workspace --tests -- -D warnings
cd apps/host-desktop/src-tauri && cargo test && cargo clippy --tests -- -D warnings
cd apps/host-desktop/src-tauri && cargo check --target x86_64-pc-windows-msvc --lib
bun run test && bun run test:contract && bun run test:architecture
```

Swift shim 빌드: `cd native/macos-capture-shim && swift build 2>&1 | tail -3` (또는 저장소의 기존 dylib 빌드 스크립트).

---

## Phase 1 — 회복 정책 재설계 (원인 제거, 최우선)

### Task 1: 무한 GOP — Swift `MaxKeyFrameInterval` 3600

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift:1920-1928`

**Step 1:** `setupEncoder`의 keyframe interval 계산을 변경한다.

현재:
```swift
let nominalKeyframeInterval = mediaTransport.usesTCP
    ? max(1, fps * 60)
    : max(1, fps)
```

변경:
```swift
// Infinite GOP: recovery happens exclusively through the authenticated
// IDR request path (viewer `IDR` datagram -> kVTEncodeFrameOptionKey_ForceKeyFrame).
// A periodic IDR would re-introduce the 1-second resync ceiling this
// redesign removes. 3600 frames = 60s at 60fps; VideoToolbox treats it
// as "no periodic keyframe" for interactive sessions.
let nominalKeyframeInterval = mediaTransport.usesTCP
    ? max(1, fps * 60)
    : 3600
```

**Step 2:** 빌드 확인: `cd native/macos-capture-shim && swift build 2>&1 | tail -3` → 경고 없음.

**Step 3:** Host Tauri 테스트에 GOP 관련 테스트가 없음을 확인(문서 주석 수준 변경이므로). `cd apps/host-desktop/src-tauri && cargo test` → 전체 통과 유지.

**Step 4:** 커밋: `git add -A native/macos-capture-shim && git commit -m "feat(stream): UDP 무한 GOP으로 IDR on-request 회복 전환"`

### Task 2: debounce 750ms → 250ms

**Files:**
- Modify: `native/android-viewer/src/media_datagram.rs:36`
- Test: `native/android-viewer/src/media_datagram.rs` (tests module, `gate` 테스트 근처)

**Step 1:** 실패 테스트 작성 (기존 `with_cooldown` 테스트 아래):

```rust
#[test]
fn recovery_cooldown_allows_four_requests_per_second() {
    // The 750ms cooldown made IDR re-request loops wait almost a full
    // second. 250ms keeps at most 4 requests/s while an RTT of ~13ms
    // leaves ample margin for the request round trip.
    assert_eq!(RECOVERY_REQUEST_COOLDOWN, Duration::from_millis(250));
}
```

**Step 2:** 실행해 실패 확인: `cargo test -p android-viewer --lib recovery_cooldown` → FAIL (750 != 250).

**Step 3:** 상수 변경:

```rust
pub const RECOVERY_REQUEST_COOLDOWN: Duration = Duration::from_millis(250);
```

**Step 4:** 통과 확인: `cargo test -p android-viewer --lib` → PASS 전체.

**Step 5:** 커밋: `git commit -am "feat(stream): IDR 재요청 debounce 750ms에서 250ms로 단축"`

### Task 3: stale 판정 히스테리시스 (연속 K=3)

**Files:**
- Modify: `native/android-viewer/src/media_datagram.rs` (히스테리시스 함수 추가)
- Modify: `native/android-viewer/src/jni.rs:1360-1410` (stale 판정 호출부)

**Step 1:** `media_datagram.rs`에 상수와 함수를 추가하고 실패 테스트부터:

```rust
/// Consecutive over-budget delta frames required before the viewer enters
/// keyframe resync. A single tail-latency frame must render late instead of
/// discarding the whole reference chain (design 2026-08-26 part 1b).
pub const STALE_RESYNC_THRESHOLD: u32 = 3;

/// Pure decision core so the resync policy is unit-testable without a
/// decoder. Returns the new consecutive-stale count and whether resync
/// should engage.
pub fn stale_streak_advance(
    consecutive_stale: u32,
    is_keyframe: bool,
    over_budget: bool,
) -> (u32, bool) {
    if is_keyframe {
        return (0, false);
    }
    if !over_budget {
        return (0, false);
    }
    let next = consecutive_stale.saturating_add(1);
    (next, next >= STALE_RESYNC_THRESHOLD)
}
```

테스트:

```rust
#[test]
fn single_stale_frame_renders_late_without_resync() {
    let (count, resync) = stale_streak_advance(0, false, true);
    assert_eq!((count, resync), (1, false));
    let (count, resync) = stale_streak_advance(1, false, true);
    assert_eq!((count, resync), (2, false));
}

#[test]
fn third_consecutive_stale_frame_engages_resync() {
    let (count, resync) = stale_streak_advance(2, false, true);
    assert_eq!((count, resync), (3, true));
}

#[test]
fn fresh_frame_resets_the_streak() {
    let (count, resync) = stale_streak_advance(2, false, false);
    assert_eq!((count, resync), (0, false));
}

#[test]
fn keyframe_resets_the_streak_and_never_resyncs() {
    let (count, resync) = stale_streak_advance(2, true, true);
    assert_eq!((count, resync), (0, false));
}
```

**Step 2:** `cargo test -p android-viewer --lib stale_streak` → 함수 미정의로 FAIL 확인.

**Step 3:** 구현 추가(위 코드) → PASS 확인.

**Step 4:** `jni.rs` 호출부 교체. 현재(`jni.rs:1368-1380` 부근):

```rust
if !keyframe && capture_age_ms.is_some_and(|age| age > stale_budget_ms) {
    renderer_stats.stale_inputs = renderer_stats.stale_inputs.saturating_add(1);
    ...resync + IDR request...
} else {
    ...feed...
}
```

변경 — `RendererStats`에 `consecutive_stale: u32` 필드 추가(`jni.rs:320` 구조체) 후:

```rust
let over_budget =
    !keyframe && capture_age_ms.is_some_and(|age| age > stale_budget_ms);
let (streak, should_resync) = stale_streak_advance(
    renderer_stats.consecutive_stale,
    keyframe,
    over_budget,
);
renderer_stats.consecutive_stale = streak;
if over_budget {
    renderer_stats.stale_inputs = renderer_stats.stale_inputs.saturating_add(1);
}
if should_resync {
    last_frame_id = Some(frame.id);
    resync_decoder_after_frame_gap(&mut awaiting_keyframe);
    request_idr_debounced(
        &control_socket,
        peer,
        &viewer_control_token,
        &mut recovery_gate,
        &control_clone,
    );
    continue; // 이 프레임과 이후 델타는 기존 정책대로 폐기
}
// streak < K: 늦게라도 렌더 (feed 경로 유지)
```

주의: 기존 `stale_inputs` 카운터 의미(재동기화 폐기 수)를 유지하되, 히스테리시스 미달 폐기는 이제 없으므로 실제로는 "예산 초과 관측 수"가 된다. 로그 문구(`Rendered {} frames; ... staleInputs={}`)는 그대로 둔다.

**Step 5:** `cargo test -p android-viewer` 전체 → PASS. `cargo clippy -p android-viewer --tests -- -D warnings` → 0 경고.

**Step 6:** 커밋: `git commit -am "feat(stream): stale 판정 히스테리시스 — 연속 3프레임 초과 시에만 IDR 재동기화"`

### Task 4: SKIP 카운터 분리 (입력 stale / 출력 burst)

**Files:**
- Modify: `native/android-viewer/src/jni.rs:70-72,780-800,1790-1800`

**Step 1:** `RendererControl`(`jni.rs:70` 부근)에 추가:

```rust
stale_input_drops: AtomicU64,
output_burst_discards: AtomicU64,
```

기존 `stale_outputs` 필드는 HUD 비트필드 하위 호환(합계)으로 유지.

**Step 2:** 저장부(`jni.rs:796`) 교체:

```rust
control.stale_outputs.store(
    dec.frames_discarded.saturating_add(stats.stale_inputs),
    Ordering::Relaxed,
);
control
    .stale_input_drops
    .store(stats.stale_inputs, Ordering::Relaxed);
control
    .output_burst_discards
    .store(dec.frames_discarded, Ordering::Relaxed);
```

**Step 3:** 주기 로그(`jni.rs:782`)에 두 분리 값 추가:

```rust
log_info!(
    "Rendered {} frames; outputDrops={} staleInputs={} staleInputDrops={} outputBurst={} ...",
    dec.frames_rendered,
    dec.frames_discarded,
    stats.stale_inputs,
    control.stale_input_drops.load(Ordering::Relaxed),
    control.output_burst_discards.load(Ordering::Relaxed),
    ...기존 인자...
);
```

**Step 4:** JNI 익스포트 하나 추가(스파이크 계측이 NDK에서 분리 값을 읽을 수 있게):

```rust
/// Packed (stale_input_drops, output_burst_discards) as two u32 halves so
/// the measurement spike can attribute SKIP amplification without parsing
/// logs. High 32 bits: input policy drops, low 32: decoder burst discards.
#[no_mangle]
pub extern "C" fn leftcar_jni_skip_breakdown(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = active_input_control(instance_c)?;
        let input = control.stale_input_drops.load(Ordering::Relaxed).min(0xffff_ffff);
        let burst = control.output_burst_discards.load(Ordering::Relaxed).min(0xffff_ffff);
        Some(((input as i64) << 32) | burst as i64)
    });
    guard.unwrap_or(-1)
}
```

`active_input_control`이 `Result`를 반환하므로 `?` 대신 match로 `None`→`-1` 변환은 기존 `leftcar_jni_stream_stats` 패턴(`jni.rs:1812-1818`)을 따른다.

**Step 5:** `cargo test -p android-viewer` + arm64 교차 확인: `cargo check -p android-viewer --target aarch64-linux-android` → PASS.

**Step 6:** 커밋: `git commit -am "feat(diagnostics): SKIP 카운터를 입력 정책 드랍과 출력 burst 폐기로 분리"`

### Task 5: Phase 1 통합 검증 + 문서 갱신

**Step 1:** 전체 게이트 실행(검증 명령 블록 전부) → 전부 green.

**Step 2:** `docs/EVIDENCE.md`에 Phase 1 항목 추가: 구현 사실 + "SKIP 461→측정 대기, 복구 시간 ~50ms 목표는 실기기 표본 필요" 명시. E15 표본과 동일 수집 방식임을 기록.

**Step 3:** 커밋: `git commit -am "docs: EVIDENCE에 회복 정책 재설계 구현 기록 추가"`

---

## Phase 2 — Reed-Solomon FEC (nanors, MIT)

### Task 6: nanors vendored 의존성 추가 + 라이선스 감사

**Files:**
- Create: `vendor/nanors/` (소스 + LICENSE)
- Modify: `apps/host-desktop/src-tauri/Cargo.toml`, `native/android-viewer/Cargo.toml`

**Step 1:** nanors 가져오기 (MIT, `sleepybishop/nanors`):

```bash
mkdir -p vendor
git clone --depth 1 https://github.com/sleepybishop/nanors vendor/nanors
rm -rf vendor/nanors/.git
head -5 vendor/nanors/LICENSE   # MIT 확인
```

**Step 2:** 양쪽 Cargo.toml에 path 의존성:

```toml
[dependencies]
reed-solomon = { path = "../../vendor/nanors" }  # host (경로는 위치에 맞게)
```
(android-viewer는 `../../../vendor/nanors` 등 상대경로 조정)

crate명이 `reed-solomon`인지 `nanors`인지 `vendor/nanors/Cargo.toml`의 `name`으로 확인 후 맞춘다.

**Step 3:** 크로스 컴파일 확인: host `cargo check` + `cargo check -p android-viewer --target aarch64-linux-android` → 둘 다 통과(nanors가 no_std/순수 Rust임을 확인).

**Step 4:** 라이선스 감사 기록: `docs/EVIDENCE.md`에 "vendor/nanors MIT — GPL 불포함 확인" 한 줄 추가.

**Step 5:** 커밋: `git add vendor/nanors && git commit -m "chore(deps): Reed-Solomon FEC용 nanors(MIT) vendored 추가"`

### Task 7: FEC 인코딩 코어 + wire 패리티 데이터그램

**Files:**
- Create: `apps/host-desktop/src-tauri/src/fec.rs`
- Modify: `apps/host-desktop/src-tauri/src/wire.rs` (`parity_datagrams` 추가)
- Modify: `apps/host-desktop/src-tauri/src/lib.rs` (`mod fec;`)

**Step 1:** 실패 테스트 (`fec.rs` 하단):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_shards_survive_two_losses() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 1200]).collect();
        let (encoded, k) = encode_group(&data);
        assert_eq!(k, 8);
        assert_eq!(encoded.len(), 10); // 8 data + 2 parity
        // 2개 유실(패리티 1개 + 데이터 1개) 복구
        let mut received = encoded.clone();
        received[0] = None; received[9] = None; // index 0 데이터, 9 패리티
        let recovered = decode_group(received, k).expect("must recover");
        assert_eq!(recovered[0], data[0]);
    }

    #[test]
    fn three_losses_are_beyond_capacity() {
        let data: Vec<Vec<u8>> = (0..8).map(|i| vec![i as u8; 300]).collect();
        let (encoded, k) = encode_group(&data);
        let mut received = encoded.clone();
        received[1] = None; received[4] = None; received[9] = None;
        assert!(decode_group(received, k).is_err());
    }

    #[test]
    fn short_tail_group_uses_shrunk_rs() {
        // 마지막 AU가 3조각이면 k=3, 패리티 1 (min(2, k-1) 규칙) — 축소 그룹
        let data: Vec<Vec<u8>> = (0..3).map(|i| vec![i as u8; 100]).collect();
        let (encoded, k) = encode_group(&data);
        assert_eq!(k, 3);
        assert_eq!(encoded.len(), 4);
    }
}
```

**Step 2:** `cargo test -p leftcar-host-desktop --lib fec` → FAIL (모듈 없음) 확인.

**Step 3:** 구현. nanors API(`reed_solomon_new(k, shards-k)`, `reed_solomon_encode`, `reed_solomon_decode`)로 `encode_group`/`decode_group`을 감싼다. 데이터그램 크기가 조각마다 다를 수 있으므로(마지막 조각) 각 조각 앞에 `len: u16` 프리픽스를 넣고 RS는 고정 폭 버퍼(최대 조각 길이)로 패딩해 인코딩한다. `decode_group`은 `Option<Vec<u8>>` 슬라이스(유실=None)를 받아 복구하거나 Err.

`wire.rs`에 패리티 송신 포맷 추가 — 기존 `G` 미디어와 구분되는 새 마커 `P`:

```rust
/// Parity datagram envelope: `P | group id LE | shard index | k | LT | wall ms | payload`.
/// Old viewers ignore unknown `P` datagrams the same way they ignore unknown
/// control markers today; new viewers feed them to the FEC decoder.
pub fn parity_datagrams(
    group_id: u16,
    k: u8,
    host_wall_ms: u64,
    parity_shards: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    parity_shards
        .iter()
        .enumerate()
        .map(|(index, shard)| {
            let mut datagram = Vec::with_capacity(17 + shard.len());
            datagram.push(b'P');
            datagram.extend_from_slice(&group_id.to_le_bytes());
            datagram.push(k);
            datagram.push(index as u8);
            datagram.extend_from_slice(b"LT");
            datagram.extend_from_slice(&host_wall_ms.to_be_bytes());
            datagram.extend_from_slice(shard);
            datagram
        })
        .collect()
}
```

wire 테스트(기존 `media_fragments_stay_under_mtu_and_preserve_au` 패턴):

```rust
#[test]
fn parity_datagrams_stay_under_mtu_and_carry_group_identity() {
    let shards = vec![vec![9u8; 500], vec![8u8; 500]];
    let datagrams = parity_datagrams(77, 8, 1_000, &shards);
    assert_eq!(datagrams.len(), 2);
    for (i, d) in datagrams.iter().enumerate() {
        assert!(d.len() <= MAX_DATAGRAM);
        assert_eq!(d[0], b'P');
        assert_eq!(u16::from_le_bytes([d[1], d[2]]), 77);
        assert_eq!(d[3], 8);
        assert_eq!(d[4] as usize, i);
    }
}
```

**Step 4:** `cargo test -p leftcar-host-desktop --lib` → PASS. `cargo clippy --tests -- -D warnings` → 0.

**Step 5:** 커밋: `git commit -am "feat(fec): RS(10,8) 인코딩 코어와 P 패리티 데이터그램 포맷"`

### Task 8: Host 송신 경로에 FEC 적용

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/windows_backend/capture.rs` (UDP 송신 루프, `au_id` 증가 직후)
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift` (macOS 송신부 — `sendToViewer`에 `media_datagrams` 결과를 내보내는 지점)

**Step 1:** macOS Swift 측: 조각 송신 후 그룹 패리티 계산·송신. Swift에서 nanors를 직접 쓸 수 없으므로(FEC 코어는 Rust), **FEC는 Rust host(Windows 경로)와 Swift host(macOS)가 각각 필요**하다. 라이선스·이중 구현 리스크를 줄이기 위해 다음 구조를 취한다:
- `fec.rs`에 C ABI 래퍼를 추가해 dylib로 노출하고 Swift가 `dlsym`으로 호출 (이미 CaptureShim이 C ABI 진입점 패턴을 사용 중)
- `leftcar_fec_encode_parity(k: u8, shards_ptr, shards_len, out_ptr) -> i32` 형태

```rust
/// C ABI so the Swift capture shim can share the exact FEC core with the
/// Windows Rust path. Single flat layout: [len u16 | bytes]* per shard.
#[no_mangle]
pub extern "C" fn leftcar_fec_encode_parity(
    k: u8,
    shards_len: usize,
    shards: *const u8,   // concatenated [len u16 LE | bytes]*
    out_capacity: usize,
    out: *mut u8,        // concatenated parity shards, same flat layout
    out_written: *mut usize,
) -> i32 {
    // parse flat layout, call encode_group parity part, serialize back.
    // 0 = ok, negative = error
}
```

이 래퍼는 host crate의 `cdylib` 빌드가 아니므로, **별도 tiny crate `crates/fec-ffi`** 를 만들어 `leftcar-fec` dylib로 빌드한다:

```toml
# crates/fec-ffi/Cargo.toml
[lib]
crate-type = ["cdylib"]
[dependencies]
leftcar-host-desktop = { path = "../../apps/host-desktop/src-tauri" }  # 또는 fec 모듈을 crates/fec-core로 분리
```

(구현 단계에서 `fec.rs`를 `crates/fec-core` crate으로 분리하는 편이 깔끔하면 그렇게 한다 — host와 fec-ffi가 같은 코어를 의존)

**Step 2:** Swift 송신부(`CaptureShim.swift` `media_datagrams` 결과를 sendToViewer로 보내는 루프, `sendPackets`/동등 함수) 뒤에: AU 조각을 flat buffer로 만들어 `leftcar_fec_encode_parity` 호출, `P` 데이터그램 조립·송신. group_id는 `au_id` 재사용(1:1 대응). 조각 수 < 3이면 패리티 1, 아니면 2.

**Step 3:** Windows `capture.rs` 송신 루프: `let datagrams = wire::media_datagrams(...)` 뒤 동일 규칙 적용.

**Step 4:** 단위 테스트: 송신 측은 "k개 조각 + min(2, k-1) 패리티가 같은 group id로 나간다"를 모의 소켓 테스트(host e2e 패턴, 기존 `transport-api/tests/loopback.rs` 아님 — host Tauri e2e 테스트에 추가)로 검증.

**Step 5:** `cargo test --workspace` + Swift 빌드 + MSVC 교차 check → 전부 green.

**Step 6:** 커밋: `git commit -am "feat(fec): macOS/Windows 송신 경로에 RS 패리티 추가"`

### Task 9: Viewer FEC 복호 — 수신→재조립 사이 삽입

**Files:**
- Modify: `native/android-viewer/src/media_datagram.rs` (`parse_parity`, `FecGroup` 버퍼)
- Modify: `native/android-viewer/src/jni.rs` (수신 루프: `P` 데이터그램 분기 → 그룹 완성/복구 → `G` 조각으로 환원)

**Step 1:** 실패 테스트:

```rust
#[test]
fn parity_restores_one_lost_fragment_before_reassembly() {
    // 8조각 AU, 조각 3 유실, 패리티 2 수신 → 조각 3 복구되어 FrameReassembler가 AU 완성
    let mut group = FecGroup::new(8, 77);
    for i in 0..8 {
        if i != 3 { group.push_data(parse_fragment(&datagram(i, 8, 77, 1, 2, 3, &[i as u8; 64])).unwrap()); }
    }
    for p in parity_shards_of(77) {
        group.push_parity(p);
    }
    let restored = group.try_restore();
    assert_eq!(restored.len(), 8); // 모든 데이터 조각이 채워짐
}

#[test]
fn group_is_evicted_when_incomplete() {
    // 패리티 부족 → 복구 실패 → 기존 gap 경로로 폐기, 메모리 무한 증가 없음
    let mut group = FecGroup::new(8, 78);
    for i in 0..4 { /* push */ }
    assert!(group.try_restore().is_none());
    assert!(group.is_empty_after_eviction());
}
```

**Step 2:** FAIL 확인 → 구현:
- `parse_parity(bytes) -> Option<ParityFragment { group_id, k, index, payload }>`
- `FecGroup { k, group_id, data: Vec<Option<Vec<u8>>>, parity: Vec<Option<Vec<u8>>> }` — `nanors` decode 호출은 Rust이므로 android-viewer Cargo.toml에도 path 의존성 추가
- `try_restore`: 유실 데이터 수 ≤ 가용 패리티 수면 nanors decode로 복구, 아니면 None
- 수신 루프(`jni.rs` UDP read 분기): 첫 바이트 `b'P'`면 `FecGroup` 버퍼에 push. `G` 조각은 기존처럼 재조립기에, 단 그룹에 구멍이 있으면 즉시 `try_restore` 시도, 성공 시 복구된 조각을 재조립기에 순서대로 투입
- 그룹 버퍼 상한: 동시 4그룹(AU 4개분), 초과 시 가장 오래된 것 폐기(메모리 bound — 기존 `fragment_flood_stays_within_memory_bound` 정신)

**Step 3:** `cargo test -p android-viewer` → PASS. `cargo check -p android-viewer --target aarch64-linux-android` → OK.

**Step 4:** 커밋: `git commit -am "feat(fec): viewer 수신 경로에서 패리티 복구 후 재조립"`

### Task 10: FEC 수용 시나리오 e2e (인위 유실)

**Files:**
- Test: `apps/host-desktop/src-tauri/tests/` (기존 e2e 파일에 추가)

**Step 1:** 기존 e2e 하네스(송신→viewer 모의)에 lossy 소켓 래퍼(30% 확정적 유실, 시드 고정)를 끼워 "3% 유실에서 frame gap 없이 AU 전달"을 검증하는 테스트 추가. 확정적 유실은 `if (seq % 33) == 0 { drop }` 패턴.

**Step 2:** 통과 기준: 유실 데이터그램이 모두 패리티로 복구되어 `frame_gaps` 증가 없음.

**Step 3:** `cargo test -p leftcar-host-desktop` → PASS. 커밋: `git commit -am "test(fec): 3% 인위 유실에서 gap 없는 전달 e2e"`

---

## Phase 3 — ABR 피드백 확장

### Task 11: 피드백에 분리 카운터 추가

**Files:**
- Modify: `native/android-viewer/src/input_protocol.rs:226-243` (`ReceiverFeedback` + `encode_receiver_feedback`)
- Modify: `native/android-viewer/src/jni.rs` (`send_receiver_feedback` 호출부)
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift:2250-2360` (LCF1 파싱부)

**Step 1:** 구형 혼합 안전을 위해 **뒤에 필드 추가**(기존 6필드 순서 불변):

```rust
pub struct ReceiverFeedback {
    pub frame_gaps: u32,
    pub input_drops: u32,
    pub incomplete_aus: u32,
    pub stale_frames: u32,
    pub network_rtt_ms: u16,
    pub wire_to_decoder_ms: u16,
    // v2 additions (2026-08-26): separated SKIP attribution for ABR.
    pub stale_input_drops: u32,
    pub output_burst_discards: u32,
}
```

`encode_receiver_feedback`은 두 필드를 뒤에 append(기존 바이트와 앞부분 동일). **구형 Host가 새 Viewer 패킷을 파싱할 때**: Swift 파서가 고정 길이를 읽다 실패하면 기존 6필드만 사용(뒤 초과분 무시)하도록 `min(count, expected)` 처리 — 이것이 "명시적 거부" 대신 안전한 확장인 이유: LCF1은 토큰 접미사 검증에서 길이를 명시하지 않으므로 파서가 뒷자르기 허용으로 바꾼다. 반대(새 Host + 구형 Viewer)는 필드가 적게 오므로 0 처리.

주의: v0.2 HMAC 계획이 LCF1 포맷을 바꾼다면 이 Task는 그 이후로 재정렬. 구현 시 `git log --oneline -5`로 wire.rs HMAC 커밋 존재 여부 확인 후 진행.

**Step 2:** Swift 파싱(`CaptureShim.swift:2250` LCF1 브랜치): 기존 `receiverFrameGaps/InputDrops/IncompleteAUs/StaleFrames/RttMs/WireMs` 읽기 뒤에, 바이트가 남으면 두 u32 추가 읽기, 아니면 0.

**Step 3:** 크로스 정합성 테스트: input_protocol 테스트에 "인코딩→파싱 왕복이 새 필드 보존" + "6필드 길이 입력도 파싱 성공(구형 시뮬레이션)" 추가. Swift 측은 단위 테스트 프레임이 없으므로 host e2e에서 새 필드 수신을 확인하는 통합 테스트로 대체(가능 시).

**Step 4:** `cargo test --workspace` → PASS. 커밋: `git commit -am "feat(abr): LCF1 피드백에 분리 SKIP 카운터 추가(후위 확장)"`

### Task 12: ABR 정책에 분리 카운터 반영

**Files:**
- Modify: `native/macos-capture-shim/Sources/CaptureShim.swift:1725-1810` (`adaptBitrateIfNeeded`)

**Step 1:** `receiverLoss` 합산(`CaptureShim.swift:1739-1742`)에서 `stale_frames` 구성이 바뀐 것을 반영:

현재 `receiverLoss = frame_gaps + input_drops + incomplete_aus + stale_frames`. stale_frames가 이제 "예산 초과 관측(폐기 아님)"을 포함하므로 congestion 신호로는 과민해질 수 있다. 변경:

```swift
let receiverLoss = UInt64(receiverFrameGaps)
    + UInt64(receiverInputDrops)
    + UInt64(receiverIncompleteAUs)
    // stale_frames now counts over-budget observations that mostly render
    // late (hysteresis K=3). Only the resynced subset is real loss for
    // congestion purposes; use stale_input_drops when the v2 field is
    // present (non-nil), else fall back to legacy stale_frames.
    + UInt64(receiverStaleInputDrops ?? receiverStaleFrames)
```

`receiverStaleInputDrops`는 `UInt32?`(미수신=nil). 기존 창 로직(2연속 창 확인, 8창 안정 후 4→8→16% 회복, IDR burst grace)은 그대로 — 이미 설계의 계단식 정책이 구현되어 있다. `output_burst_discards`는 디코더 지연 신호이므로 congestion 지표에서 제외(로그용).

**Step 2:** Swift 빌드 + 전체 게이트 → green.

**Step 3:** 커밋: `git commit -am "feat(abr): 혼잡 판정을 분리 카운터 기반으로 교체"`

---

## Phase 4 — Windows WGC 제로카피

### Task 13: MF 인코더 D3D11 디바이스 연결 (IMFDXGIDeviceManager)

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/windows_backend/capture.rs:259-500` (`HardwareH264Encoder`)

**Step 1:** `HardwareH264Encoder::new`에 D3D 장치 연결 추가(실패 테스트는 소스 수준 — MSVC 교차 check가 게이트):

```rust
// Hardware MFTs accept D3D11 textures only after the DXGI device manager
// handshake. MFCreateDXGIDeviceManager + SetDevice, then
// MFT_MESSAGE_SET_D3D_MANAGER on the transform.
let mut reset_token = 0u32;
let mut manager: Option<IMFDXGIDeviceManager> = None;
unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager) }
    .map_err(win("create DXGI device manager"))?;
let manager = manager.ok_or("MFCreateDXGIDeviceManager returned null")?;
unsafe { manager.ResetDevice(&device, reset_token) }
    .map_err(win("attach D3D11 device to DXGI manager"))?;
unsafe {
    transform.ProcessMessage(
        MFT_MESSAGE_SET_D3D_MANAGER,
        *(&manager as *const _ as *const _),
    )
}
.map_err(win("bind DXGI device manager to encoder"))?;
```

`encoder_dxgi_manager: Option<IMFDXGIDeviceManager>` 필드 보유(Drop 시 선행 해제 순서 보장).

**Step 2:** 인코더 입력을 NV12 CPU 버퍼 대신 텍스처로: `MFCreateDXGISurfaceBuffer`로 `ID3D11Texture2D`를 감싸 `IMFSample`에 attach. 첫 프레임에 장치 접근이 필요하므로 `encode(&mut self, nv12_texture: &ID3D11Texture2D)`로 시그니처 변경.

**Step 3:** `cargo check --target x86_64-pc-windows-msvc --lib` → 컴파일 통과가 이 단계의 게이트(물리 장치 검증은 E13).

### Task 14: GPU BGRA→NV12 변환 경로

**Files:**
- Modify: `apps/host-desktop/src-tauri/src/windows_backend/capture.rs:104-160` (캡처 루프), `607-728` (`bgra_to_nv12` 대체)

**Step 1:** 변환 선택 스파이크 결과 반영:
- **우선**: 하드웨어 MFT가 `MFVideoFormat_ARGB32` 입력을 직접 수락하는지 `IMFTransform::GetInputAvailableType` 열거로 런타임 확인 → 수락하면 셰이더 없이 MFT 입력 타입을 ARGB32로 전환(코드 최소)
- **아니면**: 컴퓨트셰이더 BGRA→NV12 (`ID3D11Device::CreateComputeShader`, root constants 없는 단순 2픽셀당 Y/UV 작성 셰이더 바이너리를 포함). 셰이더는 DXBC를 빌드 시스템에서 컴파일하지 않고 미리 컴파일된 blob을 `include_bytes!`로 포함 — 윈도우 SDK 의존 제거. 셰이더 소스는 저장소에 함께 보관(`windows_backend/bgra_to_nv12.hlsl`, 컴파일 지침 주석)

구현은 다음 순서로:
1. 캡처 루프에서 staging 생성·`CopyResource`·`Map`·`bgra_to_nv12` 제거
2. WGC 텍스처 → (MFT 직접 or 컴퓨트셰이더 NV12 텍스처) → `encode(&texture)`
3. NV12 출력 텍스처는 `D3D11_BIND_RENDER_TARGET`가 아닌 `D3D11_BIND_SHADER_RESOURCE` + 인코더 입력 호환 플래그로 재사용 풀 2장 유지(프레임 파이프라인 겹침)

**Step 2:** `bgra_to_nv12` CPU 함수와 관련 테스트 제거(경로 사멸). 리사이즈 경로(`pool.Recreate`)에서 NV12 텍스처 풀도 재생성.

**Step 3:** MSVC 교차 check + host 테스트(Windows 유닛은 모의 수준) → green.

**Step 4:** 커밋: `git commit -am "feat(windows): WGC→인코더 GPU 상주 경로로 전환, CPU readback 제거"`

### Task 15: 제로카피 게이트 문서화 + 전체 회귀

**Step 1:** `docs/windows-remote-host.md`의 capture 행 갱신: "D3D11 GPU 상주 BGRA→NV12 → MF 텍스처 입력, CPU readback 제거(소스 수준). 물리 검증은 E13 게이트 유지".

**Step 2:** 전체 검증 블록 + `cargo check --target x86_64-pc-windows-msvc --lib` 재실행.

**Step 3:** `docs/EVIDENCE.md`에 Phase 2~4 항목 추가(달성/대기 구분).

**Step 4:** 커밋: `git commit -am "docs: FEC·ABR·Windows 제로카피 구현 증거 갱신"`

---

## 마무리 (전 Phase 공통)

- 성공 기준 체크리스트(설계 문서)에서 소스 수준 항목 달성 표시, 실기기 항목(SKIP<50, 복구 p95≤100ms, 3% 유실 유지, kbps 수렴)은 "측정 대기"로 명시
- react-doctor: React/TSX 변경이 없으므로 불필요 — 단 Phase 3에서 TS 파일을 건드리면 `npx -y react-doctor@latest . --verbose` 100/100 필요
- 사용자의 측정 스파이크와 `leftcar_jni_skip_breakdown`을 공유 지표로 사용

## 실행 순서 및 의존성

```
Task 1 ─┐
Task 2 ─┼─ Phase 1 (독립, 순서 무관하나 1→2→3→4 권장: 정책→계측 순서가 스파이크에 유리)
Task 3 ─┤   Task 4는 Task 3의 RendererStats 변경 이후
Task 4 ─┘   Task 5는 Phase 1 전부 이후

Task 6 → Task 7 → Task 8 → Task 9 → Task 10  (Phase 2, 엄격 순차)
Task 11 → Task 12  (Phase 3, Task 4 분리 카운터 필요)
Task 13 → Task 14 → Task 15  (Phase 4, Phase 1과 독립)
```

Phase 1 먼저 완료 후 커밋해 두면 스파이크가 새 빌드로 측정을 시작할 수 있다(가장 빠른 가치 전달).

## 구현 상태 (2026-08-26)

- Phase 1~3 소스 구현과 테스트: 완료. `nanors` 계획 항목은 C 저장소로 확인되어 `crates/fec-core` 순수 Rust GF(256) 구현으로 대체했으며, GPL 코드는 추가하지 않았다.
- Phase 4 소스 구현: WGC staging/CPU readback 및 `bgra_to_nv12`를 제거하고 D3D11 texture를 `MFCreateDXGISurfaceBuffer`로 hardware H.264 MFT에 전달한다. Windows USB transport도 AOAP framed media/input 경로를 사용한다.
- 검증: root/Host/Android 테스트, Android aarch64 check, Kotlin/TS/contract/architecture, Swift dylib compile, React Doctor `100 / 100`, Windows MSVC cross `cargo check --lib`를 통과했다.
- 실기기/물리 게이트: SKIP 감소·복구 p95·3% loss soak, 실제 Windows GPU/MFT, AOAP 물리 핸드셰이크와 E6/E7은 장치 확보 후 측정 대기다.
