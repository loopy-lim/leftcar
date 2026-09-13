//! 세션 감사 로그: 허용한 진단 이벤트와 제한된 타입 필드만 JSONL로 남긴다.
//! `data_dir/leftcar-host/sessions.jsonl`과 한 세대 백업은 Unix에서 0600이다.
//! 장치는 프로세스 동안만 안정적인 가명으로 기록하고 원문 식별자, IP,
//! 화면 제목, 파일명, 토큰 및 키는 기록하지 않는다.

use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 감사 로그 회전 크기. 넘으면 현재 파일을 `.jsonl.1`으로 밀어내고 새로
/// 시작한다(백업 한 세대만 유지 — 감사 로그는 운영 진단용이다).
const ROTATE_BYTES: u64 = 5 * 1024 * 1024;
/// 고정 스키마가 실수로 커져도 한 레코드가 로그 크기를 독점하지 못한다.
const MAX_RECORD_BYTES: usize = 1024;
/// 입력 재할당은 상세 세션 ID를 제한하고 원래 개수는 별도 숫자로 남긴다.
const MAX_FROM_SESSION_IDS: usize = 32;

static PROCESS_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static PROCESS_DEVICE_KEY: OnceLock<[u8; 16]> = OnceLock::new();

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

    fn rotate_if_large(path: &std::path::Path) -> io::Result<()> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata.len() <= ROTATE_BYTES {
            return Ok(());
        }
        let backup = path.with_extension("jsonl.1");
        match std::fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::rename(path, &backup)?;
        secure_existing_file(&backup)
    }

    pub fn log(&self, event: &str, fields: serde_json::Value) {
        let Some(mut record) = sanitized_fields(event, &fields) else {
            return;
        };
        let Some(path) = &self.path else { return };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        record.insert("ts".into(), serde_json::json!(format!("unix:{ts}")));
        record.insert("event".into(), serde_json::json!(event));
        let line = serde_json::Value::Object(record).to_string();
        if line.len() > MAX_RECORD_BYTES {
            eprintln!("leftcar: audit record exceeded the safe size cap");
            return;
        }

        let _guard = PROCESS_IO_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = append_secure(path, line.as_bytes()) {
            // 감사 실패는 스트림을 죽이지 않는다. 보안 권한이나 회전을
            // 보장할 수 없으면 기존 파일에 우회 append하지 않는다.
            eprintln!("leftcar: audit write skipped: {error}");
        }
    }
}

fn append_secure(path: &std::path::Path, line: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    secure_existing_file(path)?;
    let backup = path.with_extension("jsonl.1");
    secure_existing_file(&backup)?;
    SessionAudit::rotate_if_large(path)?;

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    secure_open_file(&file)?;
    file.write_all(line)?;
    file.write_all(b"\n")
}

#[cfg(unix)]
fn secure_open_file(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    let mode = file.metadata()?.permissions().mode() & 0o777;
    if mode == 0o600 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("audit file mode is {mode:o}, expected 600"),
        ))
    }
}

#[cfg(not(unix))]
fn secure_open_file(_file: &std::fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_existing_file(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let repaired = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if repaired == 0o600 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("audit file mode is {repaired:o}, expected 600"),
        ))
    }
}

#[cfg(not(unix))]
fn secure_existing_file(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}

fn sanitized_fields(
    event: &str,
    fields: &serde_json::Value,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let source = fields.as_object()?;
    let mut safe = serde_json::Map::new();
    match event {
        "lock_on_disconnect_changed" | "privacy_curtain_changed" | "file_share_changed" => {
            copy_bool(source, &mut safe, "enabled");
        }
        "privacy_curtain" => copy_bool(source, &mut safe, "shown"),
        "screen_locked" => copy_enum(source, &mut safe, "reason", &["last_session_ended"]),
        "session_started" => {
            copy_u32(source, &mut safe, "session");
            copy_device(source, &mut safe);
            copy_enum(source, &mut safe, "transport", &["udp", "tcp", "usb"]);
        }
        "session_stopped" => {
            copy_u32(source, &mut safe, "session");
            copy_device(source, &mut safe);
            copy_stop_reason(source, &mut safe);
        }
        "device_revoked" => {
            copy_device(source, &mut safe);
            copy_u64(source, &mut safe, "stopped_sessions");
        }
        "devices_revoked_all" => copy_u64(source, &mut safe, "devices"),
        "input_reassigned" => {
            copy_session_list(source, &mut safe);
            copy_u32(source, &mut safe, "to");
        }
        "file_send_begin" => {
            copy_device(source, &mut safe);
            copy_u64(source, &mut safe, "size");
        }
        "file_received" => {
            copy_device(source, &mut safe);
            copy_u64(source, &mut safe, "bytes");
        }
        "file_fetched" => copy_device(source, &mut safe),
        "clipboard_write" | "clipboard_read" => {
            copy_device(source, &mut safe);
            copy_u64(source, &mut safe, "bytes");
            copy_enum(source, &mut safe, "kind", &["image"]);
        }
        _ => return None,
    }
    Some(safe)
}

