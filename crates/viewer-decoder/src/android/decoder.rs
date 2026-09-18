use super::ffi::*;
use super::output::{ReadyOutput, MAX_PARKED_OUTPUTS, MAX_RENDERABLE_OUTPUTS};
use crate::annex_b::*;
use std::collections::VecDeque;

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

// Qualcomm's decode-order extension is a candidate for streams without B-frames.
// Restrict it to our H.264 QTI path. Every rejected attempt is deleted before
// retrying low-latency-only, then standard configuration, then optional MIME.
fn decoder_configuration_plan(
    codec: VideoCodec,
    codec_name: Option<&str>,
    allow_mime_fallback: bool,
) -> Vec<(DecoderCandidate<'_>, u8)> {
    decoder_candidate_plan(codec_name, allow_mime_fallback)
        .into_iter()
        .flat_map(|candidate| {
            let qti = codec == VideoCodec::H264
                && matches!(candidate, DecoderCandidate::Named(name)
                    if name.starts_with("c2.qti.") || name.starts_with("OMX.qcom."));
            if qti {
                vec![(candidate, 2), (candidate, 1), (candidate, 0)]
            } else {
                vec![(candidate, 0)]
            }
        })
        .collect()
}

// Vendor key spelling follows Qualcomm's extensions, also used by upstream
// Moonlight MediaCodecHelper. Standard low-latency alone was accepted on the
// tablet, so effectiveness of these additional hints must be measured live.
fn low_latency_format_entries(fps: u32, qti_extensions: u8) -> Vec<(&'static str, i32)> {
    let fps_val = fps.clamp(1, 90) as i32;
    let mut entries = vec![
        ("frame-rate", fps_val),
        ("operating-rate", fps_val),
        ("priority", 0),
        ("low-latency", 1),
    ];
    if qti_extensions >= 1 {
        entries.push(("vendor.qti-ext-dec-low-latency.enable", 1));
    }
    if qti_extensions >= 2 {
        entries.push(("vendor.qti-ext-dec-picture-order.enable", 1));
    }
    entries
}

#[cfg(test)]
mod low_latency_tests {
    use super::*;

    #[test]
    fn low_latency_setup_requests_standard_and_qti_vendor_mode() {
        let entries = low_latency_format_entries(60, 2);
        assert!(entries.contains(&("low-latency", 1)));
        assert!(entries.contains(&("vendor.qti-ext-dec-low-latency.enable", 1)));
        assert!(entries.contains(&("vendor.qti-ext-dec-picture-order.enable", 1)));
        assert!(entries.contains(&("priority", 0)));
        assert!(entries.contains(&("operating-rate", 60)));
    }

    #[test]
    fn strict_qti_selection_retries_same_codec_without_vendor_keys() {
        let name = "c2.qti.avc.decoder.low_latency";
        let candidate = DecoderCandidate::Named(name);
        assert_eq!(
            decoder_configuration_plan(VideoCodec::H264, Some(name), false),
            vec![(candidate, 2), (candidate, 1), (candidate, 0)]
        );
        assert_eq!(
            decoder_configuration_plan(VideoCodec::H264, Some(name), true),
            vec![
                (candidate, 2),
                (candidate, 1),
                (candidate, 0),
                (DecoderCandidate::MimeType, 0)
            ]
        );
        assert!(low_latency_format_entries(60, 0)
            .iter()
            .all(|(key, _)| !key.starts_with("vendor.")));
    }

