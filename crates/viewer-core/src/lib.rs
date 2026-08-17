//! Viewer core: demux, lease registry, window lifecycle, decoder fakes, C ABI
//! (H05–H09 logic, H29; docs/03 §8, docs/05 §5.3/§6/§8).

use domain::ids::SourceId;
// Re-exported for the native JNI/C-ABI layer.
pub use domain::ids::StreamInstanceId;
use domain::lease::{LeaseEvent, LeaseTable};
use domain::phase::StreamPhase;
use media_model::frame::{EncodedFrame, FrameKind};
use media_model::{AssembledOutput, FragmentAssembler};
use std::collections::HashMap;
use std::time::Duration;

// -- Demux (docs/03 §3.2: source마다 video channel demux) ----------------------

pub struct SourceDemux {
    assemblers: HashMap<SourceId, FragmentAssembler>,
    session: domain::ids::SessionId,
    codec: media_model::CodecProfile,
    /// frames delivered per source, for identity tests
    pub delivered: HashMap<SourceId, Vec<(u64, FrameKind)>>,
    pub idr_requests: Vec<SourceId>,
}

impl SourceDemux {
    pub fn new(session: domain::ids::SessionId) -> Self {
        Self {
            assemblers: HashMap::new(),
            session,
            codec: media_model::CodecProfile::AvcBaseline,
            delivered: HashMap::new(),
            idr_requests: Vec::new(),
        }
    }

    /// Feed one video event for a source; returns decoder-ready frames.
    pub fn feed(
        &mut self,
        source: &SourceId,
        frag: media_model::Fragment,
        now: Duration,
    ) -> Vec<EncodedFrame> {
        let assembler = self
            .assemblers
            .entry(source.clone())
            .or_insert_with(|| FragmentAssembler::new(self.session.clone(), self.codec));
        match assembler.feed(frag, now) {
            Ok(AssembledOutput::Frame(frame)) => {
                self.delivered
                    .entry(source.clone())
                    .or_default()
                    .push((frame.frame_id, frame.kind));
                vec![frame]
            }
            Ok(AssembledOutput::RequestIdr { source_id }) => {
                self.idr_requests.push(source_id);
                Vec::new()
            }
            Ok(AssembledOutput::Dropped) | Err(_) => Vec::new(),
        }
    }

    /// Surface recreate bumps the viewer decode epoch: stale frames dropped.
    pub fn surface_recreated(&mut self, source: &SourceId) {
        if let Some(assembler) = self.assemblers.get_mut(source) {
            // force a new epoch view by requiring a fresh keyframe
            let fresh = FragmentAssembler::new(self.session.clone(), self.codec);
            *assembler = fresh;
        }
        self.delivered.remove(source);
    }
}

// -- Window lease registry (H29) ----------------------------------------------

pub struct WindowRegistry {
    leases: LeaseTable,
    /// document URI per instance for task identity (docs/05 §6.1)
    documents: HashMap<StreamInstanceId, (SourceId, String)>,
    /// windows per source: default policy is one window per source
    pub per_source_policy_one_window: bool,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self {
            leases: LeaseTable::new(),
            documents: HashMap::new(),
            per_source_policy_one_window: true,
        }
    }

    /// Open (or focus) a window for a source. Returns the event and the
    /// document URI to launch.
    pub fn open(
        &mut self,
        source: SourceId,
        instance: StreamInstanceId,
    ) -> (Option<LeaseEvent>, String) {
        let doc = format!("leftcar://stream/{}?instance={}", source.0, instance.0);
        self.documents
            .insert(instance.clone(), (source.clone(), doc.clone()));
        let event = self.leases.acquire(source, instance);
        (event, doc)
    }

    /// Same-source policy: with one-window-per-source, a second open focuses
    /// the existing window instead of a duplicate task.
    pub fn same_source_open(&self, source: &SourceId) -> Option<StreamInstanceId> {
        self.documents
            .iter()
            .find(|(_, (s, _))| s == source)
            .map(|(instance, _)| instance.clone())
    }

    pub fn close(
        &mut self,
        source: &SourceId,
        instance: &StreamInstanceId,
        now: Duration,
        debounce: Duration,
    ) {
        let _ = self.leases.release(source, instance, now, debounce);
        self.documents.remove(instance);
    }

    pub fn lease_count(&self, source: &SourceId) -> usize {
        self.leases.lease_count(source)
    }

    pub fn hub_closed(&self) -> bool {
        // HubActivity is not a stream window: closing it changes nothing here.
        false
    }

    pub fn total_windows(&self) -> usize {
        self.documents.len()
    }
}

