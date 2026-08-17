#!/bin/bash
# Leftcar viewer APK build (C: real app, no Gradle — direct SDK tools).
# Usage: tools/build_apk.sh [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

SDK="$HOME/Library/Android/sdk"
BT="$SDK/build-tools/36.1.0"
PLATFORM="$SDK/platforms/android-36/android.jar"
KT_JAR="$HOME/.gradle/caches/modules-2/files-2.1/org.jetbrains.kotlin/kotlin-compiler-embeddable/2.1.20/4ef56b3316798316bfac7a0ae443391c9e900ea1/kotlin-compiler-embeddable-2.1.20.jar"
KT_STDLIB=$(find ~/.gradle/caches/modules-2 -name 'kotlin-stdlib-2.1.20.jar' | head -1)
[ -z "$KT_STDLIB" ] && KT_STDLIB=$(find ~/.gradle/caches/modules-2 -name 'kotlin-stdlib-2.1*.jar' | head -1)
COROUTINES=$(find ~/.gradle/caches/modules-2 -name 'kotlinx-coroutines-core-jvm-1.9.0.jar' | grep -v sources | head -1)
TROVE=$(find ~/.gradle/caches/modules-2 -name 'trove4j-*.jar' | head -1)
ANNOTATIONS=$(find ~/.gradle/caches/modules-2 -path '*org.jetbrains/annotations/23.0.0*' -name 'annotations-23.0.0.jar' | head -1)
OUT=build/apk
MANIFEST=apps/viewer-android/android/app/src/main/AndroidManifest.xml
RES=apps/viewer-android/android/app/src/main/res
KOTLIN_SRC=apps/viewer-android/android/app/src/main/java
NATIVE_LIB=target/aarch64-linux-android/release/libleftcar_viewer.so

rm -rf "$OUT"
mkdir -p "$OUT/classes" "$OUT/obj" "$OUT/lib/arm64-v8a" "$OUT/compiled-res"

echo "[1/6] aapt2 link (resources + manifest -> base APK)"
cp "$NATIVE_LIB" "$OUT/lib/arm64-v8a/"
"$BT/aapt2" link -o "$OUT/base.apk" \
  -I "$PLATFORM" \
  --manifest "$MANIFEST" \
  --min-sdk-version 29 --target-sdk-version 36 \
  --version-code 1 --version-name 0.1.0 \
  -A "$OUT/assets" 2>/dev/null || \
"$BT/aapt2" link -o "$OUT/base.apk" \
  -I "$PLATFORM" \
  --manifest "$MANIFEST" \
  --min-sdk-version 29 --target-sdk-version 36 \
  --version-code 1 --version-name 0.1.0

echo "[2/6] kotlinc -> class files"
find "$KOTLIN_SRC" -name '*.kt' > "$OUT/sources.txt"
java -cp "$KT_JAR:$KT_STDLIB:$COROUTINES:$TROVE:$ANNOTATIONS" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -classpath "$PLATFORM:$KT_STDLIB" \
  -d "$OUT/classes" \
  -no-reflect -no-stdlib \
  -jvm-target 17 \
  @"$OUT/sources.txt"

echo "[3/6] d8 -> classes.dex"
find "$OUT/classes" -name '*.class' > "$OUT/classlist.txt"
"$BT/d8" --release --min-api 29 \
  --lib "$PLATFORM" \
  --output "$OUT/obj" \
  "$KT_STDLIB" \
  $(cat "$OUT/classlist.txt")

echo "[4/6] package native lib + dex into APK"
cd "$OUT"
cp obj/classes.dex .
zip -q -u base.apk classes.dex
zip -q -u base.apk lib/arm64-v8a/libleftcar_viewer.so
cd ../..

echo "[5/6] zipalign"
"$BT/zipalign" -f 4 "$OUT/base.apk" "$OUT/aligned.apk"

echo "[6/6] apksigner (debug key)"
KS="$HOME/.android/debug.keystore"
if [ ! -f "$KS" ]; then
  keytool -genkeypair -keystore "$KS" -storepass android -keypass android \
    -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" \
    -keyalg RSA -keysize 2048 -validity 10000
fi
"$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android \
  --out "$OUT/leftcar-viewer.apk" "$OUT/aligned.apk"

echo "BUILT: $OUT/leftcar-viewer.apk"
"$BT/aapt2" dump badging "$OUT/leftcar-viewer.apk" | head -6
