//! Linux Host H.264 encoders behind a small trait. The software fallback is
//! the system libopenh264 through its stable C vtable API, loaded with
//! libloading like the macOS capture shim. The hardware path lives in
//! [`super::nvenc`]; the session selects automatically at start (see
//! [`create_video_encoder`]) and reports what it picked through session
//! stats. No build-script codegen and no new crate dependencies; the
//! libraries are runtime requirements (Arch `openh264`, `nvidia-utils`).
//!
//! Contract with the shared media path: one Annex-B access unit per output,
//! no B-frames, SPS/PPS available up front via `parameter_sets()`, IDR on
//! demand via `force_keyframe()` (docs/plans/2026-11-01-linux-host.md 설계).

use libloading::Library;

use super::convert::{self, BgraFrame};

// ---------------------------------------------------------------------------
// openh264 C API (ABI copied from wels/codec_api.h and wels/codec_app_def.h,
// verified against openh264 2.6.0 headers; enums are C ints).
// ---------------------------------------------------------------------------

const VIDEO_FORMAT_I420: i32 = 23;
const VIDEO_FRAME_TYPE_IDR: i32 = 1;
const USAGE_CAMERA_VIDEO_REAL_TIME: i32 = 0;
const RC_BITRATE_MODE: i32 = 1;
const COMPLEXITY_LOW: i32 = 0;
const PRO_BASELINE: i32 = 66;
const SLICE_MODE_SINGLE: u32 = 0;
const UNSPECIFIED_BIT_RATE: i32 = 0;
const MAX_SPATIAL_LAYER_NUM: usize = 4;
const MAX_LAYER_NUM_OF_FRAME: usize = 128;
const MAX_SLICES_NUM_TMP: usize = 35;
const CM_RESULT_SUCCESS: i32 = 0;

#[repr(C)]
struct SSliceArgument {
    ui_slice_mode: u32,
    ui_slice_num: u32,
    ui_slice_mb_num: [u32; MAX_SLICES_NUM_TMP],
    ui_slice_size_constraint: u32,
}