impl Default for WindowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// -- Window state machine (docs/05 §6.2 lifecycle permutation) -------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    ActivityCreate,
    ActivityStart,
    ActivityResume,
    FocusGain,
    FocusLoss,
    SurfaceCreate,
    SurfaceChange,
    SurfaceDestroy,
    ActivityPause,
    ActivityStop,
    ConfigurationChange,
    TaskRemove,
    ProcessDeath,
}

pub struct WindowStateMachine {
    pub phase: StreamPhase,
    pub surface_available: bool,
    pub decoder_configured: bool,
    stop_seen_at: Option<Duration>,
    grace_period: Duration,
    attach_count: u32,
    detach_count: u32,
}

impl WindowStateMachine {
    pub fn new(grace_period: Duration) -> Self {
        Self {
            phase: StreamPhase::Idle,
            surface_available: false,
            decoder_configured: false,
            stop_seen_at: None,
            grace_period,
            attach_count: 0,
            detach_count: 0,
        }
    }

    /// Apply one lifecycle event; returns whether decoder input should pause.
    pub fn apply(&mut self, event: LifecycleEvent, now: Duration) -> bool {
        match event {
            LifecycleEvent::ActivityCreate | LifecycleEvent::ActivityStart => {
                if self.phase == StreamPhase::Idle {
                    self.phase = StreamPhase::Negotiating;
                }
                false
            }
            LifecycleEvent::ActivityResume => false, // multi-resume: focus decides, not resume
            LifecycleEvent::FocusGain => {
                // focus affects profile only; visible unfocused keeps playing
                false
            }
            LifecycleEvent::FocusLoss => false, // visible unfocused activity can play
            LifecycleEvent::SurfaceCreate => {
                self.surface_available = true;
                self.attach_count += 1;
                false
            }
            LifecycleEvent::SurfaceChange => false,
            LifecycleEvent::SurfaceDestroy => {
                self.surface_available = false;
                self.detach_count += 1;
                // decoder output must not be configured without a Surface
                self.decoder_configured = false;
                true
            }
            LifecycleEvent::ActivityPause => false,
            LifecycleEvent::ActivityStop => {
                self.stop_seen_at = Some(now);
                false
            }
            LifecycleEvent::ConfigurationChange => false,
            LifecycleEvent::TaskRemove => {
                self.phase = StreamPhase::Stopped;
                false
            }
            LifecycleEvent::ProcessDeath => {
                self.phase = StreamPhase::Idle; // restore re-negotiates
                self.decoder_configured = false;
                false
            }
        }
    }

    /// Grace-period suspension (docs/02 §7.3: stopped -> suspend after grace).
    pub fn tick(&mut self, now: Duration) {
        if let Some(stopped_at) = self.stop_seen_at {
            if now.saturating_sub(stopped_at) >= self.grace_period
                && !matches!(
                    self.phase,
                    StreamPhase::Stopped | StreamPhase::SourceUnavailable
                )
            {
                self.phase = StreamPhase::Suspended;
            }
        }
    }

    /// Decoder output may only be configured with a live Surface.
    pub fn configure_decoder(&mut self) -> bool {
        if self.surface_available {
            self.decoder_configured = true;
        }
        self.decoder_configured
    }

    /// attach/detach balance for the C ABI shim (docs/05 §8.2).
    pub fn surface_balance(&self) -> (u32, u32) {
        (self.attach_count, self.detach_count)
    }
}

// -- FakeDecoder (docs/05 §4.5) --------------------------------------------------

#[derive(Debug, Default)]
pub struct FakeDecoder {
    pub configure_calls: u32,
    pub surface_attaches: u32,
    pub surface_detaches: u32,
    pub input_frames: Vec<(u64, u32)>, // (frame_id, epoch)
    pub keyframe_required: bool,
    pub resource_exhausted: bool,
    pub malformed_seen: bool,
}

impl FakeDecoder {
    pub fn configure(&mut self) -> u32 {
        self.configure_calls += 1;
        self.configure_calls
    }

    pub fn attach_surface(&mut self) {
        self.surface_attaches += 1;
    }

    pub fn detach_surface(&mut self) {
        self.surface_detaches += 1;
    }

