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
fn real_output_drain_paces_one_release_per_vsync_slot() {
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
