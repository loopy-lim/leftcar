//! 호스트 설정 영속화(U5): `data_dir/leftcar-host/settings.json`(0600).
//! identity.rs의 읽기/쓰기 패턴을 따른다 — 손상·판독 실패는 기본값으로
//! 되돌아간다. 기본값이 곧 안전 쪽(클립보드 동기화 꺼짐)이다.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSettings {
    /// 페어링된 기기와의 클립보드 텍스트 동기화 허용. 기본 꺼짐 — 토글이
    /// 닫혀 있으면 모든 클립보드 제어 명령이 거부된다(docs/07 §20).
    pub clipboard_share: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            clipboard_share: false,
        }
    }
}

/// `dirs::data_dir()/leftcar-host/settings.json` (None when the platform
/// has no data dir — settings then stay in-memory per process).
pub fn default_settings_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("leftcar-host").join("settings.json"))
}

/// Load persisted settings. A corrupt or unreadable file falls back to the
/// defaults instead of refusing to start.
pub fn load(path: Option<&Path>) -> HostSettings {
    let Some(path) = path else {
        return HostSettings::default();
    };
    load_from(path).unwrap_or_else(|error| {
        eprintln!("leftcar: host settings unavailable, using defaults: {error}");
        HostSettings::default()
    })
}

fn load_from(path: &Path) -> Result<HostSettings, String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
    Ok(HostSettings {
        clipboard_share: parsed
            .get("clipboardShare")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

pub fn persist(path: &Path, settings: &HostSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let body =
        serde_json::json!({ "v": 1, "clipboardShare": settings.clipboard_share }).to_string();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .map_err(|e| format!("write: {e}"))
    }
    #[cfg(not(unix))]
    std::fs::write(path, body).map_err(|e| format!("write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("leftcar-settings-{tag}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn settings_roundtrip_preserves_clipboard_share() {
        let path = temp_path("roundtrip");
        persist(&path, &HostSettings { clipboard_share: true }).unwrap();
        let loaded = load(Some(&path));
        assert!(loaded.clipboard_share);
        persist(&path, &HostSettings { clipboard_share: false }).unwrap();
        assert!(!load(Some(&path)).clipboard_share);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        // 손상 파일은 기본값(꺼짐)으로 되돌아간다 — 안전 쪽이 기본이다.
        assert!(!load(Some(&path)).clipboard_share);
        let _ = std::fs::remove_file(&path);
        assert!(!load(Some(&path)).clipboard_share);
    }

    #[test]
    fn persisted_settings_have_restricted_permissions() {
        let path = temp_path("perms");
        persist(&path, &HostSettings::default()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "settings must be 0600");
        }
        let _ = std::fs::remove_file(&path);
    }
}