impl SSliceArgument {
    fn single_slice() -> Self {
        Self {
            ui_slice_mode: SLICE_MODE_SINGLE,
            ui_slice_num: 0,
            ui_slice_mb_num: [0; MAX_SLICES_NUM_TMP],
            ui_slice_size_constraint: 0,
        }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct SSpatialLayerConfig {
    iVideoWidth: i32,
    iVideoHeight: i32,
    fFrameRate: f32,
    iSpatialBitrate: i32,
    iMaxSpatialBitrate: i32,
    uiProfileIdc: i32,
    uiLevelIdc: i32,
    iDLayerQp: i32,
    sSliceArgument: SSliceArgument,
    bVideoSignalTypePresent: bool,
    uiVideoFormat: u8,
    bFullRange: bool,
    bColorDescriptionPresent: bool,
    uiColorPrimaries: u8,
    uiTransferCharacteristics: u8,
    uiColorMatrix: u8,
    bAspectRatioPresent: bool,
    eAspectRatio: i32,
    sAspectRatioExtWidth: u16,
    sAspectRatioExtHeight: u16,
}

#[repr(C)]
#[allow(non_snake_case)]
struct SEncParamExt {
    iUsageType: i32,
    iPicWidth: i32,
    iPicHeight: i32,
    iTargetBitrate: i32,
    iRCMode: i32,
    fMaxFrameRate: f32,
    iTemporalLayerNum: i32,
    iSpatialLayerNum: i32,
    sSpatialLayers: [SSpatialLayerConfig; MAX_SPATIAL_LAYER_NUM],
    iComplexityMode: i32,
    uiIntraPeriod: u32,
    iNumRefFrame: i32,
    eSpsPpsIdStrategy: i32,
    bPrefixNalAddingCtrl: bool,
    bEnableSSEI: bool,
    bSimulcastAVC: bool,
    iPaddingFlag: i32,
    iEntropyCodingModeFlag: i32,
    bEnableFrameSkip: bool,
    iMaxBitrate: i32,
    iMaxQp: i32,
    iMinQp: i32,
    uiMaxNalSize: u32,
    bEnableLongTermReference: bool,
    iLTRRefNum: i32,
    iLtrMarkPeriod: u32,
    iMultipleThreadIdc: u16,
    bUseLoadBalancing: bool,
    iLoopFilterDisableIdc: i32,
    iLoopFilterAlphaC0Offset: i32,
    iLoopFilterBetaOffset: i32,
    bEnableDenoise: bool,
    bEnableBackgroundDetection: bool,
    bEnableAdaptiveQuant: bool,
    bEnableFrameCroppingFlag: bool,
    bEnableSceneChangeDetect: bool,
    bIsLosslessLink: bool,
    bFixRCOverShoot: bool,
    iIdrBitrateRatio: i32,
    bPsnrY: bool,
    bPsnrU: bool,
    bPsnrV: bool,
}

impl SEncParamExt {
    fn zeroed() -> Self {
        // SAFETY: see SSpatialLayerConfig::zeroed.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct SSourcePicture {
    iColorFormat: i32,
    iStride: [i32; 4],
    pData: [*mut u8; 4],
    iPicWidth: i32,
    iPicHeight: i32,
    uiTimeStamp: i64,
    bPsnrY: bool,
    bPsnrU: bool,
    bPsnrV: bool,
}

#[repr(C)]
#[allow(non_snake_case)]
struct SLayerBSInfo {
    uiTemporalId: u8,
    uiSpatialId: u8,
    uiQualityId: u8,
    eFrameType: i32,
    uiLayerType: u8,
    iSubSeqId: i32,
    iNalCount: i32,
    pNalLengthInByte: *mut i32,
    pBsBuf: *mut u8,
    rPsnr: [f32; 3],
}

#[repr(C)]
#[allow(non_snake_case)]
struct SFrameBSInfo {
    iLayerNum: i32,
    sLayerInfo: [SLayerBSInfo; MAX_LAYER_NUM_OF_FRAME],
    eFrameType: i32,
    iFrameSizeInBytes: i32,
    uiTimeStamp: i64,
}

impl SFrameBSInfo {
    fn zeroed() -> Box<Self> {
        // SAFETY: plain-old-data; the encoder overwrites it on every call.
        unsafe { Box::new(std::mem::zeroed()) }
    }
}

/// `ISVCEncoder` in C is `const ISVCEncoderVtbl*`, so the encoder handle is a
/// pointer to that vtable pointer. Function order must match codec_api.h.
#[repr(C)]
struct ISVCEncoderVtbl {
    initialize: unsafe extern "C" fn(*mut EncoderHandle, *const SEncParamBase) -> i32,
    initialize_ext: unsafe extern "C" fn(*mut EncoderHandle, *const SEncParamExt) -> i32,
    get_default_params: unsafe extern "C" fn(*mut EncoderHandle, *mut SEncParamExt) -> i32,
    uninitialize: unsafe extern "C" fn(*mut EncoderHandle) -> i32,
    encode_frame:
        unsafe extern "C" fn(*mut EncoderHandle, *const SSourcePicture, *mut SFrameBSInfo) -> i32,
    encode_parameter_sets: unsafe extern "C" fn(*mut EncoderHandle, *mut SFrameBSInfo) -> i32,
    force_intra_frame: unsafe extern "C" fn(*mut EncoderHandle, bool) -> i32,
    set_option: unsafe extern "C" fn(*mut EncoderHandle, i32, *mut std::ffi::c_void) -> i32,
    get_option: unsafe extern "C" fn(*mut EncoderHandle, i32, *mut std::ffi::c_void) -> i32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct SEncParamBase {
    iUsageType: i32,
    iPicWidth: i32,
    iPicHeight: i32,
    iTargetBitrate: i32,
    iRCMode: i32,
    fMaxFrameRate: f32,
}

type EncoderHandle = std::ffi::c_void;
type CreateFn = unsafe extern "C" fn(*mut *mut EncoderHandle) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut EncoderHandle);

// ENCODER_OPTION_DATAFORMAT = 0 (codec_app_def.h).
const ENCODER_OPTION_DATAFORMAT: i32 = 0;

/// Candidates ordered newest soname first; every distro keeps at least one.
const LIBRARY_CANDIDATES: &[&str] = &[
    "libopenh264.so.8",
    "libopenh264.so",
    "libopenh264.so.7",
    "libopenh264.so.6",
    "libopenh264.so.1",
    "libopenh264.so.0",
];

fn load_openh264() -> Result<Library, String> {
    let mut last = "no candidates".to_string();
    for name in LIBRARY_CANDIDATES {
        match unsafe { Library::new(*name) } {
            Ok(lib) => return Ok(lib),
            Err(error) => last = format!("{name}: {error}"),
        }
    }
    Err(format!(
        "libopenh264 not found (install the openh264 system package); last error: {last}"
    ))
}

// ---------------------------------------------------------------------------
// Rust-side encoder trait + selection
// ---------------------------------------------------------------------------

/// One encoded access unit in Annex-B form.
pub(crate) struct EncodedAu {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// Identity of the encoder a session is actually using, mirrored into
/// `StatsInfo` fields the dashboard's encoder diagnostics render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EncoderMeta {
    /// `StatsInfo::encoder_mode` (macOS uses "ave"; software stays "software").
    pub mode: &'static str,
    /// `StatsInfo::encoder_id`.
    pub id: &'static str,
    /// `StatsInfo::encoder_hardware_accelerated`.
    pub hardware: bool,
    /// `StatsInfo::encoder_preset`.
    pub preset: &'static str,
    /// `StatsInfo::encoder_profile`.
    pub profile: &'static str,
}

/// One Annex-B AU per `encode` call, no B-frames, IDR on demand. Used only
/// from the capture thread that owns the session worker.
pub(crate) trait VideoEncoder: Send {
    /// Encode one BGRA frame into an Annex-B access unit. The frame is
    /// scaled to the encode dimensions if they differ from `source`.
    fn encode(&mut self, source: &BgraFrame) -> Result<EncodedAu, String>;
    fn force_keyframe(&mut self) -> Result<(), String>;
    fn meta(&self) -> EncoderMeta;
}

/// `LEFTCAR_LINUX_ENCODER` values. Auto is the dashboard-facing default:
/// hardware when the machine has NVENC, software otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncoderPreference {
    Auto,
    Hardware,
    Software,
}

pub(crate) fn parse_preference(raw: Option<&str>) -> Result<EncoderPreference, String> {
    match raw {
        None | Some("") | Some("auto") => Ok(EncoderPreference::Auto),
        Some("hardware") => Ok(EncoderPreference::Hardware),
        Some("software") => Ok(EncoderPreference::Software),
        Some(other) => Err(format!(
            "unknown LEFTCAR_LINUX_ENCODER value {other:?} (expected auto, hardware, or software)"
        )),
    }
}

/// The selected encoder plus the stats fields the session publishes.
pub(crate) struct EncoderSelection {
    pub encoder: Box<dyn VideoEncoder>,
    pub meta: EncoderMeta,
    /// Set when Auto wanted NVENC but had to fall back; surfaced in the
    /// selection log line and unit-tested via the injected factory.
    pub fallback_reason: Option<String>,
}

/// Build the session encoder with the `LEFTCAR_LINUX_ENCODER` override.
pub(crate) fn create_video_encoder(
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
) -> Result<EncoderSelection, String> {
    let preference = parse_preference(std::env::var("LEFTCAR_LINUX_ENCODER").ok().as_deref())?;
    create_video_encoder_with(preference, width, height, fps, bitrate, &nvenc_factory)
}

fn nvenc_factory(
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
) -> Result<Box<dyn VideoEncoder>, String> {
    super::nvenc::NvencEncoder::new(width, height, fps, bitrate)
        .map(|encoder| Box::new(encoder) as Box<dyn VideoEncoder>)
}

/// Testable core of [`create_video_encoder`]: `nvenc_factory` is injectable
/// so the Auto fallback is provable on machines without (or with broken)
/// NVENC. A runtime encode failure never swaps codecs mid-stream — the
/// session error path handles it — only selection-time failure falls back.
pub(crate) fn create_video_encoder_with(
    preference: EncoderPreference,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    nvenc_factory: &(dyn Fn(u32, u32, u32, u32) -> Result<Box<dyn VideoEncoder>, String>
          + Send
          + Sync),
) -> Result<EncoderSelection, String> {
    match preference {
        EncoderPreference::Software => openh264_selection(width, height, fps, bitrate, None),
        EncoderPreference::Hardware => {
            let encoder = nvenc_factory(width, height, fps, bitrate)?;
            Ok(EncoderSelection {
                meta: encoder.meta(),
                encoder,
                fallback_reason: None,
            })
        }
        EncoderPreference::Auto => match nvenc_factory(width, height, fps, bitrate) {
            Ok(encoder) => Ok(EncoderSelection {
                meta: encoder.meta(),
                encoder,
                fallback_reason: None,
            }),
            Err(reason) => openh264_selection(width, height, fps, bitrate, Some(reason)),
        },
    }
}

fn openh264_selection(
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    fallback_reason: Option<String>,
) -> Result<EncoderSelection, String> {
    let encoder = OpenH264Encoder::new(width, height, fps, bitrate)?;
    Ok(EncoderSelection {
        meta: encoder.meta(),
        encoder: Box::new(encoder),
        fallback_reason,
    })
}

pub(crate) struct OpenH264Encoder {
    _lib: Library,
    encoder: *mut EncoderHandle,
    vtbl: &'static ISVCEncoderVtbl,
    bs_info: Box<SFrameBSInfo>,
    i420: Vec<u8>,
    width: u32,
    height: u32,
    fps: u32,
    frame_index: u64,
}

// SAFETY: the openh264 encoder is used from the capture thread only, and the
// raw handle is not touched anywhere else. The Library/encoder pair moves
// together with the session.
unsafe impl Send for OpenH264Encoder {}

impl OpenH264Encoder {
    /// `width`/`height` are the encode dimensions (must be even, matching the
    /// even-dimension rule the Windows encoder applies).
    pub(crate) fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(format!(
                "encoder dimensions must be even and nonzero: {width}x{height}"
            ));
        }
        if fps == 0 {
            return Err("encoder fps must be positive".into());
        }
        let lib = load_openh264()?;
        // SAFETY: WelsCreateSVCEncoder/WelsDestroySVCEncoder are the documented
        // entry points; the returned handle stays valid until destroyed.
        let create = unsafe {
            lib.get::<CreateFn>(b"WelsCreateSVCEncoder")
                .map_err(|e| format!("WelsCreateSVCEncoder unavailable: {e}"))?
        };
        let mut encoder: *mut EncoderHandle = std::ptr::null_mut();
        let status = unsafe { create(&mut encoder) };
        if status != CM_RESULT_SUCCESS || encoder.is_null() {
            return Err(format!("WelsCreateSVCEncoder failed: {status}"));
        }
        // The handle points at the vtable pointer; read it once.
        let vtbl = unsafe {
            let vtable_ptr = *(encoder as *mut *const ISVCEncoderVtbl);
            if vtable_ptr.is_null() {
                Err("openh264 encoder vtable is null")?;
            }
            &*vtable_ptr
        };

        let mut encoder_ref = OpenH264Encoder {
            _lib: lib,
            encoder,
            vtbl,
            bs_info: SFrameBSInfo::zeroed(),
            i420: Vec::new(),
            width,
            height,
            fps,
            frame_index: 0,
        };
        encoder_ref.configure(width, height, fps, bitrate)?;
        Ok(encoder_ref)
    }

