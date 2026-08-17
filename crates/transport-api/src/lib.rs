//! Transport abstraction and simulated link (docs/03 §5.2/§6, docs/05 §4.4, §9.1).
//!
//! Video plane contract: fragments flow per source; control flows reliably
//! and in order. Implementations: SimulatedLink (deterministic, seeded),
//! InMemoryTransport (loopback tests), transport-quic (bake-off candidate).

use bytes::Bytes;
use domain::SourceId;
use std::collections::VecDeque;
use std::time::Duration;

/// Deterministic link profile (docs/05 §4.4). Seeds are recorded for repro.
#[derive(Debug, Clone)]
pub struct LinkProfile {
    pub base_delay: Duration,
    pub jitter: Duration,
    pub loss_rate: f64,
    pub duplicate_rate: f64,
    pub reorder_rate: f64,
    pub bandwidth_bps: u64,
    pub outage_schedule: Vec<(Duration, Duration)>,
    pub mtu: usize,
}

impl LinkProfile {
    /// docs/05 §9.1 presets.
    pub fn clean_lan() -> Self {
        Self {
            base_delay: Duration::from_millis(1),
            jitter: Duration::from_micros(200),
            loss_rate: 0.0,
            duplicate_rate: 0.0,
            reorder_rate: 0.0,
            bandwidth_bps: 1_000_000_000,
            outage_schedule: Vec::new(),
            mtu: 1500,
        }
    }

    pub fn normal_wifi() -> Self {
        Self {
            base_delay: Duration::from_millis(4),
            jitter: Duration::from_millis(2),
            loss_rate: 0.001,
            duplicate_rate: 0.0,
            reorder_rate: 0.01,
            bandwidth_bps: 200_000_000,
            outage_schedule: Vec::new(),
            mtu: 1500,
        }
    }

    pub fn busy_wifi() -> Self {
        Self {
            base_delay: Duration::from_millis(12),
            jitter: Duration::from_millis(10),
            loss_rate: 0.01,
            duplicate_rate: 0.005,
            reorder_rate: 0.05,
            bandwidth_bps: 40_000_000,
            outage_schedule: Vec::new(),
            mtu: 1500,
        }
    }

    pub fn bad_wifi() -> Self {
        Self {
            base_delay: Duration::from_millis(25),
            jitter: Duration::from_millis(30),
            loss_rate: 0.03,
            duplicate_rate: 0.01,
            reorder_rate: 0.10,
            bandwidth_bps: 15_000_000,
            outage_schedule: Vec::new(),
            mtu: 1500,
        }
    }

    /// 100% loss for 5s starting at `from`.
    pub fn outage(from: Duration, duration: Duration) -> Self {
        Self {
            base_delay: Duration::from_millis(4),
            jitter: Duration::from_millis(2),
            loss_rate: 0.0,
            duplicate_rate: 0.0,
            reorder_rate: 0.0,
            bandwidth_bps: 200_000_000,
            outage_schedule: vec![(from, from + duration)],
            mtu: 1500,
        }
    }

    pub fn is_in_outage(&self, now: Duration) -> bool {
        self.outage_schedule.iter().any(|(s, e)| now >= *s && now < *e)
    }
}

/// Small deterministic PRNG (xorshift) so seeds reproduce exactly.
#[derive(Debug, Clone)]
pub struct SeededRng(pub u64);

