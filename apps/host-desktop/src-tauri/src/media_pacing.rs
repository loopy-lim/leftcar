//! Bounded ordinary media submissions, shared by captured output and the idle pump.
//!
//! This is a local submission policy, not delivery or OS/NIC batching evidence.
use std::io;
use std::time::Duration;

const MAX_BURST_PACKETS: u64 = 8;
const MAX_BURST_BYTES: usize =
    8 * (crate::wire::MAX_DATAGRAM + secure_channel::COUNTER_LEN + secure_channel::TAG_LEN + 4);
const WAIT_SLICE: Duration = Duration::from_millis(1);
const DRAIN_DEADLINE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum DrainError {
    Cancelled,
    Deadline,
    Submission,
    PacketTooLarge,
    Terminal,
}

/// Cumulative for one capture worker. Bytes mean successful local submissions;
/// unknown partial TCP writes are reported separately, never guessed as zero.
#[derive(Debug, serde::Serialize)]
pub struct TerminalCause {
    stage: &'static str,
    kind: String,
    raw_os_error: Option<i32>,
}
#[derive(Default, Debug, serde::Serialize)]
pub struct DrainMetrics {
    pub terminal_cause: Option<TerminalCause>,
    pub submission_attempts: u64,
    pub submitted_packets: u64,
    pub submitted_wire_bytes: u64,
    pub submission_failures: u64,
    pub preparation_failures: u64,
    pub short_submissions: u64,
    pub unknown_partial_tcp_failures: u64,
    pub completed_aus: u64,
    pub cancelled_drains: u64,
    pub deadline_drains: u64,
    pub paced_delay_us: u64,
    pub max_au_drain_us: u64,
    pub bursts: u64,
    pub max_burst_packets: u64,
    pub max_burst_wire_bytes: u64,
    pub isolated_config_packets: u64,
    pub max_isolated_config_wire_bytes: u64,
}

/// The real adapter seals one packet and submits its exact envelope. Tests
/// replace only time and socket effects, not the pacing/drain policy.
pub trait DrainIo {
    fn now(&self) -> Duration;
    fn wait(&mut self, duration: Duration);
    fn cancelled(&self) -> bool;
    fn prepare(&mut self, packet: &[u8]) -> io::Result<Vec<u8>>;
    fn submit(&mut self, packet: &[u8]) -> io::Result<usize>;
}

pub struct MediaPacer {
    metrics: DrainMetrics,
    bytes_per_second: f64,
    credit: f64,
    updated: Duration,
    burst_packets: u64,
    burst_bytes: usize,
    tcp: bool,
    terminal: Option<DrainError>,
}

impl MediaPacer {
    pub fn new(bitrate: u32, fps: u32, tcp: bool, now: Duration) -> Self {
        // Encoded budget plus actual LT/AEAD/TCP framing, existing full8 FEC
        // parity (2/8), one rounded fragment and one tail parity per AU.
        // Reserve a full ordinary CFG per AU too. This bounds steady configured
        // payload without permanent debt from parity/encryption overhead.
        let overhead =
            secure_channel::COUNTER_LEN + secure_channel::TAG_LEN + if tcp { 4 } else { 0 };
        let payload_rate = u64::from(bitrate.max(1)).div_ceil(8);
        let frames = u64::from(fps.max(1));
        let fragments = payload_rate.div_ceil(crate::wire::MAX_MEDIA_PAYLOAD as u64) + frames;
        let parity_count = fragments.div_ceil(8) * fec_core::parity_count(8) as u64 + frames;
        let parity_wire =
            crate::wire::MAX_MEDIA_PAYLOAD + 2 + crate::wire::PARITY_HEADER + overhead;
        let bytes_per_second = payload_rate
            + fragments * (crate::wire::FRAME_HEADER_V1_LEN + overhead) as u64
            + parity_count * parity_wire as u64
            + frames * (crate::wire::MAX_DATAGRAM + overhead) as u64;
        Self {
            metrics: DrainMetrics::default(),
            bytes_per_second: bytes_per_second as f64,
            credit: MAX_BURST_BYTES as f64,
            updated: now,
            burst_packets: 0,
            burst_bytes: 0,
            tcp,
            terminal: None,
        }
    }

    pub fn metrics(&self) -> &DrainMetrics {
        &self.metrics
    }
    pub fn transport_bytes_per_second(&self) -> u64 {
        self.bytes_per_second as u64
    }
    pub fn terminal(&self) -> Option<DrainError> {
        self.terminal
    }

