#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

app=apps/viewer-nsh
abi="${ABI:-arm64-v8a}"
keystore="${KEYSTORE:-}"
key_alias="${KEY_ALIAS:-nsh}"
stage="$(mktemp -d "${TMPDIR:-/tmp}/leftcar-viewer-nsh.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

tar -C "$app" -cf - --exclude=.nsh --exclude=target --exclude=screens . | tar -C "$stage" -xf -

package=(package --target android . --abi "$abi")
if [ -n "$keystore" ]; then
  cp "$keystore" "$stage/release.jks"
  package+=(--keystore release.jks --key-alias "$key_alias")
fi
(cd "$stage" && nsh "${package[@]}")

mkdir -p "$app/.nsh/out"
name="$(basename "$stage"/.nsh/out/*.apk)"
cp "$stage/.nsh/out/$name" "$app/.nsh/out/$name"
echo "APK: $app/.nsh/out/$name ($abi)"
sha256sum "$app/.nsh/out/$name"
echo "Install: adb install \"$app/.nsh/out/$name\""
