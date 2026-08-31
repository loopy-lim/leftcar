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
    pub(crate) fn AMediaCodec_createCodecByName(name: *const std::ffi::c_char) -> *mut AMediaCodec;
    pub(crate) fn AMediaCodec_createDecoderByType(
        mime: *const std::ffi::c_char,
    ) -> *mut AMediaCodec;
    pub(crate) fn AMediaCodec_delete(codec: *mut AMediaCodec) -> media_status_t;
    pub(crate) fn AMediaCodec_configure(
        codec: *mut AMediaCodec,
        format: *const AMediaFormat,
        surface: *mut std::ffi::c_void, // ANativeWindow*
        crypto: *const std::ffi::c_void,
        flags: u32,
    ) -> media_status_t;
    pub(crate) fn AMediaCodec_start(codec: *mut AMediaCodec) -> media_status_t;
    pub(crate) fn AMediaCodec_stop(codec: *mut AMediaCodec) -> media_status_t;
    pub(crate) fn AMediaCodec_flush(codec: *mut AMediaCodec) -> media_status_t;
    pub(crate) fn AMediaCodec_queueInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        offset: usize,
        size: usize,
        time_us: i64,
        flags: u32,
    ) -> media_status_t;
    pub(crate) fn AMediaCodec_dequeueInputBuffer(
        codec: *mut AMediaCodec,
        timeout_us: i64,
    ) -> ssize_t;
    pub(crate) fn AMediaCodec_dequeueOutputBuffer(
        codec: *mut AMediaCodec,
        info: *mut AMediaCodecBufferInfo,
        timeout_us: i64,
    ) -> ssize_t;
    pub(crate) fn AMediaCodec_releaseOutputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        render: bool,
    ) -> media_status_t;
    pub(crate) fn AMediaCodec_releaseOutputBufferAtTime(
        codec: *mut AMediaCodec,
        idx: usize,
        timestamp_ns: i64,
    ) -> media_status_t;
    pub(crate) fn AMediaCodec_getInputBuffer(
        codec: *mut AMediaCodec,
        idx: usize,
        out_size: *mut usize,
    ) -> *mut u8;
    pub(crate) fn AMediaFormat_new() -> *mut AMediaFormat;
    pub(crate) fn AMediaFormat_delete(format: *mut AMediaFormat) -> media_status_t;
    pub(crate) fn AMediaFormat_setBuffer(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        data: *const std::ffi::c_void,
        size: usize,
    );
    pub(crate) fn AMediaFormat_setInt32(
        format: *mut AMediaFormat,
        name: *const std::ffi::c_char,
        value: i32,
    );
}

#[cfg(not(target_os = "android"))]
extern "C" {
    #[link_name = "AMediaCodec_getName"]
    pub fn AMediaCodec_getName_pub(
        codec: *mut AMediaCodec,
        out_name: *mut *mut std::ffi::c_char,
    ) -> media_status_t;
    // Host builds never call these; stubs keep the lib linking for tests.
    #[allow(clippy::missing_safety_doc)]
    pub fn AMediaCodec_createCodecByName(name: *const std::ffi::c_char) -> *mut AMediaCodec;
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
    pub fn AMediaCodec_releaseOutputBufferAtTime(
        codec: *mut AMediaCodec,
        idx: usize,
        timestamp_ns: i64,
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

// -- Annex-B parser (pure Rust, host-testable) --------------------------------
