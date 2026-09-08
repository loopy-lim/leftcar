//! TS generation binary (`bun run rustra:generate` → cargo run -p control-contract --bin generate).
//!
//! Two modes:
//! - `RUSTRA_SCHEMA_OUT` set — `rustra codegen --check` probe: emit only the
//!   host schema so the CLI verifies the Rust surface without touching the
//!   work tree. Writing both packages here would leave the viewer schema at
//!   the probe path and fail the check.
//! - Otherwise — write the Rust-rendered reference TS for both packages into
//!   `<workspace root>/packages/control-generated` (or `LEFTCAR_GENERATED_DIR`).
//!   The root anchoring keeps `rustra codegen` — which runs this binary with
//!   the app dir as cwd — from forking a second copy under the app.

fn main() {
    if let Some(dir) = std::env::var_os("RUSTRA_SCHEMA_OUT") {
        host_schema_probe(dir);
        return;
    }
    let out = match std::env::var("LEFTCAR_GENERATED_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => {
            let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.pop();
            path.pop();
            path.join("packages/control-generated")
        }
    };
    control_contract::generate_all(out).expect("rustra generation succeeds");
    println!("generated: host + viewer control packages");
}

fn host_schema_probe(dir: std::ffi::OsString) {
    use control_contract::host_package;
    host_package()
        .generate_typescript()
        .expect("host package generates")
        .write_schema_to_dir(std::path::PathBuf::from(dir))
        .expect("host schema probe write succeeds");
    println!("generated: host schema probe");
}