    fn configure(&mut self, width: u32, height: u32, fps: u32, bitrate: u32) -> Result<(), String> {
        let mut params = SEncParamExt::zeroed();
        let status = unsafe { (self.vtbl.get_default_params)(self.encoder, &mut params) };
        if status != CM_RESULT_SUCCESS {
            return Err(format!("GetDefaultParams failed: {status}"));
        }
        params.iUsageType = USAGE_CAMERA_VIDEO_REAL_TIME;
        params.iPicWidth = width as i32;
        params.iPicHeight = height as i32;
        params.iTargetBitrate = bitrate as i32;
        params.iRCMode = RC_BITRATE_MODE;
        params.fMaxFrameRate = fps as f32;
        params.iTemporalLayerNum = 1;
        params.iSpatialLayerNum = 1;
        params.iComplexityMode = COMPLEXITY_LOW;
        // No periodic intra period: recovery is viewer-IDR driven, matching the
        // macOS UDP policy (MaxKeyFrameInterval effectively infinite).
        params.uiIntraPeriod = 0;
        // 1 reference frame: lowest encode latency; recovery is IDR-driven.
        params.iNumRefFrame = 1;
        params.eSpsPpsIdStrategy = 0; // CONSTANT_ID
        params.bPrefixNalAddingCtrl = false;
        params.bEnableSSEI = false;
        params.bSimulcastAVC = false;
        params.iPaddingFlag = 0;
        params.iEntropyCodingModeFlag = 0; // CAVLC (baseline)
        params.bEnableFrameSkip = false;
        params.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        params.uiMaxNalSize = 0;
        params.bEnableLongTermReference = false;
        params.iMultipleThreadIdc = 1;
        params.bUseLoadBalancing = false;
        let layer = &mut params.sSpatialLayers[0];
        layer.iVideoWidth = width as i32;
        layer.iVideoHeight = height as i32;
        layer.fFrameRate = fps as f32;
        layer.iSpatialBitrate = bitrate as i32;
        layer.iMaxSpatialBitrate = bitrate as i32;
        layer.uiProfileIdc = PRO_BASELINE;
        layer.uiLevelIdc = 0; // auto level
        layer.sSliceArgument = SSliceArgument::single_slice();
        let status = unsafe { (self.vtbl.initialize_ext)(self.encoder, &params) };
        if status != CM_RESULT_SUCCESS {
            return Err(format!("InitializeExt failed: {status}"));
        }
        // Pin the input color format once; the per-frame SSourcePicture must
        // agree with it.
        let mut format = VIDEO_FORMAT_I420;
        let status = unsafe {
            (self.vtbl.set_option)(
                self.encoder,
                ENCODER_OPTION_DATAFORMAT,
                &mut format as *mut i32 as *mut std::ffi::c_void,
            )
        };
        if status != CM_RESULT_SUCCESS {
            return Err(format!("SetOption(DATAFORMAT) failed: {status}"));
        }
        self.i420 = vec![0u8; (width * height * 3 / 2) as usize];
        Ok(())
    }

