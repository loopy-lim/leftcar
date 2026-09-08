//! Host → viewer audio plane (LCAU).
//!
//! System audio captured by ScreenCaptureKit crosses the wire as small
//! PCM datagrams on the media socket, sharing the video transport (UDP,
//! TCP-over-USB) and its loss tolerance: a dropped chunk is a brief skip,
//! never a broken reference chain. The ring keeps only recent chunks so a
//! stalled reader cannot build latency.
//!
//! Wire layout per datagram:
//! `LCAU | sequence:u16 BE | sample_rate:u16 BE | channels:u8 | rsv:u8
//!   | frame_count:u16 BE | int16 LE interleaved PCM`

use std::collections::VecDeque;

pub const AUDIO_MAGIC: &[u8; 4] = b"LCAU";
pub const AUDIO_HEADER_LEN: usize = 12;
/// Host-side chunk cap; one datagram stays comfortably under the media MTU.
/// The receiver rejects anything larger — no legitimate sender builds it.
pub const AUDIO_MAX_FRAMES_PER_DATAGRAM: usize = 300;
/// ~170ms of audio at the host chunk rate. Drop-oldest keeps the plane
/// latency-bounded: skipping forward is the correct response to backlog.
const AUDIO_RING_CAPACITY: usize = 16;
/// Header of the drained JNI blob: rate u16 BE, channels u8, rsv u8,
/// frames u16 BE.
pub const AUDIO_BLOB_HEADER_LEN: usize = 6;
/// Viewer→host command that requests the system-audio plane. Plaintext,
/// token-authenticated on the control channel — the same frame class as
/// LCDON, so hosts predating the toggle drop it as an unknown datagram.
pub const AUDIO_STREAM_ON: &[u8] = b"SNDON";
/// Viewer→host command that stops the system-audio plane.
pub const AUDIO_STREAM_OFF: &[u8] = b"SNDOFF";

pub fn audio_stream_command(enabled: bool) -> &'static [u8] {
    if enabled {
        AUDIO_STREAM_ON
    } else {
        AUDIO_STREAM_OFF
    }
}

/// Idempotent subscription refresh shared with the cursor plane: re-assert
/// the requested state once per second so a dropped UDP command heals
/// without an ACK plane.
pub type SystemAudioDelivery = crate::cursor_protocol::CursorStreamDelivery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioChunk {
    pub sequence: u16,
    pub sample_rate: u16,
    pub channels: u8,
    pub frame_count: u16,
}

pub fn parse_audio_chunk(packet: &[u8]) -> Option<AudioChunk> {
    if packet.len() < AUDIO_HEADER_LEN || packet.get(..4)? != AUDIO_MAGIC {
        return None;
    }
    let chunk = AudioChunk {
        sequence: u16::from_be_bytes(packet[4..6].try_into().ok()?),
        sample_rate: u16::from_be_bytes(packet[6..8].try_into().ok()?),
        channels: packet[8],
        frame_count: u16::from_be_bytes(packet[10..12].try_into().ok()?),
    };
    if chunk.sample_rate == 0 || chunk.channels == 0 || chunk.channels > 2 {
        return None;
    }
    let pcm_len = chunk.frame_count as usize * chunk.channels as usize * 2;
    if pcm_len == 0
        || chunk.frame_count as usize > AUDIO_MAX_FRAMES_PER_DATAGRAM
        || packet.len() != AUDIO_HEADER_LEN + pcm_len
    {
        return None;
    }
    Some(chunk)
}

/// Bounded receive ring. Sequences are wraparound-aware and strictly
/// monotonic on the wire, so a stale reordered datagram can never displace
/// newer audio.
#[derive(Default)]
pub struct AudioRing {
    chunks: VecDeque<(AudioChunk, Vec<u8>)>,
}

