//! Host core: source registry, pipeline orchestration, teardown (H15–H20 fake path).
//!
//! Real ScreenCaptureKit/VideoToolbox adapters live behind platform facades
//! (macos-capture/macos-encode); this crate is validated with fakes (E1/E2).

use domain::ids::{SessionId, SourceId};
use domain::lease::{LeaseEvent, LeaseTable};
use domain::source::{SourceDescriptor, SourceKind, SourceRegistry};
use media_model::backpressure::{BoundedAuQueue, LatestFrameSlot};
use media_model::fragment::packetize;
use media_model::frame::{EncodedFrame, FrameKind};
use std::collections::HashMap;
use std::time::Duration;

// -- Fakes (docs/05 §4.2–4.3) -------------------------------------------------

/// FakeCapture scripts (docs/05 4.2).
#[derive(Debug, Clone)]
pub enum CaptureScript {
    FixedColor(u8),
    MovingPattern { frames: usize },
    ResizeSequence(Vec<(u32, u32)>),
    NoChangeInterval(usize),
    SourceDisappears,
    PermissionRevoked,
    TimestampDiscontinuity,
    Burst(usize),
}

pub struct FakeCapture {
    pub script: CaptureScript,
    pub running: bool,
    pub frame_count: usize,
}

impl FakeCapture {
    pub fn new(script: CaptureScript) -> Self {
        Self {
            script,
            running: false,
            frame_count: 0,
        }
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Produce the next raw frame per the script; None when the script says
    /// the source is gone or permission is revoked.
    pub fn next_frame(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        if !self.running {
            return None;
        }
        self.frame_count += 1;
        match &self.script {
            CaptureScript::FixedColor(c) => Some((64, 64, vec![*c; 64])),
            CaptureScript::MovingPattern { .. } => {
                Some((64, 64, vec![(self.frame_count % 256) as u8; 64]))
            }
            CaptureScript::ResizeSequence(sizes) => {
                let (w, h) = sizes[self.frame_count % sizes.len()];
                Some((w, h, vec![0u8; (w * h) as usize / 16]))
            }
            CaptureScript::NoChangeInterval(n) => {
                if self.frame_count.is_multiple_of(*n) {
                    Some((64, 64, vec![1u8; 64]))
                } else {
                    None // no change
                }
            }
            CaptureScript::SourceDisappears => None,
            CaptureScript::PermissionRevoked => None,
            CaptureScript::TimestampDiscontinuity => Some((64, 64, vec![2u8; 64])),
            CaptureScript::Burst(n) => Some((64, 64, vec![3u8; 64 * (*n).min(4)])),
        }
    }
}

/// FakeEncoder models frame dependency, not bytes (docs/05 4.3).
#[derive(Debug, Default)]
pub struct FakeEncoder {
    pub epoch: u32,
    pub next_frame_id: u64,
    pub config_emitted: bool,
}

impl FakeEncoder {
    pub fn new() -> Self {
        Self {
            epoch: 1,
            next_frame_id: 1,
            config_emitted: false,
        }
    }

    pub fn encode(
        &mut self,
        source: &SourceId,
        session: &SessionId,
        kind: FrameKind,
        raw: &[u8],
    ) -> EncodedFrame {
        let id = self.next_frame_id;
        self.next_frame_id += 1;
        EncodedFrame {
            session_id: session.clone(),
            source_id: source.clone(),
            stream_epoch: media_model::StreamEpoch(self.epoch),
            frame_id: id,
            kind,
            codec: media_model::CodecProfile::AvcBaseline,
            capture_time_host_ns: id * 1_000_000,
            encode_done_host_ns: id * 1_000_000 + 100_000,
            width: 64,
            height: 64,
            payload: raw.to_vec().into(),
        }
    }

    /// Simulate encoder restart: epoch bumps (docs/03 6.1).
    pub fn restart(&mut self) {
        self.epoch += 1;
        self.config_emitted = false;
    }
}

// -- Orchestrator (docs/03 §13) -------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("source not approved")]
    NotApproved,
    #[error("capture failure")]
    CaptureFailure,
}

/// Per-source pipeline state with bounded queues (docs/03 6.3).
pub struct SourcePipeline {
    pub capture: FakeCapture,
    pub encoder: FakeEncoder,
    pub capture_slot: LatestFrameSlot,
    pub au_queue: BoundedAuQueue,
    pub fragments_out: Vec<media_model::Fragment>,
}

