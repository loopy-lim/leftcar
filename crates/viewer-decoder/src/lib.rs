//! Real hardware H.264 decoder via Android NDK `AMediaCodec` (H07).
//!
//! Direct `libmediandk` linkage — no Kotlin, no MediaCodec Java API. Input:
//! Annex-B access units (SPS/PPS/IDR/delta) from the assembler. Output: frames
//! rendered to the attached `ANativeWindow` (Surface). On host (non-Android)
//! builds the externs are not linked; tests cover the pure-Rust parser.

// -- NDK externs (media/NdkMediaCodec.h, NdkMediaFormat.h) -------------------

#![allow(non_camel_case_types)]

pub type AMediaCodec = std::ffi::c_void;
pub type AMediaFormat = std::ffi::c_void;
pub type media_status_t = i32;
pub type ssize_t = isize;

pub const AMEDIA_OK: media_status_t = 0;
pub const AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED: ssize_t = -2;
pub const AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED: ssize_t = -3;
pub const AMEDIACODEC_INFO_TRY_AGAIN_LATER: ssize_t = -1;

#[cfg(target_os = "android")]
extern "C" {
    #[link_name = "AMediaFormat_toString"]
    pub fn AMediaFormat_toString_pub(format: *const AMediaFormat) -> *mut std::ffi::c_char;
    #[link_name = "AMediaCodec_getName"]
    pub fn AMediaCodec_getName_pub(
        codec: *mut AMediaCodec,
        out_name: *mut *mut std::ffi::c_char,
    ) -> media_status_t;
    #[link_name = "AMediaFormat_setString"]
    pub fn AMediaFormat_setString_pub(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_char,
    );
    fn AMediaCodec_createDecoderByType(mime: *const std::ffi::c_char) -> *mut AMediaCodec;
    fn AMediaCodec_delete(codec: *mut AMediaCodec) -> media_status_t;
    fn AMediaCodec_configure(
        codec: *mut AMediaCodec,
        format: *const AMediaFormat,
        surface: *mut std::ffi::c_void, // ANativeWindow*
        crypto: *const std::ffi::c_void,
        flags: u32,
    ) -> media_status_t;
    fn AMediaCodec_start(codec: *mut AMediaCodec) -> media_status_t;
    fn AMediaCodec_stop(codec: *mut AMediaCodec) -> media_status_t;
    fn AMediaCodec_flush(codec: *mut AMediaCodec) -> media_status_t;
    fn AMediaCodec_queueInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        offset: usize,
        size: usize,
        time_us: i64,
        flags: u32,
    ) -> media_status_t;
    fn AMediaCodec_dequeueInputBuffer(codec: *mut AMediaCodec, timeout_us: i64) -> ssize_t;
    fn AMediaCodec_dequeueOutputBuffer(
        codec: *mut AMediaCodec,
        info: *mut AMediaCodecBufferInfo,
        timeout_us: i64,
    ) -> ssize_t;
    fn AMediaCodec_releaseOutputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        render: bool,
    ) -> media_status_t;
    fn AMediaCodec_getInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        out_size: *mut usize,
    ) -> *mut u8;
    fn AMediaFormat_new() -> *mut AMediaFormat;
    fn AMediaFormat_delete(format: *mut AMediaFormat) -> media_status_t;
    fn AMediaFormat_setBuffer(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        data: *const std::ffi::c_void,
        size: usize,
    );
    fn AMediaFormat_setInt32(format: *mut AMediaFormat, name: *const std::ffi::c_char, value: i32);
}

