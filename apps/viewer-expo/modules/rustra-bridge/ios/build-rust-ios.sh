#!/bin/sh
set -eu

MODULE_DIR=$(cd "$(dirname "$0")/.." && pwd)
MANIFEST_PATH="$MODULE_DIR/../../../../native/leftcar-rustra/Cargo.toml"
PACKAGE='leftcar-rustra'
LIBRARY='leftcar_rustra'
TARGET_DIR="$MODULE_DIR/build/target"
CARGO_BIN=${CARGO_BIN:-cargo}
PROFILE=${RUSTRA_PROFILE:-release}
REL_FLAG=""
if [ "$PROFILE" = "release" ]; then REL_FLAG="--release"; fi
mkdir -p "$MODULE_DIR/ios/rust/lib"

build_target() {
  "$CARGO_BIN" build --manifest-path "$MANIFEST_PATH" -p "$PACKAGE" --lib \
    $REL_FLAG --target-dir "$TARGET_DIR" --target "$1"
}

if [ -n "${RUSTRA_IOS_TARGET:-}" ]; then
  build_target "$RUSTRA_IOS_TARGET"
  cp "$TARGET_DIR/$RUSTRA_IOS_TARGET/$PROFILE/lib$LIBRARY.a" \
    "$MODULE_DIR/ios/rust/lib/lib$LIBRARY.a"
else
  build_target aarch64-apple-ios-sim
  build_target x86_64-apple-ios
  lipo -create \
    "$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE/lib$LIBRARY.a" \
    "$TARGET_DIR/x86_64-apple-ios/$PROFILE/lib$LIBRARY.a" \
    -output "$MODULE_DIR/ios/rust/lib/lib$LIBRARY.a"
fi