/// Orchestrates all approved sources for a session.
pub struct Orchestrator {
    pub session_id: SessionId,
    registry: SourceRegistry,
    leases: LeaseTable,
    pipelines: HashMap<SourceId, SourcePipeline>,
    /// idempotent teardown bookkeeping
    closed: bool,
}

impl Orchestrator {
    pub fn new(session_id: SessionId, sources: Vec<SourceDescriptor>) -> Self {
        Self {
            session_id,
            registry: SourceRegistry::new(sources),
            leases: LeaseTable::new(),
            pipelines: HashMap::new(),
            closed: false,
        }
    }

    /// Lease acquisition starts capture only for approved sources. Returns
    /// the lease event and the instance id that owns the lease.
    pub fn open_window(
        &mut self,
        source: &SourceId,
        script: CaptureScript,
    ) -> Result<(Option<LeaseEvent>, domain::ids::StreamInstanceId), OrchestratorError> {
        if self.closed {
            return Ok((None, domain::ids::StreamInstanceId::generate()));
        }
        let approved = self
            .registry
            .snapshot()
            .sources
            .iter()
            .any(|s| &s.id == source && s.is_approved);
        if !approved {
            return Err(OrchestratorError::NotApproved);
        }
        let instance = domain::ids::StreamInstanceId::generate();
        let event = self.leases.acquire(source.clone(), instance.clone());
        if matches!(event, Some(LeaseEvent::SourceStarted(_))) {
            let mut capture = FakeCapture::new(script);
            capture.start();
            self.pipelines.insert(
                source.clone(),
                SourcePipeline {
                    capture,
                    encoder: FakeEncoder::new(),
                    capture_slot: LatestFrameSlot::new(),
                    au_queue: BoundedAuQueue::new(2),
                    fragments_out: Vec::new(),
                },
            );
        }
        Ok((event, instance))
    }

    /// One pipeline tick for a source: capture -> slot -> encode -> AU queue
    /// -> fragments. Returns fragments ready for the transport.
    pub fn pump_source(
        &mut self,
        source: &SourceId,
    ) -> Result<&[media_model::Fragment], OrchestratorError> {
        if self.closed {
            return Ok(&[]);
        }
        let session = self.session_id.clone();
        let Some(pipeline) = self.pipelines.get_mut(source) else {
            return Err(OrchestratorError::NotApproved);
        };
        // capture -> encoder boundary: latest-frame policy
        if let Some((w, h, data)) = pipeline.capture.next_frame() {
            let _ = (w, h);
            let kind = if pipeline.capture.frame_count == 1 {
                FrameKind::Key
            } else {
                FrameKind::Delta
            };
            let frame = pipeline.encoder.encode(source, &session, kind, &data);
            let evicted = pipeline.au_queue.push(frame);
            // evicted deltas are dropped (oldest delta); keys handled upstream
            let _ = evicted;
        }
        // AU queue -> packetizer
        pipeline.fragments_out.clear();
        while let Some(au) = pipeline.au_queue.pop() {
            match packetize(&au, 1200) {
                Ok(frags) => pipeline.fragments_out.extend(frags),
                Err(_) => continue, // oversized frames are dropped, never panic
            }
        }
        Ok(&pipeline.fragments_out)
    }

    /// Capture failure on one source must not stop others (NFR-005).
    pub fn fail_source(&mut self, source: &SourceId) {
        if let Some(p) = self.pipelines.get_mut(source) {
            p.capture.script = CaptureScript::SourceDisappears;
        }
    }

    pub fn is_source_active(&self, source: &SourceId) -> bool {
        self.pipelines.contains_key(source)
    }

    pub fn active_source_count(&self) -> usize {
        self.pipelines.len()
    }

    /// Close one window; last lease stops the pipeline after debounce.
    pub fn close_window(
        &mut self,
        source: &SourceId,
        instance: &domain::ids::StreamInstanceId,
        now: Duration,
        debounce: Duration,
    ) -> Option<LeaseEvent> {
        let _ = self.leases.release(source, instance, now, debounce);
        if self.leases.lease_count(source) == 0 {
            // stop pipeline (debounce modeled as immediate here; LeaseTable
            // owns the real timing bookkeeping)
            self.pipelines.remove(source);
        }
        None
    }