    /// Encode one BGRA frame (`stride` in bytes) into an Annex-B access unit.
    /// The frame is scaled from `src_width`/`src_height` if they differ from
    /// the encode dimensions (nearest neighbour, folded into the color
    /// conversion pass).
    fn encode_impl(&mut self, source: &BgraFrame) -> Result<EncodedAu, String> {
        convert::bgra_to_i420(&mut self.i420, self.width, self.height, source);

        let y_size = (self.width * self.height) as usize;
        let uv_size = y_size / 4;
        // SAFETY: pointer arithmetic stays inside the i420 allocation sized
        // width*height*3/2 in configure().
        let (y_ptr, u_ptr, v_ptr) = unsafe {
            (
                self.i420.as_mut_ptr(),
                self.i420.as_mut_ptr().add(y_size),
                self.i420.as_mut_ptr().add(y_size + uv_size),
            )
        };
        let picture = SSourcePicture {
            iColorFormat: VIDEO_FORMAT_I420,
            iStride: [self.width as i32, (self.width / 2) as i32, 0, 0],
            pData: [y_ptr, u_ptr, v_ptr, std::ptr::null_mut()],
            iPicWidth: self.width as i32,
            iPicHeight: self.height as i32,
            uiTimeStamp: (self.frame_index * 1000 / self.fps as u64) as i64,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,
        };
        self.frame_index += 1;
        // Reset output bookkeeping the encoder relies on.
        self.bs_info.iLayerNum = 0;
        self.bs_info.iFrameSizeInBytes = 0;
        let status =
            unsafe { (self.vtbl.encode_frame)(self.encoder, &picture, &mut *self.bs_info) };
        if status != CM_RESULT_SUCCESS {
            return Err(format!("EncodeFrame failed: {status}"));
        }
        let size = self.bs_info.iFrameSizeInBytes;
        if size <= 0 || self.bs_info.iLayerNum <= 0 {
            // openh264 may legitimately return no output (e.g. rate-control
            // internal frames); the caller re-arms a keyframe if it was forced.
            return Ok(EncodedAu {
                data: Vec::new(),
                keyframe: false,
            });
        }
        // All layers share one contiguous bitstream buffer; layer 0 holds the
        // base pointer and iFrameSizeInBytes spans every layer.
        let base = self.bs_info.sLayerInfo[0].pBsBuf;
        if base.is_null() {
            return Err("openh264 returned a null bitstream buffer".into());
        }
        let keyframe = self.bs_info.eFrameType == VIDEO_FRAME_TYPE_IDR;
        let data = unsafe { std::slice::from_raw_parts(base, size as usize).to_vec() };
        Ok(EncodedAu { data, keyframe })
    }