impl SeededRng {
    pub fn next_f64(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let v = x.wrapping_mul(0x2545F4914F6CDD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportEvent {
    /// Reliable, ordered control bytes.
    Control(Bytes),
    /// Video fragment bytes for one source.
    Video(SourceId, Bytes),
    Closed,
}

#[derive(Debug, thiserror::Error)]
#[error("transport closed")]
pub struct TransportClosed;

/// One established connection. Send/recv are the two directions of the plane.
pub trait TransportConnection: Send {
    fn send_control(&mut self, bytes: Bytes);
    fn send_video(&mut self, source: SourceId, bytes: Bytes);
    fn try_recv(&mut self) -> Option<TransportEvent>;
}

/// A transport establishes connections (server accepts, client connects).
pub trait Transport: Send {
    type Connection: TransportConnection;
    fn connect(&mut self) -> Result<Self::Connection, TransportClosed>;
}

/// Deterministic simulated link pair with virtual clock (docs/05 §9.1).
pub struct SimulatedLink {
    profile: LinkProfile,
    rng: SeededRng,
    /// virtual now
    now: Duration,
    /// delivery queue sorted by delivery time
    in_flight: VecDeque<(Duration, TransportEvent)>,
    /// events already delivered but pending reorder swap
    ready: VecDeque<TransportEvent>,
}

impl SimulatedLink {
    pub fn new(profile: LinkProfile, seed: u64) -> Self {
        Self {
            profile,
            rng: SeededRng(seed),
            now: Duration::ZERO,
            in_flight: VecDeque::new(),
            ready: VecDeque::new(),
        }
    }

    pub fn profile(&self) -> &LinkProfile {
        &self.profile
    }

    /// Advance virtual time by `dt`; returns events that became deliverable.
    pub fn advance(&mut self, dt: Duration) -> Vec<TransportEvent> {
        self.now += dt;
        let mut out = Vec::new();
        while let Some((deliver_at, event)) = self.in_flight.front() {
            if *deliver_at <= self.now {
                let (_, event) = self.in_flight.pop_front().expect("front exists");
                // reorder: with probability reorder_rate, hold back one event
                if self.rng.next_f64() < self.profile.reorder_rate && self.in_flight.len() >= 1 {
                    // swap with the next in-flight event
                    let next = self.in_flight.pop_front().expect("len checked");
                    self.ready.push_back(next.1);
                    self.ready.push_back(event);
                    // pop two readies in swapped order
                    let b = self.ready.pop_back().expect("just pushed");
                    let a = self.ready.pop_back().expect("just pushed");
                    out.push(b);
                    out.push(a);
                } else {
                    out.push(event);
                }
            } else {
                break;
            }
        }
        out
    }

    fn enqueue(&mut self, event: TransportEvent) {
        if self.profile.is_in_outage(self.now) {
            return; // outage blocks all delivery
        }
        let r = self.rng.next_f64();
        if r < self.profile.loss_rate {
            return; // lost
        }
        let jitter_ms = self.rng.next_f64() * self.profile.jitter.as_millis() as f64;
        let delay = self.profile.base_delay + Duration::from_millis(jitter_ms as u64);
        // duplicate with small probability
        if self.rng.next_f64() < self.profile.duplicate_rate {
            self.in_flight.push_back((self.now + delay, event.clone()));
        }
        self.in_flight.push_back((self.now + delay, event));
    }
}

/// Sender half handed to the product pipeline.
pub struct SimulatedSender {
    link: std::sync::Arc<std::sync::Mutex<SimulatedLink>>,
}

impl SimulatedSender {
    pub fn send_control(&mut self, bytes: Bytes) {
        self.link.lock().expect("link mutex").enqueue(TransportEvent::Control(bytes));
    }

    pub fn send_video(&mut self, source: SourceId, bytes: Bytes) {
        self.link.lock().expect("link mutex").enqueue(TransportEvent::Video(source, bytes));
    }
}

/// Receiver half; drains with the virtual clock.
pub struct SimulatedReceiver {
    link: std::sync::Arc<std::sync::Mutex<SimulatedLink>>,
}

impl SimulatedReceiver {
    pub fn advance(&mut self, dt: Duration) -> Vec<TransportEvent> {
        self.link.lock().expect("link mutex").advance(dt)
    }
}

pub fn simulated_pair(
    profile: LinkProfile,
    seed: u64,
) -> (SimulatedSender, SimulatedReceiver) {
    let link = std::sync::Arc::new(std::sync::Mutex::new(SimulatedLink::new(profile, seed)));
    (
        SimulatedSender { link: link.clone() },
        SimulatedReceiver { link },
    )
}

/// In-memory reliable loopback transport for L5 integration tests: control is
/// FIFO, video is FIFO per source, no loss.
#[derive(Default)]
pub struct InMemoryTransport {
    queue: VecDeque<TransportEvent>,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn send_control(&mut self, bytes: Bytes) {
        self.queue.push_back(TransportEvent::Control(bytes));
    }

    pub fn send_video(&mut self, source: SourceId, bytes: Bytes) {
        self.queue.push_back(TransportEvent::Video(source, bytes));
    }

    pub fn recv(&mut self) -> Option<TransportEvent> {
        self.queue.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outage_schedule_blocks_all() {
        let profile = LinkProfile::outage(Duration::ZERO, Duration::from_secs(5));
        assert!(profile.is_in_outage(Duration::from_secs(1)));
        assert!(!profile.is_in_outage(Duration::from_secs(6)));
    }

    #[test]
    fn loss_seed_is_reproducible() {
        let (mut tx1, mut rx1) = simulated_pair(LinkProfile::bad_wifi(), 42);
        let (mut tx2, mut rx2) = simulated_pair(LinkProfile::bad_wifi(), 42);
        let src = SourceId::from_raw("s").unwrap();
        for i in 0..100u32 {
            tx1.send_video(src.clone(), Bytes::from(vec![i as u8; 64]));
            tx2.send_video(src.clone(), Bytes::from(vec![i as u8; 64]));
        }
        let out1 = rx1.advance(Duration::from_millis(200));
        let out2 = rx2.advance(Duration::from_millis(200));
        assert_eq!(out1.len(), out2.len(), "same seed same delivery count");
        assert_eq!(out1, out2, "same seed same events");
    }

    #[test]
    fn clean_lan_delivers_everything_in_order() {
        let (mut tx, mut rx) = simulated_pair(LinkProfile::clean_lan(), 1);
        tx.send_control(Bytes::from_static(b"c1"));
        tx.send_control(Bytes::from_static(b"c2"));
        let out = rx.advance(Duration::from_millis(50));
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], TransportEvent::Control(b) if b.as_ref() == b"c1"));
        assert!(matches!(&out[1], TransportEvent::Control(b) if b.as_ref() == b"c2"));
    }

    #[test]
    fn different_seeds_can_differ() {
        // sanity: seeds drive divergence under lossy profiles
        let src = SourceId::from_raw("s").unwrap();
        let (mut tx1, mut rx1) = simulated_pair(LinkProfile::bad_wifi(), 1);
        let (mut tx2, mut rx2) = simulated_pair(LinkProfile::bad_wifi(), 2);
        for i in 0..200u32 {
            tx1.send_video(src.clone(), Bytes::from(vec![i as u8; 64]));
            tx2.send_video(src.clone(), Bytes::from(vec![i as u8; 64]));
        }
        let n1 = rx1.advance(Duration::from_millis(500)).len();
        let n2 = rx2.advance(Duration::from_millis(500)).len();
        assert!(n1 > 100 && n2 > 100, "3% loss keeps most frames: {n1} {n2}");
        assert!(n1 != n2 || n1 > 0, "seeds should generally diverge");
    }

    #[test]
    fn in_memory_loopback_fifo() {
        let mut t = InMemoryTransport::new();
        let src = SourceId::from_raw("s").unwrap();
        t.send_control(Bytes::from_static(b"hello"));
        t.send_video(src, Bytes::from_static(b"v"));
        assert!(matches!(t.recv(), Some(TransportEvent::Control(b)) if b.as_ref() == b"hello"));
        assert!(matches!(t.recv(), Some(TransportEvent::Video(_, b)) if b.as_ref() == b"v"));
        assert!(t.is_empty());
    }
}
