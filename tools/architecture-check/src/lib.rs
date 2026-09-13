//! Resolved Cargo dependency rules for the root workspace and standalone Host.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub internal_deps: Vec<String>,
    pub external_deps: Vec<String>,
}
pub type Workspace = BTreeMap<String, CrateInfo>;

/// Requires full, unfiltered, locked Cargo metadata. Package IDs resolve aliases,
/// inherited dependencies and target edges. Normal AND build edges are checked;
/// dev-only edges are excluded from production layering. Every local package is
/// checked, including path dependencies outside a standalone app's membership.
/// Invalid/incomplete metadata is fatal rather than an empty successful graph.
pub fn parse_metadata(json: &str) -> Workspace {
    let v: serde_json::Value = serde_json::from_str(json).expect("valid cargo metadata json");
    let packages = v["packages"].as_array().expect("packages array");
    let nodes = v["resolve"]["nodes"]
        .as_array()
        .expect("resolved nodes required (omit --no-deps)");
    let by_id: BTreeMap<_, _> = packages
        .iter()
        .map(|p| (p["id"].as_str().expect("package ID"), p))
        .collect();
    let mut ws = Workspace::new();
    for pkg in packages
        .iter()
        .filter(|p| p.get("source").expect("package source").is_null())
    {
        let id = pkg["id"].as_str().expect("package ID");
        let name = pkg["name"].as_str().expect("package name").to_string();
        assert!(
            std::path::Path::new(pkg["manifest_path"].as_str().expect("manifest path"))
                .is_absolute(),
            "absolute manifest path required"
        );
        let node = nodes
            .iter()
            .find(|n| n["id"] == id)
            .expect("local package resolve node");
        let mut internal = BTreeSet::new();
        let mut external = BTreeSet::new();
        for edge in node["deps"].as_array().expect("resolved deps") {
            let kinds = edge["dep_kinds"].as_array().expect("dependency kinds");
            assert!(!kinds.is_empty(), "dependency kinds must not be empty");
            for kind in kinds {
                assert!(
                    kind["kind"].is_null()
                        || matches!(kind["kind"].as_str(), Some("build" | "dev")),
                    "unknown dependency kind"
                );
            }
            if kinds.iter().all(|k| k["kind"] == "dev") {
                continue;
            }
            let dep = by_id
                .get(edge["pkg"].as_str().expect("resolved dependency ID"))
                .expect("resolved dependency package");
            let dep_name = dep["name"].as_str().expect("dependency name").to_string();
            if dep.get("source").expect("dependency source").is_null() {
                internal.insert(dep_name);
            } else {
                external.insert(dep_name);
            }
        }
        assert!(
            ws.insert(
                name.clone(),
                CrateInfo {
                    name,
                    internal_deps: internal.into_iter().collect(),
                    external_deps: external.into_iter().collect()
                }
            )
            .is_none(),
            "ambiguous duplicate local package name"
        );
    }
    assert!(!ws.is_empty(), "no local packages in metadata");
    ws
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
            ],
        ),
        ("leftcar-rustra", &["control-contract"]),
        ("viewer-decoder", &[]),
        (
            "leftcar-host-desktop",
            &[
                "control-contract",
                "domain",
                "fec-core",
                "secure-channel",
                "session",
                "usb-mux",
            ],
        ),
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
        if ![
            "control-contract",
            "android-viewer",
            "viewer-decoder",
            "leftcar-host-desktop",
        ]
        .contains(&crate_name.as_str())
        {
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