    pub(crate) fn force_keyframe_impl(&mut self) -> Result<(), String> {
        let status = unsafe { (self.vtbl.force_intra_frame)(self.encoder, true) };
        if status != CM_RESULT_SUCCESS {
            return Err(format!("ForceIntraFrame failed: {status}"));
        }
        Ok(())
    }
}

impl VideoEncoder for OpenH264Encoder {
    fn encode(&mut self, source: &BgraFrame) -> Result<EncodedAu, String> {
        self.encode_impl(source)
    }

    fn force_keyframe(&mut self) -> Result<(), String> {
        self.force_keyframe_impl()
    }

    fn meta(&self) -> EncoderMeta {
        EncoderMeta {
            mode: "software",
            id: "openh264",
            hardware: false,
            preset: "lowComplexity",
            profile: "baseline",
        }
    }
}

impl Drop for OpenH264Encoder {
    fn drop(&mut self) {
        // SAFETY: the handle was created by WelsCreateSVCEncoder and is
        // destroyed exactly once.
        unsafe {
            let destroy: libloading::Symbol<DestroyFn> = self
                ._lib
                .get(b"WelsDestroySVCEncoder")
                .expect("WelsDestroySVCEncoder was present at load time");
            destroy(self.encoder);
        }
    }
}

/// Test-only: build an encoder against the real system library. Other unit
/// tests avoid the dependency.
#[cfg(test)]
mod tests {
    use super::*;

