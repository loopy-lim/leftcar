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
/// Fixed payload width between the magic and the session token.
pub const CURSOR_SAMPLE_LEN: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSample {
    pub sequence: u32,
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}

pub fn parse_cursor_sample(packet: &[u8], token: &[u8]) -> Option<CursorSample> {
    if token.is_empty()
        || packet.len() != CURSOR_SAMPLE_LEN + token.len()
        || packet.get(..4)? != CURSOR_MAGIC
        || packet[CURSOR_SAMPLE_LEN..] != *token
    {
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

    fn encode(sequence: u32, x: u16, y: u16, visible: bool, token: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CURSOR_SAMPLE_LEN + token.len());
        bytes.extend_from_slice(CURSOR_MAGIC);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&x.to_be_bytes());
        bytes.extend_from_slice(&y.to_be_bytes());
        bytes.push(u8::from(visible));
        bytes.push(0);
        bytes.extend_from_slice(token);
        bytes
    }

    #[test]
    fn cursor_sample_round_trips_with_token_binding() {
        let token = b"session-token";
        let packet = encode(7, 0x1234, 0xabcd, true, token);
        assert_eq!(packet.len(), CURSOR_SAMPLE_LEN + token.len());
        assert_eq!(
            parse_cursor_sample(&packet, token),
            Some(CursorSample {
                sequence: 7,
                x: 0x1234,
                y: 0xabcd,
                visible: true,
            })
        );
        assert_eq!(parse_cursor_sample(&packet, b"wrong-token"), None);
    }

    #[test]
    fn same_length_wrong_token_is_rejected() {
        let token = b"session-token";
        let packet = encode(3, 1, 2, true, token);
        assert_eq!(parse_cursor_sample(&packet, b"session-tokeN"), None);
    }

    #[test]
    fn truncated_or_foreign_packets_are_rejected() {
        let token = b"nonce";
        assert_eq!(parse_cursor_sample(&[], token), None);
        assert_eq!(parse_cursor_sample(&[0u8; 13], token), None);
        let packet = encode(1, 0, 0, false, token);
        assert_eq!(parse_cursor_sample(&packet[..packet.len() - 1], token), None);
        let mut foreign = encode(1, 0, 0, false, token);
        foreign[0] = b'X';
        assert_eq!(parse_cursor_sample(&foreign, token), None);
    }

    #[test]
    fn stream_commands_are_the_documented_bytes() {
        assert_eq!(CURSOR_STREAM_ON, b"LCDON");
        assert_eq!(CURSOR_STREAM_OFF, b"LCDOFF");
    }
}
