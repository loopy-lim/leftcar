//! 호스트 설정 영속화(파일 공유 게이트 등). `data_dir/leftcar-host/settings.json`
//! (0600)에 저장하며, 없으면 기본값(모두 꺼짐)으로 만든다. 파일이 깨졌다면
//! 기본값으로 되돌린다 — 승인 토글은 안전 쪽으로 실패해야 한다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 승인 토글 성격의 호스트 설정. 둘 다 기본 꺼짐이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSettings {
    pub clipboard_share: bool,
    pub file_share: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            clipboard_share: false,
            file_share: false,
        }
    }
}

/// `dirs::data_dir()/leftcar-host/settings.json` (None when the platform has
/// no data dir — settings then stay in-memory per process).
pub fn default_settings_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("leftcar-host").join("settings.json"))
}

/// 설정 파일을 읽거나 기본값으로 만든다. 깨진 파일은 기본값으로 대체한다
/// (로그만 남기고 실패시키지 않는다 — 토글이 고착되면 회복 경로가 없다).
pub fn load_or_default(path: Option<&Path>) -> HostSettings {
    let Some(path) = path else {
        return HostSettings::default();
    };
    let Some(body) = std::fs::read_to_string(path).ok() else {
        return HostSettings::default();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
        eprintln!("leftcar: settings file is corrupt, resetting to defaults");
        return HostSettings::default();
    };
    HostSettings {
        clipboard_share: parsed
            .get("clipboardShare")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        file_share: parsed
            .get("fileShare")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

fn persist(path: &Path, settings: &HostSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let body = serde_json::json!({
        "v": 1,
        "clipboardShare": settings.clipboard_share,
        "fileShare": settings.file_share,
    })
    .to_string();
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

/// 프로세스 전역에서 공유되는 설정값(메모리 상태 + 영속 경로). ControlServer의
/// 제어 명령 게이트와 Tauri UI 명령이 같은 인스턴스를 나눠 쓴다.
pub struct SharedSettings {
    path: Option<PathBuf>,
    settings: Mutex<HostSettings>,
}

impl SharedSettings {
    pub fn load_or_default(path: Option<PathBuf>) -> Self {
        let settings = load_or_default(path.as_deref());
        Self {
            path,
            settings: Mutex::new(settings),
        }
    }

    /// 영속 경로 없이(기본 꺼짐) 시작하는 공유 인스턴스 — 테스트·폴백용.
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self::load_or_default(None))
    }

    pub fn get(&self) -> HostSettings {
        *self.settings.lock().unwrap()
    }

    pub fn file_share(&self) -> bool {
        self.get().file_share
    }

    pub fn clipboard_share(&self) -> bool {
        self.get().clipboard_share
    }

    /// 클립보드 토글 — file_share와 같은 0600 파일에 함께 영속된다.
    pub fn set_clipboard_share(&self, enabled: bool) -> Result<(), String> {
        let next = HostSettings {
            clipboard_share: enabled,
            ..self.get()
        };
        if let Some(path) = &self.path {
            persist(path, &next)?;
        }
        *self.settings.lock().unwrap() = next;
        Ok(())
    }

    /// 메모리 값을 바꾸고 디스크에 영속한다. 영속에 실패하면 오류를 반환하고
    /// 메모리 값은 바꾸지 않는다 — 호출자(UI)가 실패를 사용자에게 보여 준다.
    pub fn set_file_share(&self, enabled: bool) -> Result<(), String> {
        let next = HostSettings {
            file_share: enabled,
            ..self.get()
        };
        if let Some(path) = &self.path {
            persist(path, &next)?;
        }
        *self.settings.lock().unwrap() = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "leftcar-settings-{tag}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn missing_file_defaults_to_all_off() {
        let path = temp_path("missing");
        let settings = load_or_default(Some(&path));
        assert!(!settings.file_share);
        assert!(!settings.clipboard_share);
    }

    #[test]
    fn set_file_share_persists_and_survives_reload() {
        let path = temp_path("persist");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert!(!shared.file_share());
        shared.set_file_share(true).unwrap();
        assert!(shared.file_share());

        let reloaded = load_or_default(Some(&path));
        assert!(reloaded.file_share);
        assert!(!reloaded.clipboard_share);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "settings must be 0600");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_settings_reset_to_defaults() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert!(!shared.file_share());
        // 다시 쓰면 유효한 파일로 회복된다.
        shared.set_file_share(true).unwrap();
        assert!(load_or_default(Some(&path)).file_share);
        let _ = std::fs::remove_file(&path);
    }
}
