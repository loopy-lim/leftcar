//! 세션 감사 로그: 누가·언제·무엇을 했는지를 JSONL로 남긴다.
//! `data_dir/leftcar-host/sessions.jsonl` (0600). 토큰·키는 절대 기록하지
//! 않는다 — 장치 식별자·IP·세션 번호·사유만이다.

use std::io::Write;
use std::path::PathBuf;

/// 감사 로그 회전 크기. 넘으면 현재 파일을 `.jsonl.1`으로 밀어내고 새로
/// 시작한다(백업 한 세대만 유지 — 감사 로그는 운영 진단용이다).
const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

pub struct SessionAudit {
    path: Option<PathBuf>,
}

impl SessionAudit {
    /// `dirs::data_dir()/leftcar-host/sessions.jsonl` (None when the platform
    /// has no data dir — auditing then stays in-memory no-op).
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("leftcar-host").join("sessions.jsonl"))
    }

    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// 상한 초과 시 한 세대 백업으로 회전한다. 실패는 무시한다 — 회전 실패가
    /// 감사 기록 자체를 막아서는 안 된다.
    fn rotate_if_large(path: &std::path::Path) {
        let Ok(metadata) = std::fs::metadata(path) else {
            return;
        };
        if metadata.len() <= ROTATE_BYTES {
            return;
        }
        let _ = std::fs::rename(path, path.with_extension("jsonl.1"));
    }

    pub fn log(&self, event: &str, fields: serde_json::Value) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self::rotate_if_large(path);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut record = serde_json::Map::new();
        record.insert("ts".into(), serde_json::json!(format!("unix:{ts}")));
        record.insert("event".into(), serde_json::json!(event));
        if let serde_json::Value::Object(map) = fields {
            for (key, value) in map {
                record.insert(key, value);
            }
        }
        let line = serde_json::Value::Object(record).to_string();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(path) {
                let mode = metadata.permissions().mode() & 0o777;
                if mode != 0o600 {
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
                }
            }
        }
        // mode는 생성 시에만 적용된다 — 새 파일이 0644로 태어나 다음 log에서
        // 고쳐지는 빈틈을 막는다. 기존 레거시 파일의 권한 수리는 위에서 그대로
        // 한다.
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)
                .and_then(|mut f| writeln!(f, "{line}"))
        };
        #[cfg(not(unix))]
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = result {
            // 감사 실패는 스트림을 죽이지 않는다 — 콘솔로만 흘린다.
            eprintln!("leftcar: audit write failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("leftcar-audit-{tag}-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn audit_lines_are_jsonl_with_ts_and_event() {
        let path = temp_path("jsonl");
        let audit = SessionAudit::new(Some(path.clone()));
        audit.log(
            "session_started",
            serde_json::json!({ "device": "viewer-1", "session": 3 }),
        );
        audit.log(
            "session_stopped",
            serde_json::json!({ "session": 3, "reason": "operator" }),
        );
        let body = std::fs::read_to_string(&path).unwrap();
        let mut lines = body.lines();
        let first: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(first["event"], "session_started");
        assert_eq!(first["device"], "viewer-1");
        assert!(first["ts"].as_str().unwrap().starts_with("unix:"));
        let second: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(second["reason"], "operator");
        assert!(lines.next().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn oversized_log_rotates_to_one_backup_generation() {
        let path = temp_path("rotate");
        std::fs::write(&path, vec![b'x'; 16]).unwrap();
        // 크기 상한보다 큰 파일을 심는다.
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        let audit = SessionAudit::new(Some(path.clone()));
        audit.log("session_started", serde_json::json!({ "device": "d" }));
        let backup = path.with_extension("jsonl.1");
        assert!(backup.exists(), "the old generation must move to .jsonl.1");
        assert_eq!(std::fs::metadata(&backup).unwrap().len(), ROTATE_BYTES + 1);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("session_started"),
            "the new file starts fresh"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the post-rotation file must be owner-only, got {:o}",
                mode
            );
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup);
    }

    #[cfg(unix)]
    #[test]
    fn first_log_creates_the_file_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("first-perm");
        let audit = SessionAudit::new(Some(path.clone()));
        // 첫 호출에서 곧바로 권한을 단정한다 — 파일이 이미 있을 때만 권한을
        // 고치던 구조에서는 이 경계가 0644로 태어났다.
        audit.log("session_started", serde_json::json!({ "device": "d" }));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "a fresh audit file must not be group/world accessible, got {:o}",
            mode
        );
        let _ = std::fs::remove_file(&path);
    }
}
