#![cfg(not(target_os = "android"))]
use crate::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
};
#[derive(Default)]
struct Probe {
    next: usize,
    live: Vec<usize>,
    attempts: Vec<(usize, Vec<String>)>,
    formats: HashMap<usize, Vec<String>>,
    reject: usize,
    outputs: std::collections::VecDeque<(usize, i64)>,
    releases: Vec<(usize, Option<i64>, bool)>,
}
thread_local! { static PROBE: RefCell<Probe> = RefCell::new(Probe::default()); }
// PRESENTATION_SMOOTH is process-global; pacing tests must not interleave.
static SMOOTH_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[no_mangle]
extern "C" fn AMediaCodec_createCodecByName(_: *const c_char) -> *mut c_void {
    PROBE.with_borrow_mut(|p| {
        assert!(p.live.is_empty(), "failed codec must be freed before retry");
        p.next += 1;
        p.live.push(p.next);
        p.next as *mut c_void
    })
}
#[no_mangle]
extern "C" fn AMediaCodec_createDecoderByType(n: *const c_char) -> *mut c_void {
    AMediaCodec_createCodecByName(n)
}
#[no_mangle]
extern "C" fn AMediaCodec_delete(c: *mut c_void) -> i32 {
    PROBE.with_borrow_mut(|p| p.live.retain(|v| *v != c as usize));
    0
}
#[no_mangle]
extern "C" fn AMediaFormat_new() -> *mut c_void {
    PROBE.with_borrow_mut(|p| {
        p.next += 1;
        p.formats.insert(p.next, vec![]);
        p.next as *mut c_void
    })
}
#[no_mangle]
extern "C" fn AMediaFormat_delete(f: *mut c_void) -> i32 {
    PROBE.with_borrow_mut(|p| {
        p.formats.remove(&(f as usize));
    });
    0
}
#[no_mangle]
unsafe extern "C" fn AMediaFormat_setInt32(f: *mut c_void, k: *const c_char, _: i32) {
    let key = unsafe { CStr::from_ptr(k) }.to_str().unwrap().to_owned();
    PROBE.with_borrow_mut(|p| p.formats.get_mut(&(f as usize)).unwrap().push(key));
}
#[no_mangle]
extern "C" fn AMediaFormat_setString(_: *mut c_void, _: *const c_char, _: *const c_char) {}
#[no_mangle]
extern "C" fn AMediaFormat_setBuffer(_: *mut c_void, _: *const c_char, _: *const c_void, _: usize) {
}
#[no_mangle]
extern "C" fn AMediaCodec_configure(
    c: *mut c_void,
    f: *const c_void,
    _: *mut c_void,
    _: *const c_void,
    _: u32,
) -> i32 {
    PROBE.with_borrow_mut(|p| {
        p.attempts
            .push((c as usize, p.formats[&(f as usize)].clone()));
        if p.attempts.len() <= p.reject {
            -1
        } else {
            0
        }
    })
}
#[no_mangle]
extern "C" fn AMediaCodec_start(_: *mut c_void) -> i32 {
    0
}
#[no_mangle]
extern "C" fn AMediaCodec_stop(_: *mut c_void) -> i32 {
    0
}
#[no_mangle]
extern "C" fn AMediaCodec_flush(_: *mut c_void) -> i32 {
    0
}
#[test]
fn real_constructor_retries_fresh_both_low_only_then_standard() {
    PROBE.with_borrow_mut(|p| {
        *p = Probe::default();
        p.reject = 2;
    });
    let decoder = unsafe {
        AndroidDecoder::new_video_named(VideoDecoderConfig {
            codec: VideoCodec::H264,
            vps: None,
            sps: &[0x67],
            pps: &[0x68],
            width: 1920,
            height: 1080,
            window: 0,
            fps: 60,
            codec_name: Some("c2.qti.avc.decoder.low_latency"),
            allow_mime_fallback: false,
            max_frame_size: None,
        })
    }
    .expect("third standard attempt must succeed");
    PROBE.with_borrow(|p| {
        assert_eq!(p.attempts.len(), 3);
        let keys: Vec<Vec<&str>> = p
            .attempts
            .iter()
            .map(|(_, ks)| {
                ks.iter()
                    .filter(|s| s.starts_with("vendor."))
                    .map(String::as_str)
                    .collect()
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                vec![
                    "vendor.qti-ext-dec-low-latency.enable",
                    "vendor.qti-ext-dec-picture-order.enable"
                ],
                vec!["vendor.qti-ext-dec-low-latency.enable"],
                vec![]
            ]
        );
    });
    drop(decoder);
    PROBE.with_borrow(|p| {
        assert!(p.live.is_empty());
        assert!(p.formats.is_empty());
    });
}

