import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const CHECKER = join(fileURLToPath(new URL(".", import.meta.url)), "ts.ts");

function writeFixture(root: string, relativePath: string, contents: string): void {
  const path = join(root, relativePath);
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, contents);
}

function writeRequiredExpoSources(root: string): void {
  writeFixture(
    root,
    "apps/viewer-expo/android/app/src/main/AndroidManifest.xml",
    '<activity android:name=".StreamActivity" android:documentLaunchMode="intoExisting" />',
  );
  writeFixture(
    root,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt",
    'intent.scheme("leftcar-stream").appendPath(host).appendPath(port.toString())\nFLAG_ACTIVITY_NEW_DOCUMENT',
  );
  writeFixture(
    root,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt",
    "override fun onNewIntent(newIntent: Intent)\nsetZOrderOnTop(true)",
  );
  writeFixture(
    root,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamSurfaceLayout.kt",
    "",
  );
}

test("allows org.junit only from exact Android JVM unit-test source-set layouts", async () => {
  const root = mkdtempSync(join(tmpdir(), "leftcar-architecture-check-"));
  try {
    writeRequiredExpoSources(root);
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/test/java/dev/leftcar/viewer/AllowedJUnitTest.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/test/kotlin/dev/leftcar/viewer/AllowedKotlinJUnitTest.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/android/src/test/java/dev/leftcar/viewer/RejectedAndroidModuleJUnit.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/src/test/kotlin/dev/leftcar/viewer/RejectedRootModuleJUnit.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/test/resources/dev/leftcar/viewer/RejectedArbitraryTestPath.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/RejectedJUnit.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/androidTest/java/dev/leftcar/viewer/RejectedAndroidTestJUnit.kt",
      "import org.junit.Test",
    );
    writeFixture(
      root,
      "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/src/test/java/MisleadingJUnit.kt",
      "import org.junit.Test",
    );

    const result = spawnSync("bun", [CHECKER], {
      cwd: root,
      encoding: "utf8",
    });
    const stderr = result.stderr;

    expect(result.status).toBe(1);
    for (const rejectedFile of [
      "RejectedAndroidModuleJUnit.kt",
      "RejectedRootModuleJUnit.kt",
      "RejectedArbitraryTestPath.kt",
      "RejectedJUnit.kt",
      "RejectedAndroidTestJUnit.kt",
      "MisleadingJUnit.kt",
    ]) {
      expect(stderr).toContain(`${rejectedFile}: import org.junit.Test`);
    }
    expect(stderr).not.toContain("AllowedJUnitTest.kt: import org.junit.Test");
    expect(stderr).not.toContain("AllowedKotlinJUnitTest.kt: import org.junit.Test");
    expect(stderr).toContain("architecture-check: 6 violation(s)");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Robolectric is a JVM-test dependency and remains forbidden in production", () => {
  const root = mkdtempSync(join(tmpdir(), "leftcar-robolectric-architecture-"));
  try {
    writeRequiredExpoSources(root);
    for (const sourceSet of ["test", "main", "androidTest"]) {
      writeFixture(root, `apps/viewer-expo/android/app/src/${sourceSet}/java/dev/leftcar/viewer/${sourceSet}.kt`, "import org.robolectric.RobolectricTestRunner");
    }
    const result = spawnSync("bun", [CHECKER], {cwd:root, encoding:"utf8"});
    expect(result.status).toBe(1);
    expect(result.stderr).toContain("main.kt: import org.robolectric.RobolectricTestRunner");
    expect(result.stderr).toContain("androidTest.kt: import org.robolectric.RobolectricTestRunner");
    expect(result.stderr).not.toContain("test.kt: import org.robolectric.RobolectricTestRunner");
    expect(result.stderr).toContain("architecture-check: 2 violation(s)");
  } finally { rmSync(root, {recursive:true, force:true}); }
});

test("approved codec boundaries allow queries and Opus CSD but reject network and video", () => {
  const root = mkdtempSync(join(tmpdir(), "leftcar-codec-boundary-"));
  const base = "apps/viewer-expo/android/app/src/";
  const main = `${base}main/java/dev/leftcar/viewer/stream/`;
  const cases: Array<[string, string, boolean]> = [
    [main + "SplitDecoderCapability.kt", "import android.media.MediaCodecList\nval hints = codec.maxSupportedInstances >= 2\nvideo.areSizeAndRateSupported(width, height, fps)", true],
    [main + "OpusAudioDecoder.kt", "import android.media.MediaCodec\nimport java.nio.ByteBuffer\nimport java.nio.ByteOrder\nMediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_OPUS, 48000, 2)\nMediaCodec.createByCodecName(name)", true],
    [base + "test/java/dev/leftcar/viewer/stream/OpusAudioDecoderTest.kt", "import java.nio.ByteBuffer\nimport java.nio.ByteOrder\nMediaFormat.MIMETYPE_AUDIO_OPUS", true],
    [main + "OpusAudioDecoder.kt", "java.net.Socket(host, port)", false],
    [main + "SplitDecoderCapability.kt", "MediaCodec.createByCodecName(name)", false],
    [main + "Other.kt", "MediaCodec.createDecoderByType(\"audio/opus\")", false],
    [main + "OpusAudioDecoder.kt", "MediaFormat.createVideoFormat(\"video/avc\", 1920, 1080)\nMediaCodec.createByCodecName(name)", false],
    [main + "Other/OpusAudioDecoder.kt", "MediaCodec.createByCodecName(name)", false],
    [main + "SplitDecoderCapability.kt", "java.net.DatagramSocket()", false],
  ];
  try {
    for (const [path, source, allowed] of cases) {
      writeRequiredExpoSources(root);
      writeFixture(root, path, source);
      const result = spawnSync("bun", [CHECKER], {cwd: root, encoding: "utf8"});
      expect(result.status, `${path}: ${source}\n${result.stderr}`).toBe(allowed ? 0 : 1);
      rmSync(join(root, path));
    }
  } finally { rmSync(root, {recursive:true, force:true}); }
});
