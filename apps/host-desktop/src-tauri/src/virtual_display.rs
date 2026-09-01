//! Opt-in BetterDisplay virtual display experiments (macOS only).
//!
//! Leftcar never bundles BetterDisplay. We only shell out to
//! `betterdisplaycli` when the user explicitly enables the experiment.
//!
//! CLI contract (BetterDisplay 4.3.6 help + maintainer examples):
//! - create: `create -devicetype=virtualscreen -virtualscreenname=<name> -aspectWidth=<w> -aspectHeight=<h>`
//! - connect: `set -namelike=<name> -connected=on`

/// Builds the argv for creating a virtual display. Unit-tested on every
/// platform; the actual process spawn is exercised only on a machine with
/// BetterDisplay installed.
pub fn create_args(name: &str, aspect_w: u32, aspect_h: u32) -> Vec<String> {
    vec![
        "create".into(),
        "-devicetype=virtualscreen".into(),
        format!("-virtualscreenname={name}"),
        format!("-aspectWidth={aspect_w}"),
        format!("-aspectHeight={aspect_h}"),
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
/// name could match unrelated displays, so creation is rejected up front.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("가상 디스플레이 이름을 입력하세요. 빈 이름은 허용되지 않습니다.".into())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub const CLI: &str = "betterdisplaycli";

#[cfg(target_os = "macos")]
pub fn create_virtual_display(name: &str, aspect_w: u32, aspect_h: u32) -> Result<String, String> {
    validate_name(name)?;
    run_cli(create_args(name, aspect_w, aspect_h))?;
    run_cli(connect_args(name))
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
            create_args("Leftcar Virtual", 16, 9),
            vec![
                "create".to_string(),
                "-devicetype=virtualscreen".to_string(),
                "-virtualscreenname=Leftcar Virtual".to_string(),
                "-aspectWidth=16".to_string(),
                "-aspectHeight=9".to_string(),
            ]
        );
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
    fn valid_name_is_accepted() {
        assert!(validate_name("Leftcar Virtual").is_ok());
        assert!(validate_name(" Leftcar ").is_ok());
    }
}
