pub mod fec_stats;
pub mod output_metadata;
pub mod presentation_sync;
pub mod recovery;
#[cfg(target_os = "android")]
pub(crate) mod single_session;
#[cfg(all(test, not(target_os = "android")))]
pub(crate) mod single_session {
    pub(crate) mod health {
        include!("single_session/health.rs");
    }
}
/// V2 paired-IDR dispatch semantics (single selected path per action,
/// bounded alternate retry, episode-owned cancellation, truthful
/// attempt-not-delivery reporting). Kept out of the android gate so host
/// `cargo test` exercises the pure routing logic; the android-gated split
/// coordinator and tile workers only wire it up.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod dispatch;
/// Gap/pressure decision + batch-limit policy for the split tiles. Kept out
/// of the android gate so host `cargo test` exercises the pure logic; the
/// android-gated tile workers only wire it up.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[path = "split_session/gap_policy.rs"]
pub(crate) mod split_gap_policy;
/// Bounded local-monotonic latency telemetry for the split path. Kept out of
/// the android gate so host `cargo test` exercises the pure tracking logic;
/// the android-gated split worker only wires it up.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod split_latency;
#[cfg(target_os = "android")]
pub(crate) mod split_session;
pub mod stats;

pub use presentation_sync::*;
pub use recovery::*;

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn split_final_stop_flags(send_bye: bool, termination_reason: i8) -> [bool; 2] {
    let notify_host = send_bye && termination_reason < 0;
    [notify_host, false]
}

// ---- Shared clock-sync telemetry helpers ----------------------------------
//
// The single-session path keeps its own copies in single_session::feedback
// (they predate the split clock sync); these host-testable versions serve the
// split tile workers so both paths measure the same quantities the same way.

/// Sentinel for "no host clock offset has converged yet". Chosen outside the
/// realistic offset range (LAN offsets are small i64 values).
#[cfg(any(target_os = "android", test))]
pub(crate) const HOST_CLOCK_OFFSET_UNKNOWN_MS: i64 = i64::MAX;
/// Sentinel matching `LATENCY_UNKNOWN`: a smoothed latency that was never
/// measured stays u64::MAX and never displays as zero.
#[cfg(any(target_os = "android", test))]
pub(crate) const LATENCY_MEASUREMENT_UNKNOWN: u64 = u64::MAX;
/// Ages outside this window cannot be real delivery latency on a LAN link;
/// they indicate a bogus timestamp or a badly converged offset.
#[cfg(any(target_os = "android", test))]
const CLOCK_AGE_MAX_MS: i128 = 60_000;

#[cfg(any(target_os = "android", test))]
pub(crate) fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_millis() as u64)
        .unwrap_or(0)
}

/// EWMA (3/4 previous + 1/4 sample) matching the single-session smoother.
#[cfg(any(target_os = "android", test))]
pub(crate) fn store_smoothed_latency(target: &std::sync::atomic::AtomicU64, sample: u64) {
    use std::sync::atomic::Ordering;
    let previous = target.load(Ordering::Relaxed);
    let next = if previous == LATENCY_MEASUREMENT_UNKNOWN {
        sample
    } else {
        previous
            .saturating_mul(3)
            .saturating_add(sample)
            .saturating_div(4)
    };
    target.store(next, Ordering::Relaxed);
}

/// Capture/send wall-clock age "now - host_timestamp + offset" in
/// milliseconds, or None when the offset has not converged or the timestamp
/// is missing. Mirrors the single-session math.
#[cfg(any(target_os = "android", test))]
pub(crate) fn clock_corrected_age_ms(host_wall_ms: Option<u64>, offset_ms: i64) -> Option<u64> {
    clock_corrected_age_at_ms(host_wall_ms, offset_ms, wall_clock_ms())
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn clock_corrected_age_at_ms(
    host_wall_ms: Option<u64>,
    offset_ms: i64,
    now_ms: u64,
) -> Option<u64> {
    if offset_ms == HOST_CLOCK_OFFSET_UNKNOWN_MS {
        return None;
    }
    let host_wall_ms = host_wall_ms?;
    let age = i128::from(now_ms) - i128::from(host_wall_ms) + i128::from(offset_ms);
    (0..=CLOCK_AGE_MAX_MS).contains(&age).then_some(age as u64)
}

/// Wire encoding for latency fields: unknown stays the u16::MAX sentinel and
/// known values saturate below it, exactly like the single-session encoder.
#[cfg(any(target_os = "android", test))]
pub(crate) fn latency_feedback_value_u16(value: u64) -> u16 {
    if value == LATENCY_MEASUREMENT_UNKNOWN {
        u16::MAX
    } else {
        value.min(u64::from(u16::MAX - 1)) as u16
    }
}

#[cfg(test)]
mod clock_telemetry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn smoothed_latency_initializes_from_first_sample_then_ewmas() {
        let target = AtomicU64::new(LATENCY_MEASUREMENT_UNKNOWN);
        store_smoothed_latency(&target, 40);
        assert_eq!(target.load(Ordering::Relaxed), 40);
        store_smoothed_latency(&target, 20);
        assert_eq!(target.load(Ordering::Relaxed), 35);
    }

    #[test]
    fn clock_age_requires_converged_offset_and_sane_window() {
        let now = wall_clock_ms();
        assert_eq!(
            clock_corrected_age_ms(Some(now), HOST_CLOCK_OFFSET_UNKNOWN_MS),
            None
        );
        assert_eq!(clock_corrected_age_ms(None, 5), None);
        // Host clock 50ms ahead of ours; frame captured 20ms ago => age 70ms.
        assert_eq!(clock_corrected_age_at_ms(Some(now - 20), 50, now), Some(70));
        // A timestamp from the future (negative age) is never fabricated.
        assert_eq!(clock_corrected_age_ms(Some(now + 5_000), 0), None);
    }

    #[test]
    fn latency_wire_value_keeps_the_unknown_sentinel_and_saturates() {
        assert_eq!(
            latency_feedback_value_u16(LATENCY_MEASUREMENT_UNKNOWN),
            u16::MAX
        );
        assert_eq!(latency_feedback_value_u16(5), 5);
        assert_eq!(latency_feedback_value_u16(500_000), u16::MAX - 1);
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::split_final_stop_flags;

    #[test]
    fn final_viewer_close_notifies_host_exactly_once() {
        let flags = split_final_stop_flags(true, -1);
        assert_eq!(flags.into_iter().filter(|send| *send).count(), 1);
        assert_eq!(flags, [true, false]);
    }

    #[test]
    fn transient_detach_and_host_termination_are_silent() {
        assert_eq!(split_final_stop_flags(false, -1), [false, false]);
        assert_eq!(split_final_stop_flags(true, 2), [false, false]);
    }
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn metric_identity() -> String {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock,
    };
    static PROCESS: LazyLock<u128> = LazyLock::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    });
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "{}-{}-{}",
        std::process::id(),
        *PROCESS,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
#[cfg(test)]
mod identity_tests {
    #[test]
    fn each_renderer_owner_has_a_new_nonsecret_incarnation() {
        assert_ne!(super::metric_identity(), super::metric_identity());
    }
}
