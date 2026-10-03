import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { renderStreamUiTokens } from "../packages/ui-tokens/src/theme-kotlin";
import { renderThemeCss } from "../packages/ui-tokens/src/theme-css";

const root = fileURLToPath(new URL("..", import.meta.url));
for (const [path, expected] of [
  ["apps/host-desktop/src/theme.css", renderThemeCss("host")],
  ["apps/viewer-expo/theme.css", renderThemeCss("viewer")],
  ["apps/viewer-expo/android/app/src/main/java/dev/leftcar/viewer/stream/StreamUiTokens.kt", renderStreamUiTokens()],
] as const) {
  const file = resolve(root, path);
  if (process.argv.includes("--check")) {
    const actual = await readFile(file, "utf8");
    if (actual !== expected) throw new Error(`${path} has drifted. Run bun tools/generate-ui-theme.ts.`);
  } else {
    await writeFile(file, expected);
  }
}