impl AudioRing {
    pub fn push(&mut self, packet: &[u8]) -> bool {
        let Some(chunk) = parse_audio_chunk(packet) else {
            return false;
        };
        let newer = self
            .chunks
            .back()
            .map(|(newest, _)| (chunk.sequence.wrapping_sub(newest.sequence) as i16) > 0)
            .unwrap_or(true);
        if !newer {
            return false;
        }
        if self
            .chunks
            .front()
            .is_some_and(|(oldest, _)| oldest.sample_rate != chunk.sample_rate
                || oldest.channels != chunk.channels)
        {
            // A format change invalidates everything buffered before it.
            self.chunks.clear();
        }
        self.chunks.push_back((chunk, packet.to_vec()));
        while self.chunks.len() > AUDIO_RING_CAPACITY {
            self.chunks.pop_front();
        }
        true
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Drain the ring into one PCM blob for playback:
    /// `rate u16 BE | channels u8 | rsv | frames u16 BE | PCM int16 LE`.
    /// Returns the written length, or 0 when nothing is buffered. Only whole
    /// chunks that fit the caller's buffer are included, so the frame count
    /// in the header always matches the PCM carried behind it.
    pub fn drain_into(&mut self, out: &mut [u8]) -> usize {
        let Some(&(head, _)) = self.chunks.front() else {
            return 0;
        };
        let channels = head.channels as usize;
        let mut written = AUDIO_BLOB_HEADER_LEN;
        let mut frames = 0usize;
        let mut included = 0usize;
        for (chunk, packet) in &self.chunks {
            let pcm = &packet[AUDIO_HEADER_LEN..];
            if written + pcm.len() > out.len() {
                break;
            }
            out[written..written + pcm.len()].copy_from_slice(pcm);
            written += pcm.len();
            frames += chunk.frame_count as usize;
            included += 1;
        }
        if included == 0 {
            self.chunks.clear();
            return 0;
        }
        encode_blob_header(out, head.sample_rate, channels, frames);
        self.chunks.drain(..included);
        written
    }
}

fn encode_blob_header(out: &mut [u8], sample_rate: u16, channels: usize, frames: usize) -> usize {
    out[..2].copy_from_slice(&sample_rate.to_be_bytes());
    out[2] = channels as u8;
    out[3] = 0;
    out[4..6].copy_from_slice(&(frames as u16).to_be_bytes());
    AUDIO_BLOB_HEADER_LEN
}

/// Demux helper for the media receive loops: a recognized LCAU packet is
/// stored and fully consumed.
pub fn accept_audio_packet(packet: &[u8], ring: &mut AudioRing) -> bool {
    if packet.len() >= AUDIO_HEADER_LEN && &packet[..4] == AUDIO_MAGIC {
        ring.push(packet);
        true
    } else {
        false
    }
}

/// Build a wire packet from raw little-endian interleaved PCM. Host tests
/// and the Swift layout stay in lockstep through this encoder.
pub fn encode_audio_packet(
    sequence: u16,
    sample_rate: u16,
    channels: u8,
    pcm_int16_le: &[u8],
) -> Vec<u8> {
    let channels = channels.clamp(1, 2);
    let frame_bytes = channels as usize * 2;
    let frames = (pcm_int16_le.len() / frame_bytes).min(AUDIO_MAX_FRAMES_PER_DATAGRAM);
    let mut packet = Vec::with_capacity(AUDIO_HEADER_LEN + frames * frame_bytes);
    packet.extend_from_slice(AUDIO_MAGIC);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&sample_rate.to_be_bytes());
    packet.push(channels);
    packet.push(0);
    packet.extend_from_slice(&(frames as u16).to_be_bytes());
    packet.extend_from_slice(&pcm_int16_le[..frames * frame_bytes]);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_pcm(frames: usize) -> Vec<u8> {
        (0..frames * 4).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn audio_stream_commands_match_wire_contract() {
        assert_eq!(AUDIO_STREAM_ON, b"SNDON");
        assert_eq!(AUDIO_STREAM_OFF, b"SNDOFF");
        assert_eq!(audio_stream_command(true), b"SNDON");
        assert_eq!(audio_stream_command(false), b"SNDOFF");
    }

    #[test]
    fn delivery_refreshes_until_acked_by_cadence() {
        let mut delivery = SystemAudioDelivery::default();
        // First attempt always fires, then a repeat inside the refresh
        // window is suppressed.
        assert!(delivery.needs_send(true, 0));
        delivery.record_attempt(true, 0);
        assert!(!delivery.needs_send(true, 500_000));
        assert!(delivery.needs_send(true, 1_000_000));
        // A state change fires immediately.
        assert!(delivery.needs_send(false, 600_000));
    }

    #[test]
    fn packet_round_trips_with_exact_length() {
        let pcm = stereo_pcm(300);
        let packet = encode_audio_packet(7, 48_000, 2, &pcm);
        let chunk = parse_audio_chunk(&packet).unwrap();
        assert_eq!(
            chunk,
            AudioChunk {
                sequence: 7,
                sample_rate: 48_000,
                channels: 2,
                frame_count: 300,
            }
        );
        assert_eq!(packet.len(), AUDIO_HEADER_LEN + 300 * 2 * 2);

        // The PCM payload is carried verbatim.
        assert_eq!(&packet[AUDIO_HEADER_LEN..], &pcm[..]);
    }

    #[test]
    fn oversized_input_is_truncated_to_one_datagram() {
        let packet = encode_audio_packet(1, 44_100, 1, &vec![0u8; 100_000]);
        let chunk = parse_audio_chunk(&packet).unwrap();
        assert_eq!(chunk.frame_count as usize, AUDIO_MAX_FRAMES_PER_DATAGRAM);
    }

    #[test]
    fn parser_rejects_malformed_packets() {
        let good = encode_audio_packet(1, 48_000, 2, &stereo_pcm(4));
        assert!(parse_audio_chunk(&good[..good.len() - 1]).is_none());
        assert!(parse_audio_chunk(&stereo_pcm(4)).is_none());
        let mut zero_rate = good.clone();
        zero_rate[6] = 0;
        zero_rate[7] = 0;
        assert!(parse_audio_chunk(&zero_rate).is_none());
        let mut bad_channels = good.clone();
        bad_channels[8] = 6;
        assert!(parse_audio_chunk(&bad_channels).is_none());
    }

    #[test]
    fn ring_drops_stale_and_keeps_newest_under_backlog() {
        let mut ring = AudioRing::default();
        assert!(ring.push(&encode_audio_packet(
            10,
            48_000,
            2,
            &stereo_pcm(4)
        )));
        // Reordered duplicate must not displace newer state.
        assert!(!ring.push(&encode_audio_packet(
            9,
            48_000,
            2,
            &stereo_pcm(4)
        )));
        for sequence in 11..10 + AUDIO_RING_CAPACITY + 4 {
            ring.push(&encode_audio_packet(
                sequence as u16,
                48_000,
                2,
                &stereo_pcm(4),
            ));
        }
        assert_eq!(ring.len(), AUDIO_RING_CAPACITY);
        let mut blob = [0u8; 4096];
        let len = ring.drain_into(&mut blob);
        assert_eq!(len, AUDIO_BLOB_HEADER_LEN + AUDIO_RING_CAPACITY * 4 * 2 * 2);
        assert_eq!(ring.len(), 0);
        assert_eq!(u16::from_be_bytes([blob[0], blob[1]]), 48_000);
        assert_eq!(blob[2], 2);
        assert_eq!(
            u16::from_be_bytes([blob[4], blob[5]]) as usize,
            AUDIO_RING_CAPACITY * 4
        );
    }

    #[test]
    fn format_change_flushes_older_chunks() {
        let mut ring = AudioRing::default();
        ring.push(&encode_audio_packet(1, 48_000, 2, &stereo_pcm(2)));
        ring.push(&encode_audio_packet(2, 44_100, 2, &stereo_pcm(2)));
        assert_eq!(ring.len(), 1);
        let mut blob = [0u8; 64];
        ring.drain_into(&mut blob);
        assert_eq!(u16::from_be_bytes([blob[0], blob[1]]), 44_100);
    }

    #[test]
    fn demux_helper_matches_only_audio_magic() {
        let mut ring = AudioRing::default();
        assert!(!accept_audio_packet(b"GXXXX", &mut ring));
        assert!(accept_audio_packet(
            &encode_audio_packet(1, 48_000, 1, &[1, 2, 3, 4]),
            &mut ring
        ));
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn drain_respects_caller_capacity() {
        let mut ring = AudioRing::default();
        ring.push(&encode_audio_packet(1, 48_000, 2, &stereo_pcm(300)));
        ring.push(&encode_audio_packet(2, 48_000, 2, &stereo_pcm(300)));
        // Only whole chunks are drained, so a buffer too small for one chunk
        // yields nothing rather than a mid-frame cut.
        let mut small = [0u8; 512];
        assert_eq!(ring.drain_into(&mut small), 0);
        assert!(ring.is_empty());

        ring.push(&encode_audio_packet(3, 48_000, 2, &stereo_pcm(300)));
        ring.push(&encode_audio_packet(4, 48_000, 2, &stereo_pcm(300)));
        let mut exact = [0u8; AUDIO_BLOB_HEADER_LEN + 300 * 2 * 2];
        let len = ring.drain_into(&mut exact);
        assert_eq!(len, exact.len());
        assert_eq!(
            u16::from_be_bytes([exact[4], exact[5]]) as usize,
            300
        );
    }
}