fn copy_bool(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) {
    if let Some(value) = source.get(key).and_then(serde_json::Value::as_bool) {
        safe.insert(key.into(), serde_json::json!(value));
    }
}

fn copy_u32(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) {
    if let Some(value) = source
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        safe.insert(key.into(), serde_json::json!(value));
    }
}

fn copy_u64(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) {
    if let Some(value) = source.get(key).and_then(serde_json::Value::as_u64) {
        safe.insert(key.into(), serde_json::json!(value));
    }
}

fn copy_enum(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    allowed: &[&str],
) {
    if let Some(value) = source.get(key).and_then(serde_json::Value::as_str) {
        if allowed.contains(&value) {
            safe.insert(key.into(), serde_json::json!(value));
        }
    }
}

fn copy_stop_reason(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(reason) = source.get("reason") else {
        return;
    };
    if let Some(code) = reason.as_u64().and_then(|value| u8::try_from(value).ok()) {
        safe.insert("reason".into(), serde_json::json!(code));
        return;
    }
    copy_enum(
        source,
        safe,
        "reason",
        &["device_revoked", "devices_revoked_all", "operator_forced"],
    );
}

fn copy_session_list(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(values) = source.get("from").and_then(serde_json::Value::as_array) else {
        return;
    };
    let sessions = values
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .filter_map(|value| u32::try_from(value).ok())
        .take(MAX_FROM_SESSION_IDS)
        .collect::<Vec<_>>();
    safe.insert("from".into(), serde_json::json!(sessions));
    safe.insert("from_count".into(), serde_json::json!(values.len()));
}

fn copy_device(
    source: &serde_json::Map<String, serde_json::Value>,
    safe: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(device) = source.get("device").and_then(serde_json::Value::as_str) else {
        return;
    };
    if device.is_empty() || device == "unknown" {
        return;
    }
    safe.insert("device".into(), serde_json::json!(device_pseudonym(device)));
}

