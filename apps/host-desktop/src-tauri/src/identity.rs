//! 호스트 정체 키(Ed25519) 영속화. 최초 기동에서 생성되어
//! `data_dir/leftcar-host/host_identity.json`(0600)에 시드 hex로 저장된다.
//! 공개키는 QR(`k` 필드)과 핸드셰이크 서명으로 뷰어에 핀된다 — 이 파일이
//! 사라지면 페어링된 모든 뷰어가 핀 대조에 실패하므로 재페어링이 필요하다.

use secure_channel::HostIdentity;
use std::path::{Path, PathBuf};

/// `dirs::data_dir()/leftcar-host/host_identity.json` (None when the platform
/// has no data dir — the identity then stays in-memory per process).
pub fn default_identity_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("leftcar-host").join("host_identity.json"))
}

/// Load the persisted seed, or create and persist a fresh identity. A corrupt
/// or unreadable file is replaced (logged) — refusing to start would leave
/// the host unreachable with no recovery path except manual file surgery.
pub fn load_or_create(path: Option<&Path>) -> HostIdentity {
    let Some(path) = path else {
        return HostIdentity::generate();
    };
    if let Some(identity) = load(path) {
        return identity;
    }
    let identity = HostIdentity::generate();
    if let Err(e) = persist(path, &identity) {
        eprintln!("leftcar: host identity persistence failed: {e}");
    }
    identity
}

fn load(path: &Path) -> Option<HostIdentity> {
    let body = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&body).ok()?;
    let seed_hex = parsed.get("seed")?.as_str()?;
    if seed_hex.len() != 64 {
        eprintln!("leftcar: host identity seed has wrong length, regenerating");
        return None;
    }
    let mut seed = [0u8; 32];
    for (i, chunk) in seed_hex.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)? as u8;
        let lo = (chunk[1] as char).to_digit(16)? as u8;
        seed[i] = (hi << 4) | lo;
    }
    Some(HostIdentity::from_seed(seed))
}

fn persist(path: &Path, identity: &HostIdentity) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let seed = identity.seed();
    let seed_hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    let body = serde_json::json!({ "v": 1, "seed": seed_hex }).to_string();
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
        p.push(format!("leftcar-identity-{tag}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn identity_survives_restart_and_stays_constant() {
        let path = temp_path("restart");
        let first = load_or_create(Some(&path));
        let restarted = load_or_create(Some(&path));
        assert_eq!(first.public_key(), restarted.public_key());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_identity_is_regenerated_not_fatal() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        let identity = load_or_create(Some(&path));
        // 재생성된 정체는 유효한 32B 공개키를 갖고 파일도 갱신된다.
        assert_eq!(identity.public_key().len(), 32);
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persisted_file_has_restricted_permissions() {
        let path = temp_path("perms");
        let _ = load_or_create(Some(&path));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "identity must be 0600");
        }
        let _ = std::fs::remove_file(&path);
    }
}
