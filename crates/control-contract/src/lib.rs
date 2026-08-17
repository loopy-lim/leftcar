//! Rustra control contract (H02, H26; docs/04).
//!
//! Rustra connects the local UI <-> core boundary only. It must never carry
//! video payloads (checked by architecture tests and generated-code scans).

pub mod common;
pub mod host;
pub mod viewer;

pub use host::host_package;
pub use viewer::viewer_package;


/// Generate TypeScript + schema for both packages and write to `out_dir`.
///
/// Layout: `<out_dir>/host/` and `<out_dir>/viewer/` (rustra writes
/// schema.json, types.ts, commands.ts, contract.ts into each).
pub fn generate_all(out_dir: std::path::PathBuf) -> rustra::Result<()> {
    let host = host_package().generate_typescript()?;
    host.write_to_dir(out_dir.join("host"))?;
    let viewer = viewer_package().generate_typescript()?;
    viewer.write_to_dir(out_dir.join("viewer"))?;
    Ok(())
}

/// Combined contract hash of both generated packages. The generated TS
/// `contract.ts` constants must match this (contract test verifies).
pub fn contract_hash() -> String {
    let host_hash = host_package()
        .generate_typescript()
        .expect("host package generates")
        .contract_hash;
    let viewer_hash = viewer_package()
        .generate_typescript()
        .expect("viewer package generates")
        .contract_hash;
    let combined = format!("{host_hash}{viewer_hash}");
    // FNV-1a 64 over the two hashes
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in combined.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
