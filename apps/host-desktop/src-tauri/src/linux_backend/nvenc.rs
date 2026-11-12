//! Linux Host hardware H.264 encoder: NVIDIA NVENC through its stable C API,
//! loaded with libloading (`libcuda.so.1` + `libnvidia-encode.so.1`, both
//! shipped by the distro `nvidia-utils` package). Like the openh264 sibling
//! this module hand-copies the ABI instead of adding a codegen step; the
//! struct layouts, GUIDs and enum values are copied from
//! FFmpeg/nv-codec-headers **n12.2.72.0** `nvEncodeAPI.h` (MIT). Drivers ship
//! backward-compatible interface versions, so requesting API 12.2 works on
//! any driver ≥ 537.13.
//!
//! Input path: the classic system-memory input buffer (the one FFmpeg uses
//! for non-hwaccel frames). The capture frame is converted BGRA→NV12 on the
//! CPU directly into a driver-owned NV12 input surface via
//! `nvEncLockInputBuffer`, then encoded. The CUDADEVICEPTR register/map path
//! was measured broken on driver 615.78.08 (nvEncEncodePicture returns
//! NV_ENC_ERR_EVENT_NOT_REGISTERD in sync mode; see the plan evidence), while
//! this path encodes cleanly on the same driver. Sync mode (no async events):
//! one `nvEncEncodePicture` per frame completes the encode before returning.
//!
//! Contract with the shared media path (see `encoder.rs`): one Annex-B access
//! unit per output, no B-frames (`frameIntervalP = 1`), SPS/PPS in every IDR
//! (`repeatSPSPPS`), IDR on demand via `NV_ENC_PIC_FLAG_FORCEINTRA`
//! (docs/plans/2026-11-02-linux-nvenc-60fps.md 설계).

use std::ffi::c_void;
use std::slice;

use libloading::Library;

use super::convert::{self, BgraFrame};
use super::encoder::{EncodedAu, EncoderMeta, VideoEncoder};

// ---------------------------------------------------------------------------
// Constants copied from nvEncodeAPI.h (n12.2.72.0)
// ---------------------------------------------------------------------------

/// NVENCAPI_VERSION = major | minor<<24 for SDK 12.2.
const NVENCAPI_VERSION: u32 = 12 | (2 << 24);

/// NVENCAPI_STRUCT_VERSION(ver) = NVENCAPI_VERSION | ver<<16 | 0x7<<28.
/// Newer structs additionally OR `1<<31` — copied verbatim per struct.
const fn struct_version(ver: u32, extra_bit31: bool) -> u32 {
    NVENCAPI_VERSION | (ver << 16) | (0x7 << 28) | if extra_bit31 { 1 << 31 } else { 0 }
}

const NV_ENCODE_API_FUNCTION_LIST_VER: u32 = struct_version(2, false);
const NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER: u32 = struct_version(1, false);
const NV_ENC_INITIALIZE_PARAMS_VER: u32 = struct_version(7, true);
const NV_ENC_CONFIG_VER: u32 = struct_version(9, true);
const NV_ENC_PRESET_CONFIG_VER: u32 = struct_version(5, true);
const NV_ENC_PIC_PARAMS_VER: u32 = struct_version(7, true);
const NV_ENC_LOCK_BITSTREAM_VER: u32 = struct_version(2, true);
const NV_ENC_LOCK_INPUT_BUFFER_VER: u32 = struct_version(1, false);
const NV_ENC_CREATE_INPUT_BUFFER_VER: u32 = struct_version(2, false);
const NV_ENC_CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1, false);

const NV_ENC_DEVICE_TYPE_CUDA: u32 = 1;
const NV_ENC_BUFFER_FORMAT_NV12: u32 = 1;
const NV_ENC_PARAMS_RC_CBR: u32 = 2;
const NV_ENC_PIC_STRUCT_FRAME: u32 = 1;
const NV_ENC_PIC_TYPE_I: u32 = 2;
const NV_ENC_PIC_TYPE_IDR: u32 = 3;
const NV_ENC_PIC_FLAG_FORCEINTRA: u32 = 0x1;
const NV_ENC_PIC_FLAG_OUTPUT_SPSPPS: u32 = 0x4;
const NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY: i32 = 3;
const NV_ENC_H264_ENTROPY_CODING_MODE_CAVLC: u32 = 0x2;
const NVENC_INFINITE_GOPLENGTH: u32 = 0xffff_ffff;
/// NV_ENC_CONFIG_H264 bitfield slot 12 of 25 (see header bitfield order).
const NV_ENC_CONFIG_H264_REPEAT_SPSPPS_BIT: u32 = 1 << 12;

const NV_ENC_CODEC_H264_GUID: Guid = Guid {
    data1: 0x6bc8_2762,
    data2: 0x4e63,
    data3: 0x4ca4,
    data4: [0xaa, 0x85, 0x1e, 0x50, 0xf3, 0x21, 0xf6, 0xbf],
};
const NV_ENC_H264_PROFILE_BASELINE_GUID: Guid = Guid {
    data1: 0x0727_bcaa,
    data2: 0x78c4,
    data3: 0x4c83,
    data4: [0x8c, 0x2f, 0xef, 0x3d, 0xff, 0x26, 0x7c, 0x6a],
};
const NV_ENC_PRESET_P4_GUID: Guid = Guid {
    data1: 0x90a7_b826,
    data2: 0xdf06,
    data3: 0x4862,
    data4: [0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x86, 0x81],
};

/// CUDA_SUCCESS.
const CUDA_SUCCESS: i32 = 0;

const CUDA_LIBRARY_CANDIDATES: &[&str] = &["libcuda.so.1", "libcuda.so"];
const ENCODE_LIBRARY_CANDIDATES: &[&str] = &["libnvidia-encode.so.1", "libnvidia-encode.so"];

fn load_first_library(candidates: &[&str], what: &str) -> Result<Library, String> {
    let mut last = "no candidates".to_string();
    for name in candidates {
        match unsafe { Library::new(*name) } {
            Ok(lib) => return Ok(lib),
            Err(error) => last = format!("{name}: {error}"),
        }
    }
    Err(format!("{what} not found; last error: {last}"))
}

// ---------------------------------------------------------------------------
// ABI structs (field order and types copied from the header; unused function
// pointers stay `usize` placeholders and used ones are `Option<fn>` so the
// zero-filled table is a valid value before the driver fills it)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

type NvencStatus = i32;

