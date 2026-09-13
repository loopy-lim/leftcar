#!/bin/zsh

set -euo pipefail

if [[ -v LEFTCAR_BENCHMARK_PROFILE || -v LEFTCAR_BENCHMARK_ROOT ]]; then
  print -u2 "The normal-app updater cannot install benchmark profiles. Use bun run build -- host-macos-internal."
  exit 2
fi

tool_dir=${0:A:h}
repo_root=${tool_dir:h}
host_dir="$repo_root/apps/host-desktop"
# Resolve the same locked Cargo output used by the actual invocation. Explicit
# native --target prevents CARGO_BUILD_TARGET/config from selecting another tree.
host_plan=("${(@0)$(bun "$repo_root/tools/build.host-plan.mjs" "$host_dir/src-tauri" "Leftcar Host")}")
if (( ${#host_plan} != 10 )); then
  print -u2 "Could not resolve the Host build plan."
  exit 1
fi
export CARGO_TARGET_DIR="$host_plan[1]"
built_app="$host_plan[2]"
host_build_args=("${host_plan[@]:2}")
if [[ ${1:-} == --print-build-plan ]]; then
  printf '%s\0' "$CARGO_TARGET_DIR" "$built_app" "${host_build_args[@]}"
  exit 0
fi
if (( $# != 0 )); then
  print -u2 "Usage: dev-host-macos.zsh [--print-build-plan]"
  exit 2
fi
installed_app="/Applications/Leftcar Host.app"
process_name="leftcar-host-desktop"
shim_output="$repo_root/native/macos-capture-shim/libleftcar_capture.dylib"
built_shim="$built_app/Contents/Resources/libleftcar_capture.dylib"
installed_shim="$installed_app/Contents/Resources/libleftcar_capture.dylib"

codesign_requirement() {
  /usr/bin/codesign -d -r- "$1" 2>&1 \
    | /usr/bin/sed -n 's/^designated => //p'
}

verify_same_shim() {
  local expected=$1
  local bundled=$2
  local label=$3
  if [[ ! -f "$bundled" ]]; then
    print -u2 "$label capture shim is missing: $bundled"
    exit 1
  fi
  if ! /usr/bin/cmp -s "$expected" "$bundled"; then
    print -u2 "$label capture shim does not match the freshly built dylib."
    /usr/bin/shasum -a 256 "$expected" "$bundled" >&2
    exit 1
  fi
}

# Tauri bundles the dylib as an opaque resource. Rebuild it explicitly so a
# successful desktop build cannot silently install stale capture/encoder code.
"$repo_root/tools/build-macos-capture-shim.zsh" library "$shim_output"

cd "$host_dir"
bun run tauri build "${host_build_args[@]}"

if [[ ! -d "$built_app" ]]; then
  print -u2 "Signed Host app was not produced at: $built_app"
  exit 1
fi

verify_same_shim "$shim_output" "$built_shim" "Built Host"
/usr/bin/codesign --verify --deep --strict "$built_app"
built_requirement=$(codesign_requirement "$built_app")
if [[ -z "$built_requirement" ]]; then
  print -u2 "The built Host has no stable designated signing requirement."
  exit 1
fi

if [[ -d "$installed_app" ]]; then
  installed_requirement=$(codesign_requirement "$installed_app")
  if [[ "$installed_requirement" != "$built_requirement" ]]; then
    print -u2 "Refusing to replace the installed Host with a different signing identity."
    print -u2 "This would invalidate the existing macOS Screen Recording permission."
    print -u2 "Installed: $installed_requirement"
    print -u2 "Built:     $built_requirement"
    exit 1
  fi
fi

# Keep one stable /Applications identity. Replacing this bundle in place lets
# macOS reuse the Screen Recording approval granted to the same requirement.
/usr/bin/osascript \
  -e 'tell application id "leftcar.ll3.kr" to quit' \
  >/dev/null 2>&1 || true

for _attempt in {1..20}; do
  if ! /usr/bin/pgrep -x "$process_name" >/dev/null 2>&1; then
    break
  fi
  /bin/sleep 0.1
done

if /usr/bin/pgrep -x "$process_name" >/dev/null 2>&1; then
  /usr/bin/pkill -TERM -x "$process_name"
fi

# Stage and verify before moving the old app. Keep its verified bundle for
# rollback; the replacement helper restores it automatically on failure.
bun "$repo_root/tools/build.bundle.mjs" "$built_app" "$installed_app"
verify_same_shim "$shim_output" "$installed_shim" "Installed Host"
/usr/bin/codesign --verify --deep --strict "$installed_app"
/usr/bin/open "$installed_app"

print "Installed and launched one stable Leftcar Host: $installed_app"
print "Screen Recording should only be requested on the first signed install."