fn device_pseudonym(device: &str) -> String {
    let key = PROCESS_DEVICE_KEY.get_or_init(|| *uuid::Uuid::new_v4().as_bytes());
    let mut digest = Sha256::new();
    digest.update(b"leftcar-audit-device-v1\0");
    digest.update(key);
    digest.update((device.len() as u64).to_be_bytes());
    digest.update(device.as_bytes());
    let bytes = digest.finalize();
    let mut result = String::with_capacity(4 + 24);
    result.push_str("dev:");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes.iter().take(12) {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_records(path: &std::path::Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "leftcar-audit-{tag}-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn session_start_keeps_typed_diagnostics_without_raw_identifiers() {
        let path = temp_path("sanitized-session");
        let audit = SessionAudit::new(Some(path.clone()));
        audit.log(
            "session_started",
            serde_json::json!({
                "session": 3,
                "device": "viewer-secret-id",
                "transport": "udp",
                "viewer": "192.168.0.55",
                "source": "Payroll Q4 — secret window",
                "name": "customer-list.csv",
                "path": "/Users/alice/Secrets/customer-list.csv",
                "token": "bearer-secret",
                "key": "media-secret",
            }),
        );
        let records = read_records(&path);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["event"], "session_started");
        assert_eq!(record["session"], 3);
        assert_eq!(record["transport"], "udp");
        assert!(record["ts"].as_str().unwrap().starts_with("unix:"));
        let pseudonym = record["device"].as_str().unwrap();
        assert!(pseudonym.starts_with("dev:"));
        assert_ne!(pseudonym, "viewer-secret-id");
        assert_eq!(record.as_object().unwrap().len(), 5);
        let serialized = record.to_string();
        for secret in [
            "viewer-secret-id",
            "192.168.0.55",
            "Payroll Q4",
            "customer-list.csv",
            "bearer-secret",
            "media-secret",
        ] {
            assert!(!serialized.contains(secret), "leaked {secret}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_events_are_dropped_and_reserved_keys_cannot_override_envelope() {
        let path = temp_path("allowlist");
        let audit = SessionAudit::new(Some(path.clone()));
        audit.log(
            "attacker_event",
            serde_json::json!({ "session": 99, "token": "secret" }),
        );
        assert!(!path.exists(), "unknown events must not create a log");

        audit.log(
            "session_stopped",
            serde_json::json!({
                "event": "attacker_event",
                "ts": "forged",
                "session": 7,
                "reason": "operator_forced",
                "unexpected": { "token": "nested-secret" },
            }),
        );
        let records = read_records(&path);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["event"], "session_stopped");
        assert_ne!(record["ts"], "forged");
        assert_eq!(record["session"], 7);
        assert_eq!(record["reason"], "operator_forced");
        assert!(record.get("unexpected").is_none());
        assert!(!record.to_string().contains("nested-secret"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn typed_fields_reject_wrong_types_unknown_enums_and_bound_session_lists() {
        let path = temp_path("typed");
        let audit = SessionAudit::new(Some(path.clone()));
        audit.log(
            "session_started",
            serde_json::json!({
                "session": "7",
                "device": ["raw-device"],
                "transport": "super-secret-relay",
            }),
        );
        audit.log(
            "session_stopped",
            serde_json::json!({ "session": 8, "reason": "token=secret-value" }),
        );
        audit.log(
            "input_reassigned",
            serde_json::json!({
                "from": (0..100).collect::<Vec<u32>>(),
                "to": 101,
            }),
        );
        audit.log(
            "device_revoked",
            serde_json::json!({ "device": "viewer-a", "stopped_sessions": 4 }),
        );

        let records = read_records(&path);
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].as_object().unwrap().len(), 2);
        assert_eq!(records[1]["session"], 8);
        assert!(records[1].get("reason").is_none());
        assert_eq!(records[2]["from"].as_array().unwrap().len(), 32);
        assert_eq!(records[2]["from_count"], 100);
        assert_eq!(records[2]["to"], 101);
        assert_eq!(records[3]["stopped_sessions"], 4);
        let body = std::fs::read_to_string(&path).unwrap();
        for rejected in ["raw-device", "super-secret-relay", "secret-value"] {
            assert!(!body.contains(rejected), "leaked {rejected}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn device_pseudonym_is_stable_across_instances_in_one_process() {
        let first_path = temp_path("stable-first");
        let second_path = temp_path("stable-second");
        SessionAudit::new(Some(first_path.clone())).log(
            "device_revoked",
            serde_json::json!({ "device": "viewer-stable", "stopped_sessions": 1 }),
        );
        SessionAudit::new(Some(second_path.clone())).log(
            "clipboard_read",
            serde_json::json!({ "device": "viewer-stable", "bytes": 12 }),
        );
        let first = read_records(&first_path);
        let second = read_records(&second_path);
        assert_eq!(first[0]["device"], second[0]["device"]);
        assert_ne!(first[0]["device"], "viewer-stable");
        let _ = std::fs::remove_file(first_path);
        let _ = std::fs::remove_file(second_path);
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
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            let backup_mode = std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "the post-rotation file must be 0600, got {:o}",
                mode
            );
            assert_eq!(backup_mode, 0o600, "the rotated file must be 0600");
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

    #[cfg(unix)]
    #[test]
    fn existing_active_and_legacy_backup_permissions_are_repaired_before_append() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("legacy-perm");
        let backup = path.with_extension("jsonl.1");
        std::fs::write(&path, "").unwrap();
        std::fs::write(&backup, "legacy\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o666)).unwrap();

        SessionAudit::new(Some(path.clone()))
            .log("devices_revoked_all", serde_json::json!({ "devices": 2 }));

        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_records(&path).len(), 1);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup);
    }

    #[test]
    fn rotation_failure_drops_the_record_instead_of_appending_insecurely() {
        let path = temp_path("rotation-failure");
        let backup = path.with_extension("jsonl.1");
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        std::fs::create_dir(&backup).unwrap();
        SessionAudit::new(Some(path.clone())).log(
            "session_stopped",
            serde_json::json!({ "session": 9, "reason": "operator_forced" }),
        );
        assert_eq!(std::fs::metadata(&path).unwrap().len(), ROTATE_BYTES + 1);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(backup);
    }

    #[test]
    fn concurrent_instances_serialize_rotation_and_keep_every_record() {
        let path = temp_path("concurrent");
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        let mut threads = Vec::new();
        for session in 0..64_u32 {
            let path = path.clone();
            threads.push(std::thread::spawn(move || {
                SessionAudit::new(Some(path)).log(
                    "session_started",
                    serde_json::json!({
                        "session": session,
                        "device": "concurrent-device",
                        "transport": "tcp",
                    }),
                );
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let records = read_records(&path);
        assert_eq!(records.len(), 64);
        let mut sessions = records
            .iter()
            .map(|record| record["session"].as_u64().unwrap())
            .collect::<Vec<_>>();
        sessions.sort_unstable();
        assert_eq!(sessions, (0..64).collect::<Vec<_>>());
        let backup = path.with_extension("jsonl.1");
        assert_eq!(std::fs::metadata(&backup).unwrap().len(), ROTATE_BYTES + 1);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(backup);
    }
}