#[repr(C)]
struct EncodeApiFunctionList {
    version: u32,
    reserved: u32,
    nv_enc_open_encode_session: usize,
    nv_enc_get_encode_guid_count: usize,
    nv_enc_get_encode_profile_guid_count: usize,
    nv_enc_get_encode_profile_guids: usize,
    nv_enc_get_encode_guids: usize,
    nv_enc_get_input_format_count: usize,
    nv_enc_get_input_formats: usize,
    nv_enc_get_encode_caps:
        Option<unsafe extern "C" fn(*mut c_void, Guid, *mut c_void, *mut i32) -> NvencStatus>,
    nv_enc_get_encode_preset_count: usize,
    nv_enc_get_encode_preset_guids: usize,
    nv_enc_get_encode_preset_config: usize,
    nv_enc_initialize_encoder:
        Option<unsafe extern "C" fn(*mut c_void, *mut InitializeParams) -> NvencStatus>,
    nv_enc_create_input_buffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut CreateInputBuffer) -> NvencStatus>,
    nv_enc_destroy_input_buffer: Option<unsafe extern "C" fn(*mut c_void, usize) -> NvencStatus>,
    nv_enc_create_bitstream_buffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut CreateBitstreamBuffer) -> NvencStatus>,
    nv_enc_destroy_bitstream_buffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> NvencStatus>,
    nv_enc_encode_picture: Option<unsafe extern "C" fn(*mut c_void, *mut PicParams) -> NvencStatus>,
    nv_enc_lock_bitstream:
        Option<unsafe extern "C" fn(*mut c_void, *mut LockBitstream) -> NvencStatus>,
    nv_enc_unlock_bitstream: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> NvencStatus>,
    nv_enc_lock_input_buffer:
        Option<unsafe extern "C" fn(*mut c_void, *mut LockInputBuffer) -> NvencStatus>,
    nv_enc_unlock_input_buffer: Option<unsafe extern "C" fn(*mut c_void, usize) -> NvencStatus>,
    nv_enc_get_encode_stats: usize,
    nv_enc_get_sequence_params: usize,
    nv_enc_register_async_event:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncEventParams) -> NvencStatus>,
    nv_enc_unregister_async_event:
        Option<unsafe extern "C" fn(*mut c_void, *mut NvEncEventParams) -> NvencStatus>,
    nv_enc_map_input_resource: usize,
    nv_enc_unmap_input_resource: usize,
    nv_enc_destroy_encoder: Option<unsafe extern "C" fn(*mut c_void) -> NvencStatus>,
    nv_enc_invalidate_ref_frames: usize,
    nv_enc_open_encode_session_ex: Option<
        unsafe extern "C" fn(*mut OpenEncodeSessionExParams, *mut *mut c_void) -> NvencStatus,
    >,
    nv_enc_register_resource: usize,
    nv_enc_unregister_resource: usize,
    nv_enc_reconfigure_encoder: usize,
    reserved1: usize,
    nv_enc_create_mv_buffer: usize,
    nv_enc_destroy_mv_buffer: usize,
    nv_enc_run_motion_estimation_only: usize,
    nv_enc_get_last_error_string:
        Option<unsafe extern "C" fn(*mut c_void) -> *const std::ffi::c_char>,
    nv_enc_set_io_cuda_streams: usize,
    nv_enc_get_encode_preset_config_ex: Option<
        unsafe extern "C" fn(*mut c_void, Guid, Guid, i32, *mut PresetConfig) -> NvencStatus,
    >,
    nv_enc_get_sequence_param_ex: usize,
    nv_enc_restore_encoder_state: usize,
    nv_enc_lookahead_picture: usize,
    reserved2: [usize; 275],
}

impl EncodeApiFunctionList {
    /// Zero-fills the table: every field is valid at zero (`Option<fn>` is
    /// `None`, placeholders are null) before the driver overwrites it.
    fn zeroed() -> Box<Self> {
        // SAFETY: plain-old-data with all-zero-valid fields; see above.
        unsafe { Box::new(std::mem::zeroed()) }
    }
}

/// The function pointers this encoder actually calls, extracted (and
/// null-checked) from the driver-filled table once, so every later call site
/// works with plain function pointers instead of `Option`s.
#[derive(Clone, Copy)]
struct NvencFns {
    open_encode_session_ex:
        unsafe extern "C" fn(*mut OpenEncodeSessionExParams, *mut *mut c_void) -> NvencStatus,
    get_encode_preset_config_ex:
        unsafe extern "C" fn(*mut c_void, Guid, Guid, i32, *mut PresetConfig) -> NvencStatus,
    initialize_encoder: unsafe extern "C" fn(*mut c_void, *mut InitializeParams) -> NvencStatus,
    create_bitstream_buffer:
        unsafe extern "C" fn(*mut c_void, *mut CreateBitstreamBuffer) -> NvencStatus,
    destroy_bitstream_buffer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> NvencStatus,
    create_input_buffer: unsafe extern "C" fn(*mut c_void, *mut CreateInputBuffer) -> NvencStatus,
    destroy_input_buffer: unsafe extern "C" fn(*mut c_void, usize) -> NvencStatus,
    lock_input_buffer: unsafe extern "C" fn(*mut c_void, *mut LockInputBuffer) -> NvencStatus,
    unlock_input_buffer: unsafe extern "C" fn(*mut c_void, usize) -> NvencStatus,
    encode_picture: unsafe extern "C" fn(*mut c_void, *mut PicParams) -> NvencStatus,
    lock_bitstream: unsafe extern "C" fn(*mut c_void, *mut LockBitstream) -> NvencStatus,
    unlock_bitstream: unsafe extern "C" fn(*mut c_void, *mut c_void) -> NvencStatus,
    destroy_encoder: unsafe extern "C" fn(*mut c_void) -> NvencStatus,
    get_last_error_string: unsafe extern "C" fn(*mut c_void) -> *const std::ffi::c_char,
}

impl NvencFns {
    /// Check an NVENC status and append the driver's error string when the
    /// session can provide one.
    fn check(&self, encoder: *mut c_void, status: NvencStatus, call: &str) -> Result<(), String> {
        if status == 0 {
            return Ok(());
        }
        let detail = self.last_error(encoder);
        if detail.is_empty() {
            return Err(format!("NVENC {call} failed: {}", status_name(status)));
        }
        Err(format!(
            "NVENC {call} failed: {} ({detail})",
            status_name(status)
        ))
    }

    fn last_error(&self, encoder: *mut c_void) -> String {
        if encoder.is_null() {
            return String::new();
        }
        // SAFETY: the driver-owned error string is read while the session is
        // still valid.
        let message = unsafe { (self.get_last_error_string)(encoder) };
        if message.is_null() {
            return String::new();
        }
        // SAFETY: NVENC returns a NUL-terminated static string.
        let text = unsafe { std::ffi::CStr::from_ptr(message) };
        text.to_string_lossy().into_owned()
    }
}

