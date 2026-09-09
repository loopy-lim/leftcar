//! 세션 감사 로그: 누가·언제·무엇을 했는지를 JSONL로 남긴다.
//! `data_dir/leftcar-host/sessions.jsonl` (0600). 토큰·키는 절대 기록하지
//! 않는다 — 장치 식별자·IP·세션 번호·사유만이다.

use std::io::Write;
use std::path::PathBuf;

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

    pub fn log(&self, event: &str, fields: serde_json::Value) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
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
        #[cfg(unix)]
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| writeln!(f, "{line}"));
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
        audit.log("session_stopped", serde_json::json!({ "session": 3, "reason": "operator" }));
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
}