    fn refill(&mut self, now: Duration) {
        self.credit = (self.credit
            + now.saturating_sub(self.updated).as_secs_f64() * self.bytes_per_second)
            .min(MAX_BURST_BYTES as f64);
        self.updated = now;
    }

    fn boundary(io: &impl DrainIo, deadline: Duration) -> Result<(), DrainError> {
        if io.cancelled() {
            Err(DrainError::Cancelled)
        } else if io.now() >= deadline {
            Err(DrainError::Deadline)
        } else {
            Ok(())
        }
    }

    fn pause(
        &mut self,
        io: &mut impl DrainIo,
        wait: Duration,
        deadline: Duration,
    ) -> Result<(), DrainError> {
        Self::boundary(io, deadline)?;
        let started = io.now();
        io.wait(wait.min(WAIT_SLICE).min(deadline.saturating_sub(started)));
        let ended = io.now();
        self.metrics.paced_delay_us += ended.saturating_sub(started).as_micros() as u64;
        self.refill(ended);
        self.burst_packets = 0;
        self.burst_bytes = 0;
        Self::boundary(io, deadline)
    }

    /// Any incomplete drain poisons the worker's reference chain. Subsequent
    /// output (including another output in the same encoder vector) cannot send.
    pub fn drain(
        &mut self,
        packets: &[Vec<u8>],
        access_unit: bool,
        io: &mut impl DrainIo,
    ) -> Result<(), DrainError> {
        if self.terminal.is_some() {
            return Err(DrainError::Terminal);
        }
        let started = io.now();
        let result = self.drain_inner(packets, io, started + DRAIN_DEADLINE);
        if access_unit {
            self.metrics.max_au_drain_us = self
                .metrics
                .max_au_drain_us
                .max(io.now().saturating_sub(started).as_micros() as u64);
        }
        match result {
            Ok(()) => self.metrics.completed_aus += u64::from(access_unit),
            Err(error) => {
                self.terminal = Some(error);
                self.metrics.cancelled_drains += u64::from(error == DrainError::Cancelled);
                self.metrics.deadline_drains += u64::from(error == DrainError::Deadline);
            }
        }
        result
    }

