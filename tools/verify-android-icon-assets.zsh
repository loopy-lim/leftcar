#!/bin/zsh

set -euo pipefail

repo_root=${0:A:h:h}
viewer_root="$repo_root/apps/viewer-expo"
res_root="$viewer_root/android/app/src/main/res"

source_icon="$viewer_root/assets/branding/leftcar-viewer-icon-source.png"
foreground_icon="$viewer_root/assets/branding/leftcar-viewer-icon-foreground.png"
monochrome_icon="$viewer_root/assets/branding/leftcar-viewer-icon-monochrome.png"

for asset in "$source_icon" "$foreground_icon" "$monochrome_icon"; do
  [[ -f "$asset" ]] || {
    print -u2 "missing icon asset: ${asset#$repo_root/}"
    exit 1
  }
done

source_size=$(magick identify -format '%wx%h' "$source_icon")
foreground_size=$(magick identify -format '%wx%h' "$foreground_icon")
monochrome_size=$(magick identify -format '%wx%h' "$monochrome_icon")

[[ "$source_size" == "1024x1024" ]] || {
  print -u2 "source icon must be 1024x1024, got $source_size"
  exit 1
}
[[ "$foreground_size" == "1024x1024" ]] || {
  print -u2 "foreground icon must be 1024x1024, got $foreground_size"
  exit 1
}
[[ "$monochrome_size" == "1024x1024" ]] || {
  print -u2 "monochrome icon must be 1024x1024, got $monochrome_size"
  exit 1
}

for asset in "$foreground_icon" "$monochrome_icon"; do
  channels=$(magick identify -format '%[channels]' "$asset")
  [[ "$channels" == *a* ]] || {
    print -u2 "transparent icon asset must have alpha: ${asset#$repo_root/} ($channels)"
    exit 1
  }
done

rg -q 'foregroundImage: "\./assets/branding/leftcar-viewer-icon-foreground\.png"' \
  "$viewer_root/app.config.ts"
rg -q 'monochromeImage: "\./assets/branding/leftcar-viewer-icon-monochrome\.png"' \
  "$viewer_root/app.config.ts"
rg -q 'backgroundColor: "#09090B"' "$viewer_root/app.config.ts"

for version in mipmap-anydpi-v26 mipmap-anydpi-v33; do
  for resource in ic_launcher.xml ic_launcher_round.xml; do
    [[ -f "$res_root/$version/$resource" ]] || {
      print -u2 "missing adaptive icon resource: ${version}/${resource}"
      exit 1
    }
  done
done

typeset -A density_sizes=(
  mdpi 48
  hdpi 72
  xhdpi 96
  xxhdpi 144
  xxxhdpi 192
)

typeset -A adaptive_sizes=(
  mdpi 108
  hdpi 162
  xhdpi 216
  xxhdpi 324
  xxxhdpi 432
)

typeset -A splash_sizes=(
  mdpi 288
  hdpi 432
  xhdpi 576
  xxhdpi 864
  xxxhdpi 1152
)

for density expected in ${(kv)density_sizes}; do
  for resource in ic_launcher.webp ic_launcher_round.webp; do
    asset="$res_root/mipmap-$density/$resource"
    actual=$(magick identify -format '%w' "$asset")
    [[ "$actual" == "$expected" ]] || {
      print -u2 "$density/$resource must be ${expected}px, got ${actual}px"
      exit 1
    }
  done
done

for density expected in ${(kv)adaptive_sizes}; do
  for resource in ic_launcher_foreground.png ic_launcher_monochrome.png; do
    asset="$res_root/drawable-$density/$resource"
    [[ -f "$asset" ]] || {
      print -u2 "missing adaptive layer: drawable-$density/$resource"
      exit 1
    }
    actual=$(magick identify -format '%w' "$asset")
    [[ "$actual" == "$expected" ]] || {
      print -u2 "drawable-$density/$resource must be ${expected}px, got ${actual}px"
      exit 1
    }
  done
done

for density expected in ${(kv)splash_sizes}; do
  asset="$res_root/drawable-$density/splashscreen_logo.png"
  actual=$(magick identify -format '%w' "$asset")
  [[ "$actual" == "$expected" ]] || {
    print -u2 "drawable-$density/splashscreen_logo.png must be ${expected}px, got ${actual}px"
    exit 1
  }
done

print "Android icon assets verified: full, round, adaptive, and themed"