    /// Session teardown (docs/03 §13): idempotent, stops all concurrently.
    pub fn stop_all(&mut self) {
        if self.closed {
            return; // stop_all_is_idempotent
        }
        self.closed = true;
        for pipeline in self.pipelines.values_mut() {
            pipeline.capture.stop();
            pipeline.fragments_out.clear();
        }
        self.pipelines.clear();
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn registry(&self) -> &SourceRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut SourceRegistry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ids::StreamInstanceId;

    fn approved(n: &str) -> SourceDescriptor {
        SourceDescriptor {
            id: SourceId::from_raw(n).unwrap(),
            kind: SourceKind::Window,
            display_name: n.into(),
            application_name: None,
            width_px: 1920,
            height_px: 1080,
            is_approved: true,
            is_available: true,
            revision: 1,
        }
    }

    // docs/05 §5.2 names
    #[test]
    fn unapproved_source_cannot_start() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let ghost = SourceId::from_raw("ghost").unwrap();
        assert!(matches!(
            orch.open_window(&ghost, CaptureScript::FixedColor(1)),
            Err(OrchestratorError::NotApproved)
        ));
    }

    #[test]
    fn approved_source_starts_once_for_first_lease() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        let (first, _inst) = orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        assert!(matches!(first, Some(LeaseEvent::SourceStarted(_))));
        assert_eq!(orch.active_source_count(), 1);
        // second viewer lease on same source: no duplicate capture
        let (second, _) = orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        assert!(second.is_none(), "no duplicate capture for second lease");
        assert_eq!(
            orch.active_source_count(),
            1,
            "H25: policy reuse without duplicate capture"
        );
    }

    #[test]
    fn one_capture_failure_does_not_stop_other_sources() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a"), approved("b")]);
        let a = SourceId::from_raw("a").unwrap();
        let b = SourceId::from_raw("b").unwrap();
        orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        orch.open_window(&b, CaptureScript::FixedColor(2)).unwrap();
        orch.fail_source(&a);
        assert!(orch.is_source_active(&b), "b survives a's failure");
        let frags = orch.pump_source(&b).unwrap();
        assert!(!frags.is_empty(), "b still produces");
    }

    #[test]
    fn stop_all_is_idempotent() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        orch.stop_all();
        orch.stop_all(); // second call is a no-op, not a panic
        assert!(orch.is_closed());
        assert_eq!(orch.active_source_count(), 0);
    }

    #[test]
    fn pump_produces_keyframe_first() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        orch.open_window(&a, CaptureScript::MovingPattern { frames: 10 })
            .unwrap();
        let frags = orch.pump_source(&a).unwrap();
        assert!(!frags.is_empty());
        assert!(
            matches!(frags[0].header.kind, FrameKind::Key),
            "first frame is a keyframe"
        );
        // second pump produces a delta
        let frags = orch.pump_source(&a).unwrap();
        assert!(matches!(frags[0].header.kind, FrameKind::Delta));
    }

    #[test]
    fn no_orphan_capture_after_close() {
        // T-11: session close stops capture; reopening after close is rejected
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        orch.stop_all();
        let (after, _) = orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        assert!(after.is_none(), "no new windows after session close");
        assert_eq!(orch.active_source_count(), 0, "no orphan capture");
    }

    #[test]
    fn epoch_increments_on_encoder_restart() {
        let mut enc = FakeEncoder::new();
        assert_eq!(enc.epoch, 1);
        enc.restart();
        assert_eq!(enc.epoch, 2, "docs/03 6.1: restart bumps stream_epoch");
    }

    #[test]
    fn last_lease_stops_capture_after_debounce() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        let (_event, instance) = orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        orch.close_window(&a, &instance, Duration::ZERO, Duration::from_secs(1));
        assert!(!orch.is_source_active(&a), "last lease stops the pipeline");
    }

    #[test]
    fn task_removal_releases_exactly_one_lease() {
        let mut orch = Orchestrator::new(SessionId::generate(), vec![approved("a")]);
        let a = SourceId::from_raw("a").unwrap();
        let (_, first_instance) = orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        orch.open_window(&a, CaptureScript::FixedColor(1)).unwrap();
        assert_eq!(orch.leases.lease_count(&a), 2);
        orch.close_window(&a, &first_instance, Duration::ZERO, Duration::ZERO);
        assert_eq!(orch.leases.lease_count(&a), 1, "one release = one lease");
        // releasing an unknown instance is a no-op
        orch.close_window(
            &a,
            &StreamInstanceId::generate(),
            Duration::ZERO,
            Duration::ZERO,
        );
        assert_eq!(orch.leases.lease_count(&a), 1);
    }
}