#[cfg(not(target_os = "android"))]
extern "C" {
    #[link_name = "AMediaFormat_toString"]
    pub fn AMediaFormat_toString_pub(format: *const AMediaFormat) -> *mut std::ffi::c_char;
    #[link_name = "AMediaCodec_getName"]
    pub fn AMediaCodec_getName_pub(
        codec: *mut AMediaCodec,
        out_name: *mut *mut std::ffi::c_char,
    ) -> media_status_t;
    // Host builds never call these; stubs keep the lib linking for tests.
    #[allow(clippy::missing_safety_doc)]
    pub fn AMediaCodec_createDecoderByType(mime: *const std::ffi::c_char) -> *mut AMediaCodec;
    pub fn AMediaCodec_delete(codec: *mut AMediaCodec) -> media_status_t;
    pub fn AMediaCodec_configure(
        codec: *mut AMediaCodec,
        format: *const AMediaFormat,
        surface: *mut std::ffi::c_void,
        crypto: *const std::ffi::c_void,
        flags: u32,
    ) -> media_status_t;
    pub fn AMediaCodec_start(codec: *mut AMediaCodec) -> media_status_t;
    pub fn AMediaCodec_stop(codec: *mut AMediaCodec) -> media_status_t;
    pub fn AMediaCodec_flush(codec: *mut AMediaCodec) -> media_status_t;
    pub fn AMediaCodec_queueInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        offset: usize,
        size: usize,
        time_us: i64,
        flags: u32,
    ) -> media_status_t;
    pub fn AMediaCodec_dequeueInputBuffer(codec: *mut AMediaCodec, timeout_us: i64) -> ssize_t;
    pub fn AMediaCodec_dequeueOutputBuffer(
        codec: *mut AMediaCodec,
        info: *mut AMediaCodecBufferInfo,
        timeout_us: i64,
    ) -> ssize_t;
    pub fn AMediaCodec_releaseOutputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        render: bool,
    ) -> media_status_t;
    pub fn AMediaCodec_getInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        out_size: *mut usize,
    ) -> *mut u8;
    pub fn AMediaFormat_new() -> *mut AMediaFormat;
    pub fn AMediaFormat_delete(format: *mut AMediaFormat) -> media_status_t;
    pub fn AMediaFormat_setBuffer(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        data: *const std::ffi::c_void,
        size: usize,
    );
    pub fn AMediaFormat_setInt32(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        value: i32,
    );
    #[link_name = "AMediaFormat_setString"]
    pub fn AMediaFormat_setString_pub(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_char,
    );
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AMediaCodecBufferInfo {
    pub offset: u32,
    pub size: u32,
    pub presentation_time_us: i64,
    pub flags: u32,
}

pub const AMEDIACODEC_CONFIGURE_FLAG_ENCODE: u32 = 1;
pub const BUFFER_FLAG_KEY_FRAME: u32 = 1;

// -- Annex-B parser (pure Rust, host-testable) --------------------------------

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

// -- Decoder session ------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DecoderError {
    #[error("codec creation failed for {mime}")]
    CreateFailed { mime: String },
    #[error("configure failed: status {status}")]
    ConfigureFailed { status: i32 },
    #[error("configure failed: status {status}, codec={codec_name}, format={format}")]
    ConfigureFailedWithFormat {
        status: i32,
        codec_name: String,
        format: String,
    },
    #[error("start failed: status {status}")]
    StartFailed { status: i32 },
    #[error("codec op failed: status {status}")]
    OpFailed { status: i32 },
    #[error("no input buffer within timeout")]
    InputTimeout,
    #[error("decoder not started")]
    NotStarted,
}

/// Result of a non-blocking input submission. A missing codec input buffer is
/// a normal real-time condition: the caller should drop this AU rather than
/// wait behind older video and increase interaction latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedStatus {
    Queued { rendered: bool },
    InputUnavailable,
}

/// A live hardware decoder session bound to one Surface.
pub struct AndroidDecoder {
    codec: *mut AMediaCodec,
    format: *mut AMediaFormat,
    started: bool,
    width: i32,
    height: i32,
    pub frames_rendered: u64,
}

unsafe impl Send for AndroidDecoder {}

