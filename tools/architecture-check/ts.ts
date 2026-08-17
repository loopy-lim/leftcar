/**
 * TS/Kotlin architecture rules (H01): regex-level enforcement that mirrors
 * tools/architecture-check (cargo) for non-Rust sources.
 *
 * Rules (docs/05 L0):
 * - generated TS contains no video payload types
 * - viewer app sources contain no input-injection symbols
 * - Kotlin shim imports only allowlisted packages (no java.net, no codec)
 * - Kotlin shim contains no business-layer markers (network/codec policy)
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

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
    if (name === "node_modules" || name === "dist" || name === ".git") continue;
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

function checkFiles(dir: string, patterns: Array<[RegExp, string]>, rule: string): void {
  for (const file of walk(dir)) {
    if (!/\.(ts|tsx|kt)$/.test(file)) continue;
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

// 2. viewer app: no input commands anywhere (P-01, T-06)
checkFiles(
  join(ROOT, "apps/viewer-android"),
  [
    [/sendKey|sendMouse|injectInput|sendTouch/i, "input-injection symbol in viewer"],
    [/readClipboard|writeClipboard|readFile|writeFile/, "file/clipboard access in viewer"],
  ],
  "viewer-no-input-commands",
);

// 3. Kotlin shim: import allowlist (docs/05 L0 kotlin_shim_imports_only_allowlisted_packages)
const KOTLIN_ALLOW = /^import (android\.|androidx\.|com\.facebook\.react\.|java\.lang\.|java\.util\.|kotlin\.)/;
for (const file of walk(join(ROOT, "apps/viewer-android/android"))) {
  if (!file.endsWith(".kt")) continue;
  const text = readFileSync(file, "utf8");
  for (const line of text.split("\n")) {
    const m = line.match(/^import\s+(.+)$/);
    if (m && !KOTLIN_ALLOW.test(line)) {
      fail("kotlin-import-allowlist", `${file}: ${line}`);
    }
  }
  // no codec/network policy in the shim (docs/09 §9 금지 타협)
  if (/MediaCodec|AMediaCodec|DatagramSocket|Socket\(/.test(text)) {
    fail("kotlin-no-policy", `${file}: codec/network symbols belong to the Rust core`);
  }
}

if (failures > 0) {
  console.error(`architecture-check: ${failures} violation(s)`);
  process.exit(1);
} else {
  console.log("architecture-check: TS/Kotlin rules clean");
}
