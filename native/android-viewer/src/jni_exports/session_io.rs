use crate::input_protocol::{normalized_axis, InputEvent};
use crate::jni::*;
use std::ffi::{c_char, CStr};
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn active_input_control(instance_c: *const c_char) -> Result<Arc<RendererControl>, i32> {
    if instance_c.is_null() {
        return Err(LEFTCAR_ERR_NULL);
    }
    let instance = unsafe { CStr::from_ptr(instance_c) }
        .to_str()
        .map_err(|_| LEFTCAR_ERR_INVALID)?;
    active_renderer(instance).ok_or(LEFTCAR_ERR_STATE)
}

/// Return the authenticated Host input state for the in-stream lock badge.
/// -1 means the status packet has not arrived yet; 0/1 are locked/enabled.
#[no_mangle]
pub extern "C" fn leftcar_jni_input_status(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        i32::from(control.input_enabled.load(Ordering::SeqCst))
    });
    guard.unwrap_or(-1)
}

fn pack_stream_stats(control: &RendererControl) -> i64 {
    const FRAME_MASK: u64 = (1 << 28) - 1;
    let rendered = control
        .rendered_frames
        .load(Ordering::Relaxed)
        .min(FRAME_MASK);
    let stale = control.stale_outputs.load(Ordering::Relaxed).min(0x0fff);
    let input_drops = control
        .decoder_input_drops
        .load(Ordering::Relaxed)
        .min(0xff);
    let gaps = control.frame_gaps.load(Ordering::Relaxed).min(0xff);
    let feed_ms = control
        .last_feed_us
        .load(Ordering::Relaxed)
        .saturating_add(500)
        / 1_000;
    (rendered | (stale << 28) | (input_drops << 40) | (gaps << 48) | (feed_ms.min(0xff) << 56))
        as i64
}

/// Compact native diagnostics for the in-stream HUD.
/// bits 0..27 rendered, 28..39 stale skips, 40..47 decoder input drops,
/// 48..55 frame gaps, 56..63 latest decoder feed milliseconds.
#[no_mangle]
pub extern "C" fn leftcar_jni_stream_stats(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        pack_stream_stats(&control)
    });
    guard.unwrap_or(-1)
}

/// Pack separated stale-input and decoder-burst discard counters for the
/// measurement spike. The high 32 bits are input-policy observations and the
/// low 32 bits are decoder output-burst discards.
#[no_mangle]
pub extern "C" fn leftcar_jni_skip_breakdown(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        let input = control
            .stale_input_drops
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX));
        let burst = control
            .output_burst_discards
            .load(Ordering::Relaxed)
            .min(u64::from(u32::MAX));
        ((input << 32) | burst) as i64
    });
    guard.unwrap_or(-1)
}

/// Authenticated stage latency for the HUD, packed as four unsigned 16-bit
/// milliseconds: LAN RTT, capture-to-decoder, encode-to-decoder, wire-to-decoder.
/// `0xffff` means the NTP-style probe or L2 timestamp has not converged yet.
#[no_mangle]
pub extern "C" fn leftcar_jni_stream_latency(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        let encode = |value: u64| {
            if value == LATENCY_UNKNOWN {
                0xffff
            } else {
                value.min(0xfffe)
            }
        };
        let network = encode(control.network_rtt_ms.load(Ordering::Relaxed));
        let capture = encode(control.capture_to_decoder_ms.load(Ordering::Relaxed));
        let encoded = encode(control.encode_to_decoder_ms.load(Ordering::Relaxed));
        let wire = encode(control.wire_to_decoder_ms.load(Ordering::Relaxed));
        (network | (capture << 16) | (encoded << 32) | (wire << 48)) as i64
    });
    guard.unwrap_or(-1)
}

/// Host or local termination reason for this stream, or -1 while active.
/// 1 = feedback health check, 2 = host operator forced stop, 3 = ordinary
/// host stop, 4 = authenticated control peer unreachable, 5 = Surface render
/// stalled. The Activity polls this alongside stats and closes on any
/// non-negative value, including a reason retained after renderer cleanup.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn leftcar_jni_termination_reason(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        if instance_c.is_null() {
            return -1;
        }
        let instance = unsafe { CStr::from_ptr(instance_c) }
            .to_string_lossy()
            .into_owned();
        renderer_termination_reason(&instance)
            .map(i32::from)
            .unwrap_or(-1)
    });
    guard.unwrap_or(-1)
}

