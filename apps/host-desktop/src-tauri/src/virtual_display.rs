//! Opt-in BetterDisplay virtual display experiments (macOS only).
//!
//! Leftcar never bundles BetterDisplay. We only shell out to
//! `betterdisplaycli` when the user explicitly enables the experiment.
//!
//! CLI contract (BetterDisplay 4.3.6, verified on-device 2026-09-02):
//! - create: `create -devicetype=virtualscreen -virtualscreenname=<name>
//!   -aspectWidth=<w> -aspectHeight=<h> -virtualScreenHiDPI=off
//!   -multiplierStep=1 -limitMultiplierSize=on -multiplierMinWidth=<w>
//!   -multiplierMinHeight=<h> -multiplierMaxWidth=<w> -multiplierMaxHeight=<h>`
//! - connect: `set -namelike=<name> -connected=on`
//! - discard: `discard -namelike=<name>`
//!
//! Despite the name, `aspectWidth/aspectHeight` are PIXEL dimensions, not
//! ratio numbers: with HiDPI on and a free multiplier, `16x9` produced a
//! 6400x4000 backing store whose UI looked tiny when streamed to a tablet.
//! HiDPI off plus a 1x-only multiplier yields exactly one WxH mode.
//!
//! Discard deliberately uses `-namelike`: `-virtualscreenname` is not in the
//! `betterdisplaycli` help identifier list, and an unspecified identifier
//! discards ALL discardable devices — hence the hard empty-name guard on the
//! remove path.

/// Builds the argv for creating a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn create_args(name: &str, width: u32, height: u32) -> Vec<String> {
    vec![
        "create".into(),
        "-devicetype=virtualscreen".into(),
        format!("-virtualscreenname={name}"),
        format!("-aspectWidth={width}"),
        format!("-aspectHeight={height}"),
        "-virtualScreenHiDPI=off".into(),
        "-multiplierStep=1".into(),
        "-limitMultiplierSize=on".into(),
        format!("-multiplierMinWidth={width}"),
        format!("-multiplierMinHeight={height}"),
        format!("-multiplierMaxWidth={width}"),
        format!("-multiplierMaxHeight={height}"),
    ]
}

/// HiDPI creation contract: width/height are logical points and the selected
/// multiplier is the backing pixel scale. BetterDisplay's generated mode
/// bounds use backing pixels, while `aspectWidth/Height` remain logical.
pub fn create_hidpi_args(
    name: &str,
    logical_width: u32,
    logical_height: u32,
    scale: u8,
) -> Vec<String> {
    vec![
        "create".into(),
        "-devicetype=virtualscreen".into(),
        format!("-virtualscreenname={name}"),
        format!("-aspectWidth={logical_width}"),
        format!("-aspectHeight={logical_height}"),
        format!(
            "-virtualScreenHiDPI={}",
            if scale == 2 { "on" } else { "off" }
        ),
        "-multiplierStep=1".into(),
        "-limitMultiplierSize=on".into(),
        format!("-multiplierMinWidth={logical_width}"),
        format!("-multiplierMinHeight={logical_height}"),
        format!("-multiplierMaxWidth={logical_width}"),
        format!("-multiplierMaxHeight={logical_height}"),
    ]
}

/// Builds the argv for connecting a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn connect_args(name: &str) -> Vec<String> {
    vec![
        "set".into(),
        format!("-name={name}"),
        "-connected=on".into(),
    ]
}

pub fn query_mode_args(name: &str) -> Vec<String> {
    vec![
        "get".into(),
        format!("-name={name}"),
        "-resolution".into(),
        "-hiDPI".into(),
    ]
}

pub fn query_identifier_args(name: &str, identifier: &str) -> Vec<String> {
    vec![
        "get".into(),
        format!("-name={name}"),
        format!("-identifier={identifier}"),
    ]
}

pub fn parse_mode_query(output: &str) -> Result<(bool, u32, u32), String> {
    let value = output.trim();
    let (first, second) = value
        .split_once(',')
        .ok_or_else(|| format!("BetterDisplay 모드 조회 형식 오류: {value}"))?;
    let (hidpi, resolution) = if matches!(first, "on" | "off") {
        (first, second)
    } else if matches!(second, "on" | "off") {
        (second, first)
    } else {
        return Err(format!("BetterDisplay HiDPI 상태 오류: {value}"));
    };
    let (width, height) = resolution
        .split_once('x')
        .ok_or_else(|| format!("BetterDisplay 해상도 조회 형식 오류: {value}"))?;
    if !matches!(hidpi, "on" | "off") {
        return Err(format!("BetterDisplay HiDPI 상태 오류: {hidpi}"));
    }
    Ok((
        hidpi == "on",
        width
            .parse()
            .map_err(|_| "조회된 폭이 숫자가 아닙니다".to_string())?,
        height
            .parse()
            .map_err(|_| "조회된 높이가 숫자가 아닙니다".to_string())?,
    ))
}

