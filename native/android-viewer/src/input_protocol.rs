//! Native viewer-to-host input plane.
//!
//! Pointer motion is deliberately lossy and coalesced: only the newest
//! position is useful. Buttons, wheel steps, keys, and release-all are sent
//! one-at-a-time with acknowledgements so a lost datagram cannot leave the
//! remote machine with a key or mouse button held down.

use std::collections::VecDeque;

pub const INPUT_MAGIC: &[u8; 4] = b"LCI1";
pub const ACK_MAGIC: &[u8; 4] = b"LCA1";
pub const STATUS_MAGIC: &[u8; 4] = b"LCS1";
pub const LATENCY_PROBE_MAGIC: &[u8; 4] = b"LCP1";
pub const LATENCY_RESPONSE_MAGIC: &[u8; 4] = b"LCP2";
pub const INPUT_HEADER_LEN: usize = 10;
pub const INPUT_FLAG_RELIABLE: u8 = 1;
pub const MAX_RELIABLE_QUEUE: usize = 256;
const RELIABLE_RETRY_US: u64 = 20_000;
const MAX_RELIABLE_ATTEMPTS: u8 = 12;

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    PointerMove {
        x: u16,
        y: u16,
        buttons: u32,
    },
    PointerButton {
        x: u16,
        y: u16,
        button: u8,
        down: bool,
        buttons: u32,
    },
    Scroll {
        horizontal_milli_lines: i32,
        vertical_milli_lines: i32,
    },
    Key {
        key_code: u16,
        scan_code: u16,
        meta_state: u32,
        down: bool,
        repeat: u16,
    },
    ReleaseAll,
}

impl InputEvent {
    pub fn is_reliable(&self) -> bool {
        !matches!(self, Self::PointerMove { .. })
    }

    fn kind(&self) -> u8 {
        match self {
            Self::PointerMove { .. } => 1,
            Self::PointerButton { .. } => 2,
            Self::Scroll { .. } => 3,
            Self::Key { .. } => 4,
            Self::ReleaseAll => 5,
        }
    }

