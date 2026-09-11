//! 호스트 설정 영속화(파일 공유 게이트 등). `data_dir/leftcar-host/settings.json`
//! (0600)에 저장하며, 없으면 기본값(모두 꺼짐)으로 만든다. 파일이 깨졌다면
//! 기본값으로 되돌린다 — 승인 토글은 안전 쪽으로 실패해야 한다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 승인 토글 성격의 호스트 설정. 모두 기본 꺼짐이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostSettings {
    pub clipboard_share: bool,
    pub file_share: bool,
    /// 마지막 스트림 세션이 끝나면 화면을 잠근다(docs/07 §잔여 — 잠금 on
    /// disconnect).
    pub lock_on_disconnect: bool,
    /// 커튼 모드: 스트리밍 중 호스트 물리 화면을 검은 오버레이로 가린다.
    pub privacy_curtain: bool,
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
        lock_on_disconnect: parsed
            .get("lockOnDisconnect")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        privacy_curtain: parsed
            .get("privacyCurtain")
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
        "lockOnDisconnect": settings.lock_on_disconnect,
        "privacyCurtain": settings.privacy_curtain,
    })
    .to_string();
    // 임시 파일에 쓰고 같은 디렉터리의 rename으로 갈아끼운다 — 대상 파일을
    // 곧바로 truncate하지 않으므로 동시 쓰기나 중간 크래시로도 settings.json이
    // 반쯤 잘린 상태로 남지 않는다. rename은 유닉스에서 같은 파일시스템 안에서
    // 원자적이고, Windows의 std::fs::rename도 기존 파일을 대상으로 덮어쓴다.
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .map_err(|e| format!("write: {e}"))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, body).map_err(|e| format!("write: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
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

    pub fn lock_on_disconnect(&self) -> bool {
        self.get().lock_on_disconnect
    }

    pub fn privacy_curtain(&self) -> bool {
        self.get().privacy_curtain
    }

    /// 메모리 값을 바꾸고 디스크에 영속한다. 영속에 실패하면 오류를 반환하고
    /// 메모리 값은 바꾸지 않는다 — 호출자(UI)가 실패를 사용자에게 보여 준다.
    fn update_field(&self, mutate: impl FnOnce(&mut HostSettings)) -> Result<(), String> {
        // 읽기→수정→영속→반영 전체를 잠금 하나에서 끝낸다. 잠금 사이에
        // 놓이면 두 세터가 같은 사본을 고쳐 나중 토글이 먼저 토글을 지워
        // 버린다(갱신 유실). 설정 변경은 드문 UI 동작이므로 파일 IO 동안
        // 잠금을 쥐고 있어도 충분하다. 실패한 토글이 메모리 값을 바꿔 놓으면
        // UI가 실패 값을 보여 주므로, 디스크 쓰기가 성공한 뒤에만 반영한다.
        let mut settings = self.settings.lock().unwrap();
        let mut next = *settings;
        mutate(&mut next);
        if let Some(path) = &self.path {
            persist(path, &next)?;
        }
        *settings = next;
        Ok(())
    }

    pub fn set_lock_on_disconnect(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.lock_on_disconnect = enabled)
    }

    pub fn set_privacy_curtain(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.privacy_curtain = enabled)
    }

    /// 클립보드 토글 — file_share와 같은 0600 파일에 함께 영속된다.
    pub fn set_clipboard_share(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.clipboard_share = enabled)
    }

    /// 메모리 값을 바꾸고 디스크에 영속한다. 영속에 실패하면 오류를 반환하고
    /// 메모리 값은 바꾸지 않는다 — 호출자(UI)가 실패를 사용자에게 보여 준다.
    pub fn set_file_share(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.file_share = enabled)
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
        assert!(!settings.lock_on_disconnect);
        assert!(!settings.privacy_curtain);
    }

    #[test]
    fn privacy_toggles_persist_and_survive_reload() {
        let path = temp_path("privacy");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        shared.set_lock_on_disconnect(true).unwrap();
        shared.set_privacy_curtain(true).unwrap();
        let reloaded = load_or_default(Some(&path));
        assert!(reloaded.lock_on_disconnect);
        assert!(reloaded.privacy_curtain);
        assert!(!reloaded.file_share, "independent fields must not leak");
        let _ = std::fs::remove_file(&path);
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

    #[test]
    fn persist_failure_leaves_memory_unchanged() {
        // persist 경로가 디렉터리면 create_dir_all은 통과해도 쓰기가 실패한다.
        let dir =
            std::env::temp_dir().join(format!("leftcar-settings-faildir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shared = SharedSettings::load_or_default(Some(dir.clone()));
        assert!(!shared.file_share());
        assert!(shared.set_file_share(true).is_err());
        assert!(
            !shared.file_share(),
            "failed persist must not flip the in-memory value the UI reads"
        );
        assert!(shared.set_clipboard_share(true).is_err());
        assert!(!shared.clipboard_share());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_setters_do_not_lose_updates_or_tear_the_file() {
        let path = temp_path("race");
        let shared = std::sync::Arc::new(SharedSettings::load_or_default(Some(path.clone())));
        let tmp = path.with_extension("json.tmp");
        let mut handles = Vec::new();
        for i in 0..16 {
            let shared = shared.clone();
            handles.push(std::thread::spawn(move || {
                let on = i % 2 == 0;
                // 네 세터를 모두 두드린다 — 어느 필드 하나 유실되면 안 된다.
                let _ = shared.set_lock_on_disconnect(on);
                let _ = shared.set_privacy_curtain(!on);
                let _ = shared.set_clipboard_share(on);
                let _ = shared.set_file_share(!on);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        // 갱신 유실이 없으려면 마지막으로 끄닥린 세터의 상태가 메모리와
        // 파일 양쪽에 그대로 있어야 한다(찢어진 JSON은 기본값으로 읽혀
        // 불일치로 잡힌다).
        let memory = shared.get();
        let file = load_or_default(Some(&path));
        assert_eq!(memory, file, "memory and file must agree after the race");
        assert!(
            !tmp.exists(),
            "a successful persist must leave no .tmp leftover"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_goes_through_a_tmp_rename_and_survives_a_stale_tmp() {
        let path = temp_path("tmp-rename");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        // 이전 크래시가 남긴 어금니 있는 .tmp가 있어도 동작에 영향이 없다.
        std::fs::write(path.with_extension("json.tmp"), b"stale").unwrap();
        shared.set_file_share(true).unwrap();
        assert!(load_or_default(Some(&path)).file_share);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the stale .tmp must be consumed by the next persist"
        );
        // 성공 경로에는 .tmp가 남지 않는다.
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }
}
