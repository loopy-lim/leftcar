//! A manager owns every generation until its worker has finished and been joined.
use super::{cancelled, Cancellation, IO_POLL};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const STOP_BUDGET: Duration = Duration::from_millis(250);
pub(super) type ProxyJob = Box<dyn FnOnce() + Send>;

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Starting {
        generation: u64,
        stop: Cancellation,
    },
    Worker {
        generation: u64,
        stop: Cancellation,
        handle: JoinHandle<()>,
        stopping: bool,
    },
    Reaping {
        generation: u64,
        stop: Cancellation,
    },
}

impl Phase {
    fn owner(&self) -> Option<(u64, &Cancellation)> {
        match self {
            Self::Idle => None,
            Self::Starting { generation, stop }
            | Self::Worker {
                generation, stop, ..
            }
            | Self::Reaping { generation, stop } => Some((*generation, stop)),
        }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    phase: Phase,
}

#[derive(Default)]
pub(super) struct ProxyManager(Mutex<State>);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StopOutcome {
    Complete,
    Incomplete,
}

struct Reservation<'a> {
    manager: &'a ProxyManager,
    generation: u64,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut state = self.manager.0.lock().unwrap();
        if matches!(state.phase, Phase::Starting { generation, .. } if generation == self.generation)
        {
            state.phase = Phase::Idle;
        }
    }
}

impl ProxyManager {
    pub(super) fn start(
        &self,
        prepare: impl FnOnce(Cancellation) -> Result<ProxyJob, String>,
        spawn: impl FnOnce(ProxyJob) -> io::Result<JoinHandle<()>>,
    ) -> Result<(), String> {
        let predecessor = {
            let state = self.0.lock().unwrap();
            match &state.phase {
                Phase::Idle => None,
                Phase::Worker {
                    generation,
                    handle,
                    stopping,
                    ..
                } if *stopping || handle.is_finished() => Some(*generation),
                _ => return Err("USB media proxy is already active or stopping".into()),
            }
        };
        if let Some(generation) = predecessor {
            if self.reap(generation) == StopOutcome::Incomplete {
                return Err("USB media proxy is still stopping".into());
            }
        }
        let (generation, stop) = {
            let mut state = self.0.lock().unwrap();
            if !matches!(state.phase, Phase::Idle) {
                return Err("USB media proxy is already active or stopping".into());
            }
            state.generation += 1;
            let generation = state.generation;
            let stop = Arc::new(AtomicBool::new(false));
            state.phase = Phase::Starting {
                generation,
                stop: stop.clone(),
            };
            (generation, stop)
        };
        let _reservation = Reservation {
            manager: self,
            generation,
        };
        let job = prepare(stop.clone())?;
        if cancelled(&stop) {
            return Err("USB media proxy start was cancelled".into());
        }
        let handle =
            spawn(job).map_err(|error| format!("USB media proxy thread failed: {error}"))?;
        let was_cancelled = {
            let mut state = self.0.lock().unwrap();
            let was_cancelled = cancelled(&stop);
            state.phase = Phase::Worker {
                generation,
                stop,
                handle,
                stopping: was_cancelled,
            };
            was_cancelled
        };
        if was_cancelled {
            let _ = self.reap(generation);
            return Err("USB media proxy start was cancelled".into());
        }
        Ok(())
    }

    // Compatible synchronous teardown (including Drop callers) occupies its
    // executor thread for at most this polling budget plus short lock/join work.
    // Only already-finished handles are joined; timeout never detaches a worker.
    pub(super) fn stop(&self) -> StopOutcome {
        let generation = {
            let mut state = self.0.lock().unwrap();
            let Some((generation, stop)) = state.phase.owner() else {
                return StopOutcome::Complete;
            };
            stop.store(true, Ordering::Release);
            if let Phase::Worker { stopping, .. } = &mut state.phase {
                *stopping = true;
            }
            generation
        };
        self.reap(generation)
    }

