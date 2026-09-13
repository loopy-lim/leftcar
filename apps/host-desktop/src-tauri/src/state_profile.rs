//! Explicit internal benchmark state isolation. No environment changes or I/O
//! to credentials. Call once before platform initialization and pass the result.
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct StateProfile {
    directory: Option<PathBuf>,
    credential_service: String,
    benchmark: bool,
}
impl StateProfile {
    pub fn resolve(
        slug: Option<&str>,
        root: Option<&Path>,
        normal: Option<&Path>,
    ) -> Result<Self, String> {
        match (slug, root) {
            (None, None) => Ok(Self {
                directory: normal.map(|p| p.join("leftcar-host")),
                credential_service: "leftcar-host".into(),
                benchmark: false,
            }),
            (Some(slug), Some(root)) => {
                if slug.is_empty()
                    || slug.len() > 32
                    || !slug.as_bytes()[0].is_ascii_lowercase()
                    || !slug
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                {
                    return Err("LEFTCAR_BENCHMARK_PROFILE must match [a-z][a-z0-9-]{0,31}".into());
                }
                if !root.is_absolute()
                    || root
                        .components()
                        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
                {
                    return Err("LEFTCAR_BENCHMARK_ROOT must be an absolute dedicated directory without dot components".into());
                }
                let root = root
                    .canonicalize()
                    .map_err(|e| format!("benchmark root must already exist: {e}"))?;
                if !root.is_dir() {
                    return Err("benchmark root is not a directory".into());
                }
                if let Some(normal) = normal {
                    let normal = normal.join("leftcar-host");
                    let normal = normal.canonicalize().unwrap_or(normal);
                    if root.starts_with(&normal) {
                        return Err("benchmark root overlaps normal Host state".into());
                    }
                }
                let directory = root.join(format!("host-{slug}"));
                if let Ok(meta) = std::fs::symlink_metadata(&directory) {
                    if meta.file_type().is_symlink() || !meta.is_dir() {
                        return Err(
                            "benchmark child must be a real directory, not a symlink or file"
                                .into(),
                        );
                    }
                    // Existing per-file aliases must not reach normal identity/state.
                    for name in [
                        "source_grants.json",
                        ".source-grants.lock",
                        "settings.json",
                        "host_identity.json",
                        "paired_devices.json",
                        "sessions.jsonl",
                        "paired_devices.tokens",
                    ] {
                        if let Ok(meta) = std::fs::symlink_metadata(directory.join(name)) {
                            if !meta.is_file() || meta.file_type().is_symlink() {
                                return Err(format!("invalid benchmark state file: {name}"));
                            }
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::MetadataExt;
                                if meta.nlink() != 1 {
                                    return Err(format!("linked benchmark state file: {name}"));
                                }
                            }
                        }
                    }
                }
                Ok(Self {
                    directory: Some(directory),
                    credential_service: format!("leftcar-host.benchmark.{slug}"),
                    benchmark: true,
                })
            }
            _ => Err(
                "LEFTCAR_BENCHMARK_PROFILE and LEFTCAR_BENCHMARK_ROOT must be supplied together"
                    .into(),
            ),
        }
    }
    pub fn from_environment(normal: Option<&Path>) -> Result<Self, String> {
        let slug = std::env::var("LEFTCAR_BENCHMARK_PROFILE");
        let root = std::env::var_os("LEFTCAR_BENCHMARK_ROOT");
        let slug = match slug {
            Ok(s) => Some(s),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err("benchmark profile is not UTF-8".into()),
        };
        if let Some(audio) = std::env::var_os("LEFTCAR_BENCHMARK_SYSTEM_AUDIO") {
            if audio != "off" || slug.is_none() {
                return Err(
                    "System audio restriction requires an explicit benchmark profile and value off"
                        .into(),
                );
            }
        }
        Self::validate_build_profile(
            option_env!("LEFTCAR_BENCHMARK_BUILD_PROFILE"),
            slug.as_deref(),
        )?;
        Self::resolve(slug.as_deref(), root.as_deref().map(Path::new), normal)
    }

    fn validate_build_profile(built: Option<&str>, configured: Option<&str>) -> Result<(), String> {
        if let Some(built) = built {
            if Some(built) != configured {
                return Err("This benchmark Host requires its matching LEFTCAR_BENCHMARK_PROFILE and dedicated LEFTCAR_BENCHMARK_ROOT at launch".into());
            }
        }
        Ok(())
    }
    pub fn file(&self, name: &str) -> Option<PathBuf> {
        self.directory.as_ref().map(|d| d.join(name))
    }
    pub fn credential_service(&self) -> &str {
        &self.credential_service
    }
    pub fn is_benchmark(&self) -> bool {
        self.benchmark
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let p = std::env::temp_dir().join(format!("leftcar-profile-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn benchmark_bundle_without_matching_runtime_profile_fails_closed() {
        assert!(StateProfile::validate_build_profile(Some("baseline068"), None).is_err());
        assert!(
            StateProfile::validate_build_profile(Some("baseline068"), Some("candidate")).is_err()
        );
        assert!(
            StateProfile::validate_build_profile(Some("baseline068"), Some("baseline068")).is_ok()
        );
        assert!(StateProfile::validate_build_profile(None, None).is_ok());
    }
    #[test]
    fn default_paths_and_credentials_are_compatible() {
        let p = StateProfile::resolve(None, None, Some(Path::new("/normal"))).unwrap();
        assert_eq!(
            p.file("settings.json").unwrap(),
            Path::new("/normal/leftcar-host/settings.json")
        );
        assert_eq!(p.credential_service(), "leftcar-host");
        assert!(!p.is_benchmark());
    }
    #[test]
    fn all_state_and_credentials_use_the_same_isolated_profile() {
        let root = fixture();
        let p = StateProfile::resolve(Some("baseline068"), Some(&root), Some(Path::new("/normal")))
            .unwrap();
        for file in [
            "settings.json",
            "host_identity.json",
            "paired_devices.json",
            "sessions.jsonl",
        ] {
            assert_eq!(
                p.file(file).unwrap(),
                root.canonicalize()
                    .unwrap()
                    .join("host-baseline068")
                    .join(file)
            );
        }
        assert_eq!(p.credential_service(), "leftcar-host.benchmark.baseline068");
        assert!(p.is_benchmark());
    }
    #[test]
    fn incomplete_or_invalid_explicit_profile_never_falls_back() {
        let root = fixture();
        for slug in ["", "../prod", "Prod", "a.b", "-bad", "a b"] {
            assert!(
                StateProfile::resolve(Some(slug), Some(&root), None).is_err(),
                "{slug}"
            );
        }
        assert!(StateProfile::resolve(Some("test"), None, None).is_err());
        assert!(StateProfile::resolve(None, Some(&root), None).is_err());
        assert!(StateProfile::resolve(Some("test"), Some(Path::new("relative")), None).is_err());
    }
    #[test]
    fn normal_state_or_symlink_child_cannot_be_reused() {
        let root = fixture();
        #[cfg(unix)]
        {
            let child = root.join("host-symlink");
            let _ = std::fs::remove_file(&child);
            std::os::unix::fs::symlink(&root, &child).unwrap();
            assert!(StateProfile::resolve(Some("symlink"), Some(&root), None).is_err());
            std::fs::remove_file(child).unwrap();
        }
        let normal = root.join("leftcar-host");
        std::fs::create_dir_all(&normal).unwrap();
        assert!(StateProfile::resolve(Some("test"), Some(&normal), Some(&root)).is_err());
    }
}