impl AndroidDecoder {
    /// Create + configure + start an H.264 decoder rendering to `window`
    /// (an ANativeWindow* as usize; 0 = decode without display).
    ///
    /// # Safety
    /// `window` must be a valid ANativeWindow* (from ANativeWindow_fromSurface)
    /// or 0. Caller keeps the window alive for the session lifetime.
    pub unsafe fn new_h264(
        sps: &[u8],
        pps: &[u8],
        width: u32,
        height: u32,
        window: usize,
        fps: u32,
    ) -> Result<Self, DecoderError> {
        // strip optional Annex-B start codes so both conventions work
        fn strip_sc(b: &[u8]) -> &[u8] {
            if b.len() >= 4 && b[..4] == [0, 0, 0, 1] {
                &b[4..]
            } else if b.len() >= 3 && b[..3] == [0, 0, 1] {
                &b[3..]
            } else {
                b
            }
        }
        let sps_nal = strip_sc(sps);
        let (sw, sh) = parse_sps_dimensions(sps_nal).unwrap_or((width, height));
        let mime = c"video/avc".as_ptr();
        let codec = unsafe { AMediaCodec_createDecoderByType(mime) };
        if codec.is_null() {
            return Err(DecoderError::CreateFailed {
                mime: "video/avc".into(),
            });
        }
        let format = unsafe { AMediaFormat_new() };
        unsafe {
            // NDK samples set the mime key on the format even for decoders
            // created by type; some vendors reject without it.
            AMediaFormat_setString_pub(format, c"mime".as_ptr(), mime);
            AMediaFormat_setBuffer(format, c"csd-0".as_ptr(), sps.as_ptr().cast(), sps.len());
            AMediaFormat_setBuffer(format, c"csd-1".as_ptr(), pps.as_ptr().cast(), pps.len());
            AMediaFormat_setInt32(format, c"width".as_ptr(), sw as i32);
            AMediaFormat_setInt32(format, c"height".as_ptr(), sh as i32);
            // Tell platform decoders this is an interactive, real-time
            // stream. These are optional MediaFormat keys, so older/vendor
            // codecs can ignore them while modern codecs avoid extra queueing.
            let fps = fps.clamp(1, 60) as i32;
            AMediaFormat_setInt32(format, c"frame-rate".as_ptr(), fps);
            AMediaFormat_setInt32(format, c"operating-rate".as_ptr(), fps);
            AMediaFormat_setInt32(format, c"priority".as_ptr(), 0);
            AMediaFormat_setInt32(format, c"low-latency".as_ptr(), 1);
            let surface = if window == 0 {
                std::ptr::null_mut()
            } else {
                window as *mut std::ffi::c_void
            };
            let status = AMediaCodec_configure(codec, format, surface, std::ptr::null(), 0);
            if status != AMEDIA_OK {
                let fmt_str = AMediaFormat_toString_pub(format);
                let fmt_cstr = if fmt_str.is_null() {
                    "<null>".to_string()
                } else {
                    std::ffi::CStr::from_ptr(fmt_str)
                        .to_string_lossy()
                        .into_owned()
                };
                let mut name_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
                let name = if AMediaCodec_getName_pub(codec, &mut name_ptr) == AMEDIA_OK
                    && !name_ptr.is_null()
                {
                    // leak the small name buffer: no delete API in this NDK
                    std::ffi::CStr::from_ptr(name_ptr)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    "<unknown>".to_string()
                };
                AMediaFormat_delete(format);
                AMediaCodec_delete(codec);
                return Err(DecoderError::ConfigureFailedWithFormat {
                    status,
                    format: fmt_str_cstr(fmt_cstr),
                    codec_name: name,
                });
            }
            let status = AMediaCodec_start(codec);
            if status != AMEDIA_OK {
                AMediaFormat_delete(format);
                AMediaCodec_delete(codec);
                return Err(DecoderError::StartFailed { status });
            }
        }
        Ok(Self {
            codec,
            format,
            started: true,
            width: sw as i32,
            height: sh as i32,
            frames_rendered: 0,
        })
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Feed one Annex-B access unit; returns whether an output buffer was
    /// dequeued and rendered this call.
    pub fn feed_au(
        &mut self,
        au: &[u8],
        pts_us: i64,
        timeout_us: i64,
    ) -> Result<bool, DecoderError> {
        match self.feed_au_status(au, pts_us, timeout_us)? {
            FeedStatus::Queued { rendered } => Ok(rendered),
            FeedStatus::InputUnavailable => Ok(false),
        }
    }

    /// Non-blocking variant used by the live renderer so decoder backpressure
    /// becomes an explicit frame drop instead of stale-video accumulation.
    pub fn feed_au_status(
        &mut self,
        au: &[u8],
        pts_us: i64,
        timeout_us: i64,
    ) -> Result<FeedStatus, DecoderError> {
        if !self.started {
            return Err(DecoderError::NotStarted);
        }
        let idx = unsafe { AMediaCodec_dequeueInputBuffer(self.codec, timeout_us) };
        if idx < 0 {
            return Ok(FeedStatus::InputUnavailable);
        }
        let idx = idx as usize;
        let mut capacity = 0usize;
        let buf = unsafe { AMediaCodec_getInputBuffer(self.codec, idx, &mut capacity) };
        if buf.is_null() || au.len() > capacity {
            // drop oversized AU rather than corrupt the codec
            unsafe {
                AMediaCodec_queueInputBuffer(self.codec, idx, 0, 0, pts_us, 0);
            }
            return Ok(FeedStatus::InputUnavailable);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(au.as_ptr(), buf, au.len());
            let q = AMediaCodec_queueInputBuffer(self.codec, idx, 0, au.len(), pts_us, 0);
            if q != AMEDIA_OK {
                return Err(DecoderError::OpFailed { status: q });
            }
        }
        Ok(FeedStatus::Queued {
            rendered: self.pump_output(timeout_us)?,
        })
    }

    /// Dequeue and render any ready output buffers.
    pub fn pump_output(&mut self, timeout_us: i64) -> Result<bool, DecoderError> {
        let mut info = AMediaCodecBufferInfo {
            offset: 0,
            size: 0,
            presentation_time_us: 0,
            flags: 0,
        };
        let idx = unsafe { AMediaCodec_dequeueOutputBuffer(self.codec, &mut info, timeout_us) };
        match idx {
            AMEDIACODEC_INFO_TRY_AGAIN_LATER
            | AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
            | AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => Ok(false),
            i if i >= 0 => {
                let r = unsafe { AMediaCodec_releaseOutputBuffer(self.codec, i as usize, true) };
                if r != AMEDIA_OK {
                    return Err(DecoderError::OpFailed { status: r });
                }
                self.frames_rendered += 1;
                Ok(true)
            }
            err => Err(DecoderError::OpFailed { status: err as i32 }),
        }
    }

    /// Flush for an epoch reset (surface recreate): drop in-flight refs,
    /// caller feeds a fresh IDR after.
    pub fn flush(&mut self) -> Result<(), DecoderError> {
        let s = unsafe { AMediaCodec_flush(self.codec) };
        if s != AMEDIA_OK {
            return Err(DecoderError::OpFailed { status: s });
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.started {
            unsafe {
                AMediaCodec_stop(self.codec);
            }
            self.started = false;
        }
    }
}

impl Drop for AndroidDecoder {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            AMediaFormat_delete(self.format);
            AMediaCodec_delete(self.codec);
        }
    }
}

