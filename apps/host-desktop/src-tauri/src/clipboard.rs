//! 호스트 클립보드 접근 추상화(U5, docs/07 §20). 실제 앱에서는
//! tauri-plugin-clipboard-manager의 Rust API를 lib.rs setup에서 주입해 쓰고,
//! 테스트·플러그인 미주입 환경에서는 macOS pbcopy/pbpaste로 폴백한다
//! (문서에 허용된 경로 — 플러그인 Rust API가 이 버전에서 macOS 텍스트
//! 설정에 문제를 보이면 이 폴백이 곧 정식 구현이다).
//! 텍스트와 이미지(PNG)를 다룬다 — 파일 클립보드는 범위 밖이다.

use tauri_plugin_clipboard_manager::ClipboardExt;

pub trait ClipboardBackend: Send + Sync {
    /// Native change sequence, unavailable when no reliable signal exists.
    fn revision(&self) -> Result<Option<u64>, String> {
        Ok(None)
    }
    fn read_text(&self) -> Result<String, String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
    /// 클립보드의 이미지를 PNG 바이트로 돌려준다. 이미지가 없으면
    /// Ok(None) — 플러그인 백엔드는 "이미지 없음"과 실패을 구분하지
    /// 못하므로 오류도 없음으로 흡수한다(호출자는 텍스트를 먼저 본다).
    fn read_image_png(&self) -> Result<Option<Vec<u8>>, String>;
    /// PNG 바이트를 클립보드에 쓴다.
    fn write_image_png(&self, png: &[u8]) -> Result<(), String>;
}

/// Query only the native revision, never a clipboard payload. A missing or
/// unusable native sequence forces the caller to read normally.
#[cfg(target_os = "macos")]
fn native_revision() -> Result<Option<u64>, String> {
    use std::ffi::{c_char, c_void};
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
    }
    struct Pool(*mut c_void);
    impl Drop for Pool {
        fn drop(&mut self) {
            unsafe { objc_autoreleasePoolPop(self.0) };
        }
    }
    // NSPasteboard is provided by AppKit already linked by the Host. Calls
    // have no payload arguments and use the documented NSInteger return ABI.
    unsafe {
        let _pool = Pool(objc_autoreleasePoolPush());
        let class = objc_getClass(c"NSPasteboard".as_ptr());
        if class.is_null() {
            return Ok(None);
        }
        let object_send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let integer_send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let pasteboard = object_send(class, sel_registerName(c"generalPasteboard".as_ptr()));
        if pasteboard.is_null() {
            return Ok(None);
        }
        let value = integer_send(pasteboard, sel_registerName(c"changeCount".as_ptr()));
        Ok(u64::try_from(value).ok())
    }
}

#[cfg(target_os = "windows")]
fn native_revision() -> Result<Option<u64>, String> {
    #[link(name = "user32")]
    extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }
    // Zero means inaccessible window station or unavailable sequence.
    let value = unsafe { GetClipboardSequenceNumber() };
    Ok((value != 0).then_some(u64::from(value)))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn native_revision() -> Result<Option<u64>, String> {
    Ok(None)
}

/// RGBA(프리멀티플일 수 있음)를 PNG으로 인코딩한다.
fn rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png data: {e}"))?;
    writer.finish().map_err(|e| format!("png finish: {e}"))?;
    Ok(out)
}

/// PNG 디코딩 상한 — 압축 폭탄 방어. ≤8MiB PNG도 헤더에 거대한 크기를
/// 선언해 디코딩 버퍼를 수 GiB로 부풀릴 수 있으므로(32768² RGBA ≈ 4GiB),
/// 할당 전에 폭·높이 ≤ 8192이고 디코딩 버퍼 ≤ 64MiB인지 검사한다.
const PNG_MAX_DIMENSION: u32 = 8192;
const PNG_MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

/// PNG을 RGBA로 디코딩한다(플러그인 write_image가 RGBA만 받는다).
fn png_to_rgba(png_bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    // 디코더의 중간 버퍼(행 단위)에도 같은 상한을 건다.
    let limits = png::Limits {
        bytes: PNG_MAX_DECODED_BYTES,
    };
    decoder.set_limits(limits);
    let mut reader = decoder.read_info().map_err(|e| format!("png info: {e}"))?;
    // output_buffer_size는 IHDR이 선언한 크기에 비례한다 — 할당 전에 거부한다.
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width > PNG_MAX_DIMENSION || height > PNG_MAX_DIMENSION {
        return Err("clipboard image is too large".into());
    }
    let decoded = reader.output_buffer_size();
    if decoded > PNG_MAX_DECODED_BYTES {
        return Err("clipboard image is too large".into());
    }
    let mut rgba = vec![0u8; decoded];
    let info = reader
        .next_frame(&mut rgba)
        .map_err(|e| format!("png frame: {e}"))?;
    Ok((rgba, info.width, info.height))
}

/// tauri-plugin-clipboard-manager(데스크톱은 arboard) 백엔드.
pub struct TauriClipboard {
    app: tauri::AppHandle,
}