    fn reap(&self, generation: u64) -> StopOutcome {
        let deadline = Instant::now() + STOP_BUDGET;
        loop {
            let finished = {
                let mut state = self.0.lock().unwrap();
                if state
                    .phase
                    .owner()
                    .is_none_or(|(current, _)| current != generation)
                {
                    return StopOutcome::Complete;
                }
                if matches!(&state.phase, Phase::Worker { handle, .. } if handle.is_finished()) {
                    let Phase::Worker { stop, handle, .. } = std::mem::take(&mut state.phase)
                    else {
                        unreachable!()
                    };
                    state.phase = Phase::Reaping { generation, stop };
                    Some(handle)
                } else {
                    None
                }
            };
            if let Some(handle) = finished {
                if handle.join().is_err() {
                    eprintln!("USB media proxy worker panicked");
                }
                let mut state = self.0.lock().unwrap();
                if matches!(state.phase, Phase::Reaping { generation: current, .. } if current == generation)
                {
                    state.phase = Phase::Idle;
                }
                return StopOutcome::Complete;
            }
            if Instant::now() >= deadline {
                return StopOutcome::Incomplete;
            }
            thread::sleep(IO_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Barrier};

    fn spawn(job: ProxyJob) -> io::Result<JoinHandle<()>> {
        thread::Builder::new().spawn(job)
    }
    fn accepting(stop: Cancellation) -> Result<ProxyJob, String> {
        Ok(Box::new(move || {
            while !cancelled(&stop) {
                thread::park_timeout(IO_POLL);
            }
        }))
    }

    #[test]
    fn stop_reaps_before_immediate_restart_and_repeated_stop_is_safe() {
        let manager = ProxyManager::default();
        manager.start(accepting, spawn).unwrap();
        assert!(manager.start(accepting, spawn).is_err());
        assert_eq!(manager.stop(), StopOutcome::Complete);
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }

    #[test]
    fn timeout_retains_worker_and_rejects_restart_until_barrier_released() {
        let manager = ProxyManager::default();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        manager
            .start(
                move |_| {
                    Ok(Box::new(move || {
                        worker_barrier.wait();
                    }))
                },
                spawn,
            )
            .unwrap();
        let before = Instant::now();
        assert_eq!(manager.stop(), StopOutcome::Incomplete);
        assert!(before.elapsed() < Duration::from_secs(2));
        assert!(manager.start(accepting, spawn).is_err());
        barrier.wait();
        assert_eq!(manager.stop(), StopOutcome::Complete);
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }

    #[test]
    fn stop_cancels_setup_and_competing_start_cannot_reserve() {
        let manager = Arc::new(ProxyManager::default());
        let owner = manager.clone();
        let (ready, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = thread::spawn(move || {
            owner.start(
                move |stop| {
                    ready.send(()).unwrap();
                    wait.recv().unwrap();
                    accepting(stop)
                },
                spawn,
            )
        });
        started.recv().unwrap();
        assert!(manager.start(accepting, spawn).is_err());
        assert_eq!(manager.stop(), StopOutcome::Incomplete);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }

    #[test]
    fn prepare_spawn_failure_and_setup_panic_release_reservation() {
        let manager = ProxyManager::default();
        assert!(manager
            .start(|_| Err("bind/channel failed".into()), spawn)
            .is_err());
        assert!(manager
            .start(accepting, |_| Err(io::Error::other(
                "injected spawn failure"
            )))
            .is_err());
        let panic = std::panic::catch_unwind(|| manager.start(|_| panic!("setup panic"), spawn));
        assert!(panic.is_err());
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }

    #[test]
    fn naturally_finished_and_panicked_workers_are_reaped_on_start() {
        let manager = ProxyManager::default();
        for panics in [false, true] {
            let (done, finished) = mpsc::channel::<()>();
            manager
                .start(
                    move |_| {
                        Ok(Box::new(move || {
                            drop(done);
                            assert!(!panics, "injected worker panic");
                        }))
                    },
                    spawn,
                )
                .unwrap();
            assert!(finished.recv().is_err());
            // Completion notification precedes the runtime marking the handle.
            let generation = manager.0.lock().unwrap().phase.owner().unwrap().0;
            assert_eq!(manager.reap(generation), StopOutcome::Complete);
        }
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }
    #[test]
    fn concurrent_stops_and_stale_reaper_preserve_later_generation() {
        let manager = Arc::new(ProxyManager::default());
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        manager
            .start(
                move |_| {
                    Ok(Box::new(move || {
                        worker_barrier.wait();
                    }))
                },
                spawn,
            )
            .unwrap();
        let old_generation = manager.0.lock().unwrap().phase.owner().unwrap().0;
        let stoppers: Vec<_> = (0..4)
            .map(|_| {
                let manager = manager.clone();
                thread::spawn(move || manager.stop())
            })
            .collect();
        // Wait for the published stopping transition, not a guessed sleep.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(
                manager.0.lock().unwrap().phase,
                Phase::Worker { stopping: true, .. }
            ) {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        barrier.wait();
        for stopper in stoppers {
            assert_eq!(stopper.join().unwrap(), StopOutcome::Complete);
        }
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.reap(old_generation), StopOutcome::Complete);
        assert!(manager.start(accepting, spawn).is_err());
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }

    #[test]
    fn cancellation_while_spawning_cannot_publish_uncancelled_success() {
        let manager = Arc::new(ProxyManager::default());
        let owner = manager.clone();
        let (ready, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = thread::spawn(move || {
            owner.start(accepting, |job| {
                let handle = spawn(job)?;
                ready.send(()).unwrap();
                wait.recv().unwrap();
                Ok(handle)
            })
        });
        started.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Incomplete);
        release.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(manager.stop(), StopOutcome::Complete);
        manager.start(accepting, spawn).unwrap();
        assert_eq!(manager.stop(), StopOutcome::Complete);
    }
}