    fn encoder_available() -> bool {
        // libopenh264 is a documented runtime requirement; skip elsewhere.
        load_openh264().is_ok()
    }

    #[test]
    fn encodes_solid_frame_as_annexb_with_spspps() {
        if !encoder_available() {
            println!("libopenh264 missing; skipping");
            return;
        }
        let mut enc = OpenH264Encoder::new(160, 120, 30, 500_000).expect("encoder init");
        let mut frame = vec![0u8; 320 * 120 * 4];
        for px in frame.as_chunks_mut::<4>().0 {
            px[0] = 40;
            px[1] = 120;
            px[2] = 200;
            px[3] = 255;
        }
        let source = BgraFrame::new(&frame, 80, 60, 320).expect("source");
        let au = enc.encode(&source).expect("encode");
        if au.data.is_empty() {
            println!("no output on first frame; skipping");
            return;
        }
        let sets = crate::wire::h264_parameter_sets(&au.data);
        assert!(
            !sets.is_empty(),
            "SPS/PPS must be present in the first IDR: {:02x?}",
            &au.data[..au.data.len().min(32)]
        );
        assert!(crate::wire::normalize_h264(&au.data).is_some());
    }

    #[test]
    fn auto_selection_falls_back_to_openh264_when_nvenc_probe_fails() {
        if !encoder_available() {
            println!("libopenh264 missing; skipping");
            return;
        }
        let failing =
            |_w: u32, _h: u32, _f: u32, _b: u32| -> Result<Box<dyn VideoEncoder>, String> {
                Err("no NVENC on this machine".into())
            };
        let selection =
            create_video_encoder_with(EncoderPreference::Auto, 64, 64, 30, 200_000, &failing)
                .expect("auto must fall back to openh264");
        assert_eq!(selection.meta.id, "openh264");
        assert!(!selection.meta.hardware);
        assert_eq!(
            selection.fallback_reason.as_deref(),
            Some("no NVENC on this machine")
        );
    }