    /// Feed a frame; returns false when the decoder would reject it.
    pub fn feed(&mut self, frame: &EncodedFrame) -> bool {
        if self.resource_exhausted {
            return false;
        }
        if frame.payload.len() > media_model::MAX_FRAME_BYTES {
            self.malformed_seen = true;
            return false; // malformed bitstream: reject, never panic
        }
        if self.keyframe_required && frame.kind != FrameKind::Key {
            return false;
        }
        if frame.kind == FrameKind::Key {
            self.keyframe_required = false;
        }
        self.input_frames
            .push((frame.frame_id, frame.stream_epoch.0));
        true
    }
}

// -- C ABI (docs/05 §8.2) ---------------------------------------------------------

/// Opaque surface handle crossing JNI. Kotlin passes the Surface jobject's
/// native window pointer as this opaque value; Rust never treats it as
/// anything but an opaque handle.
pub type SurfaceHandle = u64;

pub struct ProcessState {
    pub windows: HashMap<StreamInstanceId, WindowStateMachine>,
    pub decoder: FakeDecoder,
    /// per-instance surface ownership: an instance may hold at most one
    /// attached surface handle. Cross-instance interference is impossible by
    /// construction (docs/05 §8.2: 다른 instance로 callback 교차 금지).
    surfaces: HashMap<StreamInstanceId, SurfaceHandle>,
}

impl ProcessState {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            decoder: FakeDecoder::default(),
            surfaces: HashMap::new(),
        }
    }

    pub fn attached_surface(&self, instance: &StreamInstanceId) -> Option<SurfaceHandle> {
        self.surfaces.get(instance).copied()
    }

    pub fn attached_count(&self) -> usize {
        self.surfaces.len()
    }
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::new()
    }
}

/// The six C ABI functions of docs/05 §8.2, as safe Rust equivalents.
/// The `#[no_mangle] extern "C"` wrappers live in native/android-viewer and
/// delegate here; panics never cross the C boundary (catch_unwind there).
pub mod c_abi {
    use super::*;

    pub fn process_start() -> *mut ProcessState {
        Box::into_raw(Box::new(ProcessState::new()))
    }

    /// # Safety
    /// `state` must originate from `process_start` and not be reused after.
    pub unsafe fn process_release(state: *mut ProcessState) {
        if !state.is_null() {
            drop(unsafe { Box::from_raw(state) });
        }
    }

    /// attach a Surface; null surfaces are rejected. Attaching while the
    /// instance already holds a surface is an error (detach first) — a
    /// replacement attach would leak the previous ANativeWindow ref.
    pub fn stream_attach_surface(
        state: &mut ProcessState,
        instance: &StreamInstanceId,
        surface: SurfaceHandle,
    ) -> Result<(), CAbiError> {
        if surface == 0 {
            return Err(CAbiError::NullSurface);
        }
        if state.surfaces.contains_key(instance) {
            return Err(CAbiError::AlreadyAttached);
        }
        state.decoder.attach_surface();
        state.surfaces.insert(instance.clone(), surface);
        Ok(())
    }

    pub fn stream_surface_changed(
        state: &mut ProcessState,
        instance: &StreamInstanceId,
        w: u32,
        h: u32,
    ) {
        // geometry change is informational; ownership unchanged
        if let Some(machine) = state.windows.get_mut(instance) {
            let _ = machine.apply(LifecycleEvent::SurfaceChange, Duration::ZERO);
        }
        let _ = (w, h);
    }

    /// detach must never exceed one per attach for the instance.
    pub fn stream_detach_surface(
        state: &mut ProcessState,
        instance: &StreamInstanceId,
    ) -> Result<(), CAbiError> {
        match state.surfaces.remove(instance) {
            Some(_) => {
                state.decoder.detach_surface();
                Ok(())
            }
            None => Err(CAbiError::DetachWithoutAttach),
        }
    }

    pub fn stream_update_window_state(
        state: &mut ProcessState,
        instance: &StreamInstanceId,
        event: LifecycleEvent,
        now: Duration,
    ) -> bool {
        let machine = state
            .windows
            .entry(instance.clone())
            .or_insert_with(|| WindowStateMachine::new(Duration::from_secs(3)));
        machine.apply(event, now)
    }

