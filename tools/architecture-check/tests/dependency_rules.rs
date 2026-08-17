//! Architecture rule tests (H01).
//!
//! Red–Green: each test first asserts the rule engine fires on a violating
//! fixture, then that the real workspace passes.

use architecture_check::{check_workspace, parse_metadata};

fn load_workspace() -> architecture_check::Workspace {
    let json = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata runs")
        .stdout;
    parse_metadata(&String::from_utf8(json).expect("utf8"))
}

/// Fixture manifest text with a domain crate that imports a platform dep.
fn violating_manifest() -> String {
    r#"{
        "packages": [
          {
            "name": "domain",
            "manifest_path": "tests/fixtures/domain_bad/Cargo.toml"
          }
        ]
      }"#
    .to_string()
}

fn write_fixture(dir: &std::path::Path, toml: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("Cargo.toml"), toml).unwrap();
}

#[test]
fn domain_with_platform_dependency_fails() {
    let fixture_dir = std::env::temp_dir().join("arch_fixture_domain_bad");
    write_fixture(
        &fixture_dir,
        r#"[dependencies]
tauri = "2"
serde = "1"
"#,
    );
    // point the fixture json at the temp manifest
    let json = r#"{"packages":[{"name":"domain","manifest_path":""}]}"#;
    let _ = json;
    let ws = architecture_check::Workspace::new();
    let mut ws = ws;
    let manifest = fixture_dir.join("Cargo.toml");
    // Directly exercise parse of one manifest through parse_metadata via a shim:
    // simpler — build CrateInfo through the public parse of a crafted metadata.
    let crafted = format!(
        r#"{{"packages":[{{"name":"domain","manifest_path":"{}"}}]}}"#,
        manifest.display()
    );
    let _ = violating_manifest();
    ws = parse_metadata(&crafted);
    let violations = check_workspace(&ws);
    assert!(
        violations.iter().any(|v| v.rule == "platform-dep-isolation"
            || v.rule == "domain-purity"),
        "expected domain platform-dep violation, got: {violations:?}"
    );
}

#[test]
fn real_workspace_has_no_violations() {
    let ws = load_workspace();
    let violations = check_workspace(&ws);
    assert!(
        violations.is_empty(),
        "architecture violations:\n{}",
        violations.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn domain_declares_no_internal_deps() {
    let ws = load_workspace();
    let domain = ws.get("domain").expect("domain crate exists");
    assert!(domain.internal_deps.is_empty(), "domain must not depend upward");
}

#[test]
fn layering_direction_is_acyclic_per_rules() {
    // media-model -> domain only; reverse edge must be rejected by the engine.
    let fixture_dir = std::env::temp_dir().join("arch_fixture_media_bad");
    write_fixture(
        &fixture_dir,
        r#"[dependencies]
media-model = { path = "../../crates/media-model" }
"#,
    );
    let crafted = format!(
        r#"{{"packages":[{{"name":"domain","manifest_path":"{}"}}]}}"#,
        fixture_dir.join("Cargo.toml").display()
    );
    let ws = parse_metadata(&crafted);
    let violations = check_workspace(&ws);
    assert!(
        violations.iter().any(|v| v.rule == "dependency-direction"),
        "expected dependency-direction violation, got: {violations:?}"
    );
}
