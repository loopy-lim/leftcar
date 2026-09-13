fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failed = false;
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
            .expect("run locked cargo metadata");
        if !output.status.success() {
            eprintln!("{manifest}: {}", String::from_utf8_lossy(&output.stderr));
            std::process::exit(1);
        }
        let ws = architecture_check::parse_metadata(
            &String::from_utf8(output.stdout).expect("UTF-8 metadata"),
        );
        let violations = architecture_check::check_workspace(&ws);
        for violation in &violations {
            eprintln!("{manifest}: {violation}");
        }
        failed |= !violations.is_empty();
        println!(
            "{manifest}: {} local packages, {} violations",
            ws.len(),
            violations.len()
        );
    }
    if failed {
        std::process::exit(1);
    }
}
