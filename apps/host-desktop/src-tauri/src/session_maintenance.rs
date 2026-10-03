//! Listener-owned maintenance: UI visibility and accept capacity are unrelated
//! to retiring native sessions that already reached a terminal state.
use super::ControlServer;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

pub(super) struct SessionMaintenance(JoinHandle<()>);

impl SessionMaintenance {
    pub(super) fn start(server: Arc<ControlServer>) -> Self {
        Self(tokio::spawn(async move {
            let mut shutdown = server.connection_shutdown.subscribe();
            let period = Duration::from_millis(500);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                if *shutdown.borrow() {
                    return;
                }
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => return,
                    _ = interval.tick() => {}
                }
                if *shutdown.borrow() {
                    return;
                }
                let sampled = server.clone();
                let completion = tokio::task::spawn_blocking(move || sampled.snapshot());
                // Await the only in-flight sample before accepting another
                // tick. Native teardown may block; it must not multiply jobs
                // or consume the async listener's executor thread.
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => return,
                    result = completion => {
                        if let Err(error) = result { eprintln!("session maintenance failed: {error}"); }
                    }
                }
            }
        }))
    }
}

impl Drop for SessionMaintenance {
    fn drop(&mut self) {
        // Cancelling/dropping run() also retires its maintenance loop. A native
        // operation already admitted to spawn_blocking finishes under existing
        // source/lifecycle guards; Tokio cannot cancel an executing closure.
        self.0.abort();
    }
}
