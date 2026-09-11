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
    let manifest = fixture_dir.join("Cargo.toml");
    let crafted = format!(
        r#"{{"packages":[{{"name":"domain","manifest_path":"{}"}}]}}"#,
        manifest.display()
    );
    let _ = violating_manifest();
    let ws = parse_metadata(&crafted);
    let violations = check_workspace(&ws);
    assert!(
        violations
            .iter()
            .any(|v| v.rule == "platform-dep-isolation" || v.rule == "domain-purity"),
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
        violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn domain_declares_no_internal_deps() {
    let ws = load_workspace();
    let domain = ws.get("domain").expect("domain crate exists");
    assert!(
        domain.internal_deps.is_empty(),
        "domain must not depend upward"
    );
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

/// Fixture layout for `{ workspace = true }` resolution:
///   <root>/Cargo.toml            — [workspace.dependencies]
///   <root>/crates/host-core/Cargo.toml — member manifest under test
fn write_workspace_fixture(root: &std::path::Path, member_rel: &str, member_toml: &str) {
    let member = root.join(member_rel);
    write_fixture(&member, member_toml);
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
[workspace.dependencies]
domain = { path = "crates/domain" }
control-contract = { path = "crates/control-contract" }
thiserror = "2"
"#,
    )
    .unwrap();
}

#[test]
fn workspace_true_path_dep_is_internal_and_enforced() {
    // The declared "host-core" -> ["domain", "media-model"] edge uses
    // { workspace = true } in the real manifest; it must surface as an
    // internal edge now (previously invisible to the parser).
    let root = std::env::temp_dir().join("arch_fixture_ws_internal");
    write_workspace_fixture(
        &root,
        "crates/host-core",
        r#"[dependencies]
domain = { workspace = true }
thiserror = { workspace = true }
"#,
    );
    let crafted = format!(
        r#"{{"packages":[{{"name":"host-core","manifest_path":"{}"}}]}}"#,
        root.join("crates/host-core/Cargo.toml").display()
    );
    let ws = parse_metadata(&crafted);
    let host_core = ws.get("host-core").expect("host-core fixture crate");
    assert!(
        host_core.internal_deps.iter().any(|d| d == "domain"),
        "workspace=true path dep must be internal, got: {:?}",
        host_core.internal_deps
    );
    assert!(
        host_core.external_deps.iter().any(|d| d == "thiserror"),
        "workspace=true version-only dep must stay external, got: {:?}",
        host_core.external_deps
    );
    // host-core -> domain is an allowed edge: no violations.
    let violations = check_workspace(&ws);
    assert!(
        violations.is_empty(),
        "allowed workspace=true edge must pass, got: {violations:?}"
    );
}

#[test]
fn forbidden_workspace_true_edge_fails() {
    // media-model must not depend on control-contract; declaring the edge as
    // { workspace = true } must not hide it from the layering rule.
    let root = std::env::temp_dir().join("arch_fixture_ws_forbidden");
    write_workspace_fixture(
        &root,
        "crates/media-model",
        r#"[dependencies]
control-contract = { workspace = true }
"#,
    );
    let crafted = format!(
        r#"{{"packages":[{{"name":"media-model","manifest_path":"{}"}}]}}"#,
        root.join("crates/media-model/Cargo.toml").display()
    );
    let ws = parse_metadata(&crafted);
    let violations = check_workspace(&ws);
    assert!(
        violations.iter().any(|v| v.rule == "dependency-direction"),
        "forbidden workspace=true edge must fail, got: {violations:?}"
    );
}

#[test]
fn target_specific_dependency_edges_are_checked() {
    // fec-core must stay dependency-free; a cfg-gated path dep is still a
    // declared edge of the crate and must be caught.
    let fixture_dir = std::env::temp_dir().join("arch_fixture_target_bad");
    write_fixture(
        &fixture_dir,
        r#"[target.'cfg(target_os = "macos")'.dependencies]
domain = { path = "../../crates/domain" }
"#,
    );
    let crafted = format!(
        r#"{{"packages":[{{"name":"fec-core","manifest_path":"{}"}}]}}"#,
        fixture_dir.join("Cargo.toml").display()
    );
    let ws = parse_metadata(&crafted);
    let fec_core = ws.get("fec-core").expect("fec-core fixture crate");
    assert!(
        fec_core.internal_deps.iter().any(|d| d == "domain"),
        "target-specific path dep must be internal, got: {:?}",
        fec_core.internal_deps
    );
    let violations = check_workspace(&ws);
    assert!(
        violations.iter().any(|v| v.rule == "dependency-direction"),
        "target-specific forbidden edge must fail, got: {violations:?}"
    );
}

#[test]
fn target_specific_workspace_dep_resolves_internal() {
    // session -> domain is allowed; declared via workspace=true inside a
    // target-specific table it must resolve to an internal edge and pass.
    let root = std::env::temp_dir().join("arch_fixture_target_ws");
    write_workspace_fixture(
        &root,
        "crates/session",
        r#"[target.'cfg(target_os = "android")'.dependencies]
domain = { workspace = true }
"#,
    );
    let crafted = format!(
        r#"{{"packages":[{{"name":"session","manifest_path":"{}"}}]}}"#,
        root.join("crates/session/Cargo.toml").display()
    );
    let ws = parse_metadata(&crafted);
    let session = ws.get("session").expect("session fixture crate");
    assert!(
        session.internal_deps.iter().any(|d| d == "domain"),
        "target-specific workspace=true path dep must be internal, got: {:?}",
        session.internal_deps
    );
    let violations = check_workspace(&ws);
    assert!(
        violations.is_empty(),
        "allowed target-specific workspace edge must pass, got: {violations:?}"
    );
}
