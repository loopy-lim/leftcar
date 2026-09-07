//! JNI surface for the Kotlin shim (docs/05 §8.2 JNI 경계).
//!
//! Leftcar JNI rules (docs/07 §14):
//! - null/invalid jobject validated
//! - ANativeWindow acquire/release balanced
//! - exceptions checked/cleared, never leaked across the boundary
//! - panics never cross JNI (catch_unwind everywhere)

#[cfg(target_os = "android")]
use std::ffi::c_char;

use crate::input_protocol::InputScheduler;
use crate::net_guard::host_is_valid;
use crate::prepared_tcp::PreparedTcpBridge;
use crate::prepared_udp::PreparedUdpReceiver;
use crate::usb_bridge::UsbBridge;

pub(crate) const LEFTCAR_OK: i32 = 0;
pub(crate) const LEFTCAR_ERR_NULL: i32 = 1;
pub(crate) const LEFTCAR_ERR_STATE: i32 = 2;
pub(crate) const LEFTCAR_ERR_PANIC: i32 = 3;
pub(crate) const LEFTCAR_ERR_INVALID: i32 = 4;
pub(crate) const LATENCY_UNKNOWN: u64 = u64::MAX;

pub(crate) type StatePtr = *mut viewer_core::ProcessState;

#[cfg(target_os = "android")]
extern "C" {
    fn __android_log_print(prio: i32, tag: *const c_char, fmt: *const c_char, ...) -> i32;
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::jni::android_log_info(format!($($arg)*))
    };
}

// Host test builds compile the receiver handoff helpers below, so their log
// paths must not reference the Android log symbol. Same split as
// prepared_tcp::bridge_log.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn android_log_info(msg: String) {
    if let Ok(c_msg) = std::ffi::CString::new(msg) {
        let tag = b"LeftcarNative\0";
        let fmt = b"%s\0";
        unsafe {
            __android_log_print(
                4,
                tag.as_ptr() as *const c_char,
                fmt.as_ptr() as *const c_char,
                c_msg.as_ptr(),
            );
        }
    }
}

#[cfg(not(target_os = "android"))]
#[doc(hidden)]
pub fn android_log_info(_msg: String) {}

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

pub(crate) struct RendererControl {
    pub(crate) port: u16,
    pub(crate) split: bool,
    pub(crate) input: Mutex<InputScheduler>,
    // -1 = waiting for authenticated Host state, 0 = locked, 1 = enabled.
    pub(crate) input_enabled: AtomicI8,
    pub(crate) rendered_frames: AtomicU64,
    pub(crate) stale_outputs: AtomicU64,
    pub(crate) stale_input_drops: AtomicU64,
    pub(crate) output_burst_discards: AtomicU64,
    pub(crate) decoder_input_drops: AtomicU64,
    pub(crate) frame_gaps: AtomicU64,
    pub(crate) last_feed_us: AtomicU64,
    pub(crate) network_rtt_ms: AtomicU64,
    pub(crate) capture_to_decoder_ms: AtomicU64,
    pub(crate) encode_to_decoder_ms: AtomicU64,
    pub(crate) wire_to_decoder_ms: AtomicU64,
    // Capture wall time of the newest output minus the moment MediaCodec
    // released it to the Surface. This is a compositor-handoff approximation,
    // not panel presentation or true glass-to-glass latency.
    pub(crate) capture_to_surface_release_ms: AtomicU64,
    // Repeated SurfaceView geometry updates during freeform resize can make
    // decoder/compositor stalls look like packet loss. Defer recovery IDRs
    // until the geometry has stayed stable, then emit at most one through the
    // existing RecoveryRequestGate.
    pub(crate) resize_recovery_suppressed_until_us: AtomicU64,
    pub(crate) stop: AtomicBool,
    // Surface destruction is not always the end of the Activity. During
    // freeform resize, release MediaCodec's ANativeWindow promptly but keep
    // the UDP listener alive until either a replacement Surface attaches or
    // the Activity performs its final release.
    pub(crate) suspend: AtomicBool,
    pub(crate) suspended: AtomicBool,
    // A surface can disappear briefly during a freeform resize/reconfiguration.
    // In that case the host must see EOF and use its existing reconnect path,
    // rather than receiving BYE and permanently stopping capture.
    pub(crate) send_bye: AtomicBool,
    // SurfaceHolder.surfaceDestroyed must not return while MediaCodec still
    // owns the ANativeWindow. The callback waits on this bounded flag before
    // releasing the native window reference.
    pub(crate) finished: AtomicBool,
    // Termination reason code: Host LCT1 uses 1 = feedback health check,
    // 2 = operator forced stop, and 3 = ordinary stop. Local watchdogs use
    // 4 = host unreachable and 5 = render stalled. Negative means no notice.
    pub(crate) termination_reason: AtomicI8,
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
}

