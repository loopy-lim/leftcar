/// One Annex-B NAL unit boundary within a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalView<'a> {
    pub bytes: &'a [u8],
}

/// Split an Annex-B access unit into NAL units (handles 3- and 4-byte start
/// codes). Returns empty on no NALs found.
pub fn split_annexb(au: &[u8]) -> Vec<NalView<'_>> {
    let mut nals = Vec::new();
    let mut i = 0;
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (nal_start, sc_len)
    while i + 3 <= au.len() {
        if au[i] == 0 && au[i + 1] == 0 {
            if au[i + 2] == 1 {
                starts.push((i + 3, 3));
                i += 3;
                continue;
            } else if i + 3 < au.len() && au[i + 2] == 0 && au[i + 3] == 1 {
                starts.push((i + 4, 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    for w in 0..starts.len() {
        let (start, _) = starts[w];
        let end = if w + 1 < starts.len() {
            starts[w + 1].0 - starts[w + 1].1
        } else {
            au.len()
        };
        if start <= end && end > start {
            nals.push(NalView {
                bytes: &au[start..end],
            });
        }
    }
    nals
}

/// NAL type from the first payload byte (after start code): 5 bits.
pub fn nal_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|b| b & 0x1f)
}

/// Codec selected for one media session. The id is carried only in the new
/// configuration packet; legacy `CFG` packets are always H.264.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
}

impl VideoCodec {
    pub const fn id(self) -> u8 {
        match self {
            Self::H264 => 1,
            Self::Hevc => 2,
        }
    }

    pub const fn mime(self) -> &'static str {
        match self {
            Self::H264 => "video/avc",
            Self::Hevc => "video/hevc",
        }
    }

    pub const fn parameter_set_count(self) -> usize {
        match self {
            Self::H264 => 2,
            Self::Hevc => 3,
        }
    }

    pub const fn csd_names(self) -> &'static [&'static str] {
        match self {
            Self::H264 => &["csd-0", "csd-1"],
            Self::Hevc => &["csd-0", "csd-1", "csd-2"],
        }
    }

    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::H264),
            2 => Some(Self::Hevc),
            _ => None,
        }
    }
}

/// Codec configuration extracted from either the legacy H.264 `CFG` packet
/// or the codec-tagged `CF2` packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecConfig {
    pub codec: VideoCodec,
    pub vps: Option<Vec<u8>>,
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
}

/// Borrowed decoder construction parameters shared by H.264 and HEVC.
pub struct VideoDecoderConfig<'a> {
    pub codec: VideoCodec,
    pub vps: Option<&'a [u8]>,
    pub sps: &'a [u8],
    pub pps: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub window: usize,
    pub fps: u32,
    pub codec_name: Option<&'a str>,
    pub allow_mime_fallback: bool,
}

impl CodecConfig {
    pub fn requires_decoder_reset(&self, current: Option<&Self>) -> bool {
        current != Some(self)
    }

    pub fn is_complete(&self) -> bool {
        self.sps.is_some()
            && self.pps.is_some()
            && (self.codec == VideoCodec::H264 || self.vps.is_some())
    }
}

fn strip_annexb_start_code(nal: &[u8]) -> &[u8] {
    if nal.len() >= 4 && nal[..4] == [0, 0, 0, 1] {
        &nal[4..]
    } else if nal.len() >= 3 && nal[..3] == [0, 0, 1] {
        &nal[3..]
    } else {
        nal
    }
}

/// HEVC NAL type from the two-byte NAL header (bits 1..6 of the first byte).
pub fn hevc_nal_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|b| (b >> 1) & 0x3f)
}

/// Parse one complete codec configuration datagram. Lengths are bounded by
/// the packet itself, and every parameter set must have the expected NAL type
/// for the selected codec. Returning `None` makes malformed configuration a
/// normal recoverable input rather than a decoder panic.
pub fn parse_codec_config(packet: &[u8]) -> Option<CodecConfig> {
    let (codec, mut offset) = if packet.starts_with(b"CFG") {
        (VideoCodec::H264, 3)
    } else if packet.starts_with(b"CF2") {
        let codec = VideoCodec::from_id(*packet.get(3)?)?;
        (codec, 4)
    } else {
        return None;
    };

    let mut config = CodecConfig {
        codec,
        vps: None,
        sps: None,
        pps: None,
    };
    while offset < packet.len() {
        let length = u32::from_be_bytes(packet.get(offset..offset + 4)?.try_into().ok()?) as usize;
        offset += 4;
        if length == 0 || offset.checked_add(length)? > packet.len() {
            return None;
        }
        let nal = packet.get(offset..offset + length)?;
        offset += length;
        let raw = strip_annexb_start_code(nal);
        match codec {
            VideoCodec::H264 => match nal_type(raw) {
                Some(NAL_SPS) => config.sps = Some(nal.to_vec()),
                Some(NAL_PPS) => config.pps = Some(nal.to_vec()),
                _ => return None,
            },
            VideoCodec::Hevc => match hevc_nal_type(raw) {
                Some(32) => config.vps = Some(nal.to_vec()),
                Some(33) => config.sps = Some(nal.to_vec()),
                Some(34) => config.pps = Some(nal.to_vec()),
                _ => return None,
            },
        }
    }
    config.is_complete().then_some(config)
}

pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;
pub const NAL_IDR: u8 = 5;
pub const NAL_NON_IDR: u8 = 1;

