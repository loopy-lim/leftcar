//! Workspace architecture rule engine (H01).
//!
//! Enforces the ADR-0002 dependency rules from docs/03 §4.1 by parsing
//! `cargo metadata --no-deps` and the crate Cargo.toml manifests.

use std::collections::BTreeMap;
use std::path::Path;

/// A workspace crate: name and its declared workspace-internal dependencies.
#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    /// workspace-internal deps (crate names, not paths)
    pub internal_deps: Vec<String>,
    /// external (non-workspace) deps
    pub external_deps: Vec<String>,
}

pub type Workspace = BTreeMap<String, CrateInfo>;

/// Parse `cargo metadata --no-deps --format-version 1` output.
pub fn parse_metadata(json: &str) -> Workspace {
    let v: serde_json::Value = serde_json::from_str(json).expect("valid cargo metadata json");
    let mut ws = Workspace::new();
    for pkg in v["packages"].as_array().expect("packages array") {
        let name = pkg["name"].as_str().expect("name").to_string();
        let mut internal_deps = Vec::new();
        let mut external_deps = Vec::new();
        // dependencies from manifest fields; cargo metadata "dependencies" includes
        // resolved names for path deps in a workspace only with --deps, so read
        // the manifest tables directly instead.
        let manifest_path = pkg["manifest_path"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if let Some(info) = read_manifest_deps(&manifest_path) {
            internal_deps = info.0;
            external_deps = info.1;
        }
        ws.insert(
            name.clone(),
            CrateInfo {
                name,
                internal_deps,
                external_deps,
            },
        );
    }
    ws
}

type Deps = (Vec<String>, Vec<String>);

fn read_manifest_deps(manifest_path: &str) -> Option<Deps> {
    let text = std::fs::read_to_string(Path::new(manifest_path)).ok()?;
    let v: toml::Value = toml::from_str(&text).ok()?;
    let deps = v.get("dependencies")?;
    let mut internal = Vec::new();
    let mut external = Vec::new();
    if let Some(obj) = deps.as_table() {
        for (dep_name, spec) in obj {
            let is_path = spec
                .as_table()
                .map(|t| t.contains_key("path"))
                .unwrap_or(false);
            if is_path {
                internal.push(dep_name.clone());
            } else if spec.as_str().is_some() || spec.as_table().is_some() {
                external.push(dep_name.clone());
            }
        }
    }
    Some((internal, external))
}

#[derive(Debug)]
pub struct Violation {
    pub rule: String,
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.rule, self.detail)
    }
}

/// External dependency allowlists per docs/03 §4.1 (domain <- ...; domain may
/// not depend on platform SDKs; core crates stay pure).
const DOMAIN_EXTERNAL_ALLOWLIST: &[&str] = &[
    "serde",
    "serde_json",
    "thiserror",
    "uuid",
    "bytes",
    "proptest",
];

const FORBIDDEN_PLATFORM_DEPS: &[&str] = &[
    "tauri",
    "wry",
    "winit",
    "objc2",
    "cocoa",
    "windows",
    "jni",
    "ndk",
    "android-activity",
    "screencapturekit",
    "objc",
    "core-video",
    "video-toolbox",
];

/// Run all workspace rules. Returns violations (empty = pass).
pub fn check_workspace(ws: &Workspace) -> Vec<Violation> {
    let mut out = Vec::new();

    // Layering: allowed internal dependency edges (ADR-0002).
    let allowed_edges: &[(&str, &[&str])] = &[
        ("domain", &[]),
        ("media-model", &["domain"]),
        ("network-protocol", &["domain"]),
        ("control-contract", &["domain", "media-model"]),
        ("session", &["domain", "media-model"]),
        ("transport-api", &["domain", "media-model"]),
        (
            "transport-quic",
            &["domain", "media-model", "transport-api"],
        ),
        (
            "host-core",
            &[
                "domain",
                "media-model",
                "network-protocol",
                "transport-api",
                "session",
            ],
        ),
        (
            "viewer-core",
            &["domain", "media-model", "transport-api", "session"],
        ),
        ("diagnostics", &["domain"]),
        ("macos-capture", &["domain", "media-model"]),
        ("macos-encode", &["domain", "media-model"]),
        (
            "android-viewer",
            &["domain", "viewer-core", "viewer-decoder", "libc"],
        ),
        ("leftcar-rustra", &["control-contract"]),
        ("viewer-decoder", &["media-model", "libc"]),
        ("architecture-check", &[]),
    ];

    for (crate_name, info) in ws {
        let Some((_, allowed)) = allowed_edges.iter().find(|(n, _)| n == crate_name) else {
            out.push(Violation {
                rule: "unknown-crate".into(),
                detail: format!("crate `{crate_name}` is not in the allowed layer table; update the rule list deliberately"),
            });
            continue;
        };
        for dep in &info.internal_deps {
            if !allowed.contains(&dep.as_str()) {
                out.push(Violation {
                    rule: "dependency-direction".into(),
                    detail: format!("{crate_name} -> {dep} is not allowed by ADR-0002 layering"),
                });
            }
        }
        if crate_name == "domain" {
            for dep in &info.external_deps {
                if !DOMAIN_EXTERNAL_ALLOWLIST.contains(&dep.as_str()) {
                    out.push(Violation {
                        rule: "domain-purity".into(),
                        detail: format!("domain depends on non-allowlisted external crate `{dep}`"),
                    });
                }
            }
        }
        // Video hot path: crates in the video plane must not depend on the
        // control contract, and the contract must not appear in media-model.
        if matches!(
            crate_name.as_str(),
            "media-model" | "transport-api" | "transport-quic"
        ) && info.internal_deps.iter().any(|d| d == "control-contract")
        {
            out.push(Violation {
                rule: "video-plane-has-no-control-contract".into(),
                detail: format!("{crate_name} must not depend on control-contract"),
            });
        }
        // No crate except platform facades and apps may touch platform SDKs.
        if !matches!(
            crate_name.as_str(),
            "macos-capture" | "macos-encode" | "control-contract"
        ) {
            for dep in &info.external_deps {
                if FORBIDDEN_PLATFORM_DEPS.contains(&dep.as_str()) {
                    out.push(Violation {
                        rule: "platform-dep-isolation".into(),
                        detail: format!(
                            "{crate_name} depends on platform crate `{dep}`; only facades may"
                        ),
                    });
                }
            }
        }
    }
    out
}
