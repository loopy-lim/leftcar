use super::fec_stats::FecRuntimeStats;
use super::presentation_sync::{
    drain_available_events, PairPresentationCoordinator, PairReleaseAcknowledgements, ReadyFrame,
    SyncDecision, TileSide, WorkerCommand,
};
use super::recovery::{PairedRecoveryGate, RecoveryAction};
use crate::jni::{remove_renderer_if_current, RendererControl};
use crate::log_info;
use crate::prepared_udp::PreparedUdpReceiver;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

mod tile_worker;

use tile_worker::{monotonic_ns, spawn_tile_worker, TileWorkerLaunch};

pub(crate) struct SplitRendererLaunch {
    pub instance: String,
    pub expected_host: String,
    pub fps: u32,
    pub left_window: usize,
    pub right_window: usize,
    pub left_receiver: PreparedUdpReceiver,
    pub right_receiver: PreparedUdpReceiver,
    pub decoder_name: String,
    pub control: Arc<RendererControl>,
}

#[derive(Default)]
struct RuntimeStats {
    left_fec: FecRuntimeStats,
    right_fec: FecRuntimeStats,
    left_rendered: AtomicU64,
    right_rendered: AtomicU64,
    joined_rendered: AtomicU64,
    frame_gaps: AtomicU64,
    input_drops: AtomicU64,
    keyframe_gap_recoveries: AtomicU32,
    delta_gap_recoveries: AtomicU32,
    pair_sync_timeouts: AtomicU32,
    unmatched_output_drops: AtomicU32,
    ready_delta_max_us: AtomicU32,
    ready_delta_samples_us: Mutex<Vec<u32>>,
}

impl RuntimeStats {
    fn fec(&self, side: TileSide) -> &FecRuntimeStats {
        match side {
            TileSide::Left => &self.left_fec,
            TileSide::Right => &self.right_fec,
        }
    }

    fn record_paired_idr_episode(&self) {
        self.left_fec.record_paired_idr_episode();
        self.right_fec.record_paired_idr_episode();
    }

    fn record_suppressed_recovery_request(&self) {
        self.left_fec.record_suppressed_recovery_request();
        self.right_fec.record_suppressed_recovery_request();
    }

    fn record_ready_delta(&self, delta_us: u32) {
        self.ready_delta_max_us
            .fetch_max(delta_us, Ordering::Relaxed);
        let mut samples = self.ready_delta_samples_us.lock().unwrap();
        samples.push(delta_us);
        if samples.len() > 300 {
            let excess = samples.len() - 300;
            samples.drain(..excess);
        }
    }

    fn ready_delta_p95(&self) -> u32 {
        let mut samples = self.ready_delta_samples_us.lock().unwrap().clone();
        if samples.is_empty() {
            return 0;
        }
        samples.sort_unstable();
        let index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
        samples[index]
    }
}

enum CoordinatorEvent {
    Ready {
        side: TileSide,
        frame: ReadyFrame,
        ready_ns: i64,
    },
    NetworkGap(TileSide),
    DecoderFailure(TileSide),
    Idr {
        side: TileSide,
        generation: u64,
    },
    Presented {
        side: TileSide,
        pts_us: i64,
        generation: u64,
        succeeded: bool,
    },
    Fatal,
}

enum TileCommand {
    PresentAt(viewer_decoder::ReadyOutput, i64, u64),
    Discard(viewer_decoder::ReadyOutput),
    EnterRecovery,
    RequestIdr,
    Stop,
}