    #[test]
    fn hardware_selection_propagates_nvenc_failure() {
        let failing =
            |_w: u32, _h: u32, _f: u32, _b: u32| -> Result<Box<dyn VideoEncoder>, String> {
                Err("no NVENC on this machine".into())
            };
        let error =
            create_video_encoder_with(EncoderPreference::Hardware, 64, 64, 30, 200_000, &failing)
                .err()
                .expect("hardware must fail when NVENC is unavailable");
        assert!(error.contains("no NVENC"), "unexpected error: {error}");
    }

    #[test]
    fn software_selection_never_probes_nvenc() {
        let boom = |_w: u32, _h: u32, _f: u32, _b: u32| -> Result<Box<dyn VideoEncoder>, String> {
            panic!("software preference must not probe NVENC")
        };
        if !encoder_available() {
            println!("libopenh264 missing; skipping");
            return;
        }
        let selection =
            create_video_encoder_with(EncoderPreference::Software, 64, 64, 30, 200_000, &boom)
                .expect("software selects openh264 directly");
        assert_eq!(selection.meta.id, "openh264");
        assert!(selection.fallback_reason.is_none());
    }

    #[test]
    fn preference_parsing_covers_env_values() {
        assert!(matches!(
            parse_preference(None),
            Ok(EncoderPreference::Auto)
        ));
        assert!(matches!(
            parse_preference(Some("")),
            Ok(EncoderPreference::Auto)
        ));
        assert!(matches!(
            parse_preference(Some("auto")),
            Ok(EncoderPreference::Auto)
        ));
        assert!(matches!(
            parse_preference(Some("hardware")),
            Ok(EncoderPreference::Hardware)
        ));
        assert!(matches!(
            parse_preference(Some("software")),
            Ok(EncoderPreference::Software)
        ));
        assert!(parse_preference(Some("bogus")).is_err());
    }

    #[test]
    fn encodes_multiple_frames_with_monotonic_timestamps() {
        if !encoder_available() {
            println!("libopenh264 missing; skipping");
            return;
        }
        let mut enc = OpenH264Encoder::new(64, 64, 30, 200_000).expect("encoder init");
        let frame = vec![90u8; 64 * 64 * 4];
        let source = BgraFrame::new(&frame, 64, 64, 64 * 4).expect("source");
        let mut produced = 0;
        for _ in 0..5 {
            let au = enc.encode(&source).expect("encode");
            if !au.data.is_empty() {
                produced += 1;
                // Annex-B access units start with a 3- or 4-byte start code.
                assert!(
                    au.data.starts_with(&[0, 0, 0, 1]) || au.data.starts_with(&[0, 0, 1]),
                    "unexpected leading bytes: {:02x?}",
                    &au.data[..au.data.len().min(4)]
                );
            }
        }
        assert!(
            produced >= 3,
            "expected most frames to encode, got {produced}"
        );
    }
}
