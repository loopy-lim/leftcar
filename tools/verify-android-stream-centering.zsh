#!/bin/zsh

set -euo pipefail

serial=${1:?"usage: $0 <adb-serial> [source-width] [source-height]"}
source_width=${2:-3840}
source_height=${3:-2160}

if (( source_width <= 0 || source_height <= 0 )); then
  print -u2 "source dimensions must be positive"
  exit 2
fi

if command -v adb >/dev/null 2>&1; then
  adb_bin=$(command -v adb)
elif [[ -x /Users/loopy/Library/Android/sdk/platform-tools/adb ]]; then
  adb_bin=/Users/loopy/Library/Android/sdk/platform-tools/adb
else
  print -u2 "adb not found"
  exit 2
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf -- "$tmp_dir"' EXIT
remote_xml=/sdcard/leftcar-stream-centering.xml
local_xml="$tmp_dir/window.xml"

"$adb_bin" -s "$serial" shell uiautomator dump "$remote_xml" >/dev/null
"$adb_bin" -s "$serial" pull "$remote_xml" "$local_xml" >/dev/null

root_bounds=$(/usr/bin/xmllint --xpath 'string(/hierarchy/node/@bounds)' "$local_xml")
left_bounds=$(/usr/bin/xmllint --xpath \
  'string((//node[@class="android.view.SurfaceView"])[1]/@bounds)' "$local_xml")
right_bounds=$(/usr/bin/xmllint --xpath \
  'string((//node[@class="android.view.SurfaceView"])[2]/@bounds)' "$local_xml")
surface_count=$(/usr/bin/xmllint --xpath \
  'count(//node[@class="android.view.SurfaceView"])' "$local_xml")

if [[ "$surface_count" != "2" ]]; then
  print -u2 "expected two split SurfaceViews, found $surface_count"
  exit 1
fi

if [[ ! "$root_bounds" =~ '^\[([0-9]+),([0-9]+)\]\[([0-9]+),([0-9]+)\]$' ]]; then
  print -u2 "invalid root bounds: $root_bounds"
  exit 1
fi
root_left=$match[1]
root_top=$match[2]
root_right=$match[3]
root_bottom=$match[4]
root_width=$(( root_right - root_left ))
root_height=$(( root_bottom - root_top ))

if (( root_width * source_height <= root_height * source_width )); then
  render_width=$root_width
  render_height=$(( root_width * source_height / source_width ))
else
  render_height=$root_height
  render_width=$(( root_height * source_width / source_height ))
fi

render_left=$(( root_left + (root_width - render_width) / 2 ))
render_top=$(( root_top + (root_height - render_height) / 2 ))
render_right=$(( render_left + render_width ))
render_bottom=$(( render_top + render_height ))
split_x=$(( render_left + render_width / 2 ))

expected_left="[$render_left,$render_top][$split_x,$render_bottom]"
expected_right="[$split_x,$render_top][$render_right,$render_bottom]"

if [[ "$left_bounds" != "$expected_left" || "$right_bounds" != "$expected_right" ]]; then
  print -u2 "split stream is not centered"
  print -u2 "root:     $root_bounds"
  print -u2 "left:     $left_bounds (expected $expected_left)"
  print -u2 "right:    $right_bounds (expected $expected_right)"
  exit 1
fi

print "split stream centered: left=$left_bounds right=$right_bounds"
