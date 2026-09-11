//! Workspace architecture rule engine (H01).
//!
//! Enforces the ADR-0002 dependency rules from docs/03 §4.1 by parsing
//! `cargo metadata --no-deps` and the crate Cargo.toml manifests.

use std::collections::{BTreeMap, BTreeSet};
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

/// Locate the enclosing `[workspace.dependencies]` table for a member
/// manifest by walking up the directory tree to the owning `[workspace]`
/// manifest. `{ workspace = true }` specs are resolved against it.
fn workspace_dependencies(manifest_path: &str) -> Option<toml::Value> {
    let mut dir = Path::new(manifest_path).parent()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            if let Ok(v) = text.parse::<toml::Value>() {
                let ws_deps = v.get("workspace").and_then(|w| w.get("dependencies"));
                if ws_deps.is_some() {
                    return ws_deps.cloned();
                }
            }
        }
        dir = dir.parent()?;
    }
}

/// Classify one dependency spec as workspace-internal or external.
/// Internal means: a plain `{ path = ... }` spec, or `{ workspace = true }`
/// whose entry in the enclosing `[workspace.dependencies]` carries a path.
/// An unresolvable `{ workspace = true }` spec counts as external (it cannot
/// be a workspace-internal edge we can reason about).
fn classify_dep(
    dep_name: &str,
    spec: &toml::Value,
    ws_deps: Option<&toml::Value>,
    internal: &mut BTreeSet<String>,
    external: &mut BTreeSet<String>,
) {
    let spec_table = spec.as_table();
    let is_workspace_spec = spec_table
        .and_then(|t| t.get("workspace"))
        .and_then(|w| w.as_bool())
        .unwrap_or(false);
    if is_workspace_spec {
        let resolved = ws_deps.and_then(|d| d.get(dep_name));
        let has_path = resolved
            .and_then(|r| r.as_table())
            .map(|t| t.contains_key("path"))
            .unwrap_or(false);
        if has_path {
            internal.insert(dep_name.to_string());
        } else {
            external.insert(dep_name.to_string());
        }
        return;
    }
    if spec_table.map(|t| t.contains_key("path")).unwrap_or(false) {
        internal.insert(dep_name.to_string());
    } else if spec.as_str().is_some() || spec_table.is_some() {
        external.insert(dep_name.to_string());
    }
}

fn read_manifest_deps(manifest_path: &str) -> Option<Deps> {
    let text = std::fs::read_to_string(Path::new(manifest_path)).ok()?;
    let v: toml::Value = toml::from_str(&text).ok()?;
    let ws_deps = workspace_dependencies(manifest_path);
    // Declared edges come from [dependencies] and every
    // [target.'cfg(...)'.dependencies] table; [dev-dependencies] stays out.
    let mut dep_tables: Vec<&toml::Value> = Vec::new();
    if let Some(deps) = v.get("dependencies") {
        dep_tables.push(deps);
    }
    if let Some(targets) = v.get("target").and_then(|t| t.as_table()) {
        for target in targets.values() {
            if let Some(deps) = target.get("dependencies") {
                dep_tables.push(deps);
            }
        }
    }
    let mut internal = BTreeSet::new();
    let mut external = BTreeSet::new();
    for deps in dep_tables {
        let Some(obj) = deps.as_table() else {
            continue;
        };
        for (dep_name, spec) in obj {
            classify_dep(
                dep_name,
                spec,
                ws_deps.as_ref(),
                &mut internal,
                &mut external,
            );
        }
    }
    Some((
        internal.into_iter().collect(),
        external.into_iter().collect(),
    ))
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
        // Shared protocol/codec primitives stay dependency-free so both
        // desktop and Android facades can use the same bounded hot path.
        ("fec-core", &[]),
        ("usb-mux", &[]),
        // 세션 암호 프리미티브 — fec-core와 같은 무의존 하위 계층.
        ("secure-channel", &[]),
        ("media-model", &["domain"]),
        ("control-contract", &["domain", "media-model"]),
        ("session", &["domain"]),
        ("host-core", &["domain", "media-model"]),
        ("viewer-core", &["domain", "media-model"]),
        (
            "android-viewer",
            &[
                "domain",
                "viewer-core",
                "viewer-decoder",
                "fec-core",
                "usb-mux",
                // 미디어 경로 AEAD 봉인 — fec-core와 같은 무의존 하위 계층.
                "secure-channel",
                "libc",
            ],
        ),
        ("leftcar-rustra", &["control-contract"]),
        ("viewer-decoder", &["libc"]),
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
        if crate_name == "media-model" && info.internal_deps.iter().any(|d| d == "control-contract")
        {
            out.push(Violation {
                rule: "video-plane-has-no-control-contract".into(),
                detail: format!("{crate_name} must not depend on control-contract"),
            });
        }
        // No crate except platform facades and apps may touch platform SDKs.
        if crate_name != "control-contract" {
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