    fn drain_inner(
        &mut self,
        packets: &[Vec<u8>],
        io: &mut impl DrainIo,
        deadline: Duration,
    ) -> Result<(), DrainError> {
        for packet in packets {
            Self::boundary(io, deadline)?;
            let envelope = io.prepare(packet).map_err(|error| {
                self.metrics.terminal_cause = Some(TerminalCause {
                    stage: "preparation",
                    kind: format!("{:?}", error.kind()),
                    raw_os_error: error.raw_os_error(),
                });
                self.metrics.preparation_failures += 1;
                DrainError::Submission
            })?;
            let size = envelope.len();
            let isolated_config = size > MAX_BURST_BYTES && packet.starts_with(b"CFG");
            let maximum = secure_channel::MAX_DATAGRAM
                + secure_channel::COUNTER_LEN
                + secure_channel::TAG_LEN
                + if self.tcp { 4 } else { 0 };
            if (size > MAX_BURST_BYTES && !isolated_config) || size > maximum {
                self.metrics.preparation_failures += 1;
                return Err(DrainError::PacketTooLarge);
            }
            Self::boundary(io, deadline)?;
            self.refill(io.now());
            if self.burst_packets > 0
                && (isolated_config
                    || self.burst_packets >= MAX_BURST_PACKETS
                    || self.burst_bytes + size > MAX_BURST_BYTES)
            {
                self.pause(io, WAIT_SLICE, deadline)?;
            }
            // Reserve the whole sealed packet. A large CFG can temporarily make
            // credit negative, but must repay it before its isolated submission.
            // No AU reset and no enlarged idle bucket: following media starts
            // with only the actual remainder after the full charge.
            self.credit -= size as f64;
            while self.credit < 0.0 {
                let wait =
                    Duration::from_secs_f64((-self.credit / self.bytes_per_second).max(1e-9));
                self.pause(io, wait, deadline)?;
            }
            Self::boundary(io, deadline)?;
            self.metrics.submission_attempts += 1;
            let submitted = match io.submit(&envelope) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.metrics.terminal_cause = Some(TerminalCause {
                        stage: "submission",
                        kind: format!("{:?}", error.kind()),
                        raw_os_error: error.raw_os_error(),
                    });
                    self.metrics.submission_failures += 1;
                    self.metrics.unknown_partial_tcp_failures += u64::from(self.tcp);
                    return Err(DrainError::Submission);
                }
            };
            self.metrics.submitted_wire_bytes += submitted.min(size) as u64;
            if submitted != size {
                self.metrics.terminal_cause = Some(TerminalCause {
                    stage: "submission",
                    kind: "ShortWrite".into(),
                    raw_os_error: None,
                });
                self.metrics.short_submissions += 1;
                self.metrics.submission_failures += 1;
                return Err(DrainError::Submission);
            }
            self.metrics.submitted_packets += 1;
            if isolated_config {
                self.metrics.isolated_config_packets += 1;
                self.metrics.max_isolated_config_wire_bytes =
                    self.metrics.max_isolated_config_wire_bytes.max(size as u64);
                // Force a separate following burst, still using charged credit.
                self.burst_packets = MAX_BURST_PACKETS;
                self.burst_bytes = size;
            } else {
                if self.burst_packets == 0 {
                    self.metrics.bursts += 1;
                }
                self.burst_packets += 1;
                self.burst_bytes += size;
                self.metrics.max_burst_packets =
                    self.metrics.max_burst_packets.max(self.burst_packets);
                self.metrics.max_burst_wire_bytes = self
                    .metrics
                    .max_burst_wire_bytes
                    .max(self.burst_bytes as u64);
            }
            // A synchronous socket call/lock cannot be interrupted here. Even
            // when its final packet succeeds, an expired AU is not complete.
            Self::boundary(io, deadline)?;
        }
        Self::boundary(io, deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fec, wire};
    struct FakeIo {
        now: Duration,
        tcp: bool,
        tx: secure_channel::DatagramSealer,
        sent: Vec<(Duration, usize)>,
        waits: Vec<Duration>,
        cancel_at: Option<usize>,
        cancel_on_wait: bool,
        failure: Option<io::ErrorKind>,
        short: bool,
        send_time: Duration,
    }
    impl FakeIo {
        fn new(tcp: bool) -> Self {
            Self {
                now: Duration::ZERO,
                tcp,
                tx: secure_channel::DatagramSealer::new([7; 32]),
                sent: vec![],
                waits: vec![],
                cancel_at: None,
                cancel_on_wait: false,
                failure: None,
                short: false,
                send_time: Duration::ZERO,
            }
        }
    }
    impl DrainIo for FakeIo {
        fn now(&self) -> Duration {
            self.now
        }
        fn wait(&mut self, duration: Duration) {
            self.now += duration;
            self.waits.push(duration);
        }
        fn cancelled(&self) -> bool {
            self.cancel_at.is_some_and(|count| self.sent.len() >= count)
                || (self.cancel_on_wait && !self.waits.is_empty())
        }
        fn prepare(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
            wire::seal_media_packet(&self.tx, packet, self.tcp)
        }
        fn submit(&mut self, packet: &[u8]) -> io::Result<usize> {
            self.now += self.send_time;
            if let Some(kind) = self.failure {
                return Err(io::Error::from(kind));
            }
            self.sent.push((self.now, packet.len()));
            Ok(if self.short {
                packet.len() - 1
            } else {
                packet.len()
            })
        }
    }
    fn au(size: usize) -> Vec<Vec<u8>> {
        let mut packets = wire::media_datagrams(1, 0, &vec![0; size]);
        packets.extend(fec::parity_datagrams_for_media(1, 0, &packets));
        packets
    }
    #[test]
    fn large_config_is_isolated_fully_charged_and_does_not_reset_media_credit() {
        for tcp in [false, true] {
            let mut io = FakeIo::new(tcp);
            let mut pacer = MediaPacer::new(4_000_000, 60, tcp, io.now);
            let mut config = vec![0; 65_536];
            config[..3].copy_from_slice(b"CFG");
            pacer.drain(&[config], false, &mut io).unwrap();
            let cost = 65_536 + 24 + if tcp { 4 } else { 0 };
            // Independent wire budget: payload500000; ceil(payload/1367)+60=426
            // fragments; ceil(426/8)*2+60=168 parity; plus60CFGs.
            let overhead = 24 + if tcp { 4 } else { 0 };
            let rate = 500_000
                + 426 * (17 + overhead)
                + 168 * (1367 + 2 + 19 + overhead)
                + 60 * (1400 + overhead);
            let expected = (cost as f64 - 11_424.0) / rate as f64;
            assert!((io.now.as_secs_f64() - expected).abs() < 0.000001);
            assert!(
                (pacer.credit - (11_424.0 + io.now.as_secs_f64() * rate as f64 - cost as f64))
                    .abs()
                    < 0.01
            );
            let after_config = io.now;
            pacer.drain(&au(1367 * 8), true, &mut io).unwrap();
            let first_media_cost = 1367 + 17 + overhead;
            let next_delay = (first_media_cost as f64 / rate as f64).max(0.001);
            assert!(
                (io.sent[1].0.saturating_sub(after_config).as_secs_f64() - next_delay).abs()
                    < 0.000002
            );
        }
    }

    #[test]
    fn oversized_config_wait_honors_cancellation() {
        let mut io = FakeIo::new(true);
        io.cancel_on_wait = true;
        let mut pacer = MediaPacer::new(4_000_000, 60, true, io.now);
        let mut config = vec![0; 20_000];
        config[..3].copy_from_slice(b"CFG");
        assert_eq!(
            pacer.drain(&[config], false, &mut io),
            Err(DrainError::Cancelled)
        );
        assert!(io.sent.is_empty());
    }
    #[test]
    fn config_above_sealer_limit_and_config_deadline_fail_before_submission() {
        for (size, bitrate, expected) in [
            (65_537, 4_000_000, DrainError::Submission),
            (65_536, 1, DrainError::Deadline),
        ] {
            let mut io = FakeIo::new(true);
            let mut pacer = MediaPacer::new(bitrate, 1, true, io.now);
            let mut config = vec![0; size];
            config[..3].copy_from_slice(b"CFG");
            assert_eq!(pacer.drain(&[config], false, &mut io), Err(expected));
            assert!(io.sent.is_empty());
            assert_eq!(pacer.metrics().completed_aus, 0);
            assert!(io.now <= Duration::from_secs(1));
        }
    }

    #[test]
    fn already_cancelled_drain_does_not_even_prepare_or_send() {
        let mut io = FakeIo::new(false);
        io.cancel_at = Some(0);
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        assert_eq!(
            pacer.drain(&au(10), true, &mut io),
            Err(DrainError::Cancelled)
        );
        assert_eq!(pacer.metrics().submission_attempts, 0);
        assert_eq!(pacer.metrics().submitted_wire_bytes, 0);
        assert_eq!(pacer.metrics().completed_aus, 0);
    }

    #[test]
    fn bursts_stay_bounded_across_access_units_and_idle_credit() {
        let mut io = FakeIo::new(false);
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        let packets = au(1367 * 6);
        for _ in 0..3 {
            pacer.drain(&packets, true, &mut io).unwrap();
        }
        assert!(
            io.now > Duration::ZERO,
            "old loop releases every AU immediately"
        );
        let initial = io
            .sent
            .iter()
            .filter(|(time, _)| *time == Duration::ZERO)
            .count();
        assert!(initial <= 8);
        io.now += Duration::from_secs(3600);
        let idle = io.now;
        let begin = io.sent.len();
        pacer.drain(&au(1367 * 30), true, &mut io).unwrap();
        assert!(
            io.sent[begin..]
                .iter()
                .filter(|(time, _)| *time == idle)
                .count()
                <= 8
        );
        assert!(pacer.metrics().max_burst_packets <= 8);
        assert!(pacer.metrics().max_burst_wire_bytes <= 11_424);
    }
    #[test]
    fn cancellation_after_submission_stops_entire_reference_chain() {
        let mut io = FakeIo::new(false);
        io.cancel_at = Some(3);
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        assert_eq!(
            pacer.drain(&au(100_000), true, &mut io),
            Err(DrainError::Cancelled)
        );
        assert_eq!(io.sent.len(), 3);
        assert_eq!(pacer.metrics().completed_aus, 0);
        io.cancel_at = None;
        assert!(pacer.drain(&au(100), true, &mut io).is_err());
        assert_eq!(io.sent.len(), 3);
    }
    #[test]
    fn pacing_wait_is_interruptible_before_the_next_send() {
        let mut io = FakeIo::new(false);
        io.cancel_on_wait = true;
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        assert_eq!(
            pacer.drain(&au(100_000), true, &mut io),
            Err(DrainError::Cancelled)
        );
        assert!(io
            .waits
            .iter()
            .all(|wait| *wait <= Duration::from_millis(1)));
        assert_eq!(io.waits.len(), 1);
        assert!(io.sent.len() <= 8);
    }
    #[test]
    fn deadline_after_a_blocking_send_is_terminal_and_not_complete() {
        let mut io = FakeIo::new(false);
        io.send_time = Duration::from_secs(2);
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        assert_eq!(
            pacer.drain(&au(100), true, &mut io),
            Err(DrainError::Deadline)
        );
        assert_eq!(pacer.metrics().submitted_packets, 1);
        assert_eq!(pacer.metrics().completed_aus, 0);
        assert_eq!(pacer.metrics().deadline_drains, 1);
    }
    #[test]
    fn would_block_and_short_send_never_complete_or_retry_an_au() {
        for tcp in [false, true] {
            for short in [false, true] {
                let mut io = FakeIo::new(tcp);
                io.short = short;
                if !short {
                    io.failure = Some(io::ErrorKind::WouldBlock);
                }
                let mut pacer = MediaPacer::new(4_000_000, 60, tcp, io.now);
                assert_eq!(
                    pacer.drain(&au(5000), true, &mut io),
                    Err(DrainError::Submission)
                );
                assert_eq!(pacer.metrics().completed_aus, 0);
                assert_eq!(pacer.metrics().submission_attempts, 1);
                assert_eq!(pacer.metrics().submission_failures, 1);
                let cause = serde_json::to_value(pacer.metrics()).unwrap();
                assert_eq!(cause["terminal_cause"]["stage"], "submission");
                assert_eq!(
                    cause["terminal_cause"]["kind"],
                    if short { "ShortWrite" } else { "WouldBlock" }
                );
                assert_eq!(
                    pacer.metrics().unknown_partial_tcp_failures,
                    u64::from(tcp && !short)
                );
                io.short = false;
                io.failure = None;
                assert!(pacer.drain(&au(5000), true, &mut io).is_err());
                assert_eq!(pacer.metrics().submission_attempts, 1);
            }
        }
    }
    #[test]
    fn actual_sealed_data_and_parity_are_charged_for_both_transports() {
        for tcp in [false, true] {
            let packets = au(1367 * 8);
            assert_eq!(packets.len(), 10);
            let mut io = FakeIo::new(tcp);
            let mut pacer = MediaPacer::new(4_000_000, 60, tcp, io.now);
            pacer.drain(&packets, true, &mut io).unwrap();
            // 8*(1367+17+24) + 2*(1367+2+19+24), plus TCP4 per packet.
            assert_eq!(
                pacer.metrics().submitted_wire_bytes,
                14_088 + if tcp { 40 } else { 0 }
            );
            assert_eq!(pacer.metrics().submitted_packets, 10);
            assert_eq!(pacer.metrics().completed_aus, 1);
            assert!(
                io.sent.iter().any(|(_, size)| *size > 1400),
                "Windows plaintext1400 is not sealed ceiling"
            );
        }
    }
    #[test]
    fn sustained_configured_rate_and_large_idrs_do_not_build_permanent_debt() {
        for tcp in [false, true] {
            for bitrate in [4_000_000u32, 50_000_000] {
                let fps = 60;
                let average = bitrate as usize / 8 / fps;
                let mut io = FakeIo::new(tcp);
                let mut pacer = MediaPacer::new(bitrate, fps as u32, tcp, io.now);
                for index in 0..600 {
                    let scheduled = Duration::from_secs_f64(index as f64 / 60.0);
                    io.now = io.now.max(scheduled);
                    // An8x IDR, paid back by7 smaller AUs per2s; same payload rate.
                    let size = match index % 120 {
                        0 => average * 8,
                        1..=14 => average / 2,
                        _ => average,
                    };
                    pacer.drain(&au(size), true, &mut io).unwrap();
                    assert!(io.now.saturating_sub(scheduled) < Duration::from_millis(200));
                }
                assert!(io.now < Duration::from_secs(10));
                assert_eq!(pacer.metrics().completed_aus, 600);
            }
        }
    }
    #[test]
    fn excessive_idr_stops_at_absolute_deadline_without_idr_retry_loop() {
        let mut io = FakeIo::new(false);
        let mut pacer = MediaPacer::new(4_000_000, 60, false, io.now);
        assert_eq!(
            pacer.drain(&au(2_000_000), true, &mut io),
            Err(DrainError::Deadline)
        );
        assert_eq!(io.now, Duration::from_secs(1));
        assert_eq!(pacer.metrics().completed_aus, 0);
        assert!(pacer.drain(&au(2_000_000), true, &mut io).is_err());
    }
}
