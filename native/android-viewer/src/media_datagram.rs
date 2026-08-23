//! Low-latency UDP media datagram framing shared by the Android receiver and
//! host-side protocol tests.
//!
//! A video access unit can be much larger than the network MTU. Each datagram
//! therefore carries one fragment using this fixed header:
//!
//! ```text
//! G | fragment_index:u16 BE | fragment_count:u16 BE | au_id:u16 LE
//!   | L2 | capture_ms:u64 BE | encode_ms:u64 BE | send_ms:u64 BE
//!   | Annex-B bytes
//! ```
//!
//! The legacy `LT | send_ms` header is still accepted during rolling upgrades.

use std::collections::{HashMap, VecDeque};

pub const FRAME_MARKER: u8 = b'G';
pub const FRAME_HEADER_V1_LEN: usize = 17;
pub const FRAME_HEADER_V2_LEN: usize = 33;
pub const MAX_DATAGRAM_BYTES: usize = 1_200;
pub const MAX_FRAGMENT_PAYLOAD: usize = MAX_DATAGRAM_BYTES - FRAME_HEADER_V2_LEN;
// Two AUs tolerate normal cross-frame UDP reordering without allowing an
// incomplete old frame to occupy the receiver for more than roughly one
// display interval. Recovery then proceeds from a fresh IDR.
const MAX_IN_FLIGHT_AUS: usize = 2;
const MAX_FRAGMENTS_PER_AU: usize = 16_384;
const MAX_AU_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFragment {
    pub index: u16,
    pub count: u16,
    pub id: u16,
    pub capture_wall_ms: Option<u64>,
    pub encode_wall_ms: Option<u64>,
    pub send_wall_ms: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassembledFrame {
    pub id: u16,
    pub capture_wall_ms: Option<u64>,
    pub encode_wall_ms: Option<u64>,
    pub send_wall_ms: u64,
    pub au: Vec<u8>,
}

struct PartialFrame {
    capture_wall_ms: Option<u64>,
    encode_wall_ms: Option<u64>,
    send_wall_ms: u64,
    fragments: Vec<Option<Vec<u8>>>,
    received: usize,
    bytes: usize,
}

#[derive(Default)]
pub struct FrameReassembler {
    partial: HashMap<u16, PartialFrame>,
    insertion_order: VecDeque<u16>,
}

pub fn parse_fragment(datagram: &[u8]) -> Option<FrameFragment> {
    if datagram.len() <= FRAME_HEADER_V1_LEN || datagram[0] != FRAME_MARKER {
        return None;
    }
    let index = u16::from_be_bytes(datagram[1..3].try_into().ok()?);
    let count = u16::from_be_bytes(datagram[3..5].try_into().ok()?);
    let id = u16::from_le_bytes(datagram[5..7].try_into().ok()?);
    if count == 0 || index >= count {
        return None;
    }
    if usize::from(count) > MAX_FRAGMENTS_PER_AU {
        return None;
    }
    let (capture_wall_ms, encode_wall_ms, send_wall_ms, header_len) = match datagram.get(7..9)? {
        [b'L', b'2'] if datagram.len() > FRAME_HEADER_V2_LEN => (
            Some(u64::from_be_bytes(datagram[9..17].try_into().ok()?)),
            Some(u64::from_be_bytes(datagram[17..25].try_into().ok()?)),
            u64::from_be_bytes(datagram[25..33].try_into().ok()?),
            FRAME_HEADER_V2_LEN,
        ),
        [b'L', b'T'] => (
            None,
            None,
            u64::from_be_bytes(datagram[9..17].try_into().ok()?),
            FRAME_HEADER_V1_LEN,
        ),
        _ => return None,
    };
    Some(FrameFragment {
        index,
        count,
        id,
        capture_wall_ms,
        encode_wall_ms,
        send_wall_ms,
        payload: datagram[header_len..].to_vec(),
    })
}

impl FrameReassembler {
    pub fn clear(&mut self) {
        self.partial.clear();
        self.insertion_order.clear();
    }

    pub fn push(&mut self, fragment: FrameFragment) -> Option<ReassembledFrame> {
        let expected_count = usize::from(fragment.count);
        let needs_reset = self
            .partial
            .get(&fragment.id)
            .map(|partial| {
                partial.fragments.len() != expected_count
                    || partial.capture_wall_ms != fragment.capture_wall_ms
                    || partial.encode_wall_ms != fragment.encode_wall_ms
                    || partial.send_wall_ms != fragment.send_wall_ms
            })
            .unwrap_or(false);
        if needs_reset {
            self.remove(fragment.id);
        }

        if !self.partial.contains_key(&fragment.id) {
            while self.partial.len() >= MAX_IN_FLIGHT_AUS {
                let Some(oldest) = self.insertion_order.pop_front() else {
                    break;
                };
                self.partial.remove(&oldest);
            }
            self.insertion_order.push_back(fragment.id);
            self.partial.insert(
                fragment.id,
                PartialFrame {
                    capture_wall_ms: fragment.capture_wall_ms,
                    encode_wall_ms: fragment.encode_wall_ms,
                    send_wall_ms: fragment.send_wall_ms,
                    fragments: vec![None; expected_count],
                    received: 0,
                    bytes: 0,
                },
            );
        }

        let partial = self.partial.get_mut(&fragment.id)?;
        let slot = partial.fragments.get_mut(usize::from(fragment.index))?;
        if slot.is_none() {
            partial.bytes = partial.bytes.saturating_add(fragment.payload.len());
            if partial.bytes > MAX_AU_BYTES {
                self.remove(fragment.id);
                return None;
            }
            *slot = Some(fragment.payload);
            partial.received += 1;
        }
        if partial.received != partial.fragments.len() {
            return None;
        }

        let mut completed = self.partial.remove(&fragment.id)?;
        self.insertion_order.retain(|id| *id != fragment.id);
        let mut au = Vec::with_capacity(completed.bytes);
        for bytes in &mut completed.fragments {
            au.extend(bytes.take()?);
        }
        Some(ReassembledFrame {
            id: fragment.id,
            capture_wall_ms: completed.capture_wall_ms,
            encode_wall_ms: completed.encode_wall_ms,
            send_wall_ms: completed.send_wall_ms,
            au,
        })
    }

    fn remove(&mut self, id: u16) {
        self.partial.remove(&id);
        self.insertion_order.retain(|queued| *queued != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_datagram(index: u16, count: u16, id: u16, wall: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![FRAME_MARKER];
        bytes.extend_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(b"LT");
        bytes.extend_from_slice(&wall.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn datagram(
        index: u16,
        count: u16,
        id: u16,
        capture: u64,
        encode: u64,
        send: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![FRAME_MARKER];
        bytes.extend_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(b"L2");
        bytes.extend_from_slice(&capture.to_be_bytes());
        bytes.extend_from_slice(&encode.to_be_bytes());
        bytes.extend_from_slice(&send.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn parses_and_reassembles_out_of_order_fragments() {
        let mut reassembler = FrameReassembler::default();
        let second = parse_fragment(&datagram(1, 2, 42, 1_200, 1_220, 1_225, b"world")).unwrap();
        let first = parse_fragment(&datagram(0, 2, 42, 1_200, 1_220, 1_225, b"hello ")).unwrap();
        assert!(reassembler.push(second).is_none());
        assert_eq!(
            reassembler.push(first),
            Some(ReassembledFrame {
                id: 42,
                capture_wall_ms: Some(1_200),
                encode_wall_ms: Some(1_220),
                send_wall_ms: 1_225,
                au: b"hello world".to_vec(),
            })
        );
    }

    #[test]
    fn duplicate_fragment_does_not_finish_early() {
        let mut reassembler = FrameReassembler::default();
        let first = parse_fragment(&datagram(0, 2, 7, 1, 2, 3, b"a")).unwrap();
        assert!(reassembler.push(first.clone()).is_none());
        assert!(reassembler.push(first).is_none());
        let second = parse_fragment(&datagram(1, 2, 7, 1, 2, 3, b"b")).unwrap();
        assert_eq!(reassembler.push(second).unwrap().au, b"ab");
    }

    #[test]
    fn rejects_malformed_or_oversized_fragment_headers() {
        assert!(parse_fragment(b"short").is_none());
        assert!(parse_fragment(&datagram(2, 2, 1, 1, 2, 3, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, 0, 1, 1, 2, 3, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, u16::MAX, 1, 1, 2, 3, b"x")).is_none());
    }

    #[test]
    fn accepts_legacy_send_only_timestamp() {
        let fragment = parse_fragment(&legacy_datagram(0, 1, 9, 7_777, b"frame")).unwrap();
        assert_eq!(fragment.capture_wall_ms, None);
        assert_eq!(fragment.encode_wall_ms, None);
        assert_eq!(fragment.send_wall_ms, 7_777);
        assert_eq!(fragment.payload, b"frame");
    }
}
