#!/bin/zsh

set -euo pipefail

tool_dir=${0:A:h}
repo_root=${tool_dir:h}
host_dir="$repo_root/apps/host-desktop"
built_app="$host_dir/src-tauri/target/release/bundle/macos/Leftcar Host.app"
installed_app="/Applications/Leftcar Host.app"
process_name="leftcar-host-desktop"

codesign_requirement() {
  /usr/bin/codesign -d -r- "$1" 2>&1 \
    | /usr/bin/sed -n 's/^designated => //p'
}

cd "$host_dir"
bun run tauri build \
  --config src-tauri/tauri.macos.conf.json \
  --bundles app

if [[ ! -d "$built_app" ]]; then
  print -u2 "Signed Host app was not produced at: $built_app"
  exit 1
fi

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

/usr/bin/ditto "$built_app" "$installed_app"
/usr/bin/codesign --verify --deep --strict "$installed_app"
/usr/bin/open "$installed_app"

print "Installed and launched one stable Leftcar Host: $installed_app"
print "Screen Recording should only be requested on the first signed install."