/// Capture-to-MediaCodec Surface release approximation in milliseconds.
/// `0xffff` means the clock-corrected estimate has not converged. Surface
/// release is not compositor presentation or panel photon output.
#[no_mangle]
pub extern "C" fn leftcar_jni_surface_release_latency(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| match active_input_control(instance_c) {
        Ok(control) => crate::surface_release_latency_value(
            control
                .capture_to_surface_release_ms
                .load(Ordering::Relaxed),
        ),
        Err(_) => 0xffff,
    });
    guard.unwrap_or(0xffff)
}

/// Backward-compatible C ABI alias for older Kotlin shims.
#[no_mangle]
pub extern "C" fn leftcar_jni_render_latency(instance_c: *const c_char) -> i32 {
    leftcar_jni_surface_release_latency(instance_c)
}

/// Queue a native Android pointer event. `x` and `y` are normalized to the
/// actual video Surface before crossing JNI; Rust clamps once more at the
/// fixed-point wire boundary.
#[no_mangle]
pub extern "C" fn leftcar_jni_input_pointer(
    instance_c: *const c_char,
    action: u32,
    x: f32,
    y: f32,
    buttons: u32,
    action_button: u32,
    horizontal_scroll: f32,
    vertical_scroll: f32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        let event = match action {
            1 => InputEvent::PointerMove {
                x: normalized_axis(x),
                y: normalized_axis(y),
                buttons,
            },
            2 | 3 => InputEvent::PointerButton {
                x: normalized_axis(x),
                y: normalized_axis(y),
                button: u8::try_from(action_button).unwrap_or(0),
                down: action == 2,
                buttons,
            },
            4 => InputEvent::Scroll {
                horizontal_milli_lines: (horizontal_scroll.clamp(-1000.0, 1000.0) * 1_000.0).round()
                    as i32,
                vertical_milli_lines: (vertical_scroll.clamp(-1000.0, 1000.0) * 1_000.0).round()
                    as i32,
            },
            _ => return LEFTCAR_ERR_INVALID,
        };
        control.input.lock().unwrap().push(event);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[no_mangle]
pub extern "C" fn leftcar_jni_input_key(
    instance_c: *const c_char,
    key_code: u32,
    scan_code: u32,
    meta_state: u32,
    down: bool,
    repeat: u32,
) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        let (Ok(key_code), Ok(scan_code), Ok(repeat)) = (
            u16::try_from(key_code),
            u16::try_from(scan_code),
            u16::try_from(repeat),
        ) else {
            return LEFTCAR_ERR_INVALID;
        };
        control.input.lock().unwrap().push(InputEvent::Key {
            key_code,
            scan_code,
            meta_state,
            down,
            repeat,
        });
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

/// Pack the newest LCD1 cursor sample for frame-rate polling.
/// bits 0..15 x, 16..31 y, 32..61 sequence (low 30 bits), 63 visible.
/// Returns -1 while the host has not opted in or no sample arrived yet.
/// Coordinates within the screen bounds cannot produce the all-ones payload
/// (x = y = 0xffff with the top-of-range sequence), so real samples are
/// distinguishable from the sentinel.
#[no_mangle]
pub extern "C" fn leftcar_jni_cursor_state(instance_c: *const c_char) -> i64 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(_) => return -1,
        };
        if control.cursor_active.load(Ordering::SeqCst) <= 0 {
            return -1;
        }
        let x = i64::from(control.cursor_x.load(Ordering::SeqCst));
        let y = i64::from(control.cursor_y.load(Ordering::SeqCst));
        let sequence = u64::from(control.cursor_sequence.load(Ordering::SeqCst)) & 0x3fff_ffff;
        let visible = i64::from(control.cursor_visible.load(Ordering::SeqCst));
        x | (y << 16) | ((sequence as i64) << 32) | (visible << 63)
    });
    guard.unwrap_or(-1)
}

/// Record the viewer-side cursor stream opt-in. Applied at the next control
/// token establishment (attach or same-window rebind).
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

#[no_mangle]
pub extern "C" fn leftcar_jni_input_release_all(instance_c: *const c_char) -> i32 {
    let guard = std::panic::catch_unwind(|| {
        let control = match active_input_control(instance_c) {
            Ok(control) => control,
            Err(code) => return code,
        };
        control.input.lock().unwrap().push(InputEvent::ReleaseAll);
        LEFTCAR_OK
    });
    guard.unwrap_or(LEFTCAR_ERR_PANIC)
}