    #[test]
    fn unrelated_codecs_keep_standard_configuration() {
        for (codec, name) in [
            (VideoCodec::H264, Some("c2.android.avc.decoder")),
            (VideoCodec::Hevc, Some("c2.qti.hevc.decoder")),
            (VideoCodec::H264, None),
        ] {
            assert!(decoder_configuration_plan(codec, name, true)
                .iter()
                .all(|(_, vendor)| *vendor == 0));
        }
        assert!(low_latency_format_entries(240, 0).contains(&("operating-rate", 90)));
    }
}

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
    /// Actual last released output PTS; not the currently submitted input.
    pub last_released_pts_us: Option<i64>,
    output_target_ns: Option<i64>,
    /// Outputs held while balanced pacing waits for the next display vsync
    /// slot. Bounded by [`MAX_PARKED_OUTPUTS`]; overflow discards the oldest.
    parked_outputs: VecDeque<ReadyOutput>,
    /// Vsync target that already received a release. One release per slot
    /// keeps bursts from double-rendering inside a single vsync period.
    last_paced_target_ns: Option<i64>,
    /// Adaptive park depth driver: high-water mark of the parked queue,
    /// sampled before trimming. Direct demand signal — a stall-then-burst
    /// transit pattern parks N frames once, and the buffer deepens to N+1
    /// for the next burst. Pre-hold by construction, so it cannot chase its
    /// own latency (the release-age feedback failure mode).
    park_demand_window: std::collections::VecDeque<usize>,
    adaptive_park_cap: usize,
}

/// Micro-jitter smoothing switch, mirrored to the client so the Viewer UI can
/// toggle it at runtime (전문 설정). On by default: converting a one-frame
/// skip into one frame held costs at most a frame period of latency when a
/// pair arrives together, and the hold disengages the moment arrivals bunch
/// harder than one frame.
pub static PRESENTATION_SMOOTH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 프레임 스무딩(적응 지터 버퍼) park 깊이 하한/상한(프레임 수). 도착
/// 지터 p95에 맞춰 그 사이에서 자동 조절한다(상한 10프레임 ≈ 166ms@60fps
/// — 실측 버스트 117-133ms를 흡수하고도 여유).
pub const SMOOTH_PARK_MIN: usize = 2;
pub const SMOOTH_PARK_MAX: usize = 10;
/// park 수요 수위 관측 창(펌프 횟수).
const ARRIVAL_JITTER_WINDOW: usize = 128;

