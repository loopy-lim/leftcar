//! Single-surface renderer session and its bounded UDP/TCP receive loop.

use crate::input_protocol::{
    encode_input, encode_latency_probe, encode_receiver_feedback, estimate_latency, parse_ack,
    parse_input_status, parse_latency_probe_response, parse_termination, InputEvent,
    InputScheduler, ReceiverFeedback,
};
use crate::jni::*;
use crate::log_info;
use crate::media_datagram::{
    classify_frame_gap, parse_fragment, parse_parity, recovery_request_suppressed,
    rendered_fps_from_feedback, select_live_edge_frames, should_feed_frame,
    should_resync_after_network_loss, stale_frame_budget_ms, stale_streak_advance,
    CompletedFecGroups, CompletedFrameSequencer, FecGroup, FrameFragment, FrameGapReason,
    FrameReassembler, ReassembledFrame, ReceiverPressure, RecoveryRequestGate, RestoredFragment,
    PARITY_MARKER,
};
use crate::net_guard::peer_allowed;
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

mod decoder;
mod feed;
mod feedback;
mod frame_queue;
mod health;
mod network;
mod presentation;

use decoder::*;
use feed::*;
use feedback::*;
use frame_queue::*;
use health::*;
use network::*;
use presentation::*;

pub(crate) fn suppress_resize_recovery(instance_str: &str) {
    let control = active_renderer(instance_str);
    if let Some(control) = control {
        control.resize_recovery_suppressed_until_us.fetch_max(
            monotonic_us().saturating_add(RESIZE_RECOVERY_SUPPRESSION_US),
            Ordering::Relaxed,
        );
    }
}

const MEDIA_BATCH_SIZE: usize = 16;
const MEDIA_DATAGRAM_BYTES: usize = 2_048;
/// Media is disposable. Waiting for a codec slot or output buffer would make
/// every newer frame arrive behind an older one, so the hot path is strictly
/// non-blocking and recovers from a missed AU at the next IDR.
const DECODER_FEED_TIMEOUT_US: i64 = 0;

mod runtime;

pub(crate) use runtime::spawn_live_stream_renderer;

pub(crate) fn suspend_live_stream_renderer(instance_str: &str) {
    let control = active_renderer(instance_str);
    if let Some(control) = control {
        control.suspend.store(true, Ordering::SeqCst);
        // The socket read timeout is 100 ms. Wait until the decoder has been
        // dropped before releasing the native window reference.
        for _ in 0..40 {
            if control.suspended.load(Ordering::SeqCst) || control.finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

pub(crate) fn stop_live_stream_renderer(instance_str: &str, send_bye: bool) {
    let control = active_renderer(instance_str);
    if let Some(control) = control {
        // Activity.onDestroy follows a host-initiated finish. Preserve the
        // renderer's earlier decision not to send BYE back to a host that has
        // already torn the session down.
        let should_send_bye = send_bye && control.termination_reason() < 0;
        control.send_bye.store(should_send_bye, Ordering::SeqCst);
        control.suspend.store(false, Ordering::SeqCst);
        control.stop.store(true, Ordering::SeqCst);
        wait_for_renderer(&control);
    }
}