// -- host-side tests (pure parser) ----------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn au(parts: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(&[0, 0, 0, 1]);
            v.extend_from_slice(p);
        }
        v
    }

    #[test]
    fn split_annexb_finds_three_and_four_byte_start_codes() {
        let mut buf = vec![0, 0, 1, 0x67, 0xAA]; // 3-byte SC + SPS-ish
        buf.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xBB]); // 4-byte SC + PPS-ish
        let nals = split_annexb(&buf);
        assert_eq!(nals.len(), 2);
        assert_eq!(nal_type(nals[0].bytes), Some(7));
        assert_eq!(nal_type(nals[1].bytes), Some(8));
        assert_eq!(nals[0].bytes, &[0x67, 0xAA]);
        assert_eq!(nals[1].bytes, &[0x68, 0xBB]);
    }

    #[test]
    fn split_annexb_empty_and_garbage() {
        assert!(split_annexb(&[]).is_empty());
        assert!(split_annexb(&[1, 2, 3]).is_empty());
        // start code at end with no payload -> no NAL
        assert!(split_annexb(&[0, 0, 1]).is_empty());
    }

    #[test]
    fn extract_config_returns_sps_pps_with_start_codes() {
        let key = au(&[&[0x67, 0x64], &[0x68, 0x1F]]);
        let (sps, pps) = extract_config(&[&key]).unwrap();
        assert_eq!(&sps[..4], &[0, 0, 0, 1]);
        assert_eq!(sps[4], 0x67);
        assert_eq!(pps[4], 0x68);
    }

    #[test]
    fn extract_config_none_without_sps_pps() {
        let delta = au(&[&[0x41, 0x9A]]);
        assert!(extract_config(&[&delta]).is_none());
    }

    #[test]
    fn nal_type_masks_low_five_bits() {
        // 0x65 = IDR with ref/idc bits; 0x41 = non-IDR slice
        assert_eq!(nal_type(&[0x65]), Some(NAL_IDR));
        assert_eq!(nal_type(&[0x41]), Some(NAL_NON_IDR));
    }

    #[test]
    fn frame_id_gap_requires_decoder_resync() {
        assert!(frame_id_is_next(41, 42));
        assert!(frame_id_is_next(u16::MAX, 0));
        assert!(!frame_id_is_next(41, 43));
        assert!(!frame_id_is_next(41, 41));
    }

    #[test]
    fn au_without_trailing_zero_no_nal_split_bug() {
        // regression guard: a NAL containing 00 00 01 inside payload? Not
        // valid in real streams (emulation prevention); parser treats found
        // start codes as boundaries — documented behavior.
        let buf = au(&[&[0x65, 0x00, 0x00, 0x01, 0x99]]);
        let nals = split_annexb(&buf);
        assert_eq!(
            nals.len(),
            2,
            "start-code-looking payload splits (documented)"
        );
    }
}