/// Release-age p95 the adaptive jitter buffer steers toward (45ms).
pub const PRESENTATION_AGE_TARGET_US: u64 = 45_000;
/// Adaptive jitter-buffer ceiling (4 frames at 60fps ≈ 66ms).
pub const PRESENTATION_DELAY_MAX_US: u64 = 66_000;

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
        let candidates = decoder_configuration_plan(codec, codec_name, allow_mime_fallback);

        let mut last_error = None;
        for (candidate, qti_extensions) in candidates {
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
                for (key, value) in low_latency_format_entries(fps, qti_extensions) {
                    let c_key = std::ffi::CString::new(key).expect("format key has no NUL");
                    AMediaFormat_setInt32(format, c_key.as_ptr(), value);
                }
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
                last_released_pts_us: None,
                output_target_ns: None,
                parked_outputs: VecDeque::new(),
                last_paced_target_ns: None,
                park_demand_window: std::collections::VecDeque::new(),
                adaptive_park_cap: SMOOTH_PARK_MIN,
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

    /// Drain currently ready decoder outputs. Two release policies share the
    /// decoder-to-Surface queue bound:
    ///
    /// * Freshness (no display target): render only the newest image now.
    ///   Older decoded images are no longer useful for pointer-following
    ///   latency, but compressed reference inputs were still submitted in
    ///   order so decoder correctness is kept.
    /// * Balanced pacing (display target): park ready outputs up to
    ///   [`MAX_PARKED_OUTPUTS`] and release at most one per vsync slot, so
    ///   bursty decodes spread one frame per display refresh instead of
    ///   double-rendering inside a single vsync period. The parked tail is
    ///   one frame of jitter buffer, matching Moonlight's balanced pacing.
    pub fn set_output_target(&mut self, target_ns: Option<i64>) {
        self.output_target_ns = target_ns;
    }

    pub fn pump_latest_output(&mut self, timeout_us: i64) -> Result<bool, DecoderError> {
        let mut ready = [(0usize, 0i64); 64];
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
                        ready[ready_count] = (i as usize, info.presentation_time_us);
                        ready_count += 1;
                    }
                }
                err => return Err(DecoderError::OpFailed { status: err as i32 }),
            }
        }
        for &(idx, pts_us) in &ready[..ready_count] {
            self.parked_outputs.push_back(ReadyOutput {
                index: idx,
                pts_us,
                arrived_at_ns: crate::android::output::monotonic_now_ns(),
            });
        }
        // 트림 전 수요 수위를 기록해 다음 버스트의 park 깊이를 정한다.
        self.park_demand_window.push_back(self.parked_outputs.len());
        while self.park_demand_window.len() > ARRIVAL_JITTER_WINDOW {
            self.park_demand_window.pop_front();
        }
        if let Some(&peak) = self.park_demand_window.iter().max() {
            self.adaptive_park_cap = (peak + 1).clamp(SMOOTH_PARK_MIN, SMOOTH_PARK_MAX);
        }
        if self.parked_outputs.is_empty() {
            return Ok(false);
        }

        match self.output_target_ns {
            None => {
                self.last_paced_target_ns = None;
                // Latency-first freshness: keep the newest, discard the rest.
                // Any transit burst shows up as a skipped frame.
                while self.parked_outputs.len() > MAX_RENDERABLE_OUTPUTS {
                    let output = self.parked_outputs.pop_front().expect("length checked");
                    self.discard_output(output)?;
                }
                let output = self.parked_outputs.pop_front().expect("budget keeps one");
                let r = unsafe { AMediaCodec_releaseOutputBuffer(self.codec, output.index, true) };
                if r != AMEDIA_OK {
                    return Err(DecoderError::OpFailed { status: r });
                }
                self.frames_rendered += 1;
                self.last_released_pts_us = Some(output.pts_us);
                Ok(true)
            }
            Some(target) => {
                // 프레임 스무딩(적응 지터 버퍼): 도착 지터 p95에 맞춰
                // 2~10프레임(33~166ms)까지 park 깊이를 자동 조절해 도착
                // 뭉침을 흡수한다. 초과분은 즉시 렌더로 소화(폐기 없음).
                let park_cap = if PRESENTATION_SMOOTH.load(std::sync::atomic::Ordering::Relaxed) {
                    self.adaptive_park_cap
                } else {
                    MAX_PARKED_OUTPUTS
                };
                while self.parked_outputs.len() > park_cap {
                    let output = self.parked_outputs.pop_front().expect("length checked");
                    if PRESENTATION_SMOOTH.load(std::sync::atomic::Ordering::Relaxed) {
                        // 스무딩 모드의 초과분은 폐기하지 않고 즉시 렌더로
                        // 소화한다 — 버스트가 park 상한(≈83ms)을 넘어도
                        // 프레임은 손실되지 않고, 뭉침만 앞당겨 표시된다.
                        let r = unsafe {
                            AMediaCodec_releaseOutputBuffer(self.codec, output.index, true)
                        };
                        if r != AMEDIA_OK {
                            return Err(DecoderError::OpFailed { status: r });
                        }
                        self.frames_rendered += 1;
                        self.last_released_pts_us = Some(output.pts_us);
                    } else {
                        self.discard_output(output)?;
                    }
                }
                if self.last_paced_target_ns == Some(target) {
                    // This vsync slot already received its frame; hold the
                    // parked tail for the next slot.
                    return Ok(false);
                }
                let output = match self.parked_outputs.pop_front() {
                    Some(output) => output,
                    None => return Ok(false),
                };
                let r =
                    unsafe { AMediaCodec_releaseOutputBufferAtTime(self.codec, output.index, target) };
                if r != AMEDIA_OK {
                    return Err(DecoderError::OpFailed { status: r });
                }
                self.frames_rendered += 1;
                self.last_released_pts_us = Some(output.pts_us);
                self.last_paced_target_ns = Some(target);
                Ok(true)
            }
        }
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
    /// caller feeds a fresh IDR after. Parked output indices are reclaimed by
    /// the codec and must never be released afterwards.
    pub fn flush(&mut self) -> Result<(), DecoderError> {
        self.parked_outputs.clear();
        self.last_paced_target_ns = None;
        let s = unsafe { AMediaCodec_flush(self.codec) };
        if s != AMEDIA_OK {
            return Err(DecoderError::OpFailed { status: s });
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.parked_outputs.clear();
        self.last_paced_target_ns = None;
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