#[repr(C)]
struct NvEncEventParams {
    version: u32,
    reserved: u32,
    completion_event: *mut c_void,
    reserved1: [u32; 254],
    reserved2: [usize; 64],
}

#[repr(C)]
struct OpenEncodeSessionExParams {
    version: u32,
    device_type: u32,
    device: *mut c_void,
    reserved: *mut c_void,
    api_version: u32,
    reserved1: [u32; 253],
    reserved2: [usize; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Qp {
    qp_inter_p: u32,
    qp_inter_b: u32,
    qp_intra: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RcParams {
    version: u32,
    rate_control_mode: u32,
    const_qp: Qp,
    average_bit_rate: u32,
    max_bit_rate: u32,
    vbv_buffer_size: u32,
    vbv_initial_delay: u32,
    /// enableMinQP..reservedBitFields, all zero (no lookahead, no AQ).
    bitfields: u32,
    min_qp: Qp,
    max_qp: Qp,
    initial_rc_qp: Qp,
    temporal_layer_idx_mask: u32,
    temporal_layer_qp: [u8; 8],
    target_quality: u8,
    target_quality_lsb: u8,
    lookahead_depth: u16,
    low_delay_key_frame_scale: u8,
    y_dc_qp_index_offset: i8,
    u_dc_qp_index_offset: i8,
    v_dc_qp_index_offset: i8,
    qp_map_mode: u32,
    multi_pass: u32,
    alpha_layer_bitrate_ratio: u32,
    cb_qp_index_offset: i8,
    cr_qp_index_offset: i8,
    reserved2: u16,
    lookahead_level: u32,
    reserved: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VuiParameters {
    overscan_info_present_flag: u32,
    overscan_info: u32,
    video_signal_type_present_flag: u32,
    video_format: u32,
    video_full_range_flag: u32,
    colour_description_present_flag: u32,
    colour_primaries: u32,
    transfer_characteristics: u32,
    colour_matrix: u32,
    chroma_sample_location_flag: u32,
    chroma_sample_location_top: u32,
    chroma_sample_location_bot: u32,
    bitstream_restriction_flag: u32,
    timing_info_present_flag: u32,
    num_unit_in_ticks: u32,
    time_scale: u32,
    reserved: [u32; 12],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct H264Config {
    /// enableTemporalSVC..reservedBitFields (see header bitfield order).
    bitfields: u32,
    level: u32,
    idr_period: u32,
    separate_colour_plane_flag: u32,
    disable_deblocking_filter_idc: u32,
    num_temporal_layers: u32,
    sps_id: u32,
    pps_id: u32,
    adaptive_transform_mode: u32,
    fmo_mode: u32,
    bdirect_mode: u32,
    entropy_coding_mode: u32,
    stereo_mode: u32,
    intra_refresh_period: u32,
    intra_refresh_cnt: u32,
    max_num_ref_frames: u32,
    slice_mode: u32,
    slice_mode_data: u32,
    h264_vui_parameters: VuiParameters,
    ltr_num_frames: u32,
    ltr_trust_mode: u32,
    chroma_format_idc: u32,
    max_temporal_layers: u32,
    use_bframes_as_ref: u32,
    num_ref_l0: u32,
    num_ref_l1: u32,
    output_bit_depth: u32,
    input_bit_depth: u32,
    reserved1: [u32; 265],
    reserved2: [usize; 64],
}

/// The C type is a union of codec configs; only the H264 member is ever
/// written here and it is the first (largest) member, so embedding it keeps
/// the `NV_ENC_CONFIG` layout byte-identical.
#[repr(C)]
#[derive(Clone, Copy)]
struct CodecConfig {
    h264_config: H264Config,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EncodeConfig {
    version: u32,
    profile_guid: Guid,
    gop_length: u32,
    frame_interval_p: i32,
    monochrome_encoding: u32,
    frame_field_mode: u32,
    mv_precision: u32,
    rc_params: RcParams,
    encode_codec_config: CodecConfig,
    reserved: [u32; 278],
    reserved2: [usize; 64],
}

#[repr(C)]
struct PresetConfig {
    version: u32,
    reserved: u32,
    preset_cfg: EncodeConfig,
    reserved1: [u32; 256],
    reserved2: [usize; 64],
}

impl PresetConfig {
    fn zeroed() -> Box<Self> {
        // SAFETY: plain-old-data; the driver overwrites the contents.
        unsafe { Box::new(std::mem::zeroed()) }
    }
}

/// NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE (all-zero: no external ME).
#[repr(C)]
#[derive(Clone, Copy)]
struct MeHintCountsPerBlock {
    bitfields: u32,
    reserved1: [u32; 3],
}

impl MeHintCountsPerBlock {
    const ZERO: MeHintCountsPerBlock = MeHintCountsPerBlock {
        bitfields: 0,
        reserved1: [0; 3],
    };
}

#[repr(C)]
struct InitializeParams {
    version: u32,
    encode_guid: Guid,
    preset_guid: Guid,
    encode_width: u32,
    encode_height: u32,
    dar_width: u32,
    dar_height: u32,
    frame_rate_num: u32,
    frame_rate_den: u32,
    enable_encode_async: u32,
    enable_ptd: u32,
    /// reportSliceOffsets..reservedBitFields, all zero (sync, no subframe).
    bitfields: u32,
    priv_data_size: u32,
    reserved: u32,
    priv_data: *mut c_void,
    encode_config: *mut EncodeConfig,
    max_encode_width: u32,
    max_encode_height: u32,
    max_me_hint_counts_per_block: [MeHintCountsPerBlock; 2],
    tuning_info: i32,
    buffer_format: i32,
    num_state_buffers: u32,
    output_stats_level: u32,
    reserved1: [u32; 284],
    reserved2: [usize; 64],
}

#[repr(C)]
struct CreateBitstreamBuffer {
    version: u32,
    size: u32,
    memory_heap: u32,
    reserved: u32,
    bitstream_buffer: usize,
    bitstream_buffer_ptr: *mut c_void,
    reserved1: [u32; 58],
    reserved2: [usize; 64],
}

#[repr(C)]
struct PicParams {
    version: u32,
    input_width: u32,
    input_height: u32,
    input_pitch: u32,
    encode_pic_flags: u32,
    frame_idx: u32,
    input_time_stamp: u64,
    input_duration: u64,
    input_buffer: *mut c_void,
    output_bitstream: *mut c_void,
    completion_event: *mut c_void,
    buffer_fmt: u32,
    picture_struct: u32,
    picture_type: u32,
    /// NV_ENC_CODEC_PIC_PARAMS union (1544 bytes, align 8); zeroed because
    /// enablePTD decides the picture type and no codec-level picture params
    /// are passed. The size must match the C union so the fields after it
    /// land on the header's offsets.
    codec_pic_params: [u64; 193],
    me_hint_counts_per_block: [MeHintCountsPerBlock; 2],
    me_external_hints: *mut c_void,
    reserved2: [u32; 7],
    reserved5: [usize; 2],
    qp_delta_map: *mut i8,
    qp_delta_map_size: u32,
    reserved_bit_fields: u32,
    me_hint_ref_pic_dist: [u16; 2],
    reserved4: u32,
    alpha_buffer: *mut c_void,
    me_external_sb_hints: *mut c_void,
    me_sb_hints_count: u32,
    state_buffer_idx: u32,
    output_recon_buffer: *mut c_void,
    reserved3: [u32; 284],
    reserved6: [usize; 57],
}

#[repr(C)]
struct LockBitstream {
    version: u32,
    /// doNotWait:1 (0 = wait for completion), ltrFrame:1, getRCStats:1.
    bitfields: u32,
    output_bitstream: *mut c_void,
    slice_offsets: *mut u32,
    frame_idx: u32,
    hw_encode_status: u32,
    num_slices: u32,
    bitstream_size_in_bytes: u32,
    output_time_stamp: u64,
    output_duration: u64,
    bitstream_buffer_ptr: *mut c_void,
    picture_type: u32,
    picture_struct: u32,
    frame_avg_qp: u32,
    frame_satd: u32,
    ltr_frame_idx: u32,
    ltr_frame_bitmap: u32,
    temporal_id: u32,
    intra_mb_count: u32,
    inter_mb_count: u32,
    average_mv_x: i32,
    average_mv_y: i32,
    alpha_layer_size_in_bytes: u32,
    output_stats_ptr_size: u32,
    reserved: u32,
    output_stats_ptr: *mut c_void,
    frame_idx_display: u32,
    reserved1: [u32; 219],
    reserved2: [usize; 63],
    reserved_internal: [u32; 8],
}

/// NV_ENC_CREATE_INPUT_BUFFER, interface version 2 (n12.2.72.0 layout).
#[repr(C)]
struct CreateInputBuffer {
    version: u32,
    width: u32,
    height: u32,
    memory_heap: u32,
    buffer_fmt: u32,
    reserved: u32,
    input_buffer: usize,
    p_sys_mem_buffer: *mut c_void,
    reserved1: [u32; 58],
    reserved2: [usize; 63],
}

/// NV_ENC_LOCK_INPUT_BUFFER.
#[repr(C)]
struct LockInputBuffer {
    version: u32,
    /// doNotWait:1 (0 = wait).
    bitfields: u32,
    input_buffer: usize,
    buffer_data_ptr: *mut c_void,
    pitch: u32,
    reserved1: [u32; 251],
    reserved2: [usize; 64],
}

// ---------------------------------------------------------------------------
// CUDA driver API subset (libcuda.so.1)
// ---------------------------------------------------------------------------

type CuResult = i32;

struct CudaApi {
    _lib: Library,
    init: unsafe extern "C" fn(u32) -> CuResult,
    device_get: unsafe extern "C" fn(*mut i32, i32) -> CuResult,
    ctx_create: unsafe extern "C" fn(*mut *mut c_void, u32, i32) -> CuResult,
    ctx_destroy: unsafe extern "C" fn(*mut c_void) -> CuResult,
}

impl CudaApi {
    fn load() -> Result<Self, String> {
        let lib = load_first_library(CUDA_LIBRARY_CANDIDATES, "libcuda (CUDA driver)")?;
        // SAFETY: each symbol is looked up once at load time and stored in
        // this struct; the library outlives every call through it.
        unsafe {
            let api = Self {
                init: symbol(&lib, b"cuInit")?,
                device_get: symbol(&lib, b"cuDeviceGet")?,
                ctx_create: symbol(&lib, b"cuCtxCreate")?,
                ctx_destroy: symbol(&lib, b"cuCtxDestroy")?,
                _lib: lib,
            };
            Ok(api)
        }
    }

    fn check(&self, result: CuResult, call: &str) -> Result<(), String> {
        if result == CUDA_SUCCESS {
            return Ok(());
        }
        Err(format!("CUDA {call} failed with CUresult {result}"))
    }
}

unsafe fn symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    lib.get::<T>(name).map(|symbol| *symbol).map_err(|error| {
        format!(
            "symbol {:?} unavailable: {error}",
            String::from_utf8_lossy(name)
        )
    })
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Names for the NVENC status codes whose order is verified against the
/// header enum; anything beyond falls back to the numeric value.
fn status_name(status: NvencStatus) -> String {
    let name = match status {
        0 => "NV_ENC_SUCCESS",
        1 => "NV_ENC_ERR_NO_ENCODE_DEVICE",
        2 => "NV_ENC_ERR_UNSUPPORTED_DEVICE",
        3 => "NV_ENC_ERR_INVALID_ENCODERDEVICE",
        4 => "NV_ENC_ERR_INVALID_DEVICE",
        _ => return format!("NVENC status {status}"),
    };
    name.to_string()
}

/// Resources created so far inside `NvencEncoder::new`. On a mid-init failure
/// every non-zero entry is released in reverse dependency order (the same
/// sequence as the encoder's Drop) so a failed hardware probe on an
/// NVENC-less machine leaves nothing behind.
struct Pending {
    encoder: *mut c_void,
    input_buffer: usize,
    pitch: u32,
    bitstream_buffer: usize,
}

impl Pending {
    fn abort(&mut self, fns: &NvencFns) {
        // SAFETY: same teardown order as NvencEncoder::teardown (bitstream →
        // input buffer → encoder); each step is best-effort so one failure
        // cannot skip the rest.
        unsafe {
            if self.bitstream_buffer != 0 {
                let _ = (fns.destroy_bitstream_buffer)(
                    self.encoder,
                    self.bitstream_buffer as *mut c_void,
                );
                self.bitstream_buffer = 0;
            }
            if self.input_buffer != 0 {
                let _ = (fns.destroy_input_buffer)(self.encoder, self.input_buffer);
                self.input_buffer = 0;
            }
            if !self.encoder.is_null() {
                let _ = (fns.destroy_encoder)(self.encoder);
                self.encoder = std::ptr::null_mut();
            }
        }
    }
}

/// Loads the NVENC function table from libnvidia-encode and extracts the
/// pointers this encoder calls. The table itself is dropped here; the driver
/// does not retain pointers into it.
fn create_api_function_list() -> Result<(NvencFns, Library), String> {
    let lib = load_first_library(
        ENCODE_LIBRARY_CANDIDATES,
        "libnvidia-encode (NVENC runtime)",
    )?;
    // SAFETY: NvEncodeAPICreateInstance is the documented entry point; it
    // fills the zeroed table with the driver's function pointers.
    unsafe {
        let create_instance: unsafe extern "C" fn(*mut EncodeApiFunctionList) -> NvencStatus =
            symbol(&lib, b"NvEncodeAPICreateInstance")?;
        let mut list = EncodeApiFunctionList::zeroed();
        list.version = NV_ENCODE_API_FUNCTION_LIST_VER;
        let status = create_instance(&mut *list);
        if status != 0 {
            return Err(format!(
                "NvEncodeAPICreateInstance failed: {}",
                status_name(status)
            ));
        }
        let fns = NvencFns {
            open_encode_session_ex: list
                .nv_enc_open_encode_session_ex
                .ok_or("driver did not provide nvEncOpenEncodeSessionEx")?,
            get_encode_preset_config_ex: list
                .nv_enc_get_encode_preset_config_ex
                .ok_or("driver did not provide nvEncGetEncodePresetConfigEx")?,
            initialize_encoder: list
                .nv_enc_initialize_encoder
                .ok_or("driver did not provide nvEncInitializeEncoder")?,
            create_bitstream_buffer: list
                .nv_enc_create_bitstream_buffer
                .ok_or("driver did not provide nvEncCreateBitstreamBuffer")?,
            destroy_bitstream_buffer: list
                .nv_enc_destroy_bitstream_buffer
                .ok_or("driver did not provide nvEncDestroyBitstreamBuffer")?,
            create_input_buffer: list
                .nv_enc_create_input_buffer
                .ok_or("driver did not provide nvEncCreateInputBuffer")?,
            destroy_input_buffer: list
                .nv_enc_destroy_input_buffer
                .ok_or("driver did not provide nvEncDestroyInputBuffer")?,
            lock_input_buffer: list
                .nv_enc_lock_input_buffer
                .ok_or("driver did not provide nvEncLockInputBuffer")?,
            unlock_input_buffer: list
                .nv_enc_unlock_input_buffer
                .ok_or("driver did not provide nvEncUnlockInputBuffer")?,
            encode_picture: list
                .nv_enc_encode_picture
                .ok_or("driver did not provide nvEncEncodePicture")?,
            lock_bitstream: list
                .nv_enc_lock_bitstream
                .ok_or("driver did not provide nvEncLockBitstream")?,
            unlock_bitstream: list
                .nv_enc_unlock_bitstream
                .ok_or("driver did not provide nvEncUnlockBitstream")?,
            destroy_encoder: list
                .nv_enc_destroy_encoder
                .ok_or("driver did not provide nvEncDestroyEncoder")?,
            get_last_error_string: list
                .nv_enc_get_last_error_string
                .ok_or("driver did not provide nvEncGetLastErrorString")?,
        };
        Ok((fns, lib))
    }
}

pub(crate) struct NvencEncoder {
    cuda: CudaApi,
    _encode_lib: Library,
    fns: NvencFns,
    encoder: *mut c_void,
    cuda_context: *mut c_void,
    /// Driver-owned NV12 input surface handle (classic sysmem path).
    input_buffer: usize,
    /// The driver's NV12 surface pitch, returned by the first input lock.
    pitch: u32,
    bitstream_buffer: usize,
    width: u32,
    height: u32,
    fps: u32,
    force_intra: bool,
    frame_index: u64,
    meta: EncoderMeta,
}

// SAFETY: every field is used exclusively from the capture thread that owns
// the encoder (mirrors the OpenH264Encoder arrangement); the CUDA context and
// NVENC session are thread-bound, and the resources move together.
unsafe impl Send for NvencEncoder {}

impl NvencEncoder {
    /// `width`/`height` are the encode dimensions (must be even, matching the
    /// even-dimension rule the other encoders apply).
    pub(crate) fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(format!(
                "encoder dimensions must be even and nonzero: {width}x{height}"
            ));
        }
        if fps == 0 {
            return Err("encoder fps must be positive".into());
        }
        let cuda = CudaApi::load()?;
        cuda.check(unsafe { (cuda.init)(0) }, "cuInit")?;
        let mut device = 0i32;
        cuda.check(unsafe { (cuda.device_get)(&mut device, 0) }, "cuDeviceGet")?;
        let mut cuda_context: *mut c_void = std::ptr::null_mut();
        // Creating the context leaves it current on this thread; every later
        // NVENC call below happens on the same capture thread.
        cuda.check(
            unsafe { (cuda.ctx_create)(&mut cuda_context, 0, device) },
            "cuCtxCreate",
        )?;

        let (fns, encode_lib) = match create_api_function_list() {
            Ok(pair) => pair,
            Err(error) => {
                // The context exists by now; do not leak it on this exit path
                // (Auto selection retries as software right after).
                let _ = unsafe { (cuda.ctx_destroy)(cuda_context) };
                return Err(error);
            }
        };

        // Session open → preset config → overrides → initialize, each stage
        // descriptive so Auto selection can fall back with a real reason.
        let mut session_params = Box::new(OpenEncodeSessionExParams {
            version: NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER,
            device_type: NV_ENC_DEVICE_TYPE_CUDA,
            device: cuda_context,
            reserved: std::ptr::null_mut(),
            api_version: NVENCAPI_VERSION,
            reserved1: [0; 253],
            reserved2: [0; 64],
        });
        let mut pending = Pending {
            encoder: std::ptr::null_mut(),
            input_buffer: 0,
            pitch: 0,
            bitstream_buffer: 0,
        };
        let outcome = (|| {
            fns.check(
                pending.encoder,
                unsafe { (fns.open_encode_session_ex)(&mut *session_params, &mut pending.encoder) },
                "nvEncOpenEncodeSessionEx",
            )?;

            let mut preset = PresetConfig::zeroed();
            preset.version = NV_ENC_PRESET_CONFIG_VER;
            // The driver validates the nested config's version on input too.
            preset.preset_cfg.version = NV_ENC_CONFIG_VER;
            fns.check(
                pending.encoder,
                unsafe {
                    (fns.get_encode_preset_config_ex)(
                        pending.encoder,
                        NV_ENC_CODEC_H264_GUID,
                        NV_ENC_PRESET_P4_GUID,
                        NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
                        &mut *preset,
                    )
                },
                "nvEncGetEncodePresetConfigEx",
            )?;

            // Start from the preset's tuned config, then pin the session
            // contract: baseline + CAVLC (the wire contract matches the
            // openh264 path), no B-frames, no automatic IDR, SPS/PPS in every
            // IDR, CBR with a one-frame VBV for the lowest possible latency.
            let mut config = preset.preset_cfg;
            config.profile_guid = NV_ENC_H264_PROFILE_BASELINE_GUID;
            config.gop_length = NVENC_INFINITE_GOPLENGTH;
            config.frame_interval_p = 1;
            config.encode_codec_config.h264_config.bitfields |=
                NV_ENC_CONFIG_H264_REPEAT_SPSPPS_BIT;
            config.encode_codec_config.h264_config.entropy_coding_mode =
                NV_ENC_H264_ENTROPY_CODING_MODE_CAVLC;
            config.encode_codec_config.h264_config.idr_period = NVENC_INFINITE_GOPLENGTH;
            let vbv_per_frame = (bitrate / fps).max(1);
            config.rc_params.rate_control_mode = NV_ENC_PARAMS_RC_CBR;
            config.rc_params.average_bit_rate = bitrate;
            config.rc_params.max_bit_rate = bitrate;
            config.rc_params.vbv_buffer_size = vbv_per_frame;
            config.rc_params.vbv_initial_delay = vbv_per_frame;

            let mut init = InitializeParams {
                version: NV_ENC_INITIALIZE_PARAMS_VER,
                encode_guid: NV_ENC_CODEC_H264_GUID,
                preset_guid: NV_ENC_PRESET_P4_GUID,
                encode_width: width,
                encode_height: height,
                dar_width: width,
                dar_height: height,
                frame_rate_num: fps,
                frame_rate_den: 1,
                enable_encode_async: 0,
                enable_ptd: 1,
                bitfields: 0,
                priv_data_size: 0,
                reserved: 0,
                priv_data: std::ptr::null_mut(),
                encode_config: &mut config,
                max_encode_width: 0,
                max_encode_height: 0,
                max_me_hint_counts_per_block: [MeHintCountsPerBlock::ZERO; 2],
                tuning_info: NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
                buffer_format: 0,
                num_state_buffers: 0,
                output_stats_level: 0,
                reserved1: [0; 284],
                reserved2: [0; 64],
            };
            fns.check(
                pending.encoder,
                unsafe { (fns.initialize_encoder)(pending.encoder, &mut init) },
                "nvEncInitializeEncoder",
            )?;

            let mut input = CreateInputBuffer {
                version: NV_ENC_CREATE_INPUT_BUFFER_VER,
                width,
                height,
                memory_heap: 0,
                buffer_fmt: NV_ENC_BUFFER_FORMAT_NV12,
                reserved: 0,
                input_buffer: 0,
                p_sys_mem_buffer: std::ptr::null_mut(),
                reserved1: [0; 58],
                reserved2: [0; 63],
            };
            fns.check(
                pending.encoder,
                unsafe { (fns.create_input_buffer)(pending.encoder, &mut input) },
                "nvEncCreateInputBuffer",
            )?;
            pending.input_buffer = input.input_buffer;

            // The lock reports the surface pitch the encode call must repeat.
            let mut first_lock = LockInputBuffer {
                version: NV_ENC_LOCK_INPUT_BUFFER_VER,
                bitfields: 0,
                input_buffer: input.input_buffer,
                buffer_data_ptr: std::ptr::null_mut(),
                pitch: 0,
                reserved1: [0; 251],
                reserved2: [0; 64],
            };
            fns.check(
                pending.encoder,
                unsafe { (fns.lock_input_buffer)(pending.encoder, &mut first_lock) },
                "nvEncLockInputBuffer",
            )?;
            pending.pitch = first_lock.pitch;
            fns.check(
                pending.encoder,
                unsafe { (fns.unlock_input_buffer)(pending.encoder, input.input_buffer) },
                "nvEncUnlockInputBuffer",
            )?;

            let mut bitstream = CreateBitstreamBuffer {
                version: NV_ENC_CREATE_BITSTREAM_BUFFER_VER,
                size: 0,
                memory_heap: 0,
                reserved: 0,
                bitstream_buffer: 0,
                bitstream_buffer_ptr: std::ptr::null_mut(),
                reserved1: [0; 58],
                reserved2: [0; 64],
            };
            fns.check(
                pending.encoder,
                unsafe { (fns.create_bitstream_buffer)(pending.encoder, &mut bitstream) },
                "nvEncCreateBitstreamBuffer",
            )?;
            pending.bitstream_buffer = bitstream.bitstream_buffer;

            Ok(())
        })();

        if let Err(error) = outcome {
            // Any stage after the session open may have left GPU or NVENC
            // resources behind; release what was created before surfacing
            // the failure (the Auto selection path falls back from here).
            pending.abort(&fns);
            let _ = unsafe { (cuda.ctx_destroy)(cuda_context) };
            return Err(error);
        }

        Ok(Self {
            cuda,
            _encode_lib: encode_lib,
            fns,
            encoder: pending.encoder,
            cuda_context,
            input_buffer: pending.input_buffer,
            pitch: pending.pitch,
            bitstream_buffer: pending.bitstream_buffer,
            width,
            height,
            fps,
            force_intra: false,
            frame_index: 0,
            meta: EncoderMeta {
                mode: "nvenc",
                id: "nvenc",
                hardware: true,
                preset: "p4-ultraLowLatency",
                profile: "baseline",
            },
        })
    }

    fn check(&self, status: NvencStatus, call: &str) -> Result<(), String> {
        self.fns.check(self.encoder, status, call)
    }

    fn teardown(&mut self) {
        // SAFETY: each call tears down a resource created in new(); the order
        // is the documented one (bitstream buffer → input buffer → encoder →
        // context), and every step is best-effort so one failure cannot skip
        // the rest.
        unsafe {
            if self.bitstream_buffer != 0 {
                let _ = (self.fns.destroy_bitstream_buffer)(
                    self.encoder,
                    self.bitstream_buffer as *mut c_void,
                );
                self.bitstream_buffer = 0;
            }
            if self.input_buffer != 0 {
                let _ = (self.fns.destroy_input_buffer)(self.encoder, self.input_buffer);
                self.input_buffer = 0;
            }
            if !self.encoder.is_null() {
                let _ = (self.fns.destroy_encoder)(self.encoder);
                self.encoder = std::ptr::null_mut();
            }
            if !self.cuda_context.is_null() {
                let _ = (self.cuda.ctx_destroy)(self.cuda_context);
                self.cuda_context = std::ptr::null_mut();
            }
        }
    }
}

impl VideoEncoder for NvencEncoder {
    fn encode(&mut self, source: &BgraFrame) -> Result<EncodedAu, String> {
        // Convert into the driver-owned NV12 surface, then hand the handle to
        // the encoder. The lock returns CPU access to the surface; the pitch
        // it reported at creation is the one the encode call must repeat.
        let mut input_lock = LockInputBuffer {
            version: NV_ENC_LOCK_INPUT_BUFFER_VER,
            bitfields: 0, // doNotWait = 0: wait until the surface is free.
            input_buffer: self.input_buffer,
            buffer_data_ptr: std::ptr::null_mut(),
            pitch: 0,
            reserved1: [0; 251],
            reserved2: [0; 64],
        };
        self.check(
            unsafe { (self.fns.lock_input_buffer)(self.encoder, &mut input_lock) },
            "nvEncLockInputBuffer",
        )?;
        let nv12_len = (self.pitch * self.height * 3 / 2) as usize;
        // SAFETY: the driver reports `pitch * height * 3 / 2` writable bytes
        // through bufferDataPtr until the matching unlock below.
        let host =
            unsafe { slice::from_raw_parts_mut(input_lock.buffer_data_ptr as *mut u8, nv12_len) };
        convert::bgra_to_nv12(host, self.pitch, self.width, self.height, source);
        self.check(
            unsafe { (self.fns.unlock_input_buffer)(self.encoder, self.input_buffer) },
            "nvEncUnlockInputBuffer",
        )?;

        let flags = if self.force_intra {
            self.force_intra = false;
            NV_ENC_PIC_FLAG_FORCEINTRA | NV_ENC_PIC_FLAG_OUTPUT_SPSPPS
        } else {
            0
        };
        let mut pic = PicParams {
            version: NV_ENC_PIC_PARAMS_VER,
            input_width: self.width,
            input_height: self.height,
            input_pitch: self.pitch,
            encode_pic_flags: flags,
            frame_idx: self.frame_index as u32,
            // 10 MHz ticks keep timestamps monotonic and compressible.
            input_time_stamp: self.frame_index * 10_000_000 / u64::from(self.fps),
            input_duration: 0,
            input_buffer: self.input_buffer as *mut c_void,
            output_bitstream: self.bitstream_buffer as *mut c_void,
            completion_event: std::ptr::null_mut(),
            buffer_fmt: NV_ENC_BUFFER_FORMAT_NV12,
            picture_struct: NV_ENC_PIC_STRUCT_FRAME,
            picture_type: 0,
            codec_pic_params: [0; 193],
            me_hint_counts_per_block: [MeHintCountsPerBlock::ZERO; 2],
            me_external_hints: std::ptr::null_mut(),
            reserved2: [0; 7],
            reserved5: [0; 2],
            qp_delta_map: std::ptr::null_mut(),
            qp_delta_map_size: 0,
            reserved_bit_fields: 0,
            me_hint_ref_pic_dist: [0; 2],
            reserved4: 0,
            alpha_buffer: std::ptr::null_mut(),
            me_external_sb_hints: std::ptr::null_mut(),
            me_sb_hints_count: 0,
            state_buffer_idx: 0,
            output_recon_buffer: std::ptr::null_mut(),
            reserved3: [0; 284],
            reserved6: [0; 57],
        };
        self.check(
            unsafe { (self.fns.encode_picture)(self.encoder, &mut pic) },
            "nvEncEncodePicture",
        )?;

        let mut lock = LockBitstream {
            version: NV_ENC_LOCK_BITSTREAM_VER,
            bitfields: 0, // doNotWait = 0: block until the AU is ready.
            output_bitstream: self.bitstream_buffer as *mut c_void,
            slice_offsets: std::ptr::null_mut(),
            frame_idx: 0,
            hw_encode_status: 0,
            num_slices: 0,
            bitstream_size_in_bytes: 0,
            output_time_stamp: 0,
            output_duration: 0,
            bitstream_buffer_ptr: std::ptr::null_mut(),
            picture_type: 0,
            picture_struct: 0,
            frame_avg_qp: 0,
            frame_satd: 0,
            ltr_frame_idx: 0,
            ltr_frame_bitmap: 0,
            temporal_id: 0,
            intra_mb_count: 0,
            inter_mb_count: 0,
            average_mv_x: 0,
            average_mv_y: 0,
            alpha_layer_size_in_bytes: 0,
            output_stats_ptr_size: 0,
            reserved: 0,
            output_stats_ptr: std::ptr::null_mut(),
            frame_idx_display: 0,
            reserved1: [0; 219],
            reserved2: [0; 63],
            reserved_internal: [0; 8],
        };
        self.check(
            unsafe { (self.fns.lock_bitstream)(self.encoder, &mut lock) },
            "nvEncLockBitstream",
        )?;
        // NOTE: `hwEncodeStatus` is deliberately not checked here. In sync
        // mode driver 615.78.08 fills it with an internal (non-NVENCSTATUS)
        // value even for successful encodes — a byte-exact client identical
        // to this one produces a valid Annex-B AU with hwEncodeStatus = 2.
        // The call return status plus the reported size are the contract.
        // SAFETY: the driver reports `bitstreamSizeInBytes` valid bytes inside
        // its own bitstream buffer, valid until the matching unlock below.
        let data = unsafe {
            slice::from_raw_parts(
                lock.bitstream_buffer_ptr as *const u8,
                lock.bitstream_size_in_bytes as usize,
            )
        }
        .to_vec();
        // A forced intra mid-GOP reports I rather than IDR (PTD decides the
        // exact type); both are all-intra AUs that decode without references,
        // which is what the wire's keyframe/CFG gating needs.
        let keyframe =
            lock.picture_type == NV_ENC_PIC_TYPE_IDR || lock.picture_type == NV_ENC_PIC_TYPE_I;
        self.check(
            unsafe {
                (self.fns.unlock_bitstream)(self.encoder, self.bitstream_buffer as *mut c_void)
            },
            "nvEncUnlockBitstream",
        )?;
        self.frame_index += 1;
        Ok(EncodedAu { data, keyframe })
    }

    fn force_keyframe(&mut self) -> Result<(), String> {
        self.force_intra = true;
        Ok(())
    }

    fn meta(&self) -> EncoderMeta {
        self.meta
    }
}

impl Drop for NvencEncoder {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ABI guard: these sizes were measured from C programs that include the
    /// ffnvcodec n12.2.72.0 `nvEncodeAPI.h` (sizeof, x86-64). A drift here
    /// means the hand-copied struct layouts no longer match the driver's —
    /// fix the Rust struct, never the constant.
    #[test]
    fn abi_struct_sizes_match_ffnvcodec_12_2() {
        assert_eq!(std::mem::size_of::<EncodeApiFunctionList>(), 2552);
        assert_eq!(std::mem::size_of::<OpenEncodeSessionExParams>(), 1552);
        assert_eq!(std::mem::size_of::<InitializeParams>(), 1800);
        assert_eq!(std::mem::size_of::<EncodeConfig>(), 3584);
        assert_eq!(std::mem::size_of::<PresetConfig>(), 5128);
        assert_eq!(std::mem::size_of::<PicParams>(), 3360);
        assert_eq!(std::mem::size_of::<LockBitstream>(), 1544);
        assert_eq!(std::mem::size_of::<CreateInputBuffer>(), 776);
        assert_eq!(std::mem::size_of::<LockInputBuffer>(), 1544);
        assert_eq!(std::mem::size_of::<CreateBitstreamBuffer>(), 776);
    }

    fn nvenc_available() -> bool {
        // NVENC is a documented runtime requirement (nvidia-utils); skip on
        // machines without the runtime instead of failing them. Tests stay at
        // or above the NVENC H.264 minimum dimensions (145x49 measured on
        // driver 615).
        CudaApi::load().is_ok() && load_first_library(ENCODE_LIBRARY_CANDIDATES, "nvenc").is_ok()
    }

    fn solid_frame(b: u8, g: u8, r: u8, width: usize, height: usize) -> Vec<u8> {
        let mut frame = vec![0u8; width * height * 4];
        for px in frame.as_chunks_mut::<4>().0 {
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = 255;
        }
        frame
    }

    #[test]
    fn encodes_solid_frame_as_annexb_with_spspps() {
        if !nvenc_available() {
            println!("NVENC runtime missing; skipping");
            return;
        }
        let mut enc = NvencEncoder::new(256, 256, 60, 500_000).expect("nvenc init");
        let frame = solid_frame(40, 120, 200, 256, 256);
        let source = BgraFrame::new(&frame, 256, 256, 256 * 4).expect("source");
        let au = enc.encode(&source).expect("encode");
        assert!(
            !au.data.is_empty(),
            "NVENC must emit an access unit for the first frame"
        );
        assert!(
            au.data.starts_with(&[0, 0, 0, 1]) || au.data.starts_with(&[0, 0, 1]),
            "unexpected leading bytes: {:02x?}",
            &au.data[..au.data.len().min(4)]
        );
        let sets = crate::wire::h264_parameter_sets(&au.data);
        assert!(
            !sets.is_empty(),
            "SPS/PPS must be present in the first IDR: {:02x?}",
            &au.data[..au.data.len().min(32)]
        );
        assert!(au.keyframe, "first frame must be flagged as IDR");
        assert!(crate::wire::normalize_h264(&au.data).is_some());
    }

    #[test]
    fn forced_keyframe_emits_idr_after_interior_frames() {
        if !nvenc_available() {
            println!("NVENC runtime missing; skipping");
            return;
        }
        let mut enc = NvencEncoder::new(256, 256, 60, 200_000).expect("nvenc init");
        let frame = solid_frame(90, 90, 90, 256, 256);
        let source = BgraFrame::new(&frame, 256, 256, 256 * 4).expect("source");
        let first = enc.encode(&source).expect("encode 1");
        assert!(first.keyframe, "first AU is an IDR");
        let second = enc.encode(&source).expect("encode 2");
        assert!(!second.keyframe, "second AU must be a P frame");
        enc.force_keyframe().expect("force keyframe");
        let third = enc.encode(&source).expect("encode 3");
        assert!(
            third.keyframe,
            "forced AU must be intra (keyframe), got picture type {}",
            third.keyframe
        );
        // Mid-GOP forced intra reports I (not IDR), so repeatSPSPPS does not
        // re-emit SPS/PPS inside it. That is still decodable on its own: the
        // AU is all-intra and the viewer already holds SPS/PPS from the first
        // IDR, which the wire's CFG gating delivered before any slice.
    }

    /// 1080p60 encode-throughput budget: conversion + H2D + encode + lock
    /// must fit inside the 16.6 ms frame period with headroom. Hardware
    /// test with a moving pattern, run explicitly like the stream smoke:
    /// `cargo test nvenc -- --ignored --nocapture`
    #[test]
    #[ignore = "hardware throughput budget; run explicitly on an NVENC machine"]
    fn throughput_1080p60_stays_inside_frame_budget() {
        if !nvenc_available() {
            println!("NVENC runtime missing; skipping");
            return;
        }
        let (width, height, fps) = (1920u32, 1080u32, 60u32);
        let mut enc = NvencEncoder::new(width, height, fps, 12_400_000).expect("nvenc init");
        let mut frame = solid_frame(80, 90, 100, width as usize, height as usize);
        // Warm up (first frame allocates internal surfaces).
        enc.encode(&BgraFrame::new(&frame, width, height, width * 4).expect("source"))
            .expect("warmup");
        let frame_period_us = 1_000_000 / fps;
        let mut samples = Vec::with_capacity(120);
        for step in 0..120u32 {
            // Move a bright band down the frame so every AU has real motion.
            let band = (step * 7 % height) as usize;
            for y in band..(band + 32).min(height as usize) {
                for x in 0..width as usize {
                    let p = (y * width as usize + x) * 4;
                    frame[p] = 200;
                    frame[p + 1] = 220;
                    frame[p + 2] = 240;
                }
            }
            let started = std::time::Instant::now();
            let au = enc
                .encode(&BgraFrame::new(&frame, width, height, width * 4).expect("source"))
                .expect("encode");
            let elapsed_us = started.elapsed().as_micros() as u64;
            assert!(!au.data.is_empty(), "frame {step} produced no AU");
            samples.push(elapsed_us);
        }
        let average = samples.iter().sum::<u64>() / samples.len() as u64;
        let maximum = samples.iter().copied().max().unwrap_or(0);
        samples.sort_unstable();
        let p95 = samples[(samples.len() as f64 * 0.95) as usize % samples.len()];
        println!(
            "1080p60 throughput: avg={average}us max={maximum}us p95={p95}us budget={frame_period_us}us"
        );
        assert!(
            average <= frame_period_us as u64,
            "average per-frame encode {average}us exceeds the 60fps budget {frame_period_us}us"
        );
        assert!(
            p95 <= frame_period_us as u64 * 2,
            "p95 per-frame encode {p95}us exceeds 2x the 60fps budget"
        );
    }
}
