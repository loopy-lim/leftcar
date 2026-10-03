//! 호스트 정체 키(Ed25519) 영속화. 최초 기동에서 생성되어
//! `data_dir/leftcar-host/host_identity.json`(0600)에 시드 hex로 저장된다.
//! 공개키는 QR(`k` 필드)과 핸드셰이크 서명으로 뷰어에 핀된다 — 이 파일이
//! 사라지면 페어링된 모든 뷰어가 핀 대조에 실패하므로 재페어링이 필요하다.

use secure_channel::HostIdentity;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// `dirs::data_dir()/leftcar-host/host_identity.json` (None when the platform
/// has no data dir).
pub fn default_identity_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("leftcar-host").join("host_identity.json"))
}

/// Create only when no identity exists. Invalid or inaccessible state must
/// remain intact: replacing it silently invalidates every Viewer's pinned key.
pub fn load_or_create(path: Option<&Path>) -> Result<HostIdentity, String> {
    let path = path.ok_or("Host identity directory unavailable")?;
    if let Some(identity) = load(path)? {
        return Ok(identity);
    }
    let identity = HostIdentity::generate();
    persist(path, &identity)?;
    Ok(identity)
}

fn load(path: &Path) -> Result<Option<HostIdentity>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Host identity cannot be read: {error}")),
    };
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err("Host identity is not a valid seed file; existing state was preserved".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Host identity cannot be read: {error}"))?;
    let mut body = String::new();
    (&mut file)
        .take(4097)
        .read_to_string(&mut body)
        .map_err(|error| format!("Host identity cannot be read: {error}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| "Host identity is corrupt; existing state was preserved".to_string())?;
    let seed_hex = parsed
        .get("seed")
        .and_then(|value| value.as_str())
        .ok_or("Host identity has no valid seed; existing state was preserved")?;
    if body.len() > 4096
        || parsed.get("v").and_then(|value| value.as_u64()) != Some(1)
        || seed_hex.len() != 64
    {
        return Err("Host identity has an unsupported format; existing state was preserved".into());
    }
    let mut seed = [0u8; 32];
    for (i, chunk) in seed_hex.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char)
            .to_digit(16)
            .ok_or("Host identity seed is invalid; existing state was preserved")?
            as u8;
        let lo = (chunk[1] as char)
            .to_digit(16)
            .ok_or("Host identity seed is invalid; existing state was preserved")?
            as u8;
        seed[i] = (hi << 4) | lo;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Host identity permissions cannot be restricted: {error}"))?;
    }
    Ok(Some(HostIdentity::from_seed(seed)))
}

fn persist(path: &Path, identity: &HostIdentity) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let seed = identity.seed();
    let seed_hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    let body = serde_json::json!({ "v": 1, "seed": seed_hex }).to_string();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("create Host identity: {e}"))?;
    let result = file
        .write_all(body.as_bytes())
        .and_then(|()| file.sync_all());
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result.map_err(|e| format!("persist Host identity: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "leftcar-identity-{tag}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn identity_survives_restart_and_stays_constant() {
        let path = temp_path("restart");
        let first = load_or_create(Some(&path)).unwrap();
        let restarted = load_or_create(Some(&path)).unwrap();
        assert_eq!(first.public_key(), restarted.public_key());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_identity_is_preserved_for_explicit_recovery() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_or_create(Some(&path)).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"{not json",
            "unreadable identity must not silently rotate the pinned host key"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_seed_has_its_permissions_restricted_without_rotation() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("existing-perms");
        let first = load_or_create(Some(&path)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let reopened = load_or_create(Some(&path)).unwrap();
        assert_eq!(first.public_key(), reopened.public_key());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persisted_file_has_restricted_permissions() {
        let path = temp_path("perms");
        load_or_create(Some(&path)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "identity must be 0600");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unsupported_identity_is_preserved_and_never_used() {
        let path = temp_path("unsupported");
        let body = serde_json::json!({ "v": 2, "seed": "ab".repeat(32) }).to_string();
        std::fs::write(&path, &body).unwrap();
        assert!(load_or_create(Some(&path)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_persistence_never_returns_an_ephemeral_identity() {
        let path = temp_path("invalid-parent");
        std::fs::write(&path, b"parent is a file").unwrap();
        assert!(load_or_create(Some(&path.join("identity.json"))).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"parent is a file");
        let _ = std::fs::remove_file(&path);
    }
}