pub fn placement_args(name: &str, x: i32, y: i32) -> Vec<String> {
    vec![
        "set".into(),
        format!("-name={name}"),
        format!("-placement={x}x{y}"),
    ]
}

/// Guards against an empty `-namelike=` match on later discard calls: a blank
/// name could match unrelated displays (and `discard` without an identifier
/// removes ALL discardable devices), so blank names are rejected up front.
/// Returns the trimmed name so every CLI argv uses the exact same spelling
/// the validation checked.
pub fn validate_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err("가상 디스플레이 이름을 입력하세요. 빈 이름은 허용되지 않습니다.".into())
    } else {
        Ok(trimmed.to_string())
    }
}

/// `aspectWidth/aspectHeight` are pixel dimensions (see module docs), so
/// ratio-style inputs like 16x9 are rejected before they can create a
/// degenerate display. The smallest mainstream tablet-pixel size (1280x800)
/// is the floor.
pub fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    const MIN_DIMENSION: u32 = 800;
    if width < MIN_DIMENSION || height < MIN_DIMENSION {
        return Err(format!(
            "가로/세로 픽셀은 각각 {MIN_DIMENSION} 이상이어야 합니다 (비율 숫자가 아닌 픽셀 값, 예: 1920x1200)."
        ));
    }
    Ok(())
}

pub fn validate_scale(scale: u8) -> Result<(), String> {
    if matches!(scale, 1 | 2) {
        Ok(())
    } else {
        Err("HiDPI 배율은 1 또는 2여야 합니다.".into())
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObservedDisplay {
    pub hidpi: bool,
    pub tag_id: String,
    pub uuid: String,
    pub display_id: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayIdentity {
    pub tag_id: String,
    pub uuid: String,
    pub display_id: u32,
}

#[cfg(target_os = "macos")]
pub fn observe_coregraphics_display(display_id: u32) -> Result<ObservedDisplay, String> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayPixelsWide(id: u32) -> usize;
        fn CGDisplayPixelsHigh(id: u32) -> usize;
        fn CGDisplayCopyDisplayMode(id: u32) -> *const std::ffi::c_void;
        fn CGDisplayModeGetPixelWidth(mode: *const std::ffi::c_void) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: *const std::ffi::c_void) -> usize;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *const std::ffi::c_void);
    }
    let mode = unsafe { CGDisplayCopyDisplayMode(display_id) };
    if mode.is_null() {
        return observe_display_in_fresh_process(display_id);
    }
    let logical_width = unsafe { CGDisplayPixelsWide(display_id) as u32 };
    let logical_height = unsafe { CGDisplayPixelsHigh(display_id) as u32 };
    let pixel_width = unsafe { CGDisplayModeGetPixelWidth(mode) as u32 };
    let pixel_height = unsafe { CGDisplayModeGetPixelHeight(mode) as u32 };
    let observed = ObservedDisplay {
        hidpi: pixel_width > logical_width || pixel_height > logical_height,
        tag_id: String::new(),
        uuid: String::new(),
        display_id,
        logical_width,
        logical_height,
        pixel_width,
        pixel_height,
    };
    unsafe {
        CFRelease(mode);
    }
    Ok(observed)
}

#[cfg(target_os = "macos")]
fn observe_display_in_fresh_process(display_id: u32) -> Result<ObservedDisplay, String> {
    let inspection = crate::provider::CgvdProvider::inspect_display(display_id)
        .map_err(|error| error.message())?;
    Ok(ObservedDisplay {
        hidpi: inspection.pixel_width > inspection.logical_width
            || inspection.pixel_height > inspection.logical_height,
        tag_id: String::new(),
        uuid: String::new(),
        display_id: inspection.display_id,
        logical_width: inspection.logical_width,
        logical_height: inspection.logical_height,
        pixel_width: inspection.pixel_width,
        pixel_height: inspection.pixel_height,
    })
}