#[cfg(test)]
mod cursor_export_tests {
    use super::*;
    use std::ffi::CString;

    /// Each test installs its own registry key: `install_renderer` replaces
    /// the entry for a key, and cargo runs tests in parallel threads.
    fn installed_control(test_key: &str) -> (CString, Arc<RendererControl>) {
        let control = Arc::new(RendererControl::new_split(51_400, 60));
        install_renderer(test_key, Arc::clone(&control));
        (CString::new(test_key).unwrap(), control)
    }

    fn drop_control(test_key: &str, control: &Arc<RendererControl>) {
        remove_renderer_if_current(test_key, control);
        clear_cached_termination(test_key);
    }

    #[test]
    fn cursor_state_holds_sentinel_until_opt_in() {
        const KEY: &str = "cursor-jni-export-tests/sentinel-until-opt-in";
        let (instance, control) = installed_control(KEY);
        assert_eq!(leftcar_jni_cursor_state(instance.as_ptr()), -1);
        control.cursor_active.store(0, Ordering::SeqCst);
        assert_eq!(leftcar_jni_cursor_state(instance.as_ptr()), -1);
        drop_control(KEY, &control);
    }

    #[test]
    fn cursor_state_packs_sample_distinguishable_from_sentinel() {
        const KEY: &str = "cursor-jni-export-tests/packing";
        let (instance, control) = installed_control(KEY);
        control.cursor_active.store(1, Ordering::SeqCst);
        control.cursor_x.store(0x1234, Ordering::SeqCst);
        control.cursor_y.store(0x5678, Ordering::SeqCst);
        control.cursor_sequence.store(0xCDEF_0123, Ordering::SeqCst);
        control.cursor_visible.store(true, Ordering::SeqCst);
        let packed = leftcar_jni_cursor_state(instance.as_ptr());
        assert_eq!(packed & 0xffff, 0x1234);
        assert_eq!((packed >> 16) & 0xffff, 0x5678);
        assert_eq!((packed >> 32) & 0x3fff_ffff, 0x0def_0123);
        assert!(packed < 0, "visible flag must occupy the sign bit");
        assert_ne!(packed, -1, "a real sample must not equal the sentinel");
        drop_control(KEY, &control);
    }

    #[test]
    fn cursor_state_stays_sentinel_when_flow_stops_without_opt_out() {
        const KEY: &str = "cursor-jni-export-tests/sentinel-after-stop";
        let (instance, control) = installed_control(KEY);
        control.cursor_active.store(1, Ordering::SeqCst);
        control.cursor_visible.store(true, Ordering::SeqCst);
        assert_ne!(leftcar_jni_cursor_state(instance.as_ptr()), -1);
        // A host opt-out or session teardown drops the stream back to the
        // waiting sentinel even though stale sample fields remain.
        control.cursor_active.store(-1, Ordering::SeqCst);
        assert_eq!(leftcar_jni_cursor_state(instance.as_ptr()), -1);
        drop_control(KEY, &control);
    }

    #[test]
    fn set_cursor_stream_records_opt_in_on_active_renderer() {
        const KEY: &str = "cursor-jni-export-tests/opt-in-toggle";
        let (instance, control) = installed_control(KEY);
        assert!(!control.cursor_requested.load(Ordering::SeqCst));
        assert_eq!(
            leftcar_jni_set_cursor_stream(instance.as_ptr(), true),
            LEFTCAR_OK
        );
        assert!(control.cursor_requested.load(Ordering::SeqCst));
        assert_eq!(
            leftcar_jni_set_cursor_stream(instance.as_ptr(), false),
            LEFTCAR_OK
        );
        assert!(!control.cursor_requested.load(Ordering::SeqCst));
        drop_control(KEY, &control);
    }

    #[test]
    fn cursor_exports_fail_closed_without_a_renderer() {
        let missing = CString::new("cursor-jni-export-tests/never-installed").unwrap();
        assert_eq!(leftcar_jni_cursor_state(missing.as_ptr()), -1);
        assert_eq!(
            leftcar_jni_set_cursor_stream(missing.as_ptr(), true),
            LEFTCAR_ERR_STATE
        );
    }
}