impl TauriClipboard {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

/// 플러그인 오류를 백엔드 오류 문자열로 바꾸되, "내용 없음"만은 빈 텍스트로
/// 흡수한다(H3). 이미지 전용·빈 클립보드는 텍스트 플레이버 자체가 없어
/// arboard가 ContentNotAvailable로 실패하는데, 이를 오류로 전파하면
/// getClipboard 폴링이 이미지 분기에 도달하지 못하고 영구 오류가 된다.
/// 플러그인이 arboard 오류를 문자열로 감싸므로(Error::Clipboard(String))
/// 표시 문자열로 비교한다 — 진짜 실패(ClipboardOccupied 등)는 전파한다.
fn map_read_text_error(error: tauri_plugin_clipboard_manager::Error) -> Result<String, String> {
    if error.to_string() == arboard::Error::ContentNotAvailable.to_string() {
        Ok(String::new())
    } else {
        Err(error.to_string())
    }
}

impl ClipboardBackend for TauriClipboard {
    fn revision(&self) -> Result<Option<u64>, String> {
        native_revision()
    }
    fn read_text(&self) -> Result<String, String> {
        self.app
            .clipboard()
            .read_text()
            .or_else(map_read_text_error)
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.app
            .clipboard()
            .write_text(text)
            .map_err(|e| e.to_string())
    }

    fn read_image_png(&self) -> Result<Option<Vec<u8>>, String> {
        match self.app.clipboard().read_image() {
            Ok(image) => {
                let png = rgba_to_png(image.rgba(), image.width(), image.height())?;
                Ok(Some(png))
            }
            Err(_) => Ok(None),
        }
    }

    fn write_image_png(&self, png_bytes: &[u8]) -> Result<(), String> {
        let (rgba, width, height) = png_to_rgba(png_bytes)?;
        let image = tauri::image::Image::new_owned(rgba, width, height);
        self.app
            .clipboard()
            .write_image(&image)
            .map_err(|e| e.to_string())
    }
}

/// 플러그인이 주입되지 않은 환경(테스트, 등록 실패)의 폴백. macOS pbcopy/
/// pbpaste를 std::process::Command로 실행한다. 다른 플랫폼에서는 spawn이
/// 실패하고 그 오류가 그대로 명령 거부로 돌아간다(실제 앱은 항상 플러그인
/// 백엔드를 주입받는다).
pub struct SystemClipboard;

impl ClipboardBackend for SystemClipboard {
    fn revision(&self) -> Result<Option<u64>, String> {
        native_revision()
    }
    fn read_text(&self) -> Result<String, String> {
        let output = std::process::Command::new("pbpaste")
            .output()
            .map_err(|e| format!("pbpaste: {e}"))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn read_image_png(&self) -> Result<Option<Vec<u8>>, String> {
        // pbpaste는 텍스트만 지원한다 — 이미지는 "없음"으로 흡수한다.
        Ok(None)
    }

    fn write_image_png(&self, _png: &[u8]) -> Result<(), String> {
        Err("image clipboard requires the plugin backend".into())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("pbcopy: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "pbcopy stdin unavailable".to_string())?
            .write_all(text.as_bytes())
            .map_err(|e| format!("pbcopy write: {e}"))?;
        let status = child.wait().map_err(|e| format!("pbcopy wait: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("pbcopy failed: {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }

    fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut crc_input = Vec::with_capacity(tag.len() + data.len());
        crc_input.extend_from_slice(tag);
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    }

    /// IHDR이 선언하는 크기만 유효한 최소 PNG — 실제 픽셀 데이터는 없다.
    /// read_info는 선언 크기를 그대로 믿으므로, 할당 전 거부 검증에 충분하다.
    fn header_only_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit RGBA, 압축·필터·인터레이스 기본값
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        out.extend_from_slice(&chunk(b"IDAT", &[0x78, 0x01]));
        out
    }

    #[test]
    fn decompression_bomb_dimensions_are_rejected_before_allocating() {
        // 32768² RGBA는 디코딩 버퍼 4GiB — ≤8MiB PNG 바이트 한도 안에서도
        // 선언 가능하다. 차원 검사가 할당보다 앞서야 한다.
        let bomb = header_only_png(32768, 32768);
        assert_eq!(
            png_to_rgba(&bomb).unwrap_err(),
            "clipboard image is too large"
        );
    }

    #[test]
    fn decoded_buffer_over_the_cap_is_rejected() {
        // 폭·높이 한도는 통과하지만 8192×8192 RGBA(256MiB)는 버퍼 상한 초과.
        let wide = header_only_png(8192, 8192);
        assert_eq!(
            png_to_rgba(&wide).unwrap_err(),
            "clipboard image is too large"
        );
    }

    #[test]
    fn small_pngs_still_round_trip() {
        let png = rgba_to_png(&[10, 20, 30, 40, 50, 60, 70, 80], 2, 1).unwrap();
        let (rgba, width, height) = png_to_rgba(&png).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba.len(), 8);
    }

    /// "내용 없음"은 오류가 아니라 빈 텍스트다(H3) — 호출자가 이미지 분기로
    /// 진행할 수 있어야 하고, 진짜 오류는 그대로 전파된다.
    #[test]
    fn missing_text_flavor_is_empty_text_not_an_error() {
        let not_available = tauri_plugin_clipboard_manager::Error::Clipboard(
            arboard::Error::ContentNotAvailable.to_string(),
        );
        assert_eq!(map_read_text_error(not_available), Ok(String::new()));

        let occupied = tauri_plugin_clipboard_manager::Error::Clipboard(
            arboard::Error::ClipboardOccupied.to_string(),
        );
        assert!(map_read_text_error(occupied).is_err());
    }
}
