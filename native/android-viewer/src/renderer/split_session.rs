use super::dispatch::{DispatchLedger, IdrRequestOutcome};
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

use super::split_final_stop_flags;
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
    // Coordinator-side count of paired recovery episodes where BOTH tiles
    // observed the same IDR generation. Episodes whose IDR reached only one
    // tile stay unpaired and are retried by the recovery cooldown; the gap
    // between paired_idr_episodes and this counter exposes that stall.
    paired_idr_resumes: AtomicU32,
    // V2 dispatch truthfulness counters. idr_transmit_attempts counts UDP
    // send_to calls that the kernel accepted — transmit attempts, never
    // confirmed deliveries. idr_requests_unsent counts requests a worker
    // explicitly reported unsent (peer/token missing or send error) and the
    // coordinator retained. idr_requests_cancelled_stale counts requests
    // cancelled before the wire because their episode was already closed.
    recovery_episode: AtomicU64,
    // V3 request-instance ownership: id of the ONE outstanding stamped IDR
    // request, minted by the dispatch ledger per dispatched command
    // (selected path AND bounded alternate). Workers compare this id against
    // the stamp on their queued request immediately before any wire access,
    // so an old queued wire copy of a superseded same-episode command is
    // cancelled instead of riding a newer tick.
    recovery_request: AtomicU64,
    idr_transmit_attempts: AtomicU32,
    idr_requests_unsent: AtomicU32,
    idr_requests_cancelled_stale: AtomicU32,
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
    /// Truthful per-request report from a tile worker, stamped with the
    /// unique request id of the command it reports. `Transmitted` means one
    /// UDP send_to was accepted by the kernel — an attempt, not a delivery
    /// confirmation. The coordinator applies a report only when it carries
    /// the identity of the ONE outstanding stamped request.
    IdrRequestOutcome {
        side: TileSide,
        episode: u64,
        request: u64,
        outcome: IdrRequestOutcome,
    },
}