/// Configure-time debug: format keys AMediaCodec expects for AVC decode.
/// Kept public for the harness to cross-check.
pub const FORMAT_KEY_MIME: &str = "mime";
pub const FORMAT_KEY_CSD0: &str = "csd-0";
pub const FORMAT_KEY_CSD1: &str = "csd-1";
pub const FORMAT_KEY_WIDTH: &str = "width";
pub const FORMAT_KEY_HEIGHT: &str = "height";

fn fmt_str_cstr(s: String) -> String {
    s
}

#[cfg(test)]
mod sps_tests {
    use super::*;

    /// Real VideoToolbox-generated SPS for 320x240 baseline (level 20):
    /// full NAL with header byte 0x27, as split_annexb yields it.
    const VT_SPS_320X240: &[u8] = &[0x27, 0x42, 0x00, 0x14, 0xab, 0x40, 0xa0, 0xfc];
    /// Same SPS with an Annex-B start code, as the harness passes it.
    const VT_SPS_WITH_SC: &[u8] = &[
        0x00, 0x00, 0x00, 0x01, 0x27, 0x42, 0x00, 0x14, 0xab, 0x40, 0xa0, 0xfc,
    ];

    #[test]
    fn parses_videotoolbox_320x240_with_start_code() {
        // regression: the device run showed 32x1280 when the header byte was
        // parsed as profile_idc — this is the exact on-device input shape
        let (w, h) = parse_sps_dimensions(VT_SPS_WITH_SC).expect("parses");
        assert_eq!((w, h), (320, 240));
    }

    #[test]
    fn parses_videotoolbox_320x240() {
        let (w, h) = parse_sps_dimensions(VT_SPS_320X240).expect("parses");
        assert_eq!((w, h), (320, 240));
    }
}