    fn encode_payload(&self, out: &mut Vec<u8>) {
        match self {
            Self::PointerMove { x, y, buttons } => {
                out.extend_from_slice(&x.to_be_bytes());
                out.extend_from_slice(&y.to_be_bytes());
                out.extend_from_slice(&buttons.to_be_bytes());
            }
            Self::PointerButton {
                x,
                y,
                button,
                down,
                buttons,
            } => {
                out.extend_from_slice(&x.to_be_bytes());
                out.extend_from_slice(&y.to_be_bytes());
                out.push(*button);
                out.push(u8::from(*down));
                out.extend_from_slice(&buttons.to_be_bytes());
            }
            Self::Scroll {
                horizontal_milli_lines,
                vertical_milli_lines,
            } => {
                out.extend_from_slice(&horizontal_milli_lines.to_be_bytes());
                out.extend_from_slice(&vertical_milli_lines.to_be_bytes());
            }
            Self::Key {
                key_code,
                scan_code,
                meta_state,
                down,
                repeat,
            } => {
                out.extend_from_slice(&key_code.to_be_bytes());
                out.extend_from_slice(&scan_code.to_be_bytes());
                out.extend_from_slice(&meta_state.to_be_bytes());
                out.push(u8::from(*down));
                out.extend_from_slice(&repeat.to_be_bytes());
            }
            Self::ReleaseAll => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboundInput {
    pub sequence: u32,
    pub event: InputEvent,
}

pub fn encode_input(outbound: &OutboundInput, token: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INPUT_HEADER_LEN + 16 + token.len());
    bytes.extend_from_slice(INPUT_MAGIC);
    bytes.extend_from_slice(&outbound.sequence.to_be_bytes());
    bytes.push(outbound.event.kind());
    bytes.push(if outbound.event.is_reliable() {
        INPUT_FLAG_RELIABLE
    } else {
        0
    });
    outbound.event.encode_payload(&mut bytes);
    bytes.extend_from_slice(token);
    bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputAck {
    pub sequence: u32,
    pub enabled: Option<bool>,
}

pub fn parse_ack(packet: &[u8], token: &[u8]) -> Option<InputAck> {
    if token.is_empty() || packet.get(..4)? != ACK_MAGIC {
        return None;
    }
    let (token_offset, enabled) = match packet.len().checked_sub(token.len())? {
        8 => (8, None),
        9 => (9, Some(*packet.get(8)? != 0)),
        _ => return None,
    };
    if packet[token_offset..] != *token {
        return None;
    }
    Some(InputAck {
        sequence: u32::from_be_bytes(packet[4..8].try_into().ok()?),
        enabled,
    })
}

pub fn parse_input_status(packet: &[u8], token: &[u8]) -> Option<bool> {
    if token.is_empty()
        || packet.len() != 5 + token.len()
        || &packet[..4] != STATUS_MAGIC
        || packet[5..] != *token
    {
        return None;
    }
    match packet[4] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub fn encode_latency_probe(sequence: u32, viewer_send_ms: u64, token: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + token.len());
    bytes.extend_from_slice(LATENCY_PROBE_MAGIC);
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.extend_from_slice(&viewer_send_ms.to_be_bytes());
    bytes.extend_from_slice(token);
    bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyProbeResponse {
    pub sequence: u32,
    pub viewer_send_ms: u64,
    pub host_receive_ms: u64,
    pub host_send_ms: u64,
}

pub fn parse_latency_probe_response(packet: &[u8], token: &[u8]) -> Option<LatencyProbeResponse> {
    if token.is_empty()
        || packet.len() != 32 + token.len()
        || &packet[..4] != LATENCY_RESPONSE_MAGIC
        || packet[32..] != *token
    {
        return None;
    }
    Some(LatencyProbeResponse {
        sequence: u32::from_be_bytes(packet[4..8].try_into().ok()?),
        viewer_send_ms: u64::from_be_bytes(packet[8..16].try_into().ok()?),
        host_receive_ms: u64::from_be_bytes(packet[16..24].try_into().ok()?),
        host_send_ms: u64::from_be_bytes(packet[24..32].try_into().ok()?),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyEstimate {
    pub network_rtt_ms: u64,
    /// Host wall clock minus Android wall clock.
    pub host_clock_offset_ms: i128,
}

pub fn estimate_latency(
    response: LatencyProbeResponse,
    viewer_receive_ms: u64,
) -> Option<LatencyEstimate> {
    let t0 = i128::from(response.viewer_send_ms);
    let t1 = i128::from(response.host_receive_ms);
    let t2 = i128::from(response.host_send_ms);
    let t3 = i128::from(viewer_receive_ms);
    let network_rtt = t3 - t0 - (t2 - t1);
    if !(0..=60_000).contains(&network_rtt) {
        return None;
    }
    Some(LatencyEstimate {
        network_rtt_ms: network_rtt as u64,
        host_clock_offset_ms: (t1 - t0 + t2 - t3) / 2,
    })
}

pub fn normalized_axis(value: f32) -> u16 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
}

pub fn polling_rate_hz(stream_fps: u32) -> u32 {
    stream_fps.saturating_mul(2).clamp(30, 240)
}

#[derive(Debug, Clone)]
struct PendingReliable {
    outbound: OutboundInput,
    last_sent_us: u64,
    attempts: u8,
}

/// Bounded per-window scheduler. JNI producers only update this small state;
/// the renderer socket thread owns actual network writes.
pub struct InputScheduler {
    polling_interval_us: u64,
    latest_pointer: Option<InputEvent>,
    pointer_dirty: bool,
    last_pointer_sent_us: u64,
    next_pointer_sequence: u32,
    reliable: VecDeque<InputEvent>,
    pending: Option<PendingReliable>,
    next_reliable_sequence: u32,
    dropped: u64,
}

impl InputScheduler {
    pub fn new(stream_fps: u32) -> Self {
        let hz = polling_rate_hz(stream_fps);
        Self {
            polling_interval_us: 1_000_000 / u64::from(hz),
            latest_pointer: None,
            pointer_dirty: false,
            last_pointer_sent_us: 0,
            next_pointer_sequence: 1,
            reliable: VecDeque::new(),
            pending: None,
            next_reliable_sequence: 1,
            dropped: 0,
        }
    }

    pub fn polling_rate_hz(&self) -> u32 {
        (1_000_000 / self.polling_interval_us.max(1)) as u32
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// A reconnect establishes a new authenticated nonce and therefore a new
    /// sequence space. Never carry a queued key/button from the old transport
    /// into the replacement session.
    pub fn reset_session(&mut self) {
        self.latest_pointer = None;
        self.pointer_dirty = false;
        self.last_pointer_sent_us = 0;
        self.next_pointer_sequence = 1;
        self.reliable.clear();
        self.pending = None;
        self.next_reliable_sequence = 1;
    }

    pub fn push(&mut self, event: InputEvent) {
        if matches!(event, InputEvent::PointerMove { .. }) {
            self.latest_pointer = Some(event);
            self.pointer_dirty = true;
            return;
        }
        if matches!(event, InputEvent::ReleaseAll) {
            self.reliable.clear();
            self.pending = None;
            self.latest_pointer = None;
            self.pointer_dirty = false;
        }
        if self.reliable.len() >= MAX_RELIABLE_QUEUE {
            self.dropped = self.dropped.saturating_add(self.reliable.len() as u64 + 1);
            self.reliable.clear();
            self.pending = None;
            self.reliable.push_back(InputEvent::ReleaseAll);
            return;
        }
        self.reliable.push_back(event);
    }

    /// Returns at most one datagram candidate per socket-loop tick. Reliable
    /// input has priority only when it is due; pointer updates can continue
    /// while an acknowledgement is in flight.
    pub fn next_ready(&mut self, now_us: u64) -> Option<OutboundInput> {
        if let Some(pending) = self.pending.as_mut() {
            if pending.attempts >= MAX_RELIABLE_ATTEMPTS {
                self.dropped = self.dropped.saturating_add(1);
                self.pending = None;
                self.reliable.clear();
                self.reliable.push_back(InputEvent::ReleaseAll);
            } else if pending.attempts == 0
                || now_us.saturating_sub(pending.last_sent_us) >= RELIABLE_RETRY_US
            {
                pending.last_sent_us = now_us;
                pending.attempts = pending.attempts.saturating_add(1);
                return Some(pending.outbound.clone());
            }
        }

        if self.pending.is_none() {
            if let Some(event) = self.reliable.pop_front() {
                let sequence = self.next_reliable_sequence;
                self.next_reliable_sequence = self.next_reliable_sequence.wrapping_add(1).max(1);
                let outbound = OutboundInput { sequence, event };
                self.pending = Some(PendingReliable {
                    outbound: outbound.clone(),
                    last_sent_us: now_us,
                    attempts: 1,
                });
                return Some(outbound);
            }
        }

        if self.pointer_dirty
            && (self.last_pointer_sent_us == 0
                || now_us.saturating_sub(self.last_pointer_sent_us) >= self.polling_interval_us)
        {
            let event = self.latest_pointer.clone()?;
            let sequence = self.next_pointer_sequence;
            self.next_pointer_sequence = self.next_pointer_sequence.wrapping_add(1).max(1);
            self.pointer_dirty = false;
            self.last_pointer_sent_us = now_us;
            return Some(OutboundInput { sequence, event });
        }
        None
    }

    pub fn acknowledge(&mut self, sequence: u32) -> bool {
        let matches = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.outbound.sequence == sequence);
        if matches {
            self.pending = None;
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_rate_is_twice_stream_fps() {
        assert_eq!(polling_rate_hz(0), 30);
        assert_eq!(polling_rate_hz(30), 60);
        assert_eq!(polling_rate_hz(60), 120);
        assert_eq!(polling_rate_hz(90), 180);
        assert_eq!(polling_rate_hz(200), 240);
        assert_eq!(polling_rate_hz(240), 240);
    }

    #[test]
    fn authenticated_latency_probe_separates_rtt_and_clock_offset() {
        let token = b"session-token";
        let request = encode_latency_probe(7, 1_000, token);
        assert_eq!(&request[..4], LATENCY_PROBE_MAGIC);
        assert_eq!(&request[4..8], &7u32.to_be_bytes());
        assert_eq!(&request[8..16], &1_000u64.to_be_bytes());
        assert_eq!(&request[16..], token);

        // Android clock t0=1000/t3=1010, Host clock is +100 ms. The LAN adds
        // 5 ms each way and Host spends 1 ms constructing the response.
        let mut packet = LATENCY_RESPONSE_MAGIC.to_vec();
        packet.extend_from_slice(&7u32.to_be_bytes());
        packet.extend_from_slice(&1_000u64.to_be_bytes());
        packet.extend_from_slice(&1_105u64.to_be_bytes());
        packet.extend_from_slice(&1_106u64.to_be_bytes());
        packet.extend_from_slice(token);
        let response = parse_latency_probe_response(&packet, token).unwrap();
        assert_eq!(
            estimate_latency(response, 1_011),
            Some(LatencyEstimate {
                network_rtt_ms: 10,
                host_clock_offset_ms: 100,
            })
        );
        assert!(parse_latency_probe_response(&packet, b"wrong-token").is_none());
    }

    #[test]
    fn pointer_updates_are_coalesced_at_target_rate() {
        let mut scheduler = InputScheduler::new(60);
        scheduler.push(InputEvent::PointerMove {
            x: 1,
            y: 2,
            buttons: 0,
        });
        scheduler.push(InputEvent::PointerMove {
            x: 3,
            y: 4,
            buttons: 0,
        });
        let first = scheduler.next_ready(1).unwrap();
        assert_eq!(
            first.event,
            InputEvent::PointerMove {
                x: 3,
                y: 4,
                buttons: 0
            }
        );
        scheduler.push(InputEvent::PointerMove {
            x: 5,
            y: 6,
            buttons: 0,
        });
        assert!(scheduler.next_ready(8_000).is_none());
        assert!(scheduler.next_ready(8_400).is_some());
    }

    #[test]
    fn reliable_event_retries_until_authenticated_ack() {
        let mut scheduler = InputScheduler::new(90);
        scheduler.push(InputEvent::Key {
            key_code: 29,
            scan_code: 30,
            meta_state: 0,
            down: true,
            repeat: 0,
        });
        let first = scheduler.next_ready(100).unwrap();
        assert!(first.event.is_reliable());
        assert!(scheduler.next_ready(10_000).is_none());
        assert_eq!(
            scheduler.next_ready(20_100).unwrap().sequence,
            first.sequence
        );
        assert!(!scheduler.acknowledge(first.sequence + 1));
        assert!(scheduler.acknowledge(first.sequence));
        assert!(scheduler.next_ready(20_101).is_none());
    }

    #[test]
    fn release_all_supersedes_pending_input_and_pointer_motion() {
        let mut scheduler = InputScheduler::new(60);
        scheduler.push(InputEvent::Key {
            key_code: 29,
            scan_code: 30,
            meta_state: 0,
            down: true,
            repeat: 0,
        });
        let pending = scheduler.next_ready(1).unwrap();
        scheduler.push(InputEvent::PointerMove {
            x: 10,
            y: 20,
            buttons: 1,
        });
        scheduler.push(InputEvent::ReleaseAll);
        let release = scheduler.next_ready(2).unwrap();
        assert_eq!(release.sequence, pending.sequence + 1);
        assert_eq!(release.event, InputEvent::ReleaseAll);
        assert!(scheduler.acknowledge(release.sequence));
        assert!(scheduler.next_ready(100_000).is_none());
    }

    #[test]
    fn reconnect_resets_sequences_and_drops_stale_input() {
        let mut scheduler = InputScheduler::new(90);
        scheduler.push(InputEvent::Key {
            key_code: 29,
            scan_code: 30,
            meta_state: 0,
            down: true,
            repeat: 0,
        });
        assert_eq!(scheduler.next_ready(1).unwrap().sequence, 1);
        scheduler.reset_session();
        scheduler.push(InputEvent::ReleaseAll);
        let first = scheduler.next_ready(2).unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.event, InputEvent::ReleaseAll);
    }

    #[test]
    fn wire_format_binds_ack_to_session_token() {
        let token = b"session-token";
        let outbound = OutboundInput {
            sequence: 7,
            event: InputEvent::ReleaseAll,
        };
        let packet = encode_input(&outbound, token);
        assert_eq!(&packet[..4], INPUT_MAGIC);
        assert_eq!(&packet[4..8], &7u32.to_be_bytes());
        assert_eq!(*packet.last().unwrap(), *token.last().unwrap());

        let mut ack = ACK_MAGIC.to_vec();
        ack.extend_from_slice(&7u32.to_be_bytes());
        ack.extend_from_slice(token);
        assert_eq!(
            parse_ack(&ack, token),
            Some(InputAck {
                sequence: 7,
                enabled: None,
            })
        );
        assert_eq!(parse_ack(&ack, b"other-token"), None);

        let mut stateful_ack = ACK_MAGIC.to_vec();
        stateful_ack.extend_from_slice(&7u32.to_be_bytes());
        stateful_ack.push(1);
        stateful_ack.extend_from_slice(token);
        assert_eq!(
            parse_ack(&stateful_ack, token),
            Some(InputAck {
                sequence: 7,
                enabled: Some(true),
            })
        );

        let mut status = STATUS_MAGIC.to_vec();
        status.push(0);
        status.extend_from_slice(token);
        assert_eq!(parse_input_status(&status, token), Some(false));
        status[4] = 1;
        assert_eq!(parse_input_status(&status, token), Some(true));
    }

    #[test]
    fn every_event_matches_the_host_wire_layout() {
        let token = b"nonce";
        let cases = [
            (
                InputEvent::PointerMove {
                    x: 1,
                    y: 2,
                    buttons: 3,
                },
                1,
                false,
                18,
            ),
            (
                InputEvent::PointerButton {
                    x: 1,
                    y: 2,
                    button: 1,
                    down: true,
                    buttons: 1,
                },
                2,
                true,
                20,
            ),
            (
                InputEvent::Scroll {
                    horizontal_milli_lines: -1_000,
                    vertical_milli_lines: 1_000,
                },
                3,
                true,
                18,
            ),
            (
                InputEvent::Key {
                    key_code: 29,
                    scan_code: 30,
                    meta_state: 1,
                    down: true,
                    repeat: 2,
                },
                4,
                true,
                21,
            ),
            (InputEvent::ReleaseAll, 5, true, 10),
        ];

        for (event, kind, reliable, message_len) in cases {
            let packet = encode_input(&OutboundInput { sequence: 7, event }, token);
            assert_eq!(packet.len(), message_len + token.len());
            assert_eq!(packet[8], kind);
            assert_eq!(packet[9] & INPUT_FLAG_RELIABLE != 0, reliable);
            assert_eq!(&packet[message_len..], token);
        }
    }

    #[test]
    fn normalized_axis_clamps_and_rejects_nan() {
        assert_eq!(normalized_axis(-1.0), 0);
        assert_eq!(normalized_axis(2.0), u16::MAX);
        assert_eq!(normalized_axis(f32::NAN), 0);
    }
}