enum TileCommand {
    PresentAt(viewer_decoder::ReadyOutput, i64, u64),
    Discard(viewer_decoder::ReadyOutput),
    EnterRecovery,
    /// Paired IDR request stamped with the issuing recovery episode AND the
    /// unique per-command request id minted by the dispatch ledger. The
    /// worker re-checks BOTH immediately before any wire access and reports
    /// a truthful outcome back to the coordinator: a stale episode or a
    /// superseded request id is cancelled instead of transmitted, so an old
    /// queued wire copy can never chase a newer tick.
    RequestIdr {
        episode: u64,
        request: u64,
    },
    Stop {
        send_bye: bool,
    },
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
            let mut dispatch_ledger = DispatchLedger::new();
            let mut ready_at: HashMap<(TileSide, i64), i64> = HashMap::new();
            let initial = recovery.start_initial(monotonic_ns().max(0) as u64);
            issue_paired_idr_request(
                initial,
                &mut recovery,
                &mut dispatch_ledger,
                &stats,
                &left_tx,
                &right_tx,
                None,
            );
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
                                    // Accurate current gating (unchanged): the
                                    // gap-freeze itself is per-tile — only the
                                    // lossy tile stopped feeding deltas
                                    // (awaiting_keyframe in its process_frame);
                                    // the peer tile keeps decoding and
                                    // presenting. Recovery requests one paired
                                    // IDR through exactly ONE selected tile
                                    // socket (request-origin first, with a
                                    // bounded single alternate-path retry if
                                    // that path reports itself unready) and
                                    // never flushes either decoder (flush
                                    // happens only on DecoderFailure). Whether
                                    // both tiles resume depends on the Host
                                    // refreshing both streams; the paired gate
                                    // below completes only when both tiles
                                    // report the same IDR generation.
                                    match recovery.on_loss(side, monotonic_ns() as u64) {
                                        RecoveryAction::RequestPair => {
                                            stats.record_paired_idr_episode();
                                            log_info!(
                                                "split {:?} network gap requested paired IDR without decoder flush",
                                                side
                                            );
                                            issue_paired_idr_request(
                                                RecoveryAction::RequestPair,
                                                &mut recovery,
                                                &mut dispatch_ledger,
                                                &stats,
                                                &left_tx,
                                                &right_tx,
                                                Some(side),
                                            );
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
                                            issue_paired_idr_request(
                                                RecoveryAction::RequestPair,
                                                &mut recovery,
                                                &mut dispatch_ledger,
                                                &stats,
                                                &left_tx,
                                                &right_tx,
                                                Some(side),
                                            );
                                        }
                                        RecoveryAction::Suppress => {
                                            stats.record_suppressed_recovery_request();
                                        }
                                        _ => {}
                                    }
                                }
                                CoordinatorEvent::Idr { side, generation } => {
                                    let action = recovery.on_idr(side, generation);
                                    // A ResumePair or partial restart bumped the
                                    // episode id: publish it immediately so any
                                    // queued request stamped with the retired id
                                    // is cancelled by its worker before the wire
                                    // (no late pending PLI after the pair resumes).
                                    sync_dispatch(
                                        &recovery,
                                        &mut dispatch_ledger,
                                        &stats,
                                    );
                                    if action == RecoveryAction::ResumePair {
                                        stats
                                            .paired_idr_resumes
                                            .fetch_add(1, Ordering::Relaxed);
                                    }
                                    log_info!(
                                        "split {:?} IDR generation={} recovery={:?} episode={}",
                                        side,
                                        generation,
                                        action,
                                        recovery.episode()
                                    );
                                }
                                CoordinatorEvent::IdrRequestOutcome {
                                    side,
                                    episode,
                                    request,
                                    outcome,
                                } => {
                                    sync_dispatch(&recovery, &mut dispatch_ledger, &stats);
                                    match outcome {
                                        IdrRequestOutcome::Transmitted => {
                                            // Truthful semantics: one UDP send_to
                                            // accepted by the kernel. Delivery to
                                            // the Host is NOT confirmed by this.
                                            log_info!(
                                                "split {:?} paired IDR request transmitted (episode={}; request={}; UDP send accepted, delivery not guaranteed)",
                                                side,
                                                episode,
                                                request
                                            );
                                        }
                                        IdrRequestOutcome::CancelledStale => {
                                            stats
                                                .idr_requests_cancelled_stale
                                                .fetch_add(1, Ordering::Relaxed);
                                            log_info!(
                                                "split {:?} paired IDR request cancelled before wire: episode={} stale (current={}) or request={} superseded (current={}) (late pending PLI prevented)",
                                                side,
                                                episode,
                                                recovery.episode(),
                                                request,
                                                stats.recovery_request.load(Ordering::Relaxed)
                                            );
                                        }
                                        unsent @ (IdrRequestOutcome::UnsentNoPeer
                                        | IdrRequestOutcome::UnsentNoToken
                                        | IdrRequestOutcome::SendFailed) => {
                                            stats
                                                .idr_requests_unsent
                                                .fetch_add(1, Ordering::Relaxed);
                                            log_info!(
                                                "split {:?} paired IDR request NOT sent (episode={}, request={}, reason={:?}); retained by coordinator",
                                                side,
                                                episode,
                                                request,
                                                unsent
                                            );
                                        }
                                    }
                                    // V3 outcome ownership: the report mutates
                                    // routing only when it carries the identity
                                    // of the ONE outstanding stamped request; an
                                    // old request's report is inert and can
                                    // never dispatch a second, colliding copy.
                                    if let Some(alternate) = dispatch_ledger
                                        .on_outcome_stamped(request, side, episode, outcome)
                                    {
                                        let stamped = recovery.episode();
                                        let stamped_request = dispatch_ledger.current_request();
                                        stats
                                            .recovery_request
                                            .store(stamped_request, Ordering::Relaxed);
                                        log_info!(
                                            "split paired IDR request failed over {:?} -> {:?} (single bounded alternate, episode={}, request={})",
                                            side,
                                            alternate,
                                            stamped,
                                            stamped_request
                                        );
                                        let target = match alternate {
                                            TileSide::Left => &left_tx,
                                            TileSide::Right => &right_tx,
                                        };
                                        let _ = target.send(TileCommand::RequestIdr {
                                            episode: stamped,
                                            request: stamped_request,
                                        });
                                    }
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
                    if dispatch_ledger.retained_request() {
                        log_info!(
                            "split paired recovery IDR retry re-dispatches retained request (paths were unready)"
                        );
                    } else if dispatch_ledger.in_flight() {
                        log_info!(
                            "split paired recovery IDR retry skipped: the stamped request is still in flight (exactly one outstanding command until its outcome)"
                        );
                    } else {
                        log_info!("split paired recovery IDR retry");
                    }
                    issue_paired_idr_request(
                        RecoveryAction::RequestPair,
                        &mut recovery,
                        &mut dispatch_ledger,
                        &stats,
                        &left_tx,
                        &right_tx,
                        None,
                    );
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
            let send_bye = control.send_bye.load(Ordering::SeqCst);
            let termination_reason = control.termination_reason();
            log_info!(
                "split shutdown requested sendBye={} terminationReason={}",
                send_bye,
                termination_reason
            );
            let stop_flags = split_final_stop_flags(send_bye, termination_reason);
            let _ = left_tx.send(TileCommand::Stop {
                send_bye: stop_flags[0],
            });
            let _ = right_tx.send(TileCommand::Stop {
                send_bye: stop_flags[1],
            });
            let _ = left_handle.join();
            let _ = right_handle.join();
            remove_renderer_if_current(&instance, &control);
            control.mark_finished();
        })
        .map(|_| ())
        .map_err(|error| format!("failed to spawn split coordinator: {error}"))
}

