//! 생성된 Swift 표를 렌더링해서 기본 경로(native/macos-capture-shim)에 쓴다.
//!
//! ```text
//! cargo run -p keymap --bin gen-swift [출력경로]
//! ```

use std::path::PathBuf;

const DEFAULT_OUTPUT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos-capture-shim/Sources/Transport/AndroidKeyMap.swift"
);

fn main() {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT));
    let rendered = keymap::render_swift();
    let mappings = rendered.lines().filter(|l| l.trim_start().starts_with(char::is_numeric)).count();
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).expect("create output dir");
    }
    std::fs::write(&output, rendered).expect("write swift table");
    println!("wrote {mappings} mappings to {}", output.display());
}
