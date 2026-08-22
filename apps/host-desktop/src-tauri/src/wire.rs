//! Platform-neutral Leftcar UDP media and remote-input wire helpers.
//!
//! Both native host implementations use the same MTU, timestamps, nonce
//! authentication and reliable input sequencing. Keeping those rules here
//! prevents a Windows host from subtly diverging from the macOS shim.

pub const MAX_DATAGRAM: usize = 1_200;
const MEDIA_HEADER: usize = 17;
pub const MAX_MEDIA_PAYLOAD: usize = MAX_DATAGRAM - MEDIA_HEADER;

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
        horizontal_milli: i32,
        vertical_milli: i32,
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

#[derive(Debug, Clone, PartialEq)]
pub enum InputDecision {
    Ignore,
    Apply(InputEvent),
    ApplyAndAck { sequence: u32, event: InputEvent },
    AckDuplicate(u32),
}

#[derive(Default)]
pub struct InputSequencer {
    last_reliable: u32,
    last_pointer: u32,
}

impl InputSequencer {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn accept(&mut self, message: &[u8]) -> InputDecision {
        if message.len() < 10 || &message[..4] != b"LCI1" {
            return InputDecision::Ignore;
        }
        let sequence = u32::from_be_bytes(message[4..8].try_into().unwrap());
        let kind = message[8];
        let reliable = message[9] & 1 == 1;

        if !reliable {
            if kind != 1 || message.len() != 18 || !is_newer(sequence, self.last_pointer) {
                return InputDecision::Ignore;
            }
            self.last_pointer = sequence;
            return InputDecision::Apply(InputEvent::PointerMove {
                x: read_u16(message, 10),
                y: read_u16(message, 12),
                buttons: read_u32(message, 14),
            });
        }

        if sequence == self.last_reliable {
            return InputDecision::AckDuplicate(sequence);
        }
        // ReleaseAll is a fail-safe and may jump over one lost transition.
        if kind == 5 {
            if message.len() != 10 || !is_newer(sequence, self.last_reliable) {
                return InputDecision::Ignore;
            }
            self.last_reliable = sequence;
            return InputDecision::ApplyAndAck {
                sequence,
                event: InputEvent::ReleaseAll,
            };
        }
        if sequence != self.last_reliable.wrapping_add(1) {
            return InputDecision::Ignore;
        }
        let event = match kind {
            2 if message.len() == 20 => InputEvent::PointerButton {
                x: read_u16(message, 10),
                y: read_u16(message, 12),
                button: message[14],
                down: message[15] != 0,
                buttons: read_u32(message, 16),
            },
            3 if message.len() == 18 => InputEvent::Scroll {
                horizontal_milli: read_i32(message, 10),
                vertical_milli: read_i32(message, 14),
            },
            4 if message.len() == 21 => InputEvent::Key {
                key_code: read_u16(message, 10),
                scan_code: read_u16(message, 12),
                meta_state: read_u32(message, 14),
                down: message[18] != 0,
                repeat: read_u16(message, 19),
            },
            _ => return InputDecision::Ignore,
        };
        self.last_reliable = sequence;
        InputDecision::ApplyAndAck { sequence, event }
    }
}

fn is_newer(candidate: u32, previous: u32) -> bool {
    candidate.wrapping_sub(previous) as i32 > 0
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

pub fn authenticated<'a>(datagram: &'a [u8], token: &[u8]) -> Option<&'a [u8]> {
    datagram
        .strip_suffix(token)
        .filter(|message| !message.is_empty())
}

pub fn challenge(token: &[u8]) -> Vec<u8> {
    [b"LCH1".as_slice(), token].concat()
}

pub fn input_ack(sequence: u32, token: &[u8]) -> Vec<u8> {
    let mut ack = Vec::with_capacity(8 + token.len());
    ack.extend_from_slice(b"LCA1");
    ack.extend_from_slice(&sequence.to_be_bytes());
    ack.extend_from_slice(token);
    ack
}

/// Fragment an Annex-B H.264 access unit using the exact Android viewer wire
/// envelope: `G | fragment index/count | AU id LE | LT | wall ms | payload`.
pub fn media_datagrams(au_id: u16, host_wall_ms: u64, annex_b: &[u8]) -> Vec<Vec<u8>> {
    let count = annex_b.len().div_ceil(MAX_MEDIA_PAYLOAD).max(1);
    if count > u16::MAX as usize {
        return Vec::new();
    }
    (0..count)
        .map(|index| {
            let start = index * MAX_MEDIA_PAYLOAD;
            let end = annex_b.len().min(start + MAX_MEDIA_PAYLOAD);
            let mut datagram = Vec::with_capacity(MEDIA_HEADER + end - start);
            datagram.push(b'G');
            datagram.extend_from_slice(&(index as u16).to_be_bytes());
            datagram.extend_from_slice(&(count as u16).to_be_bytes());
            datagram.extend_from_slice(&au_id.to_le_bytes());
            datagram.extend_from_slice(b"LT");
            datagram.extend_from_slice(&host_wall_ms.to_be_bytes());
            datagram.extend_from_slice(&annex_b[start..end]);
            datagram
        })
        .collect()
}