impl RendererControl {
    pub(crate) fn termination_reason(&self) -> i8 {
        self.termination_reason.load(Ordering::SeqCst)
    }

    pub(crate) fn record_termination_reason(&self, reason: i8) -> i8 {
        crate::record_first_termination_reason(&self.termination_reason, reason)
    }

    pub(crate) fn new_split(port: u16, fps: u32) -> Self {
        Self {
            port,
            split: true,
            input: Mutex::new(InputScheduler::new(fps)),
            input_enabled: AtomicI8::new(-1),
            rendered_frames: AtomicU64::new(0),
            stale_outputs: AtomicU64::new(0),
            stale_input_drops: AtomicU64::new(0),
            output_burst_discards: AtomicU64::new(0),
            decoder_input_drops: AtomicU64::new(0),
            frame_gaps: AtomicU64::new(0),
            last_feed_us: AtomicU64::new(0),
            network_rtt_ms: AtomicU64::new(LATENCY_UNKNOWN),
            capture_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
            encode_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
            wire_to_decoder_ms: AtomicU64::new(LATENCY_UNKNOWN),
            capture_to_surface_release_ms: AtomicU64::new(LATENCY_UNKNOWN),
            resize_recovery_suppressed_until_us: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            suspend: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            send_bye: AtomicBool::new(true),
            finished: AtomicBool::new(false),
            termination_reason: AtomicI8::new(-1),
            cursor_active: AtomicI8::new(-1),
            cursor_x: AtomicU16::new(0),
            cursor_y: AtomicU16::new(0),
            cursor_visible: AtomicBool::new(false),
            cursor_sequence: AtomicU32::new(0),
            cursor_requested: AtomicBool::new(false),
        }
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub(crate) fn request_stop(&self, send_bye: bool) {
        self.send_bye.store(send_bye, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }

    pub(crate) fn mark_finished(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    pub(crate) fn record_split_joined_frames(&self, frames: u64) {
        self.rendered_frames.store(frames, Ordering::Relaxed);
    }

    pub(crate) fn record_split_loss(&self, frame_gaps: u64, input_drops: u64) {
        self.frame_gaps.store(frame_gaps, Ordering::Relaxed);
        self.decoder_input_drops
            .store(input_drops, Ordering::Relaxed);
    }

    pub(crate) fn is_split(&self) -> bool {
        self.split
    }
}

/// The one ownership boundary for a logical renderer instance. Active controls
/// and retained terminal reasons must move together: a replacement install
/// clears an old retained reason while publishing the new generation, and an
/// exiting generation may cache only if it is still current.
#[derive(Default)]
pub(crate) struct RendererLifecycle {
    active_renderers: HashMap<String, Arc<RendererControl>>,
    termination_reasons: HashMap<String, i8>,
}

impl RendererLifecycle {
    pub(crate) fn install_renderer(&mut self, instance: &str, control: Arc<RendererControl>) {
        self.termination_reasons.remove(instance);
        if let Some(old_control) = self.active_renderers.insert(instance.to_owned(), control) {
            old_control.stop.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn remove_renderer_if_current(
        &mut self,
        instance: &str,
        control: &Arc<RendererControl>,
    ) {
        let is_current = self
            .active_renderers
            .get(instance)
            .is_some_and(|current| Arc::ptr_eq(current, control));
        if !is_current {
            return;
        }
        self.active_renderers.remove(instance);
        #[cfg(test)]
        pause_test_after_current_removal(instance);
        if let Some(reason) = crate::cacheable_termination_reason(control.termination_reason()) {
            self.termination_reasons.insert(instance.to_owned(), reason);
        }
    }

    #[cfg(test)]
    pub(crate) fn clear_cached_termination(&mut self, instance: &str) {
        self.termination_reasons.remove(instance);
    }

    pub(crate) fn active_renderer(&self, instance: &str) -> Option<&Arc<RendererControl>> {
        self.active_renderers.get(instance)
    }

    pub(crate) fn active_renderer_for_port(&self, port: u16) -> bool {
        self.active_renderers
            .values()
            .any(|control| control.port == port)
    }

    pub(crate) fn renderers_for_port(&self, port: u16) -> Vec<Arc<RendererControl>> {
        self.active_renderers
            .values()
            .filter(|control| control.port == port)
            .cloned()
            .collect()
    }

    pub(crate) fn termination_reason(&self, instance: &str) -> Option<i8> {
        self.active_renderer(instance)
            .map(|control| control.termination_reason())
            .filter(|reason| *reason >= 0)
            .or_else(|| self.termination_reasons.get(instance).copied())
    }
}

pub(crate) static RENDERER_LIFECYCLE: LazyLock<Mutex<RendererLifecycle>> =
    LazyLock::new(|| Mutex::new(RendererLifecycle::default()));

#[cfg(test)]
type TestLifecycleHook = Arc<dyn Fn() + Send + Sync>;

#[cfg(test)]
struct TestAfterCurrentRemovalHook {
    instance: String,
    callback: TestLifecycleHook,
}

#[cfg(test)]
struct TestBeforeInstallLockHook {
    instance: String,
    callback: TestLifecycleHook,
}

#[cfg(test)]
static TEST_AFTER_CURRENT_REMOVAL_HOOK: LazyLock<Mutex<Option<TestAfterCurrentRemovalHook>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(test)]
static TEST_BEFORE_INSTALL_LOCK_HOOK: LazyLock<Mutex<Option<TestBeforeInstallLockHook>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(test)]
pub(crate) fn set_test_after_current_removal_hook(
    instance: &str,
    callback: impl Fn() + Send + Sync + 'static,
) {
    let mut hook = TEST_AFTER_CURRENT_REMOVAL_HOOK.lock().unwrap();
    assert!(
        hook.is_none(),
        "only one lifecycle test hook may be installed"
    );
    *hook = Some(TestAfterCurrentRemovalHook {
        instance: instance.to_owned(),
        callback: Arc::new(callback),
    });
}

#[cfg(test)]
pub(crate) fn clear_test_after_current_removal_hook() {
    TEST_AFTER_CURRENT_REMOVAL_HOOK.lock().unwrap().take();
}

#[cfg(test)]
pub(crate) fn set_test_before_install_lock_hook(
    instance: &str,
    callback: impl Fn() + Send + Sync + 'static,
) {
    let mut hook = TEST_BEFORE_INSTALL_LOCK_HOOK.lock().unwrap();
    assert!(
        hook.is_none(),
        "only one lifecycle test hook may be installed"
    );
    *hook = Some(TestBeforeInstallLockHook {
        instance: instance.to_owned(),
        callback: Arc::new(callback),
    });
}

#[cfg(test)]
pub(crate) fn clear_test_before_install_lock_hook() {
    TEST_BEFORE_INSTALL_LOCK_HOOK.lock().unwrap().take();
}

#[cfg(test)]
fn pause_test_after_current_removal(instance: &str) {
    let callback = TEST_AFTER_CURRENT_REMOVAL_HOOK
        .lock()
        .unwrap()
        .as_ref()
        .filter(|hook| hook.instance == instance)
        .map(|hook| Arc::clone(&hook.callback));
    if let Some(callback) = callback {
        callback();
    }
}

#[cfg(test)]
fn run_test_before_install_lock_hook(instance: &str) {
    let callback = TEST_BEFORE_INSTALL_LOCK_HOOK
        .lock()
        .unwrap()
        .as_ref()
        .filter(|hook| hook.instance == instance)
        .map(|hook| Arc::clone(&hook.callback));
    if let Some(callback) = callback {
        callback();
    }
}

pub(crate) static PREPARED_RECEIVERS: Mutex<Option<HashMap<u16, PreparedUdpReceiver>>> =
    Mutex::new(None);
pub(crate) static PREPARED_TCP_BRIDGES: Mutex<Option<HashMap<u16, PreparedTcpBridge>>> =
    Mutex::new(None);
pub(crate) static PREPARED_USB_BRIDGE: Mutex<Option<UsbBridge>> = Mutex::new(None);
pub(crate) enum MediaBridge {
    Tcp(PreparedTcpBridge),
    Usb(UsbBridge),
}

impl MediaBridge {
    pub(crate) fn control_addr(&self) -> std::net::SocketAddr {
        match self {
            Self::Tcp(bridge) => bridge.control_addr(),
            Self::Usb(bridge) => bridge.control_addr(),
        }
    }

    pub(crate) fn drain_media(&self) {
        match self {
            Self::Tcp(bridge) => bridge.drain_media(),
            Self::Usb(bridge) => bridge.drain_media(),
        }
    }

    pub(crate) fn recv_media_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            Self::Tcp(bridge) => bridge.recv_media_timeout(timeout),
            Self::Usb(bridge) => bridge.recv_media_timeout(timeout),
        }
    }
}

pub(crate) fn remove_renderer_if_current(instance: &str, control: &Arc<RendererControl>) {
    RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .remove_renderer_if_current(instance, control);
}

pub(crate) fn install_renderer(instance: &str, control: Arc<RendererControl>) {
    #[cfg(test)]
    run_test_before_install_lock_hook(instance);
    RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .install_renderer(instance, control);
}

#[cfg(test)]
pub(crate) fn clear_cached_termination(instance: &str) {
    RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .clear_cached_termination(instance);
}

pub(crate) fn active_renderer(instance: &str) -> Option<Arc<RendererControl>> {
    RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .active_renderer(instance)
        .cloned()
}

pub(crate) fn renderer_termination_reason(instance: &str) -> Option<i8> {
    RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .termination_reason(instance)
}

pub(crate) fn wait_for_renderer(control: &RendererControl) {
    // Accepted socket reads are bounded to 300 ms. Leave additional margin
    // for MediaCodec_stop/delete without hanging the Android UI indefinitely.
    for _ in 0..40 {
        if control.finished.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Stop any renderer still holding the UDP port and wait (bounded) for its
/// thread to release the socket. Without this, a re-attached surface races
/// the old thread and `bind` fails with Address-in-use.
pub(crate) fn reclaim_udp_port(port: u16) {
    let running = RENDERER_LIFECYCLE.lock().unwrap().renderers_for_port(port);
    for control in running {
        control.send_bye.store(false, Ordering::SeqCst);
        control.suspend.store(false, Ordering::SeqCst);
        control.stop.store(true, Ordering::SeqCst);
        wait_for_renderer(&control);
    }
}

pub(crate) fn cancel_prepared_receiver(port: u16) -> bool {
    let prepared = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    prepared.is_some()
}

fn prepare_tcp_bridge(port: u16, expected_host: &str, transport: &str) -> Result<(), String> {
    let (bind_host, allowed_hosts) = if matches!(transport, "tcp" | "auto") {
        // Auto must be able to accept the direct Wi-Fi attempt first and the
        // loopback ADB fallback later. Admission is still restricted to the
        // paired Host address plus loopback; binding broadly does not broaden
        // the authenticated peer set.
        ("0.0.0.0", format!("{expected_host},127.0.0.1"))
    } else {
        ("127.0.0.1", "127.0.0.1".to_owned())
    };
    let bridge = PreparedTcpBridge::bind(port, bind_host, &allowed_hosts).map_err(|error| {
        format!("failed to prepare {transport} TCP media bridge on {bind_host}:{port}: {error}")
    })?;
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(port, bridge);
    Ok(())
}

pub(crate) fn cancel_tcp_bridge(port: u16) -> bool {
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port)
        .is_some()
}

fn take_tcp_bridge(port: u16) -> Option<PreparedTcpBridge> {
    PREPARED_TCP_BRIDGES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port)
}

pub(crate) fn take_media_bridge(port: u16) -> Option<MediaBridge> {
    take_tcp_bridge(port).map(MediaBridge::Tcp).or_else(|| {
        PREPARED_USB_BRIDGE
            .lock()
            .unwrap()
            .take()
            .map(MediaBridge::Usb)
    })
}

pub(crate) fn prepare_udp_receiver(
    port: u16,
    expected_host: &str,
    transport: &str,
) -> Result<(), String> {
    if port == 0 || !host_is_valid(expected_host) {
        return Err("invalid prepared media port or host".into());
    }

    // A Host restart does not send a terminal packet to an existing UDP
    // renderer. The old Activity therefore keeps the media port and its
    // decoder alive while the control-plane recovery tries to prepare the
    // same port again. Reclaim that logical stream before binding the
    // replacement preflight listener; the subsequent Activity recreation
    // will attach a fresh renderer to the new Host session.
    reclaim_udp_port(port);

    let active_port = RENDERER_LIFECYCLE
        .lock()
        .unwrap()
        .active_renderer_for_port(port);
    if active_port {
        return Err(format!("UDP media port {port} is already active"));
    }

    // A retry for the same not-yet-opened window replaces both preflight
    // listeners, not only the UDP half.
    let _ = cancel_tcp_bridge(port);

    // A retry for the same not-yet-opened window replaces its old preflight.
    // Drop outside the map lock because the worker has a bounded join.
    let stale = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    drop(stale);

    let prepared_hosts = if matches!(transport, "tcp" | "adbTcp" | "usb" | "auto") {
        format!("{expected_host},127.0.0.1")
    } else {
        expected_host.to_owned()
    };
    let prepared = PreparedUdpReceiver::bind(port, prepared_hosts)
        .map_err(|error| format!("failed to prepare UDP media port {port}: {error}"))?;
    PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(port, prepared);
    if matches!(transport, "tcp" | "adbTcp" | "auto") {
        if let Err(error) = prepare_tcp_bridge(port, expected_host, transport) {
            let _ = cancel_prepared_receiver(port);
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn take_prepared_receiver(
    port: u16,
    expected_host: &str,
) -> Option<PreparedUdpReceiver> {
    let prepared = PREPARED_RECEIVERS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .remove(&port);
    match prepared {
        Some(prepared)
            if prepared
                .expected_host()
                .split(',')
                .map(str::trim)
                .any(|host| host == expected_host) =>
        {
            Some(prepared)
        }
        Some(_) => {
            log_info!("discarded prepared UDP port {port}: paired Host changed");
            None
        }
        None => None,
    }
}

/// Both split-stream preflight receivers claimed atomically.
pub(crate) struct SplitPreparedReceivers {
    pub(crate) left: PreparedUdpReceiver,
    pub(crate) right: PreparedUdpReceiver,
}

/// All-or-nothing claim of the two split-stream preflight sockets.
///
/// Splitting the claim across two bare `take_prepared_receiver` calls lost a
/// socket forever when only one half was present: the caller dropped the
/// taken half on the error return, closing the UDP listener, and every retry
/// then failed with LEFTCAR_ERR_STATE. Here either both sockets come back,
/// or the store is left exactly as it was.
pub(crate) fn take_split_receivers(
    left_port: u16,
    right_port: u16,
    expected_host: &str,
) -> Option<SplitPreparedReceivers> {
    let left = take_prepared_receiver(left_port, expected_host)?;
    match take_prepared_receiver(right_port, expected_host) {
        Some(right) => Some(SplitPreparedReceivers { left, right }),
        None => {
            restore_prepared_receiver(left_port, left);
            None
        }
    }
}

/// Return a claimed receiver so a later attach can retry. Never overwrites a
/// receiver another thread prepared in the meantime: the stale one is dropped
/// instead of clobbering the fresher socket.
pub(crate) fn restore_prepared_receiver(port: u16, receiver: PreparedUdpReceiver) -> bool {
    let mut receivers = PREPARED_RECEIVERS.lock().unwrap();
    match receivers.get_or_insert_with(HashMap::new).entry(port) {
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(receiver);
            true
        }
        std::collections::hash_map::Entry::Occupied(_) => false,
    }
}

#[cfg(test)]
mod renderer_lifecycle_tests {
    use super::*;
    use std::sync::{mpsc, Arc, Mutex};

    struct LifecycleTestHarness {
        release_cleanup: Option<mpsc::Sender<()>>,
        cleanup: Option<std::thread::JoinHandle<()>>,
        installer: Option<std::thread::JoinHandle<()>>,
    }

    impl LifecycleTestHarness {
        fn new(release_cleanup: mpsc::Sender<()>) -> Self {
            Self {
                release_cleanup: Some(release_cleanup),
                cleanup: None,
                installer: None,
            }
        }

        fn set_cleanup(&mut self, cleanup: std::thread::JoinHandle<()>) {
            self.cleanup = Some(cleanup);
        }

        fn set_installer(&mut self, installer: std::thread::JoinHandle<()>) {
            self.installer = Some(installer);
        }

        fn unregister_hooks(&self) {
            clear_test_after_current_removal_hook();
            clear_test_before_install_lock_hook();
        }

        fn release_cleanup(&mut self) {
            if let Some(release_cleanup) = self.release_cleanup.take() {
                let _ = release_cleanup.send(());
            }
        }

        fn join_threads(&mut self) {
            if let Some(cleanup) = self.cleanup.take() {
                let _ = cleanup.join();
            }
            if let Some(installer) = self.installer.take() {
                let _ = installer.join();
            }
        }

        fn finish(&mut self) {
            self.unregister_hooks();
            self.release_cleanup();
            self.join_threads();
        }
    }

    impl Drop for LifecycleTestHarness {
        fn drop(&mut self) {
            self.finish();
        }
    }

    #[test]
    fn replacement_install_waits_for_current_removal_cache_transition() {
        let instance = "renderer-lifecycle-after-remove-before-cache-round-5";
        let old = Arc::new(RendererControl::new_split(51_001, 60));
        old.termination_reason
            .store(crate::LOCAL_TERMINATION_RENDER_STALLED, Ordering::SeqCst);
        let new = Arc::new(RendererControl::new_split(51_001, 60));

        install_renderer(instance, Arc::clone(&old));

        let (old_removed, old_removed_rx) = mpsc::channel();
        let (release_cleanup, release_cleanup_rx) = mpsc::channel();
        let mut harness = LifecycleTestHarness::new(release_cleanup.clone());
        let release_cleanup_rx = Arc::new(Mutex::new(release_cleanup_rx));
        set_test_after_current_removal_hook(instance, {
            let release_cleanup_rx = Arc::clone(&release_cleanup_rx);
            move || {
                let _ = old_removed.send(());
                if let Ok(release_cleanup_rx) = release_cleanup_rx.lock() {
                    let _ = release_cleanup_rx.recv();
                }
            }
        });

        let cleanup_old = Arc::clone(&old);
        let cleanup = std::thread::spawn(move || {
            remove_renderer_if_current(instance, &cleanup_old);
        });
        harness.set_cleanup(cleanup);

        old_removed_rx.recv().unwrap();
        let (install_lock_checked, install_lock_checked_rx) = mpsc::channel();
        set_test_before_install_lock_hook(instance, {
            move || {
                let lock_is_held_by_cleanup = matches!(
                    RENDERER_LIFECYCLE.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                );
                let _ = install_lock_checked.send(lock_is_held_by_cleanup);
                let _ = release_cleanup.send(());
                assert!(
                    lock_is_held_by_cleanup,
                    "cleanup must own the install lock boundary"
                );
            }
        });

        let install_new = Arc::clone(&new);
        let install = std::thread::spawn(move || {
            install_renderer(instance, install_new);
        });
        harness.set_installer(install);

        assert!(install_lock_checked_rx.recv().unwrap());
        harness.finish();

        assert!(Arc::ptr_eq(
            active_renderer(instance).as_ref().unwrap(),
            &new
        ));
        assert_eq!(renderer_termination_reason(instance), None);

        remove_renderer_if_current(instance, &new);
        clear_cached_termination(instance);
    }
}

#[cfg(test)]
mod split_receiver_tests {
    use super::*;
    use crate::prepared_udp::PreparedUdpReceiver;

    const HOST: &str = "127.0.0.1";

    // PREPARED_RECEIVERS is process-global; these tests assert absolute store
    // counts, so they serialize on this lock instead of racing each other.
    static STORE_LOCK: Mutex<()> = Mutex::new(());

    fn bind_prepared() -> PreparedUdpReceiver {
        PreparedUdpReceiver::bind(0, HOST.to_owned()).unwrap()
    }

    fn insert_prepared(port: u16, receiver: PreparedUdpReceiver) {
        PREPARED_RECEIVERS
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(port, receiver);
    }

    fn store_len() -> usize {
        PREPARED_RECEIVERS
            .lock()
            .unwrap()
            .as_ref()
            .map(|receivers| receivers.len())
            .unwrap_or(0)
    }

    #[test]
    fn split_claim_returns_both_receivers_and_empties_the_store() {
        let _serial = STORE_LOCK.lock().unwrap();
        let left = bind_prepared();
        let right = bind_prepared();
        let (left_port, right_port) = (left.port().unwrap(), right.port().unwrap());
        insert_prepared(left_port, left);
        insert_prepared(right_port, right);

        let claimed =
            take_split_receivers(left_port, right_port, HOST).expect("both halves prepared");
        assert_eq!(claimed.left.port().unwrap(), left_port);
        assert_eq!(claimed.right.port().unwrap(), right_port);
        assert_eq!(store_len(), 0, "claim must drain both preflight sockets");
    }

    #[test]
    fn split_claim_with_missing_right_restores_the_left_socket() {
        let _serial = STORE_LOCK.lock().unwrap();
        let left = bind_prepared();
        let left_port = left.port().unwrap();
        insert_prepared(left_port, left);

        assert!(
            take_split_receivers(left_port, left_port.wrapping_add(1), HOST).is_none(),
            "missing right half must fail the whole claim"
        );
        // The left socket must be back in the store, still bound and still
        // match the host — a retried attach can claim it again.
        assert_eq!(store_len(), 1);
        let restored = take_prepared_receiver(left_port, HOST).expect("left socket restored");
        assert_eq!(restored.port().unwrap(), left_port);
    }

    #[test]
    fn split_claim_failure_then_retry_succeeds_after_right_is_prepared() {
        let _serial = STORE_LOCK.lock().unwrap();
        let left = bind_prepared();
        let left_port = left.port().unwrap();
        insert_prepared(left_port, left);

        // First attempt: right half missing, nothing may be lost.
        assert!(take_split_receivers(left_port, left_port.wrapping_add(1), HOST).is_none());
        assert_eq!(store_len(), 1);

        // Retry after the right listener has been prepared.
        let right = bind_prepared();
        let right_port = right.port().unwrap();
        insert_prepared(right_port, right);
        let claimed =
            take_split_receivers(left_port, right_port, HOST).expect("retry claims both halves");
        drop(claimed);
        assert_eq!(store_len(), 0);
    }

    #[test]
    fn restore_never_clobbers_a_fresher_prepared_receiver() {
        let _serial = STORE_LOCK.lock().unwrap();
        let stale = bind_prepared();
        let fresh = bind_prepared();
        let port = fresh.port().unwrap();
        insert_prepared(port, fresh);

        assert!(
            !restore_prepared_receiver(port, stale),
            "restoring over a live receiver must be refused"
        );
        let stored = take_prepared_receiver(port, HOST).expect("fresh receiver retained");
        assert_eq!(stored.port().unwrap(), port);
    }

    #[test]
    fn split_claim_with_no_prepared_sockets_leaves_store_untouched() {
        let _serial = STORE_LOCK.lock().unwrap();
        let before = store_len();
        let left = bind_prepared();
        let right = bind_prepared();
        assert!(take_split_receivers(left.port().unwrap(), right.port().unwrap(), HOST).is_none());
        assert_eq!(store_len(), before);
    }
}
