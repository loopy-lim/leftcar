/**
 * TS/Kotlin architecture rules (H01): regex-level enforcement that mirrors
 * tools/architecture-check (cargo) for non-Rust sources.
 *
 * Rules (docs/05 L0):
 * - generated TS contains no video payload types
 * - generated/TypeScript control sources contain no high-rate input plane
 * - Kotlin shim imports only allowlisted packages (no java.net, no codec)
 * - Kotlin shim contains no business-layer markers (network/codec policy)
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = process.cwd();
let failures = 0;

function fail(rule: string, detail: string): void {
  failures += 1;
  console.error(`[arch:${rule}] ${detail}`);
}

function* walk(dir: string): Generator<string> {
  let entries: string[];
  try {
    entries = readdirSync(dir);
  } catch {
    return;
  }
  for (const name of entries) {
    if (name === "node_modules" || name === "dist" || name === ".git" || name === ".worktrees") continue;
    const full = join(dir, name);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) yield* walk(full);
    else yield full;
  }
}

function checkFiles(
  dir: string,
  patterns: Array<[RegExp, string]>,
  rule: string,
  allow?: (file: string) => boolean,
): void {
  for (const file of walk(dir)) {
    if (!/\.(ts|tsx|kt)$/.test(file)) continue;
    if (allow?.(file)) continue;
    const text = readFileSync(file, "utf8");
    for (const [re, why] of patterns) {
      if (re.test(text)) fail(rule, `${file}: ${why}`);
    }
  }
}

// 1. generated TS: no video payload types (docs/05 L0). Scans generated
//    sources only — test fixtures legitimately name the banned symbols while
//    asserting their absence.
checkFiles(
  join(ROOT, "packages/control-generated/host"),
  [
    [/EncodedFrame|NalUnit|VideoPacket/, "generated contract leaked video types"],
  ],
  "generated-no-video-types",
);
checkFiles(
  join(ROOT, "packages/control-generated/viewer"),
  [
    [/EncodedFrame|NalUnit|VideoPacket/, "generated contract leaked video types"],
  ],
  "generated-no-video-types",
);

// 2. The 120/180Hz input plane is native Kotlin/JNI/UDP. Keep it out of
//    React/Rustra/JSON while continuing to deny file transfer. Clipboard
//    text sync is deliberately allowed again behind a double gate
//    (docs/07 §20 policy change, 2026-09-09): expo-clipboard access lives
//    ONLY in apps/viewer-expo/src/clipboard-sync.ts and the host-side gate
//    still rejects every clipboard command while its toggle is closed.
const CLIPBOARD_SYNC_FILES = new Set([
  "apps/viewer-expo/src/clipboard-sync.ts",
  // 테스트 파일은 모듈 경계 목 fixture로 모듈 이름을 적어야 한다 — 파일
  // 상단 "test fixtures legitimately name the banned symbols" 예외와 같은
  // 취지다.
  "apps/viewer-expo/src/clipboard-sync.test.ts",
]);
const isClipboardSyncModule = (file: string): boolean =>
  CLIPBOARD_SYNC_FILES.has(relative(ROOT, file).replaceAll("\\", "/"));

for (const folder of [
  join(ROOT, "apps/viewer-android/src"),
  join(ROOT, "apps/viewer-expo/src"),
  join(ROOT, "apps/viewer-expo/app"),
]) {
  checkFiles(
    folder,
    [
      [/sendKey|sendMouse|injectInput|sendTouch/i, "high-rate input leaked into TypeScript"],
      [/readClipboard|writeClipboard|readFile|writeFile/, "file/clipboard access in viewer"],
    ],
    "viewer-ts-no-high-rate-input",
  );
  // docs/07 §20(정책 변경): 게이트 없는 클립보드 접근은 여전히 금지며,
  // expo-clipboard 진입점은 단일 모듈로 한정한다.
  checkFiles(
    folder,
    [[/["']expo-clipboard["']/, "expo-clipboard access outside clipboard-sync.ts"]],
    "viewer-clipboard-single-gated-module",
    isClipboardSyncModule,
  );
}

// 3. Kotlin shim: import allowlist (docs/05 L0 kotlin_shim_imports_only_allowlisted_packages)
const KOTLIN_ALLOW = /^import (android\.|androidx\.|com\.facebook\.|expo\.|dev\.leftcar\.viewer\.|java\.lang\.|java\.util\.|java\.security\.|kotlin\.)/; // java.security = SecureRandom(CSPRNG 어댑터, docs/07 §20)
const JVM_TEST_IMPORT = /^import org\.(?:junit|robolectric)\./;
const STREAM_ACTIVITY_XR_COROUTINE_IMPORTS = new Set([
  "import kotlinx.coroutines.Dispatchers",
  "import kotlinx.coroutines.Job",
  "import kotlinx.coroutines.launch",
  "import kotlinx.coroutines.withContext",
]);

function isKeyBridgeOwnershipContractImport(file: string, line: string): boolean {
  const projectRelativePath = relative(ROOT, file).replaceAll("\\", "/");
  // KeyBridge is an optional installed app. Its AIDL contract may cross the
  // process boundary only through Leftcar's dedicated ownership adapter.
  return projectRelativePath ===
      "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/KeyBridgeInputAdapter.kt" &&
    line === "import dev.loopy.keybridge.remote.IRemoteInputOwnership";
}

function isStreamActivityXrCoroutineImport(file: string, line: string): boolean {
  const projectRelativePath = relative(ROOT, file).replaceAll("\\", "/");
  // Session.create is asynchronous and lifecycle-bound. Keep this exception
  // restricted to the XR Activity boundary and the four scheduling primitives
  // it needs; network, decoder, and adaptation policy remain in native Rust.
  return projectRelativePath ===
      "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt" &&
    STREAM_ACTIVITY_XR_COROUTINE_IMPORTS.has(line);
}

function isAndroidJvmUnitTestSource(file: string, androidProjectRoot: string): boolean {
  const projectRelativePath = relative(androidProjectRoot, file).replaceAll("\\", "/");
  return /^app\/src\/test\/(?:java|kotlin)\/.+\.kt$/.test(projectRelativePath);
}

for (const folder of [join(ROOT, "apps/viewer-android/android"), join(ROOT, "apps/viewer-expo/android")]) {
  for (const file of walk(folder)) {
    if (!file.endsWith(".kt")) continue;
    const text = readFileSync(file, "utf8");
    for (const line of text.split("\n")) {
      const m = line.match(/^import\s+(.+)$/);
      const allowsJvmUnitTestDependency = isAndroidJvmUnitTestSource(file, folder) && JVM_TEST_IMPORT.test(line);
      const allowsStreamActivityXrCoroutine = isStreamActivityXrCoroutineImport(file, line);
      const allowsKeyBridgeOwnershipContract = isKeyBridgeOwnershipContractImport(file, line);
      const audioPath = relative(ROOT, file).replaceAll("\\", "/");
      const allowsAudioBuffer = /^apps\/viewer-expo\/android\/app\/src\/(?:main\/java\/dev\/leftcar\/viewer\/stream\/OpusAudioDecoder|test\/java\/dev\/leftcar\/viewer\/stream\/OpusAudioDecoderTest)\.kt$/.test(audioPath)
        && /^import java\.nio\.(?:ByteBuffer|ByteOrder)$/.test(line);
      if (m && !KOTLIN_ALLOW.test(line) && !allowsJvmUnitTestDependency && !allowsStreamActivityXrCoroutine && !allowsKeyBridgeOwnershipContract && !allowsAudioBuffer) {
        fail(
          "kotlin-import-allowlist",
          `${file}: ${line}`,
        );
      }
    }
    const path = relative(ROOT, file).replaceAll("\\", "/");
    const base = "apps/viewer-expo/android/app/src/";
    const stream = "java/dev/leftcar/viewer/stream/";
    const query = path === `${base}main/${stream}SplitDecoderCapability.kt`;
    const audio = path === `${base}main/${stream}OpusAudioDecoder.kt`;
    const audioTest = path === `${base}test/${stream}OpusAudioDecoderTest.kt`;
    const queryTest = path === `${base}test/${stream}SplitDecoderCapabilityTest.kt`;
    if (/DatagramSocket|(?:Server)?Socket\s*\(|java\.net\./.test(text)) {
      fail("kotlin-no-policy", `${file}: network creation belongs to Rust`);
    }
    if (/MediaCodec|AMediaCodec/.test(text) && !(query || audio || audioTest || queryTest)) {
      fail("kotlin-no-policy", `${file}: codec outside approved platform boundary`);
    }
    if ((query || queryTest) && /createDecoderByType|createEncoderByType|createByCodecName|MediaCodec\s*\.\s*create/.test(text)) {
      fail("kotlin-split-capability-only", `${file}: capability query must never construct a codec`);
    }
    if ((audio || audioTest) && /createVideoFormat|MIMETYPE_VIDEO_|["']video\/|createEncoderByType|createDecoderByType/.test(text)) {
      fail("kotlin-opus-only", `${file}: only the Opus audio adapter is permitted`);
    }

  }
}

// 4. The Expo viewer keeps one document task per unique host/port. Using
//    documentLaunchMode="always" here makes every reconnect add another
//    Recents entry even though Android still has only one installed package.
const expoManifest = readFileSync(
  join(ROOT, "apps/viewer-expo/android/app/src/main/AndroidManifest.xml"),
  "utf8",
);
const streamLauncher = readFileSync(
  join(
    ROOT,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamLauncherModule.kt",
  ),
  "utf8",
);
const streamActivity = readFileSync(
  join(
    ROOT,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamActivity.kt",
  ),
  "utf8",
);
const streamSurfaceLayout = readFileSync(
  join(
    ROOT,
    "apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamSurfaceLayout.kt",
  ),
  "utf8",
);
if (!/StreamActivity[^>]+documentLaunchMode="intoExisting"/.test(expoManifest)) {
  fail("stream-task-reuse", "Expo StreamActivity must reuse an existing document task");
}
for (const [pattern, detail] of [
  [/\.scheme\("leftcar-stream"\)/, "stream intent has no stable document scheme"],
  [/\.appendPath\(host\)/, "stream intent identity does not include the host"],
  [/\.appendPath\(port\.toString\(\)\)/, "stream intent identity does not include the port"],
  [/FLAG_ACTIVITY_NEW_DOCUMENT/, "stream intent is not launched as a document"],
] as Array<[RegExp, string]>) {
  if (!pattern.test(streamLauncher)) fail("stream-task-reuse", detail);
}
if (!/override fun onNewIntent\(newIntent: Intent\)/.test(streamActivity)) {
  fail("stream-task-reuse", "reused StreamActivity does not accept the refreshed intent");
}

// 5. StreamActivity's root is opaque black. On freeform/vendor compositors a media
// overlay Surface can remain behind that root even while MediaCodec reports
// rendered output, producing a fully black stream window.
const streamSurfaceSources = `${streamActivity}\n${streamSurfaceLayout}`;
if (!/setZOrderOnTop\(true\)/.test(streamSurfaceSources)) {
  fail("stream-surface-z-order", "decoder Surface must stay above the opaque Activity root");
}
if (/setZOrderMediaOverlay\(true\)/.test(streamSurfaceSources)) {
  fail("stream-surface-z-order", "media-overlay z-order is hidden behind the opaque Activity root");
}

if (failures > 0) {
  console.error(`architecture-check: ${failures} violation(s)`);
  process.exit(1);
} else {
  console.log("architecture-check: TS/Kotlin rules clean");
}
