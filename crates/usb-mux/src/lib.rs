//! Bounded channel framing for the Leftcar AOAP bulk link.
//!
//! Each frame is `[channel:u8][length:u32 BE][payload]`. Channel zero is the
//! newline-delimited control plane and channel one carries one existing media
//! frame per mux frame.

pub const CHANNEL_CONTROL: u8 = 0;
pub const CHANNEL_MEDIA: u8 = 1;
/// Matches the reliable TCP and decoder access-unit ceiling. High-complexity
/// 4K recovery frames can exceed the old 2 MiB limit.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxFrame {
    pub channel: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EncodeError {
    InvalidChannel(u8),
    PayloadTooLarge(usize),
    PayloadEmpty,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChannel(channel) => write!(f, "invalid mux channel {channel}"),
            Self::PayloadTooLarge(size) => {
                write!(f, "mux payload {size} exceeds {MAX_FRAME_BYTES}")
            }
            Self::PayloadEmpty => write!(f, "mux payload must not be empty"),
        }
    }
}

impl std::error::Error for EncodeError {}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    InvalidChannel(u8),
    LengthZero,
    LengthTooLarge(u32),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChannel(channel) => write!(f, "invalid mux channel {channel}"),
            Self::LengthZero => write!(f, "mux frame length is zero"),
            Self::LengthTooLarge(size) => write!(f, "mux frame length {size} is too large"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn encode(channel: u8, payload: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if channel > CHANNEL_MEDIA {
        return Err(EncodeError::InvalidChannel(channel));
    }
    if payload.is_empty() {
        return Err(EncodeError::PayloadEmpty);
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(EncodeError::PayloadTooLarge(payload.len()));
    }
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(channel);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

#[derive(Default)]
pub struct MuxDecoder {
    buffer: Vec<u8>,
}

impl MuxDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<MuxFrame>, DecodeError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            if self.buffer.len() < 5 {
                return Ok(frames);
            }
            let channel = self.buffer[0];
            if channel > CHANNEL_MEDIA {
                return Err(DecodeError::InvalidChannel(channel));
            }
            let length = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap());
            if length == 0 {
                return Err(DecodeError::LengthZero);
            }
            let length_usize = length as usize;
            if length_usize > MAX_FRAME_BYTES {
                return Err(DecodeError::LengthTooLarge(length));
            }
            if self.buffer.len() < 5 + length_usize {
                return Ok(frames);
            }
            let payload = self.buffer[5..5 + length_usize].to_vec();
            self.buffer.drain(..5 + length_usize);
            frames.push(MuxFrame { channel, payload });
        }
    }

    pub fn pending(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_frame() {
        let bytes = encode(CHANNEL_MEDIA, b"frame").unwrap();
        let mut decoder = MuxDecoder::new();
        assert_eq!(
            decoder.feed(&bytes).unwrap(),
            vec![MuxFrame {
                channel: CHANNEL_MEDIA,
                payload: b"frame".to_vec()
            }]
        );
    }

    #[test]
    fn split_and_coalesced_frames_decode() {
        let mut bytes = encode(CHANNEL_CONTROL, b"one").unwrap();
        bytes.extend(encode(CHANNEL_MEDIA, b"two").unwrap());
        let mut decoder = MuxDecoder::new();
        assert!(decoder.feed(&bytes[..3]).unwrap().is_empty());
        assert_eq!(decoder.pending(), 3);
        let frames = decoder.feed(&bytes[3..]).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].payload, b"one");
        assert_eq!(frames[1].payload, b"two");
    }

    #[test]
    fn rejects_invalid_channel_and_lengths() {
        assert_eq!(
            MuxDecoder::new().feed(&[2, 0, 0, 0, 1, b'x']),
            Err(DecodeError::InvalidChannel(2))
        );
        assert_eq!(
            MuxDecoder::new().feed(&[0, 0, 0, 0, 0]),
            Err(DecodeError::LengthZero)
        );
        assert_eq!(
            MuxDecoder::new().feed(&[1, 0xff, 0xff, 0xff, 0xff]),
            Err(DecodeError::LengthTooLarge(u32::MAX))
        );
    }

    #[test]
    fn encode_validates_payload() {
        assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
        assert_eq!(encode(7, b"x"), Err(EncodeError::InvalidChannel(7)));
        assert_eq!(encode(0, b""), Err(EncodeError::PayloadEmpty));
        let too_large = vec![0; MAX_FRAME_BYTES + 1];
        assert!(matches!(
            encode(CHANNEL_CONTROL, &too_large),
            Err(EncodeError::PayloadTooLarge(_))
        ));
        assert!(encode(CHANNEL_MEDIA, &vec![1; MAX_FRAME_BYTES]).is_ok());
    }
}
