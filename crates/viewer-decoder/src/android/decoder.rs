use super::ffi::*;
use super::output::MAX_RENDERABLE_OUTPUTS;
use crate::annex_b::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderCandidate<'a> {
    Named(&'a str),
    MimeType,
}

pub fn decoder_candidate_plan(
    codec_name: Option<&str>,
    allow_mime_fallback: bool,
) -> Vec<DecoderCandidate<'_>> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(name) = codec_name {
        candidates.push(DecoderCandidate::Named(name));
    }
    if allow_mime_fallback {
        candidates.push(DecoderCandidate::MimeType);
    }
    candidates
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
    InputTooLarge { required: usize, capacity: usize },
}

/// A live hardware decoder session bound to one Surface.
pub struct AndroidDecoder {
    pub(super) codec: *mut AMediaCodec,
    pub(super) format: *mut AMediaFormat,
    pub(super) started: bool,
    width: i32,
    height: i32,
    pub frames_rendered: u64,
    pub frames_discarded: u64,
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
        unsafe { Self::new_h264_named(sps, pps, width, height, window, fps, None) }
    }

    /// Create an H.264 decoder by a Rust-owned preferred platform codec name.
    /// A failed/invalid named lookup falls back to normal MIME-type selection.
    ///
    /// # Safety
    /// Same ANativeWindow lifetime requirements as [`Self::new_h264`].
    pub unsafe fn new_h264_named(
        sps: &[u8],
        pps: &[u8],
        width: u32,
        height: u32,
        window: usize,
        fps: u32,
        codec_name: Option<&str>,
    ) -> Result<Self, DecoderError> {
        unsafe {
            Self::new_video_named(VideoDecoderConfig {
                codec: VideoCodec::H264,
                vps: None,
                sps,
                pps,
                width,
                height,
                window,
                fps,
                codec_name,
                allow_mime_fallback: true,
                max_frame_size: None,
            })
        }
    }

    /// Create a decoder for either H.264 or HEVC. HEVC requires VPS, SPS,
    /// and PPS in that order for Android's `csd-0`, `csd-1`, and `csd-2`.
    /// A failed/invalid named lookup falls back to normal MIME-type selection.
    ///
    /// # Safety
    /// `window` must be a valid ANativeWindow* (from ANativeWindow_fromSurface)
    /// or 0. Caller keeps the window alive for the session lifetime.
    pub unsafe fn new_video_named(config: VideoDecoderConfig<'_>) -> Result<Self, DecoderError> {
        let VideoDecoderConfig {
            codec,
            vps,
            sps,
            pps,
            width,
            height,
            window,
            fps,
            codec_name,
            allow_mime_fallback,
            ..
        } = config;
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
        let (sw, sh) = if codec == VideoCodec::H264 {
            parse_sps_dimensions(sps_nal).unwrap_or((width, height))
        } else {
            (width, height)
        };
        let mime = match codec {
            VideoCodec::H264 => c"video/avc".as_ptr(),
            VideoCodec::Hevc => c"video/hevc".as_ptr(),
        };
        if codec == VideoCodec::Hevc && vps.is_none() {
            return Err(DecoderError::CreateFailed {
                mime: codec.mime().into(),
            });
        }
        let candidates = decoder_candidate_plan(codec_name, allow_mime_fallback);

        let mut last_error = None;
        for candidate in candidates {
            let codec_handle = if let DecoderCandidate::Named(name) = candidate {
                if let Ok(c_name) = std::ffi::CString::new(name) {
                    unsafe { AMediaCodec_createCodecByName(c_name.as_ptr()) }
                } else {
                    std::ptr::null_mut()
                }
            } else {
                unsafe { AMediaCodec_createDecoderByType(mime) }
            };
            if codec_handle.is_null() {
                continue;
            }
            let format = unsafe { AMediaFormat_new() };
            unsafe {
                // NDK samples set the mime key on the format even for decoders
                // created by type; some vendors reject without it.
                AMediaFormat_setString_pub(format, c"mime".as_ptr(), mime);
                match codec {
                    VideoCodec::H264 => {
                        AMediaFormat_setBuffer(
                            format,
                            c"csd-0".as_ptr(),
                            sps.as_ptr().cast(),
                            sps.len(),
                        );
                        AMediaFormat_setBuffer(
                            format,
                            c"csd-1".as_ptr(),
                            pps.as_ptr().cast(),
                            pps.len(),
                        );
                    }
                    VideoCodec::Hevc => {
                        let vps = vps.expect("HEVC VPS checked above");
                        AMediaFormat_setBuffer(
                            format,
                            c"csd-0".as_ptr(),
                            vps.as_ptr().cast(),
                            vps.len(),
                        );
                        AMediaFormat_setBuffer(
                            format,
                            c"csd-1".as_ptr(),
                            sps.as_ptr().cast(),
                            sps.len(),
                        );
                        AMediaFormat_setBuffer(
                            format,
                            c"csd-2".as_ptr(),
                            pps.as_ptr().cast(),
                            pps.len(),
                        );
                    }
                }
                AMediaFormat_setInt32(format, c"width".as_ptr(), sw as i32);
                AMediaFormat_setInt32(format, c"height".as_ptr(), sh as i32);
                // Declare the adaptive bound so surface output is allocated for
                // every picture size the stream may switch to. With adaptive
                // playback enabled by surface configure, a mid-stream size
                // change then reuses the same Surface without re-configuring.
                if let Some((max_w, max_h)) = decoder_max_frame_size(config.max_frame_size, sw, sh)
                {
                    AMediaFormat_setInt32(format, c"max-width".as_ptr(), max_w as i32);
                    AMediaFormat_setInt32(format, c"max-height".as_ptr(), max_h as i32);
                }
                // Request an input slot large enough for a worst-case IDR. Without
                // this hint, some vendor codecs size compressed input buffers for
                // average frames and reject the first high-motion/key frame.
                let max_input_size =
                    (u64::from(sw) * u64::from(sh) * 3 / 2).clamp(1 << 20, 16 << 20) as i32;
                AMediaFormat_setInt32(format, c"max-input-size".as_ptr(), max_input_size);
                // Tell platform decoders this is an interactive, real-time
                // stream. These are optional MediaFormat keys, so older/vendor
                // codecs can ignore them while modern codecs avoid extra queueing.
                let fps_val = fps.clamp(1, 90) as i32;
                AMediaFormat_setInt32(format, c"frame-rate".as_ptr(), fps_val);
                AMediaFormat_setInt32(format, c"operating-rate".as_ptr(), fps_val);
                AMediaFormat_setInt32(format, c"priority".as_ptr(), 0);
                AMediaFormat_setInt32(format, c"low-latency".as_ptr(), 1);
                let surface = if window == 0 {
                    std::ptr::null_mut()
                } else {
                    window as *mut std::ffi::c_void
                };
                let status =
                    AMediaCodec_configure(codec_handle, format, surface, std::ptr::null(), 0);
                if status != AMEDIA_OK {
                    AMediaFormat_delete(format);
                    AMediaCodec_delete(codec_handle);
                    last_error = Some(DecoderError::ConfigureFailed { status });
                    continue;
                }
                let status = AMediaCodec_start(codec_handle);
                if status != AMEDIA_OK {
                    AMediaFormat_delete(format);
                    AMediaCodec_delete(codec_handle);
                    last_error = Some(DecoderError::StartFailed { status });
                    continue;
                }
            }
            return Ok(Self {
                codec: codec_handle,
                format,
                started: true,
                width: sw as i32,
                height: sh as i32,
                frames_rendered: 0,
                frames_discarded: 0,
            });
        }
        Err(last_error.unwrap_or(DecoderError::CreateFailed {
            mime: codec.mime().into(),
        }))
    }

    /// Platform name of the instantiated codec, for runtime verification.
    pub fn codec_name(&self) -> String {
        let mut name_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        let status = unsafe { AMediaCodec_getName_pub(self.codec, &mut name_ptr) };
        if status == AMEDIA_OK && !name_ptr.is_null() {
            unsafe { std::ffi::CStr::from_ptr(name_ptr) }
                .to_string_lossy()
                .into_owned()
        } else {
            "<unknown>".to_owned()
        }
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
            FeedStatus::InputUnavailable | FeedStatus::InputTooLarge { .. } => Ok(false),
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
        // Release every output buffer that is already ready before asking for
        // another input slot. Some vendor codecs stop returning input buffers
        // while a renderable output remains queued, which otherwise turns one
        // transient miss into a permanent black-screen/drop cascade.
        let mut rendered = self.pump_latest_output(0)?;
        let mut queued = self.queue_access_unit(au, pts_us, timeout_us)?;
        if queued == FeedStatus::InputUnavailable && timeout_us == 0 {
            // A vendor codec can report no input immediately while its
            // output callback is being retired. Drain once more and retry
            // without waiting; this removes a transient drop without
            // allowing decoder backlog to become display latency.
            rendered |= self.pump_latest_output(0)?;
            queued = self.queue_access_unit(au, pts_us, 0)?;
        }
        match queued {
            FeedStatus::Queued { .. } => Ok(FeedStatus::Queued {
                rendered: self.pump_latest_output(timeout_us)? || rendered,
            }),
            other => Ok(other),
        }
    }

    /// Queue one compressed access unit without dequeuing or releasing any
    /// output. Split renderers use this to keep MediaCodec output ownership on
    /// the tile worker until the pair coordinator supplies one timestamp.
    pub fn queue_access_unit(
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
        if buf.is_null() {
            unsafe {
                AMediaCodec_queueInputBuffer(self.codec, idx, 0, 0, pts_us, 0);
            }
            return Ok(FeedStatus::InputUnavailable);
        }
        if au.len() > capacity {
            // Return the slot to MediaCodec, but report an oversized AU
            // separately. Retrying it cannot succeed and the caller must
            // rebuild/resync instead of treating this as transient pressure.
            unsafe {
                AMediaCodec_queueInputBuffer(self.codec, idx, 0, 0, pts_us, 0);
            }
            return Ok(FeedStatus::InputTooLarge {
                required: au.len(),
                capacity,
            });
        }
        unsafe {
            std::ptr::copy_nonoverlapping(au.as_ptr(), buf, au.len());
            let q = AMediaCodec_queueInputBuffer(self.codec, idx, 0, au.len(), pts_us, 0);
            if q != AMEDIA_OK {
                return Err(DecoderError::OpFailed { status: q });
            }
        }
        Ok(FeedStatus::Queued { rendered: false })
    }

    /// Drain currently ready decoder outputs while preserving only the newest
    /// image for an interactive desktop Surface. Older decoded images are no
    /// longer useful for pointer-following latency, but compressed reference
    /// inputs were still submitted in order so decoder correctness is kept.
    pub fn pump_latest_output(&mut self, timeout_us: i64) -> Result<bool, DecoderError> {
        let mut ready = [0usize; 64];
        let mut ready_count = 0usize;
        let mut next_timeout = timeout_us;
        let mut attempts = 0usize;
        while attempts < 64 {
            attempts += 1;
            let mut info = AMediaCodecBufferInfo {
                offset: 0,
                size: 0,
                presentation_time_us: 0,
                flags: 0,
            };
            let idx =
                unsafe { AMediaCodec_dequeueOutputBuffer(self.codec, &mut info, next_timeout) };
            next_timeout = 0;
            match idx {
                AMEDIACODEC_INFO_TRY_AGAIN_LATER => break,
                AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
                | AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => continue,
                i if i >= 0 => {
                    if ready_count < ready.len() {
                        ready[ready_count] = i as usize;
                        ready_count += 1;
                    }
                }
                err => return Err(DecoderError::OpFailed { status: err as i32 }),
            }
        }
        if ready_count == 0 {
            return Ok(false);
        }

        let discard_count = ready_count.saturating_sub(MAX_RENDERABLE_OUTPUTS);
        for &idx in &ready[..discard_count] {
            let r = unsafe { AMediaCodec_releaseOutputBuffer(self.codec, idx, false) };
            if r != AMEDIA_OK {
                return Err(DecoderError::OpFailed { status: r });
            }
            self.frames_discarded += 1;
        }
        for &idx in &ready[discard_count..ready_count] {
            let r = unsafe { AMediaCodec_releaseOutputBuffer(self.codec, idx, true) };
            if r != AMEDIA_OK {
                return Err(DecoderError::OpFailed { status: r });
            }
            self.frames_rendered += 1;
        }
        Ok(true)
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
