//! Low-latency UDP media datagram framing shared by the Android receiver and
//! host-side protocol tests.
//!
//! A video access unit can be much larger than the network MTU. Each datagram
//! therefore carries one fragment using this fixed header:
//!
//! ```text
//! G | fragment_index:u16 BE | fragment_count:u16 BE | au_id:u16 LE
//!   | LT | host_wall_ms:u64 BE | Annex-B bytes
//! ```

use std::collections::{HashMap, VecDeque};

pub const FRAME_MARKER: u8 = b'G';
pub const FRAME_HEADER_LEN: usize = 17;
pub const MAX_DATAGRAM_BYTES: usize = 1_200;
pub const MAX_FRAGMENT_PAYLOAD: usize = MAX_DATAGRAM_BYTES - FRAME_HEADER_LEN;
const MAX_IN_FLIGHT_AUS: usize = 8;
const MAX_FRAGMENTS_PER_AU: usize = 16_384;
const MAX_AU_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFragment {
    pub index: u16,
    pub count: u16,
    pub id: u16,
    pub host_wall_ms: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassembledFrame {
    pub id: u16,
    pub host_wall_ms: u64,
    pub au: Vec<u8>,
}

struct PartialFrame {
    host_wall_ms: u64,
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
    if datagram.len() <= FRAME_HEADER_LEN || datagram[0] != FRAME_MARKER {
        return None;
    }
    let index = u16::from_be_bytes(datagram[1..3].try_into().ok()?);
    let count = u16::from_be_bytes(datagram[3..5].try_into().ok()?);
    let id = u16::from_le_bytes(datagram[5..7].try_into().ok()?);
    if datagram[7..9] != [b'L', b'T'] || count == 0 || index >= count {
        return None;
    }
    if usize::from(count) > MAX_FRAGMENTS_PER_AU {
        return None;
    }
    Some(FrameFragment {
        index,
        count,
        id,
        host_wall_ms: u64::from_be_bytes(datagram[9..17].try_into().ok()?),
        payload: datagram[FRAME_HEADER_LEN..].to_vec(),
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
                    || partial.host_wall_ms != fragment.host_wall_ms
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
                    host_wall_ms: fragment.host_wall_ms,
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
            host_wall_ms: completed.host_wall_ms,
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

    fn datagram(index: u16, count: u16, id: u16, wall: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![FRAME_MARKER];
        bytes.extend_from_slice(&index.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(b"LT");
        bytes.extend_from_slice(&wall.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn parses_and_reassembles_out_of_order_fragments() {
        let mut reassembler = FrameReassembler::default();
        let second = parse_fragment(&datagram(1, 2, 42, 1234, b"world")).unwrap();
        let first = parse_fragment(&datagram(0, 2, 42, 1234, b"hello ")).unwrap();
        assert!(reassembler.push(second).is_none());
        assert_eq!(
            reassembler.push(first),
            Some(ReassembledFrame {
                id: 42,
                host_wall_ms: 1234,
                au: b"hello world".to_vec(),
            })
        );
    }

    #[test]
    fn duplicate_fragment_does_not_finish_early() {
        let mut reassembler = FrameReassembler::default();
        let first = parse_fragment(&datagram(0, 2, 7, 9, b"a")).unwrap();
        assert!(reassembler.push(first.clone()).is_none());
        assert!(reassembler.push(first).is_none());
        let second = parse_fragment(&datagram(1, 2, 7, 9, b"b")).unwrap();
        assert_eq!(reassembler.push(second).unwrap().au, b"ab");
    }

    #[test]
    fn rejects_malformed_or_oversized_fragment_headers() {
        assert!(parse_fragment(b"short").is_none());
        assert!(parse_fragment(&datagram(2, 2, 1, 1, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, 0, 1, 1, b"x")).is_none());
        assert!(parse_fragment(&datagram(0, u16::MAX, 1, 1, b"x")).is_none());
    }
}