#[cfg(target_os = "macos")]
fn parse_identifier(output: &str, kind: &str) -> Result<String, String> {
    let value = output.trim();
    let values: Vec<_> = value.split(',').filter(|item| !item.is_empty()).collect();
    if values.is_empty() {
        return Err(format!("{kind} 식별자 조회 형식 오류: {value}"));
    }
    if kind == "tagID" {
        return Ok(values.last().expect("non-empty").to_string());
    }
    if values.iter().all(|item| *item == values[0]) {
        Ok(values[0].to_string())
    } else {
        Err(format!("{kind} 식별자가 둘 이상입니다: {value}"))
    }
}

#[cfg(target_os = "macos")]
pub fn observe_display(name: &str) -> Result<ObservedDisplay, String> {
    let mode = run_cli(query_mode_args(name))?;
    let (hidpi, logical_width, logical_height) = parse_mode_query(&mode)?;
    let identity = resolve_display_identity(name)?;
    let core_graphics = observe_coregraphics_display(identity.display_id)?;
    Ok(ObservedDisplay {
        hidpi,
        tag_id: identity.tag_id,
        uuid: identity.uuid,
        display_id: identity.display_id,
        logical_width,
        logical_height,
        pixel_width: core_graphics.pixel_width,
        pixel_height: core_graphics.pixel_height,
    })
}

#[cfg(target_os = "macos")]
pub fn resolve_display_identity(name: &str) -> Result<DisplayIdentity, String> {
    let tag_id = parse_identifier(&run_cli(query_identifier_args(name, "tagID"))?, "tagID")?;
    let uuid = parse_identifier(&run_cli(query_identifier_args(name, "UUID"))?, "UUID")?;
    let display_id = parse_identifier(
        &run_cli(query_identifier_args(name, "displayID"))?,
        "displayID",
    )?
    .parse::<u32>()
    .map_err(|_| "displayID 조회 결과가 숫자가 아닙니다".to_string())?;
    Ok(DisplayIdentity {
        tag_id,
        uuid,
        display_id,
    })
}

#[cfg(target_os = "macos")]
pub fn set_coregraphics_placement(display_id: u32, x: i32, y: i32) -> Result<(), String> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGBeginDisplayConfiguration(config: *mut *mut std::ffi::c_void) -> i32;
        fn CGConfigureDisplayOrigin(
            config: *mut std::ffi::c_void,
            display: u32,
            x: i32,
            y: i32,
        ) -> i32;
        fn CGCompleteDisplayConfiguration(config: *mut std::ffi::c_void, option: u32) -> i32;
        fn CGCancelDisplayConfiguration(config: *mut std::ffi::c_void) -> i32;
    }
    let mut config = std::ptr::null_mut();
    let begin = unsafe { CGBeginDisplayConfiguration(&mut config) };
    if begin != 0 {
        return Err(format!("디스플레이 배치 구성 시작 실패: {begin}"));
    }
    let configure = unsafe { CGConfigureDisplayOrigin(config, display_id, x, y) };
    if configure != 0 {
        unsafe {
            CGCancelDisplayConfiguration(config);
        }
        return Err(format!("디스플레이 배치 설정 실패: {configure}"));
    }
    let complete = unsafe { CGCompleteDisplayConfiguration(config, 1) };
    if complete != 0 {
        return Err(format!("디스플레이 배치 적용 실패: {complete}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn coregraphics_display_rect(display_id: u32) -> Option<(i32, i32, u32, u32)> {
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayBounds(id: u32) -> CGRect;
    }
    let rect = unsafe { CGDisplayBounds(display_id) };
    (rect.size.width > 0.0 && rect.size.height > 0.0).then_some((
        rect.origin.x.round() as i32,
        rect.origin.y.round() as i32,
        rect.size.width.round() as u32,
        rect.size.height.round() as u32,
    ))
}

#[cfg(target_os = "macos")]
pub fn main_display_id() -> u32 {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
    }
    unsafe { CGMainDisplayID() }
}