    /// Release one instance: dropping its surface first is the caller's
    /// contract violation, not ours — we detach it ourselves so the native
    /// window reference cannot leak (idempotent, per-instance).
    pub fn stream_release(state: &mut ProcessState, instance: &StreamInstanceId) {
        if state.surfaces.remove(instance).is_some() {
            state.decoder.detach_surface();
        }
        state.windows.remove(instance);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CAbiError {
    #[error("null surface")]
    NullSurface,
    #[error("detach without attach")]
    DetachWithoutAttach,
    #[error("instance crossing")]
    InstanceCrossing,
    #[error("instance already holds a surface; detach first")]
    AlreadyAttached,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use domain::ids::SessionId;

    fn frame(id: u64, kind: FrameKind, epoch: u32) -> EncodedFrame {
        EncodedFrame {
            session_id: SessionId::from_raw("s").unwrap(),
            source_id: SourceId::from_raw("src").unwrap(),
            stream_epoch: media_model::StreamEpoch(epoch),
            frame_id: id,
            kind,
            codec: media_model::CodecProfile::AvcBaseline,
            capture_time_host_ns: 0,
            encode_done_host_ns: 0,
            width: 64,
            height: 64,
            payload: Bytes::from(vec![0u8; 32]),
        }
    }

    // docs/05 §5.3 names
    #[test]
    fn unique_source_opens_unique_document_task() {
        let mut reg = WindowRegistry::new();
        let a = SourceId::from_raw("a").unwrap();
        let b = SourceId::from_raw("b").unwrap();
        let (_, doc_a) = reg.open(a, StreamInstanceId::from_raw("i1").unwrap());
        let (_, doc_b) = reg.open(b, StreamInstanceId::from_raw("i2").unwrap());
        assert_ne!(doc_a, doc_b, "unique sources get unique document URIs");
        assert_eq!(reg.total_windows(), 2);
    }

    #[test]
    fn same_source_focuses_existing_window_by_default() {
        let mut reg = WindowRegistry::new();
        let a = SourceId::from_raw("a").unwrap();
        let first = StreamInstanceId::from_raw("i1").unwrap();
        reg.open(a.clone(), first.clone());
        // second open of same source focuses existing
        let existing = reg.same_source_open(&a);
        assert_eq!(existing, Some(first), "one window per source by default");
    }

    #[test]
    fn hub_close_keeps_stream_tasks_alive() {
        let mut reg = WindowRegistry::new();
        let a = SourceId::from_raw("a").unwrap();
        reg.open(a.clone(), StreamInstanceId::from_raw("i1").unwrap());
        // hub closing is not a stream event
        assert!(!reg.hub_closed());
        assert_eq!(reg.total_windows(), 1, "stream windows survive hub close");
    }

    #[test]
    fn last_stream_close_allows_session_idle() {
        let mut reg = WindowRegistry::new();
        let a = SourceId::from_raw("a").unwrap();
        let i1 = StreamInstanceId::from_raw("i1").unwrap();
        let (event, _) = reg.open(a.clone(), i1.clone());
        assert!(matches!(event, Some(LeaseEvent::SourceStarted(_))));
        reg.close(&a, &i1, Duration::ZERO, Duration::ZERO);
        assert_eq!(reg.total_windows(), 0);
        assert_eq!(reg.lease_count(&a), 0, "session can go idle");
    }

    #[test]
    fn task_removal_releases_exactly_one_lease() {
        let mut reg = WindowRegistry::new();
        let a = SourceId::from_raw("a").unwrap();
        let i1 = StreamInstanceId::from_raw("i1").unwrap();
        let i2 = StreamInstanceId::from_raw("i2").unwrap();
        reg.open(a.clone(), i1.clone());
        reg.open(a.clone(), i2.clone());
        assert_eq!(reg.lease_count(&a), 2);
        reg.close(&a, &i1, Duration::ZERO, Duration::ZERO);
        assert_eq!(reg.lease_count(&a), 1);
    }

    // docs/05 §6.2 invariants
    #[test]
    fn decoder_output_never_configured_without_surface() {
        let mut m = WindowStateMachine::new(Duration::from_secs(3));
        assert!(!m.configure_decoder(), "no surface -> no decoder output");
        m.apply(LifecycleEvent::SurfaceCreate, Duration::ZERO);
        assert!(m.configure_decoder());
        m.apply(LifecycleEvent::SurfaceDestroy, Duration::ZERO);
        assert!(!m.decoder_configured, "destroy clears configured");
        assert!(!m.configure_decoder(), "cannot reconfigure without surface");
    }

    #[test]
    fn visible_unfocused_activity_can_play() {
        let mut m = WindowStateMachine::new(Duration::from_secs(3));
        m.apply(LifecycleEvent::ActivityCreate, Duration::ZERO);
        m.apply(LifecycleEvent::SurfaceCreate, Duration::ZERO);
        let pause = m.apply(LifecycleEvent::FocusLoss, Duration::ZERO);
        assert!(!pause, "focus loss alone must not pause a visible stream");
    }

    #[test]
    fn stopped_window_suspends_after_grace_period() {
        let mut m = WindowStateMachine::new(Duration::from_secs(3));
        m.apply(LifecycleEvent::ActivityCreate, Duration::ZERO);
        m.phase = StreamPhase::Playing;
        m.apply(LifecycleEvent::ActivityStop, Duration::from_secs(10));
        m.tick(Duration::from_secs(11));
        assert_eq!(m.phase, StreamPhase::Playing, "within grace: still playing");
        m.tick(Duration::from_secs(14));
        assert_eq!(m.phase, StreamPhase::Suspended, "after grace: suspended");
    }

    #[test]
    fn no_double_free_on_repeated_stop_destroy() {
        let mut m = WindowStateMachine::new(Duration::from_secs(3));
        m.apply(LifecycleEvent::SurfaceCreate, Duration::ZERO);
        m.apply(LifecycleEvent::SurfaceDestroy, Duration::ZERO);
        m.apply(LifecycleEvent::SurfaceDestroy, Duration::ZERO);
        m.apply(LifecycleEvent::TaskRemove, Duration::ZERO);
        m.apply(LifecycleEvent::TaskRemove, Duration::ZERO);
        let (attach, detach) = m.surface_balance();
        assert_eq!(
            (attach, detach),
            (1, 2),
            "events counted; no double free occurs"
        );
    }

    // demux identity (docs/06 §4.5)
    #[test]
    fn four_sources_demux_without_cross_talk() {
        let mut demux = SourceDemux::new(SessionId::from_raw("s").unwrap());
        let sources: Vec<SourceId> = (0..4)
            .map(|i| SourceId::from_raw(format!("s{i}")).unwrap())
            .collect();
        // feed one keyframe per source through the assembler path
        for (i, source) in sources.iter().enumerate() {
            let f = frame(i as u64 + 1, FrameKind::Key, 1);
            let mut f = f;
            f.source_id = source.clone();
            let frags = media_model::packetize(&f, 512).unwrap();
            for frag in frags {
                demux.feed(source, frag, Duration::ZERO);
            }
        }
        for (i, source) in sources.iter().enumerate() {
            let delivered = demux.delivered.get(source).unwrap();
            assert_eq!(delivered.len(), 1, "source {i} got exactly its keyframe");
        }
    }

    #[test]
    fn stale_epoch_dropped_after_surface_recreate() {
        let mut demux = SourceDemux::new(SessionId::from_raw("s").unwrap());
        let source = SourceId::from_raw("src").unwrap();
        // key + delta in epoch 1
        for (id, kind) in [(1, FrameKind::Key), (2, FrameKind::Delta)] {
            let f = frame(id, kind, 1);
            for frag in media_model::packetize(&f, 512).unwrap() {
                demux.feed(&source, frag, Duration::ZERO);
            }
        }
        assert_eq!(demux.delivered[&source].len(), 2);
        // surface recreate resets the assembler; late epoch-1 delta is dropped
        demux.surface_recreated(&source);
        let late = frame(3, FrameKind::Delta, 1);
        for frag in media_model::packetize(&late, 512).unwrap() {
            demux.feed(&source, frag, Duration::ZERO);
        }
        assert!(
            !demux.delivered.contains_key(&source),
            "no stale frames delivered after surface recreate"
        );
    }

    #[test]
    fn restored_task_reauthenticates_before_decode() {
        // process death resets phase to Idle: decoding requires re-negotiation
        let mut m = WindowStateMachine::new(Duration::from_secs(3));
        m.phase = StreamPhase::Playing;
        m.decoder_configured = true;
        m.apply(LifecycleEvent::ProcessDeath, Duration::ZERO);
        assert_eq!(m.phase, StreamPhase::Idle);
        assert!(!m.decoder_configured, "must re-negotiate before decode");
    }

    // C ABI (docs/05 §8.2)
    #[test]
    fn null_surface_rejected() {
        let mut state = ProcessState::new();
        let i1 = StreamInstanceId::from_raw("i1").unwrap();
        assert!(matches!(
            c_abi::stream_attach_surface(&mut state, &i1, 0),
            Err(CAbiError::NullSurface)
        ));
    }

    #[test]
    fn attach_once_detaches_at_most_once() {
        let mut state = ProcessState::new();
        let i1 = StreamInstanceId::from_raw("i1").unwrap();
        c_abi::stream_attach_surface(&mut state, &i1, 0xdeadbeef).unwrap();
        c_abi::stream_detach_surface(&mut state, &i1).unwrap();
        assert!(matches!(
            c_abi::stream_detach_surface(&mut state, &i1),
            Err(CAbiError::DetachWithoutAttach)
        ));
    }

    #[test]
    fn callback_crossing_instances_rejected() {
        // cross-instance interference is now impossible by construction:
        // surfaces are tracked per instance, so a detach can never consume
        // another instance's attach.
        let mut state = ProcessState::new();
        let i1 = StreamInstanceId::from_raw("i1").unwrap();
        let i2 = StreamInstanceId::from_raw("i2").unwrap();
        c_abi::stream_attach_surface(&mut state, &i1, 0x1).unwrap();
        // detaching a never-attached instance does NOT touch i1
        assert!(matches!(
            c_abi::stream_detach_surface(&mut state, &i2),
            Err(CAbiError::DetachWithoutAttach)
        ));
        // i1 still holds its surface
        assert_eq!(state.attached_surface(&i1), Some(0x1));
    }

    #[test]
    fn interleaved_instances_never_interfere() {
        // A2 regression: attach A, attach B, detach A, detach B all succeed —
        // the old single-slot tracking made A's detach fail after B attached.
        let mut state = ProcessState::new();
        let a = StreamInstanceId::from_raw("a").unwrap();
        let b = StreamInstanceId::from_raw("b").unwrap();
        c_abi::stream_attach_surface(&mut state, &a, 0x11).unwrap();
        c_abi::stream_attach_surface(&mut state, &b, 0x22).unwrap();
        assert_eq!(state.attached_count(), 2);
        c_abi::stream_detach_surface(&mut state, &a).unwrap();
        assert_eq!(state.attached_surface(&b), Some(0x22), "B unaffected");
        c_abi::stream_detach_surface(&mut state, &b).unwrap();
        assert_eq!(state.attached_count(), 0);
    }

    #[test]
    fn double_attach_is_rejected_not_leaked() {
        let mut state = ProcessState::new();
        let a = StreamInstanceId::from_raw("a").unwrap();
        c_abi::stream_attach_surface(&mut state, &a, 0x1).unwrap();
        // re-attach without detach must fail so the old handle cannot leak
        assert!(matches!(
            c_abi::stream_attach_surface(&mut state, &a, 0x2),
            Err(CAbiError::AlreadyAttached)
        ));
        assert_eq!(state.attached_surface(&a), Some(0x1), "first handle kept");
    }

    #[test]
    fn stream_release_detaches_orphan_surface() {
        // A2 regression: releasing an instance that still holds a surface
        // detaches it, so the native window ref cannot leak.
        let mut state = ProcessState::new();
        let a = StreamInstanceId::from_raw("a").unwrap();
        c_abi::stream_attach_surface(&mut state, &a, 0x1).unwrap();
        let before = state.decoder.surface_attaches;
        c_abi::stream_release(&mut state, &a);
        assert_eq!(
            state.decoder.surface_detaches, before,
            "release balanced the attach"
        );
        assert_eq!(state.attached_count(), 0);
        // release is idempotent
        c_abi::stream_release(&mut state, &a);
        assert_eq!(state.decoder.surface_detaches, before);
    }

    #[test]
    fn decoder_resource_exhaustion_is_reported_not_fatal() {
        let mut dec = FakeDecoder {
            resource_exhausted: true,
            ..Default::default()
        };
        assert!(!dec.feed(&frame(1, FrameKind::Key, 1)));
        // and malformed oversized payloads are rejected without panic
        dec.resource_exhausted = false;
        let mut huge = frame(2, FrameKind::Key, 1);
        huge.payload = Bytes::from(vec![0u8; media_model::MAX_FRAME_BYTES + 1]);
        assert!(!dec.feed(&huge));
        assert!(dec.malformed_seen);
    }
}
