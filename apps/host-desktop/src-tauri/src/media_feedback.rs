//! Viewer feedback contracts shared by Windows runtime and host-side tests.
//! Callers authenticate the complete message before interpreting these bytes.

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ReceiverFeedback {
    pub frame_gaps: u32,
    pub input_drops: u32,
    pub incomplete_aus: u32,
    pub stale_frames: u32,
    pub rtt_ms: Option<u32>,
    pub wire_ms: Option<u32>,
    pub stale_input_drops: Option<u32>,
    pub output_burst_discards: Option<u32>,
    pub rendered_fps: Option<u32>,
}

impl ReceiverFeedback {
    pub(crate) fn parse(message: &[u8]) -> Option<Self> {
        if message.len() < 24 || message.get(..4)? != b"LCF1" {
            return None;
        }
        let u32_at = |offset| {
            message
                .get(offset..offset + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_be_bytes)
        };
        let u16_at = |offset| {
            message
                .get(offset..offset + 2)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u16::from_be_bytes)
        };
        let latency = |offset| {
            u16_at(offset)
                .filter(|value| *value != u16::MAX)
                .map(u32::from)
        };
        Some(Self {
            frame_gaps: u32_at(4)?,
            input_drops: u32_at(8)?,
            incomplete_aus: u32_at(12)?,
            stale_frames: u32_at(16)?,
            rtt_ms: latency(20),
            wire_ms: latency(22),
            stale_input_drops: u32_at(24),
            output_burst_discards: u32_at(28),
            // Zero is an observed render stall; absent is an older viewer.
            rendered_fps: u16_at(32).map(u32::from),
        })
    }
}

pub(crate) fn latency_probe_response(
    message: &[u8],
    received_ms: u64,
    sent_ms: u64,
) -> Option<[u8; 32]> {
    if message.len() != 16 || message.get(..4)? != b"LCP1" {
        return None;
    }
    let mut response = [0; 32];
    response[..4].copy_from_slice(b"LCP2");
    response[4..16].copy_from_slice(&message[4..16]);
    response[16..24].copy_from_slice(&received_ms.to_be_bytes());
    response[24..].copy_from_slice(&sent_ms.to_be_bytes());
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feedback() -> Vec<u8> {
        let mut packet = b"LCF1".to_vec();
        for value in [1u32, 2, 3, 4] {
            packet.extend_from_slice(&value.to_be_bytes());
        }
        for value in [17u16, u16::MAX] {
            packet.extend_from_slice(&value.to_be_bytes());
        }
        for value in [5u32, 6] {
            packet.extend_from_slice(&value.to_be_bytes());
        }
        packet.extend_from_slice(&0u16.to_be_bytes());
        packet
    }

    #[test]
    fn receiver_feedback_keeps_legacy_offsets_unknown_latency_and_observed_zero_fps() {
        let packet = feedback();
        assert_eq!(
            ReceiverFeedback::parse(&packet),
            Some(ReceiverFeedback {
                frame_gaps: 1,
                input_drops: 2,
                incomplete_aus: 3,
                stale_frames: 4,
                rtt_ms: Some(17),
                wire_ms: None,
                stale_input_drops: Some(5),
                output_burst_discards: Some(6),
                rendered_fps: Some(0),
            })
        );
        let legacy = ReceiverFeedback::parse(&packet[..24]).unwrap();
        assert_eq!(legacy.stale_input_drops, None);
        assert_eq!(legacy.output_burst_discards, None);
        assert_eq!(legacy.rendered_fps, None);
        for length in 0..24 {
            assert_eq!(ReceiverFeedback::parse(&packet[..length]), None);
        }
        assert_eq!(ReceiverFeedback::parse(&[0; 34]), None);
        let mut extended = packet.clone();
        extended.extend_from_slice(&[0; 120]);
        assert_eq!(
            ReceiverFeedback::parse(&extended),
            ReceiverFeedback::parse(&packet)
        );
    }

    #[test]
    fn partial_optional_fields_are_never_read_as_zero_or_out_of_bounds() {
        let packet = feedback();
        for length in 24..=34 {
            let parsed = ReceiverFeedback::parse(&packet[..length]).unwrap();
            assert_eq!(parsed.stale_input_drops, (length >= 28).then_some(5));
            assert_eq!(parsed.output_burst_discards, (length >= 32).then_some(6));
            assert_eq!(parsed.rendered_fps, (length >= 34).then_some(0));
        }
    }

    #[test]
    fn latency_response_echoes_viewer_fields_and_both_host_clock_samples() {
        let mut packet = b"LCP1".to_vec();
        packet.extend_from_slice(&0x0123_4567u32.to_be_bytes());
        packet.extend_from_slice(&9_876_543_210u64.to_be_bytes());
        let response = latency_probe_response(&packet, 10_000_000_100, 10_000_000_102).unwrap();
        assert_eq!(&response[..4], b"LCP2");
        assert_eq!(&response[4..16], &packet[4..]);
        assert_eq!(
            u64::from_be_bytes(response[16..24].try_into().unwrap()),
            10_000_000_100
        );
        assert_eq!(
            u64::from_be_bytes(response[24..].try_into().unwrap()),
            10_000_000_102
        );
        for length in 0..16 {
            assert!(latency_probe_response(&packet[..length], 0, 0).is_none());
        }
        packet.push(0);
        assert!(latency_probe_response(&packet, 0, 0).is_none());
        assert!(latency_probe_response(&[0; 16], 0, 0).is_none());
    }
}