#[cfg(target_os = "macos")]
pub fn main_display_stable_id() -> Option<String> {
    #[repr(C)]
    struct CfUuidBytes {
        bytes: [u8; 16],
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> *const std::ffi::c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFUUIDGetUUIDBytes(uuid: *const std::ffi::c_void) -> CfUuidBytes;
        fn CFRelease(value: *const std::ffi::c_void);
    }
    unsafe {
        let uuid = CGDisplayCreateUUIDFromDisplayID(CGMainDisplayID());
        if uuid.is_null() {
            return None;
        }
        let bytes = CFUUIDGetUUIDBytes(uuid).bytes;
        CFRelease(uuid);
        Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

#[cfg(target_os = "macos")]
pub fn verify_display_mode(
    name: &str,
    expected_width: u32,
    expected_height: u32,
    expected_scale: u8,
) -> Result<ObservedDisplay, String> {
    let observed = observe_display(name)?;
    let expected_pixels = expected_width
        .checked_mul(expected_scale as u32)
        .zip(expected_height.checked_mul(expected_scale as u32))
        .ok_or("요청 픽셀이 너무 큽니다")?;
    if (observed.logical_width, observed.logical_height) != (expected_width, expected_height)
        || (observed.pixel_width, observed.pixel_height) != expected_pixels
        || observed.hidpi != (expected_scale == 2)
    {
        return Err(format!("요청한 모드가 적용되지 않았습니다: 요청 logical {expected_width}x{expected_height}, backing {}x{}; 실제 logical {}x{}, backing {}x{}", expected_pixels.0, expected_pixels.1, observed.logical_width, observed.logical_height, observed.pixel_width, observed.pixel_height));
    }
    Ok(observed)
}

#[cfg(target_os = "macos")]
pub fn set_placement(name: &str, x: i32, y: i32) -> Result<String, String> {
    let name = &validate_name(name)?;
    run_cli(placement_args(name, x, y))
}

#[cfg(target_os = "macos")]
pub fn set_placement_by_uuid(uuid: &str, x: i32, y: i32) -> Result<String, String> {
    run_cli(vec![
        "set".into(),
        format!("-UUID={uuid}"),
        format!("-placement={x}x{y}"),
    ])
}

#[cfg(target_os = "macos")]
pub fn set_main_display(display_id: u32) -> Result<String, String> {
    run_cli(vec![
        "set".into(),
        format!("-displayID={display_id}"),
        "-main=on".into(),
    ])
}

#[cfg(target_os = "macos")]
pub fn remove_virtual_display_by_uuid(uuid: &str) -> Result<String, String> {
    run_cli(vec!["discard".into(), format!("-UUID={uuid}")])
}

#[cfg(not(target_os = "macos"))]
pub fn set_placement(_name: &str, _x: i32, _y: i32) -> Result<String, String> {
    Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
}

#[cfg(target_os = "macos")]
pub fn create_virtual_display_hidpi(
    name: &str,
    logical_width: u32,
    logical_height: u32,
    scale: u8,
) -> Result<String, String> {
    let name = &validate_name(name)?;
    validate_scale(scale)?;
    let backing_width = logical_width
        .checked_mul(scale as u32)
        .ok_or("backing 폭이 너무 큽니다")?;
    let backing_height = logical_height
        .checked_mul(scale as u32)
        .ok_or("backing 높이가 너무 큽니다")?;
    validate_dimensions(backing_width, backing_height)?;
    run_cli(create_hidpi_args(
        name,
        logical_width,
        logical_height,
        scale,
    ))?;
    if let Err(error) = run_cli(connect_args(name)) {
        let cleanup = run_cli(remove_args(name));
        return Err(format!("가상 화면 연결 실패: {error}; cleanup={cleanup:?}"));
    }
    let observed = match verify_display_mode(name, logical_width, logical_height, scale) {
        Ok(value) => value,
        Err(error) => {
            let cleanup = run_cli(remove_args(name));
            return Err(format!(
                "가상 화면 모드 검증 실패: {error}; cleanup={cleanup:?}"
            ));
        }
    };
    Ok(format!("verified logical={logical_width}x{logical_height} backing={}x{} tagID={} UUID={} displayID={}", observed.pixel_width, observed.pixel_height, observed.tag_id, observed.uuid, observed.display_id))
}

/// Builds the argv for discarding a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn remove_args(name: &str) -> Vec<String> {
    vec!["discard".into(), format!("-name={name}")]
}

#[cfg(target_os = "macos")]
pub const CLI: &str = "betterdisplaycli";

#[cfg(target_os = "macos")]
pub fn cli_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("LEFTCAR_BETTERDISPLAY_CLI") {
        return path.into();
    }
    for path in [
        "/opt/homebrew/bin/betterdisplaycli",
        "/usr/local/bin/betterdisplaycli",
        "/Applications/BetterDisplay.app/Contents/MacOS/BetterDisplay",
    ] {
        let candidate = std::path::PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    CLI.into()
}

#[cfg(target_os = "macos")]
pub fn cli_available() -> bool {
    let path = cli_path();
    if path.components().count() > 1 {
        path.is_file()
    } else {
        std::process::Command::new("which")
            .arg(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
pub fn create_virtual_display(name: &str, width: u32, height: u32) -> Result<String, String> {
    let name = &validate_name(name)?;
    validate_dimensions(width, height)?;
    run_cli(create_args(name, width, height))?;
    run_cli(connect_args(name))
}

#[cfg(target_os = "macos")]
pub fn remove_virtual_display(name: &str) -> Result<String, String> {
    let name = &validate_name(name)?;
    run_cli(remove_args(name))
}

#[cfg(target_os = "macos")]
fn run_cli(args: Vec<String>) -> Result<String, String> {
    let output = std::process::Command::new(cli_path())
        .args(&args)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "BetterDisplay CLI를 찾을 수 없습니다. BetterDisplay를 설치하고 설정에서 CLI 접근을 허용하세요.".to_string()
            } else {
                format!("betterdisplaycli 실행 실패: {error}")
            }
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "betterdisplaycli 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_matches_cli_contract() {
        assert_eq!(
            create_args("Leftcar Virtual", 1920, 1200),
            vec![
                "create".to_string(),
                "-devicetype=virtualscreen".to_string(),
                "-virtualscreenname=Leftcar Virtual".to_string(),
                "-aspectWidth=1920".to_string(),
                "-aspectHeight=1200".to_string(),
                "-virtualScreenHiDPI=off".to_string(),
                "-multiplierStep=1".to_string(),
                "-limitMultiplierSize=on".to_string(),
                "-multiplierMinWidth=1920".to_string(),
                "-multiplierMinHeight=1200".to_string(),
                "-multiplierMaxWidth=1920".to_string(),
                "-multiplierMaxHeight=1200".to_string(),
            ]
        );
    }

    #[test]
    fn hidpi_contract_keeps_logical_and_backing_dimensions_distinct() {
        let args = create_hidpi_args("Leftcar", 1600, 1000, 2);
        assert!(args.contains(&"-aspectWidth=1600".to_string()));
        assert!(args.contains(&"-aspectHeight=1000".to_string()));
        assert!(args.contains(&"-multiplierMinWidth=1600".to_string()));
        assert!(args.contains(&"-multiplierMaxHeight=1000".to_string()));
        assert!(args.contains(&"-virtualScreenHiDPI=on".to_string()));
    }

    #[test]
    fn mode_query_requires_observed_hidpi_and_logical_resolution() {
        assert_eq!(
            parse_mode_query("on,1600x1000\n").unwrap(),
            (true, 1600, 1000)
        );
        assert_eq!(
            parse_mode_query("1600x1000,on\n").unwrap(),
            (true, 1600, 1000)
        );
        assert!(parse_mode_query("1600x1000").is_err());
    }

    #[test]
    fn pixel_dimensions_reject_ratio_numbers() {
        // The CLI interprets aspectWidth/Height as pixels: a 16x9 request
        // would create a 16x9-pixel display (or, with HiDPI multipliers, a
        // giant HiDPI backing store). Only realistic pixel sizes are valid.
        assert!(validate_dimensions(16, 9).is_err());
        assert!(validate_dimensions(1920, 1200).is_ok());
        assert!(validate_dimensions(3200, 2000).is_ok());
        assert!(validate_dimensions(0, 1200).is_err());
        assert!(validate_dimensions(1920, 0).is_err());
    }

    #[test]
    fn connect_args_matches_cli_contract() {
        assert_eq!(
            connect_args("Leftcar Virtual"),
            vec![
                "set".to_string(),
                "-name=Leftcar Virtual".to_string(),
                "-connected=on".to_string(),
            ]
        );
    }

    #[test]
    fn placement_uses_logical_coordinate_contract() {
        assert_eq!(
            placement_args("Leftcar [id]", -1600, 0),
            vec!["set", "-name=Leftcar [id]", "-placement=-1600x0"]
        );
    }

    #[test]
    fn empty_name_is_rejected() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("\t\n").is_err());
    }

    #[test]
    fn valid_name_is_accepted_and_trimmed() {
        assert_eq!(validate_name("Leftcar Virtual").unwrap(), "Leftcar Virtual");
        assert_eq!(validate_name(" Leftcar ").unwrap(), "Leftcar");
    }

    #[test]
    fn remove_args_matches_cli_contract() {
        assert_eq!(
            remove_args("Leftcar Virtual"),
            vec!["discard".to_string(), "-name=Leftcar Virtual".to_string(),]
        );
    }
}