/// Return whether an encoded access-unit id immediately follows the previous
/// one. The wire id is intentionally u16 to keep the hot packet header small,
/// so normal wrap-around is contiguous and any other jump means that at least
/// one H.264 reference frame was dropped before reaching the decoder.
pub fn frame_id_is_next(previous: u16, current: u16) -> bool {
    current == previous.wrapping_add(1)
}

/// Return the number of missing access units between two forward frame ids.
/// A duplicate or an implausibly large backwards jump is treated as a large
/// loss so the caller takes the hard recovery path instead of feeding an
/// ambiguous reference chain to MediaCodec.
pub fn frame_id_missing_count(previous: u16, current: u16) -> u16 {
    let distance = current.wrapping_sub(previous);
    if distance == 0 || distance > u16::MAX / 2 {
        return u16::MAX;
    }
    distance - 1
}

/// Extract csd-0 (SPS) and csd-1 (PPS) from an access unit chain for
/// `AMediaFormat_setBuffer("csd-0"/"csd-1", ...)`.
pub fn extract_config(aus: &[&[u8]]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut sps = None;
    let mut pps = None;
    for au in aus {
        for nal in split_annexb(au) {
            match nal_type(nal.bytes)? {
                NAL_SPS => sps = Some(with_start_code(nal.bytes)),
                NAL_PPS => pps = Some(with_start_code(nal.bytes)),
                _ => {}
            }
        }
    }
    Some((sps?, pps?))
}

fn with_start_code(nal: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(nal.len() + 4);
    v.extend_from_slice(&[0, 0, 0, 1]);
    v.extend_from_slice(nal);
    v
}

/// Parse width/height from a raw SPS NAL (no start code). Baseline profile:
/// enough Exp-Golomb to reach pic_width_in_mbs/pic_height_in_map_units.
pub fn parse_sps_dimensions(sps: &[u8]) -> Option<(u32, u32)> {
    // accept a raw SPS RBSP payload, a full NAL (header byte with
    // nal_unit_type == 7), or a NAL with an Annex-B start code
    let mut data: &[u8] = sps;
    if data.len() >= 4 && data[..4] == [0, 0, 0, 1] {
        data = &data[4..];
    } else if data.len() >= 3 && data[..3] == [0, 0, 1] {
        data = &data[3..];
    }
    let data: &[u8] = match data.first() {
        Some(b) if b & 0x1f == NAL_SPS => &data[1..],
        _ => data,
    };
    if data.len() < 4 {
        return None;
    }
    let mut br = BitReader { data, pos: 0 };
    let _profile = br.bits(8)?;
    let _constraints = br.bits(8)?;
    let _level = br.bits(8)?;
    let _seq_id = br.golomb()?;
    if _profile == 100
        || _profile == 110
        || _profile == 122
        || _profile == 244
        || _profile == 44
        || _profile == 83
        || _profile == 86
        || _profile == 118
        || _profile == 128
    {
        let chroma = br.golomb()?;
        if chroma == 3 {
            let _ = br.bits(1)?;
        }
        let _ = br.golomb()?;
        let _ = br.golomb()?;
        let _ = br.bits(1)?;
        let seq_scaling = br.bits(1)?;
        if seq_scaling == 1 {
            for _ in 0..8 {
                let cnt = br.golomb()?;
                if cnt > 15 {
                    return None;
                }
                if cnt != 0 {
                    return None;
                } // scaling lists unsupported here
            }
        }
    }
    let log2_frame = br.golomb()? + 4;
    let poc_type = br.golomb()?;
    if poc_type == 0 {
        let _ = br.golomb()?;
    } else if poc_type == 1 {
        let _ = br.bits(1)?;
        let _ = br.golomb()?;
        let _ = br.golomb()?;
        let n: u64 = br.golomb()?;
        if n > 256 {
            return None;
        }
        for _ in 0..n {
            let _ = br.golomb()?;
        }
    }
    if poc_type > 2 {
        return None;
    }
    let _ref_frames = br.golomb()?;
    let _gaps = br.bits(1)?;
    let pic_width_mbs = br.golomb()?;
    let pic_height_units = br.golomb()?;
    let frame_mbs_only = br.bits(1)?;
    let height_mul = if frame_mbs_only == 1 { 1 } else { 2 };
    if frame_mbs_only == 0 {
        let _ = br.bits(1)?;
    }
    let _direct = br.bits(1)?;
    let _crop = br.bits(1)?;
    let (mut crop_w, mut crop_h) = (0u32, 0u32);
    if _crop == 1 {
        let l = br.golomb()? as u32;
        let r = br.golomb()? as u32;
        let t = br.golomb()? as u32;
        let b = br.golomb()? as u32;
        crop_w = (l + r) * 2;
        crop_h = (t + b) * 2 * height_mul;
    }
    let width = (pic_width_mbs as u32 + 1) * 16 - crop_w;
    let height = (pic_height_units as u32 + 1) * 16 * height_mul - crop_h;
    let _ = log2_frame;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    Some((width, height))
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn bit(&mut self) -> Option<u32> {
        if self.pos >= self.data.len() * 8 {
            return None;
        }
        let byte = self.data[self.pos / 8];
        let b = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Some(b as u32)
    }
    fn bits(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Some(v)
    }
    fn golomb(&mut self) -> Option<u64> {
        let mut zeros = 0usize;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 63 {
                return None;
            }
        }
        let mut v = 1u64;
        for _ in 0..zeros {
            v = (v << 1) | self.bit()? as u64;
        }
        Some(v - 1)
    }
}