/// Publish the gate's episode id to the dispatch ledger and to the shared
/// worker-visible counter. Workers compare this id against the stamp on their
/// queued request immediately before any wire access.
fn sync_dispatch(recovery: &PairedRecoveryGate, ledger: &mut DispatchLedger, stats: &RuntimeStats) {
    let episode = recovery.episode();
    ledger.sync_episode(episode);
    stats.recovery_episode.store(episode, Ordering::Relaxed);
}

/// Issue the paired IDR request for one recovery action through exactly ONE
/// selected tile path: the episode's sticky path (first path whose transmit
/// attempt was accepted), else the request-origin path. If that path reports
/// itself unready, the dispatch ledger owns ONE bounded alternate-path retry
/// within the same action; if no path is ready the request stays retained and
/// the unchanged 750ms cadence re-dispatches it. The 750ms cadence itself is
/// never shortened. V3: the ledger mints a unique request id per dispatched
/// command, published to the workers for the pre-wire ownership check, and
/// refuses to select while a stamped request is still outstanding — so a
/// same-episode retry can never queue a second wire copy alongside the first.
fn issue_paired_idr_request(
    action: RecoveryAction,
    recovery: &mut PairedRecoveryGate,
    ledger: &mut DispatchLedger,
    stats: &RuntimeStats,
    left_tx: &mpsc::Sender<TileCommand>,
    right_tx: &mpsc::Sender<TileCommand>,
    origin: Option<TileSide>,
) {
    sync_dispatch(recovery, ledger, stats);
    if let Some(origin) = origin {
        ledger.note_origin(origin);
    }
    let Some(path) = ledger.select_path(action) else {
        return;
    };
    let episode = recovery.episode();
    let request = ledger.current_request();
    stats.recovery_request.store(request, Ordering::Relaxed);
    let target = match path {
        TileSide::Left => left_tx,
        TileSide::Right => right_tx,
    };
    let _ = target.send(TileCommand::RequestIdr { episode, request });
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
