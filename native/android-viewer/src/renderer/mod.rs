pub mod fec_stats;
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
#[path = "split_session/gap_policy.rs"]
pub(crate) mod split_gap_policy;
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
