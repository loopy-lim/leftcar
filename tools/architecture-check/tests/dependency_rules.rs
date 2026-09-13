use architecture_check::{check_workspace, parse_metadata};
use serde_json::json;

fn graph(root: &str, dependency: &str, local: bool, kind: serde_json::Value) -> String {
    json!({"packages":[
      {"id":"root","name":root,"source":null,"manifest_path":"/unreadable/root/Cargo.toml"},
      {"id":"dep","name":dependency,"source":if local {None} else {Some("registry+fixture")},"manifest_path":"/unreadable/dep/Cargo.toml"}
    ],"workspace_members":["root"],"resolve":{"nodes":[
      {"id":"root","deps":[{"name":"renamed_dependency","pkg":"dep","dep_kinds":[{"kind":kind,"target":"cfg(target_os = \"android\")"}]}]},
      {"id":"dep","deps":[]}
    ]}}).to_string()
}
#[test]
fn resolved_renamed_target_edge_is_not_hidden_by_unreadable_manifest() {
    let ws = parse_metadata(&graph("fec-core", "domain", true, serde_json::Value::Null));
    assert_eq!(ws["fec-core"].internal_deps, vec!["domain"]);
    assert!(check_workspace(&ws)
        .iter()
        .any(|v| v.rule == "dependency-direction"));
}
#[test]
fn registry_alias_retains_domain_purity() {
    let ws = parse_metadata(&graph("domain", "tauri", false, serde_json::Value::Null));
    assert!(check_workspace(&ws)
        .iter()
        .any(|v| v.rule == "domain-purity"));
}
#[test]
fn build_edges_checked_dev_edges_excluded() {
    let build = parse_metadata(&graph("domain", "tauri", false, json!("build")));
    assert!(!check_workspace(&build).is_empty());
    let dev = parse_metadata(&graph("domain", "tauri", false, json!("dev")));
    assert!(check_workspace(&dev).is_empty());
}
#[test]
fn standalone_host_includes_local_nonmembers_and_allows_platform_facade() {
    let ws = parse_metadata(&graph(
        "leftcar-host-desktop",
        "domain",
        true,
        serde_json::Value::Null,
    ));
    assert!(ws.contains_key("domain"));
    assert!(check_workspace(&ws).is_empty());
    assert!(check_workspace(&parse_metadata(&graph(
        "leftcar-host-desktop",
        "tauri",
        false,
        serde_json::Value::Null
    )))
    .is_empty());
}
#[test]
fn unknown_local_package_rejected() {
    assert!(check_workspace(&parse_metadata(&graph(
        "session",
        "surprise",
        true,
        serde_json::Value::Null
    )))
    .iter()
    .any(|v| v.rule == "unknown-crate"));
}
#[test]
fn incomplete_metadata_fails_visibly() {
    assert!(std::panic::catch_unwind(|| parse_metadata(r#"{"packages":[]}"#)).is_err());
}
#[test]
fn real_workspace_and_standalone_host_pass() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for manifest in ["Cargo.toml", "apps/host-desktop/src-tauri/Cargo.toml"] {
        let output = std::process::Command::new("cargo")
            .current_dir(&root)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--manifest-path",
                manifest,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let ws = parse_metadata(&String::from_utf8(output.stdout).unwrap());
        let violations = check_workspace(&ws);
        assert!(violations.is_empty(), "{violations:?}");
    }
}

#[test]
fn cargo_resolves_actual_renamed_inherited_target_dependency() {
    let root = std::env::temp_dir().join(format!("leftcar-architecture-{}", std::process::id()));
    std::fs::create_dir_all(root.join("domain/src")).unwrap();
    std::fs::create_dir_all(root.join("host/src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=['domain','host']\nresolver='2'\n[workspace.dependencies]\nrenamed_domain={package='domain',path='domain'}\n").unwrap();
    std::fs::write(
        root.join("domain/Cargo.toml"),
        "[package]\nname='domain'\nversion='0.0.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("host/Cargo.toml"), "[package]\nname='host-core'\nversion='0.0.0'\nedition='2021'\n[target.'cfg(target_os = \"windows\")'.dependencies]\nrenamed_domain={workspace=true}\n").unwrap();
    for name in ["domain", "host"] {
        std::fs::write(root.join(name).join("src/lib.rs"), "").unwrap();
    }
    let output = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["metadata", "--format-version", "1", "--offline"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ws = parse_metadata(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(ws["host-core"].internal_deps, ["domain"]);
    assert!(check_workspace(&ws).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