pub fn config_datagram(parameter_sets: &[Vec<u8>]) -> Option<Vec<u8>> {
    if parameter_sets.is_empty() {
        return None;
    }
    let mut out = Vec::from(b"CFG".as_slice());
    for parameter_set in parameter_sets {
        let length = u32::try_from(parameter_set.len().checked_add(4)?).ok()?;
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(parameter_set);
    }
    Some(out)
}

/// Accept either Annex-B or four-byte AVCC length-prefixed H.264 from a Media
/// Foundation transform and normalize it for the Android MediaCodec path.
pub fn normalize_h264(sample: &[u8]) -> Option<Vec<u8>> {
    if sample.starts_with(&[0, 0, 0, 1]) || sample.starts_with(&[0, 0, 1]) {
        return Some(sample.to_vec());
    }
    let mut offset = 0usize;
    let mut out = Vec::with_capacity(sample.len() + 16);
    while offset < sample.len() {
        if sample.len() - offset < 4 {
            return None;
        }
        let length = u32::from_be_bytes(sample[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if length == 0 || offset.checked_add(length)? > sample.len() {
            return None;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&sample[offset..offset + length]);
        offset += length;
    }
    (!out.is_empty()).then_some(out)
}

pub fn h264_parameter_sets(annex_b: &[u8]) -> Vec<Vec<u8>> {
    annex_b_nals(annex_b)
        .filter(|nal| matches!(nal.first().map(|byte| byte & 0x1f), Some(7 | 8)))
        .map(<[u8]>::to_vec)
        .collect()
}

pub fn avcc_parameter_sets(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    if bytes.len() < 7 || bytes[0] != 1 {
        return None;
    }
    let mut offset = 6;
    let sps_count = bytes[5] & 0x1f;
    let mut output = Vec::new();
    for _ in 0..sps_count {
        let length = usize::from(u16::from_be_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        offset += 2;
        output.push(bytes.get(offset..offset + length)?.to_vec());
        offset += length;
    }
    let pps_count = *bytes.get(offset)?;
    offset += 1;
    for _ in 0..pps_count {
        let length = usize::from(u16::from_be_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        offset += 2;
        output.push(bytes.get(offset..offset + length)?.to_vec());
        offset += length;
    }
    (!output.is_empty()).then_some(output)
}

fn annex_b_nals(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut offset = 0;
    while offset + 3 <= data.len() {
        let prefix = if data[offset..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if data[offset..].starts_with(&[0, 0, 1]) {
            3
        } else {
            offset += 1;
            continue;
        };
        starts.push((offset, prefix));
        offset += prefix;
    }
    let nals = starts
        .iter()
        .enumerate()
        .filter_map(|(index, (start, prefix))| {
            let end = if index + 1 < starts.len() {
                starts[index + 1].0
            } else {
                data.len()
            };
            (start + prefix < end).then_some(&data[start + prefix..end])
        })
        .collect::<Vec<_>>();
    nals.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_fragments_stay_under_mtu_and_preserve_au() {
        let access_unit = vec![0x55; 3_000];
        let datagrams = media_datagrams(0x1234, 99, &access_unit);
        assert_eq!(datagrams.len(), 3);
        assert!(datagrams.iter().all(|packet| packet.len() <= MAX_DATAGRAM));
        assert_eq!(&datagrams[0][5..7], &0x1234u16.to_le_bytes());
        assert_eq!(
            datagrams
                .iter()
                .flat_map(|packet| packet[17..].to_vec())
                .collect::<Vec<_>>(),
            access_unit
        );
    }

    #[test]
    fn reliable_input_is_ordered_and_duplicate_is_acked() {
        let mut sequencer = InputSequencer::default();
        let mut packet = Vec::from(b"LCI1".as_slice());
        packet.extend_from_slice(&1u32.to_be_bytes());
        packet.extend_from_slice(&[2, 1]);
        packet.extend_from_slice(&10u16.to_be_bytes());
        packet.extend_from_slice(&20u16.to_be_bytes());
        packet.extend_from_slice(&[1, 1]);
        packet.extend_from_slice(&1u32.to_be_bytes());
        assert!(matches!(
            sequencer.accept(&packet),
            InputDecision::ApplyAndAck { sequence: 1, .. }
        ));
        assert_eq!(sequencer.accept(&packet), InputDecision::AckDuplicate(1));
        packet[7] = 3;
        assert_eq!(sequencer.accept(&packet), InputDecision::Ignore);
    }

    #[test]
    fn avcc_is_normalized_and_parameter_sets_are_found() {
        let avcc = [0, 0, 0, 2, 0x67, 1, 0, 0, 0, 2, 0x68, 2];
        let annex_b = normalize_h264(&avcc).unwrap();
        assert_eq!(
            h264_parameter_sets(&annex_b),
            vec![vec![0x67, 1], vec![0x68, 2]]
        );
    }

    #[test]
    fn decoder_configuration_record_yields_sps_and_pps() {
        let avcc = [1, 100, 0, 31, 0xff, 0xe1, 0, 2, 0x67, 1, 1, 0, 2, 0x68, 2];
        assert_eq!(
            avcc_parameter_sets(&avcc),
            Some(vec![vec![0x67, 1], vec![0x68, 2]])
        );
    }
}
