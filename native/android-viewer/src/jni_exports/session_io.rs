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
