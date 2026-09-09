//! Host-to-viewer cursor position plane (LCD1).
//!
//! Mirror of the LCI1 pointer plane: a lossy, newest-wins state stream.
//! The host sends a sample only when the cursor state changes, so a lost
//! datagram is healed by the next change and no reliability layer exists.

/// Wire magic for host→viewer cursor samples.
pub const CURSOR_MAGIC: &[u8; 4] = b"LCD1";
/// Viewer→host command that requests the cursor position stream.
pub const CURSOR_STREAM_ON: &[u8] = b"LCDON";
/// Viewer→host command that stops the cursor position stream.
pub const CURSOR_STREAM_OFF: &[u8] = b"LCDOFF";
/// Refresh the idempotent subscription state once per second. This bounds
/// control traffic while healing a dropped UDP command without an ACK plane.
pub const CURSOR_STREAM_REFRESH_US: u64 = 1_000_000;
/// Fixed payload width. The datagram is AEAD-sealed at the socket
/// boundary, so the inline session token the old format carried is gone.
pub const CURSOR_SAMPLE_LEN: usize = 14;

pub fn cursor_stream_command(enabled: bool) -> &'static [u8] {
    if enabled {
        CURSOR_STREAM_ON
    } else {
        CURSOR_STREAM_OFF
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CursorStreamDelivery {
    last_attempt: Option<(bool, u64)>,
}

impl CursorStreamDelivery {
    pub fn state_changed(&self, requested: bool) -> bool {
        self.last_attempt
            .map(|(previous, _)| previous != requested)
            .unwrap_or(true)
    }

    pub fn needs_send(&self, requested: bool, now_us: u64) -> bool {
        match self.last_attempt {
            None => true,
            Some((previous, _)) if previous != requested => true,
            Some((_, attempted_us)) => {
                now_us.saturating_sub(attempted_us) >= CURSOR_STREAM_REFRESH_US
            }
        }
    }
    pub fn record_attempt(&mut self, requested: bool, now_us: u64) {
        self.last_attempt = Some((requested, now_us));
    }
    pub fn reset(&mut self) {
        self.last_attempt = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSample {
    pub sequence: u32,
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}

pub fn parse_cursor_sample(packet: &[u8]) -> Option<CursorSample> {
    if packet.len() != CURSOR_SAMPLE_LEN || packet.get(..4)? != CURSOR_MAGIC {
        return None;
    }
    Some(CursorSample {
        sequence: u32::from_be_bytes(packet[4..8].try_into().ok()?),
        x: u16::from_be_bytes(packet[8..10].try_into().ok()?),
        y: u16::from_be_bytes(packet[10..12].try_into().ok()?),
        visible: packet[12] != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_crypto::MediaSessionCrypto;

    fn key(bytes: u8) -> [u8; 32] {
        (bytes..bytes + 32).collect::<Vec<u8>>().try_into().unwrap()
    }

    fn encode(sequence: u32, x: u16, y: u16, visible: bool) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CURSOR_SAMPLE_LEN);
        bytes.extend_from_slice(CURSOR_MAGIC);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&x.to_be_bytes());
        bytes.extend_from_slice(&y.to_be_bytes());
        bytes.push(u8::from(visible));
        bytes.push(0);
        bytes
    }

    #[test]
    fn cursor_sample_round_trips_and_is_sealed_on_the_wire() {
        let crypto = MediaSessionCrypto::new(key(1));
        let packet = encode(7, 0x1234, 0xabcd, true);
        assert_eq!(packet.len(), CURSOR_SAMPLE_LEN);
        let sealed = crypto.seal(&packet).unwrap();
        assert_eq!(
            parse_cursor_sample(&crypto.open(&sealed).unwrap()),
            Some(CursorSample {
                sequence: 7,
                x: 0x1234,
                y: 0xabcd,
                visible: true,
            })
        );
        // Without the key there is no plaintext to parse.
        assert!(parse_cursor_sample(&sealed).is_none());
    }

    #[test]
    fn truncated_or_foreign_packets_are_rejected() {
        assert_eq!(parse_cursor_sample(&[]), None);
        assert_eq!(parse_cursor_sample(&[0u8; 13]), None);
        let packet = encode(1, 0, 0, false);
        assert_eq!(parse_cursor_sample(&packet[..packet.len() - 1]), None);
        let mut foreign = packet.clone();
        foreign[0] = b'X';
        assert_eq!(parse_cursor_sample(&foreign), None);
    }

    #[test]
    fn stream_commands_are_the_documented_bytes() {
        assert_eq!(CURSOR_STREAM_ON, b"LCDON");
        assert_eq!(CURSOR_STREAM_OFF, b"LCDOFF");
    }

    #[test]
    fn delivery_retries_failures_and_tracks_on_off_transitions() {
        let mut state = CursorStreamDelivery::default();
        assert!(state.needs_send(false, 0));
        state.record_attempt(false, 0);
        assert!(!state.needs_send(false, CURSOR_STREAM_REFRESH_US - 1));
        // The first UDP datagram may be lost after a locally successful send;
        // refresh the idempotent state at a bounded interval regardless.
        assert!(state.needs_send(false, CURSOR_STREAM_REFRESH_US));
        assert!(!state.state_changed(false));
        state.record_attempt(false, CURSOR_STREAM_REFRESH_US);
        assert!(state.needs_send(true, CURSOR_STREAM_REFRESH_US));
        assert!(state.state_changed(true));
        assert_eq!(cursor_stream_command(true), b"LCDON");
        state.record_attempt(true, CURSOR_STREAM_REFRESH_US);
        assert!(!state.state_changed(true));
        assert!(state.needs_send(true, CURSOR_STREAM_REFRESH_US * 2));
        assert!(state.needs_send(false, CURSOR_STREAM_REFRESH_US));
        assert!(state.state_changed(false));
        assert_eq!(cursor_stream_command(false), b"LCDOFF");
        state.reset();
        assert!(state.needs_send(true, CURSOR_STREAM_REFRESH_US));
    }
}
