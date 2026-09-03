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

/// Builds the argv for connecting a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn connect_args(name: &str) -> Vec<String> {
    vec![
        "set".into(),
        format!("-namelike={name}"),
        "-connected=on".into(),
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

/// Builds the argv for discarding a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn remove_args(name: &str) -> Vec<String> {
    vec!["discard".into(), format!("-namelike={name}")]
}

#[cfg(target_os = "macos")]
pub const CLI: &str = "betterdisplaycli";

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
    let output = std::process::Command::new(CLI)
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
                "-namelike=Leftcar Virtual".to_string(),
                "-connected=on".to_string(),
            ]
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
            vec![
                "discard".to_string(),
                "-namelike=Leftcar Virtual".to_string(),
            ]
        );
    }
}