#[no_mangle]
unsafe extern "C" fn AMediaCodec_dequeueOutputBuffer(
    _: *mut c_void,
    info: *mut super::ffi::AMediaCodecBufferInfo,
    _: i64,
) -> isize {
    PROBE.with_borrow_mut(|p| {
        if let Some((index, pts)) = p.outputs.pop_front() {
            unsafe { (*info).presentation_time_us = pts };
            index as isize
        } else {
            -1
        }
    })
}
#[no_mangle]
extern "C" fn AMediaCodec_releaseOutputBuffer(_: *mut c_void, index: usize, render: bool) -> i32 {
    PROBE.with_borrow_mut(|p| p.releases.push((index, None, render)));
    0
}
#[no_mangle]
extern "C" fn AMediaCodec_releaseOutputBufferAtTime(_: *mut c_void, index: usize, ns: i64) -> i32 {
    PROBE.with_borrow_mut(|p| p.releases.push((index, Some(ns), true)));
    0
}
#[test]
fn smoothing_park_absorbs_burst_without_dropping_frames() {
    let _guard = SMOOTH_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // 프레임 스무딩(적응 지터 버퍼) ON: 도착 뭉침을 park가 흡수하고 첫
    // 프레임부터 순서대로 vsync 슬롯에 릴리스한다(드롭 0).
    PRESENTATION_SMOOTH.store(true, std::sync::atomic::Ordering::Relaxed);
    PROBE.with_borrow_mut(|p| *p = Probe::default());
    let mut decoder =
        unsafe { AndroidDecoder::new_h264(&[0x67], &[0x68], 1920, 1080, 0, 60) }.unwrap();
    PROBE.with_borrow_mut(|p| p.outputs.extend([(40, 1234), (41, 5678), (42, 9012)]));
    let target = 9_876_543_210;
    decoder.set_output_target(Some(target));
    assert!(decoder.pump_latest_output(0).unwrap());
    // park 한도 5 ≥ 3: 어떤 프레임도 폐기되지 않고, 맨 앞 프레임이 슬롯을
    // 받는다.
    assert_eq!(decoder.last_released_pts_us, Some(1234));
    assert_eq!(decoder.frames_discarded, 0);
    assert_eq!(decoder.frames_rendered, 1);
    PRESENTATION_SMOOTH.store(false, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn smoothing_depth_adapts_to_burst_demand() {
    // 수요 수위(HWM) 기반 적응: 첫 버스트(7프레임)는 깊이 2를 넘어 일부
    // 즉시 렌더로 소화되지만 프레임은 하나도 버려지지 않고, 기록된 수요
    // 7에 맞춰 다음 펌프의 park 깊이가 8로 늘어난다.
    let _guard = SMOOTH_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    PRESENTATION_SMOOTH.store(true, std::sync::atomic::Ordering::Relaxed);
    PROBE.with_borrow_mut(|p| *p = Probe::default());
    let mut decoder =
        unsafe { AndroidDecoder::new_h264(&[0x67], &[0x68], 1920, 1080, 0, 60) }.unwrap();
    // 첫 버스트: 7프레임 동시 도착.
    PROBE.with_borrow_mut(|p| {
        let batch: Vec<(usize, i64)> = (0..7).map(|i| (40 + i, 1000 + i as i64)).collect();
        p.outputs.extend(batch);
    });
    let target = 9_876_543_210;
    decoder.set_output_target(Some(target));
    assert!(decoder.pump_latest_output(0).unwrap());
    assert_eq!(decoder.frames_discarded, 0, "버스트에도 폐기 0");
    let rendered_first = decoder.frames_rendered;
    assert!(rendered_first >= 1);

    // 두 번째 버스트: 같은 크기. 깊이는 HWM 7+1=8로 늘어났다.
    PROBE.with_borrow_mut(|p| {
        let batch: Vec<(usize, i64)> = (0..7).map(|i| (60 + i, 2000 + i as i64)).collect();
        p.outputs.extend(batch);
    });
    let next = target + 8_333_333;
    decoder.set_output_target(Some(next));
    assert!(decoder.pump_latest_output(0).unwrap());
    // 두 버스트 모두 폐기 없이 소화.
    assert_eq!(decoder.frames_discarded, 0);
    PRESENTATION_SMOOTH.store(false, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn real_output_drain_paces_one_release_per_vsync_slot() {
    // 프레임 스무딩 OFF: park 한도 2 계약(최신 우선, 초과분 폐기) 고정.
    let _guard = SMOOTH_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    PRESENTATION_SMOOTH.store(false, std::sync::atomic::Ordering::Relaxed);
    PROBE.with_borrow_mut(|p| *p = Probe::default());
    let mut decoder =
        unsafe { AndroidDecoder::new_h264(&[0x67], &[0x68], 1920, 1080, 0, 60) }.unwrap();
    // A decode burst: three outputs land before any vsync slot passes.
    PROBE.with_borrow_mut(|p| p.outputs.extend([(40, 1234), (41, 5678), (42, 9012)]));
    let target = 9_876_543_210;
    decoder.set_output_target(Some(target));
    assert!(decoder.pump_latest_output(0).unwrap());
    // Park budget 2 drops the oldest (40); the head (41) takes the vsync slot.
    assert_eq!(decoder.last_released_pts_us, Some(5678));
    assert_eq!(decoder.frames_discarded, 1);
    assert_eq!(decoder.frames_rendered, 1);
    PROBE.with_borrow(|p| {
        assert_eq!(
            p.releases,
            vec![(40, None, false), (41, Some(target), true)]
        )
    });

    // Same vsync slot again: the newest burst frame (42) must hold, not
    // double-render inside the slot.
    PROBE.with_borrow_mut(|p| p.outputs.push_back((43, 11111)));
    assert!(!decoder.pump_latest_output(0).unwrap());
    PROBE.with_borrow(|p| assert_eq!(p.releases.len(), 2));

    // Next vsync slot releases the held frame in FIFO order.
    let next_target = target + 13_888_889;
    decoder.set_output_target(Some(next_target));
    assert!(decoder.pump_latest_output(0).unwrap());
    assert_eq!(decoder.last_released_pts_us, Some(9012));
    PROBE.with_borrow(|p| {
        assert_eq!(p.releases.last(), Some(&(42, Some(next_target), true)))
    });

    // Dropping back to freshness mode collapses the parked tail to the newest
    // image and renders it immediately.
    decoder.set_output_target(None);
    PROBE.with_borrow_mut(|p| p.outputs.push_back((44, 20000)));
    assert!(decoder.pump_latest_output(0).unwrap());
    assert_eq!(decoder.last_released_pts_us, Some(20000));
    PROBE.with_borrow(|p| {
        assert_eq!(
            p.releases.last(),
            Some(&(44, None, true))
        );
        assert_eq!(p.releases[3], (43, None, false));
    });
    assert_eq!(decoder.frames_discarded, 2);
}

#[test]
fn real_output_drain_flush_forgets_parked_outputs() {
    PROBE.with_borrow_mut(|p| *p = Probe::default());
    let mut decoder =
        unsafe { AndroidDecoder::new_h264(&[0x67], &[0x68], 1920, 1080, 0, 60) }.unwrap();
    let target = 9_876_543_210;
    PROBE.with_borrow_mut(|p| p.outputs.extend([(50, 1000), (51, 2000)]));
    decoder.set_output_target(Some(target));
    assert!(decoder.pump_latest_output(0).unwrap());
    // Two outputs fit the park budget: the oldest (50) takes the slot and 51
    // stays parked for the next one.
    PROBE.with_borrow(|p| {
        assert_eq!(p.releases, vec![(50, Some(target), true)])
    });
    // A codec flush reclaims parked indices; the next slot must not release
    // the stale index of the parked 51.
    decoder.flush().unwrap();
    let next_target = target + 13_888_889;
    decoder.set_output_target(Some(next_target));
    PROBE.with_borrow_mut(|p| p.outputs.push_back((52, 3000)));
    assert!(decoder.pump_latest_output(0).unwrap());
    assert_eq!(decoder.last_released_pts_us, Some(3000));
    PROBE.with_borrow(|p| {
        assert_eq!(
            p.releases,
            vec![
                (50, Some(target), true),
                (52, Some(next_target), true)
            ]
        )
    });
}

#[test]
fn age_drain_counts_excess_plus_front_frames_beyond_budget() {
    use crate::android::decoder::{immediate_release_count, park_drain_budget_ns};
    // 상한 초과분: cap 2에 5개면 3개 즉시 소화.
    assert_eq!(immediate_release_count(&[0, 0, 0, 0, 0], 2, 45_000_000), 3);
    // 나이 예산 초과: 맨앞부터 예산을 넘는 만큼만 추가된다.
    assert_eq!(
        immediate_release_count(&[50_000_000, 46_000_000, 10_000], 4, 45_000_000),
        2
    );
    // 전부 예산 안쪽이면 드레인 없음.
    assert_eq!(immediate_release_count(&[10_000, 20_000], 2, 45_000_000), 0);
    // FIFO 전제: 예산 초과가 맨앞이 아니면 세지 않는다.
    assert_eq!(
        immediate_release_count(&[10_000, 90_000_000], 4, 45_000_000),
        0
    );
    // 상한이 크면 초과분도 없다.
    assert_eq!(immediate_release_count(&[0, 0], 4, 45_000_000), 0);
    // 스무딩 off 예산은 무제한 — 기존 동작 보존.
    assert_eq!(park_drain_budget_ns(true), 45_000_000);
    assert_eq!(park_drain_budget_ns(false), u64::MAX);
    assert_eq!(
        immediate_release_count(&[u64::MAX], 4, park_drain_budget_ns(false)),
        0
    );
}

#[test]
fn fresh_park_arrivals_are_never_age_drained() {
    // 방금 도착한 프레임은 나이 0에 수렴하므로 드레인이 발화하지 않는다 —
    // 기존 케이던스(슬롯당 1 릴리스)가 그대로 유지된다는 회귀 핀.
    let _guard = SMOOTH_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    PRESENTATION_SMOOTH.store(true, std::sync::atomic::Ordering::Relaxed);
    PROBE.with_borrow_mut(|p| *p = Probe::default());
    let mut decoder =
        unsafe { AndroidDecoder::new_h264(&[0x67], &[0x68], 1920, 1080, 0, 60) }.unwrap();
    PROBE.with_borrow_mut(|p| p.outputs.extend([(60, 1000), (61, 2000), (62, 3000)]));
    let target = 9_876_543_210;
    decoder.set_output_target(Some(target));
    assert!(decoder.pump_latest_output(0).unwrap());
    // 수요 HWM이 3→cap 4로 맞춰주므로 초과분도 없고, 나이 예산도 안 지났다:
    // 슬롯 릴리스 1회뿐이다.
    assert_eq!(decoder.last_released_pts_us, Some(1000));
    assert_eq!(decoder.frames_rendered, 1);
    assert_eq!(decoder.park_drained_frames, 0);
    PRESENTATION_SMOOTH.store(false, std::sync::atomic::Ordering::Relaxed);
}
