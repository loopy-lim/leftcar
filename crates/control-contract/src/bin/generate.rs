//! TS generation binary (`pnpm rustra:generate` → cargo run -p control-contract --bin generate).

fn main() {
    let out = std::path::PathBuf::from(
        std::env::var("LEFTCAR_GENERATED_DIR")
            .unwrap_or_else(|_| "packages/control-generated".into()),
    );
    control_contract::generate_all(out).expect("rustra generation succeeds");
    println!("generated: host + viewer control packages");
}