pub(crate) fn spawn(launch: SplitRendererLaunch) -> Result<(), String> {
    let SplitRendererLaunch {
        instance,
        expected_host,
        fps,
        left_window,
        right_window,
        left_receiver,
        right_receiver,
        decoder_name,
        control,
    } = launch;
    std::thread::Builder::new()
        .name("leftcar-split-coordinator".into())
        .spawn(move || {
            let stats = Arc::new(RuntimeStats::default());
            let (event_tx, event_rx) = mpsc::channel();
            let (left_tx, left_rx) = mpsc::channel();
            let (right_tx, right_rx) = mpsc::channel();

            let left_handle = spawn_tile_worker(TileWorkerLaunch {
                side: TileSide::Left,
                expected_host: expected_host.clone(),
                fps,
                window: left_window,
                prepared: left_receiver,
                decoder_name: decoder_name.clone(),
                events: event_tx.clone(),
                commands: left_rx,
                stats: Arc::clone(&stats),
                control: Arc::clone(&control),
                release_window_on_exit: false,
            });
            let right_handle = spawn_tile_worker(TileWorkerLaunch {
                side: TileSide::Right,
                expected_host,
                fps,
                window: right_window,
                prepared: right_receiver,
                decoder_name,
                events: event_tx,
                commands: right_rx,
                stats: Arc::clone(&stats),
                control: Arc::clone(&control),
                release_window_on_exit: true,
            });

            let mut coordinator = PairPresentationCoordinator::new(fps);
            let mut release_acknowledgements = PairReleaseAcknowledgements::default();
            let mut presentation_generation = 0u64;
            let mut recovery = PairedRecoveryGate::default();
            let mut ready_at: HashMap<(TileSide, i64), i64> = HashMap::new();
            if recovery.start_initial(monotonic_ns().max(0) as u64) == RecoveryAction::RequestPair {
                let _ = left_tx.send(TileCommand::RequestIdr);
            }
            while !control.stop_requested() {
                match event_rx.recv_timeout(Duration::from_millis(1)) {
                    Ok(first_event) => {
                        for event in drain_available_events(first_event, &event_rx) {
                            match event {
                                CoordinatorEvent::Ready {
                                    side,
                                    frame,
                                    ready_ns,
                                } => {
                                    ready_at.insert((side, frame.pts_us), ready_ns);
                                    dispatch_decision(
                                        coordinator.push_ready(side, frame, ready_ns),
                                        &left_tx,
                                        &right_tx,
                                        &stats,
                                        &mut ready_at,
                                        presentation_generation,
                                    );
                                }
                                CoordinatorEvent::NetworkGap(side) => {
                                    control.record_split_loss(
                                        stats.frame_gaps.load(Ordering::Relaxed),
                                        stats.input_drops.load(Ordering::Relaxed),
                                    );
                                    match recovery.on_loss(side, monotonic_ns() as u64) {
                                        RecoveryAction::RequestPair => {
                                            stats.record_paired_idr_episode();
                                            log_info!(
                                                "split {:?} network gap requested paired IDR without decoder flush",
                                                side
                                            );
                                            let _ = left_tx.send(TileCommand::RequestIdr);
                                        }
                                        RecoveryAction::Suppress => {
                                            stats.record_suppressed_recovery_request();
                                        }
                                        _ => {}
                                    }
                                }
                                CoordinatorEvent::DecoderFailure(side) => {
                                    control.record_split_loss(
                                        stats.frame_gaps.load(Ordering::Relaxed),
                                        stats.input_drops.load(Ordering::Relaxed),
                                    );
                                    match recovery.on_loss(side, monotonic_ns() as u64) {
                                        RecoveryAction::RequestPair => {
                                            stats.record_paired_idr_episode();
                                            log_info!(
                                                "split {:?} decoder failure entered hard paired recovery",
                                                side
                                            );
                                            dispatch_decision(
                                                coordinator.discard_pending(),
                                                &left_tx,
                                                &right_tx,
                                                &stats,
                                                &mut ready_at,
                                                presentation_generation,
                                            );
                                            presentation_generation =
                                                presentation_generation.wrapping_add(1);
                                            release_acknowledgements.clear();
                                            let _ = left_tx.send(TileCommand::EnterRecovery);
                                            let _ = right_tx.send(TileCommand::EnterRecovery);
                                            let _ = left_tx.send(TileCommand::RequestIdr);
                                        }
                                        RecoveryAction::Suppress => {
                                            stats.record_suppressed_recovery_request();
                                        }
                                        _ => {}
                                    }
                                }
                                CoordinatorEvent::Idr { side, generation } => {
                                    let action = recovery.on_idr(side, generation);
                                    log_info!(
                                        "split {:?} IDR generation={} recovery={:?}",
                                        side,
                                        generation,
                                        action
                                    );
                                }
                                CoordinatorEvent::Presented {
                                    side,
                                    pts_us,
                                    generation,
                                    succeeded,
                                } => {
                                    if generation == presentation_generation
                                        && release_acknowledgements.record(side, pts_us, succeeded)
                                    {
                                        let joined =
                                            stats.joined_rendered.fetch_add(1, Ordering::Relaxed)
                                                + 1;
                                        control.record_split_joined_frames(joined);
                                    }
                                }
                                CoordinatorEvent::Fatal => control.request_stop(false),
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                let expiration = coordinator.expire(monotonic_ns());
                if matches!(expiration, SyncDecision::PresentMany { .. }) {
                    stats.pair_sync_timeouts.fetch_add(1, Ordering::Relaxed);
                }
                dispatch_decision(
                    expiration,
                    &left_tx,
                    &right_tx,
                    &stats,
                    &mut ready_at,
                    presentation_generation,
                );
                if recovery.retry_due(monotonic_ns().max(0) as u64) == RecoveryAction::RequestPair {
                    log_info!("split paired recovery IDR retry");
                    let _ = left_tx.send(TileCommand::RequestIdr);
                }
            }
            dispatch_decision(
                coordinator.discard_pending(),
                &left_tx,
                &right_tx,
                &stats,
                &mut ready_at,
                presentation_generation,
            );
            let _ = left_tx.send(TileCommand::Stop);
            let _ = right_tx.send(TileCommand::Stop);
            let _ = left_handle.join();
            let _ = right_handle.join();
            remove_renderer_if_current(&instance, &control);
            control.mark_finished();
        })
        .map(|_| ())
        .map_err(|error| format!("failed to spawn split coordinator: {error}"))
}

fn dispatch_decision(
    decision: SyncDecision,
    left_tx: &mpsc::Sender<TileCommand>,
    right_tx: &mpsc::Sender<TileCommand>,
    stats: &RuntimeStats,
    ready_at: &mut HashMap<(TileSide, i64), i64>,
    presentation_generation: u64,
) {
    match decision {
        SyncDecision::Wait => {}
        SyncDecision::Present {
            left,
            right,
            target_present_ns,
        } => {
            if let (Some(left_ns), Some(right_ns)) = (
                ready_at.remove(&(TileSide::Left, left.pts_us)),
                ready_at.remove(&(TileSide::Right, right.pts_us)),
            ) {
                stats.record_ready_delta(left_ns.abs_diff(right_ns) as u32 / 1_000);
            }
            let _ = left_tx.send(TileCommand::PresentAt(
                left.output,
                target_present_ns,
                presentation_generation,
            ));
            let _ = right_tx.send(TileCommand::PresentAt(
                right.output,
                target_present_ns,
                presentation_generation,
            ));
        }
        SyncDecision::PresentSingle {
            side,
            frame,
            target_present_ns,
            ready_delta_us,
        } => {
            ready_at.remove(&(side, frame.pts_us));
            if let Some(delta_us) = ready_delta_us {
                stats.record_ready_delta(delta_us);
            }
            let target = if side == TileSide::Left {
                left_tx
            } else {
                right_tx
            };
            let _ = target.send(TileCommand::PresentAt(
                frame.output,
                target_present_ns,
                presentation_generation,
            ));
        }
        SyncDecision::PresentMany { commands } => {
            for command in commands {
                let WorkerCommand::PresentAt {
                    side,
                    output,
                    target_present_ns,
                } = command
                else {
                    continue;
                };
                ready_at.remove(&(side, output.pts_us));
                let target = if side == TileSide::Left {
                    left_tx
                } else {
                    right_tx
                };
                let _ = target.send(TileCommand::PresentAt(
                    output,
                    target_present_ns,
                    presentation_generation,
                ));
            }
        }
        SyncDecision::Discard { commands } => {
            stats
                .unmatched_output_drops
                .fetch_add(commands.len() as u32, Ordering::Relaxed);
            for command in commands {
                match command {
                    WorkerCommand::PresentAt { .. } => {}
                    WorkerCommand::Discard { side, output } => {
                        ready_at.remove(&(side, output.pts_us));
                        let target = if side == TileSide::Left {
                            left_tx
                        } else {
                            right_tx
                        };
                        let _ = target.send(TileCommand::Discard(output));
                    }
                }
            }
        }
    }
}
