//! Power assertions keeping the Mac awake after the lid closes.
//!
//! Primary defense is a `caffeinate` child process: dropping the handle
//! terminates the child, so the assertion can never outlive the app.

/// `-s` (keep system awake) is only valid on AC power; on battery we degrade
/// to `-i` (prevent idle sleep) and the UI shows a stability warning.
pub fn caffeinate_args(on_battery: bool) -> Vec<String> {
    vec![if on_battery { "-i".into() } else { "-s".into() }]
}

/// Parses `pmset -g batt` output. Unknown formats conservatively assume AC
/// (stronger assertion) — the failure mode is a louder warning, not a crash.
pub fn parse_pmset_reports_battery(pmset_output: &str) -> bool {
    pmset_output.to_ascii_lowercase().contains("discharging")
}

#[cfg(target_os = "macos")]
pub struct PowerAssertion {
    child: std::process::Child,
}

#[cfg(target_os = "macos")]
impl PowerAssertion {
    /// Spawns `caffeinate` so the Mac stays usable after the lid closes.
    pub fn acquire(on_battery: bool) -> Result<Self, String> {
        let child = std::process::Command::new("caffeinate")
            .args(caffeinate_args(on_battery))
            .spawn()
            .map_err(|error| format!("caffeinate 실행 실패: {error}"))?;
        Ok(Self { child })
    }

    pub fn on_battery() -> bool {
        parse_pmset_reports_battery(
            &std::process::Command::new("pmset")
                .args(["-g", "batt"])
                .output()
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
                .unwrap_or_default(),
        )
    }
}

#[cfg(target_os = "macos")]
impl Drop for PowerAssertion {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_power_uses_the_system_sleep_assertion() {
        assert_eq!(caffeinate_args(false), vec!["-s".to_string()]);
    }

    #[test]
    fn battery_power_degrades_to_idle_prevention() {
        assert_eq!(caffeinate_args(true), vec!["-i".to_string()]);
    }

    #[test]
    fn pmset_discharging_means_battery() {
        assert!(parse_pmset_reports_battery(
            "Now drawing from 'Battery'\n -InternalBattery-0 87%; discharging; 4:20 remaining"
        ));
        assert!(!parse_pmset_reports_battery(
            "Now drawing from 'AC Power'\n -InternalBattery-0 100%; charged; 0:00 remaining"
        ));
        // Unknown format: conservatively AC.
        assert!(!parse_pmset_reports_battery("unexpected"));
    }
}
