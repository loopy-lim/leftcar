import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { compile } from "@tailwindcss/node";
import { Scanner } from "@tailwindcss/oxide";

export async function compileUiStyles(outdir) {
  for (const [name, relative] of [
    ["host", "../../apps/host-desktop/src/index.css"],
    ["viewer", "../../apps/viewer-expo/global.css"],
  ]) {
    const file = resolve(import.meta.dir, relative);
    const compiler = await compile(await readFile(file, "utf8"), { base: dirname(file), onDependency() {} });
    const scanner = new Scanner({ sources: [...compiler.sources, { base: dirname(file), pattern: "**/*", negated: false }] });
    const reset = name === "viewer" ? "@layer base { html, body, #root { height: 100%; width: 100%; margin: 0; } #root > [data-rn-view] { height: 100%; } [data-rn-view] { display: flex; flex-direction: column; } }\n" : "";
    await writeFile(`${outdir}/${name}.css`, reset + compiler.build(scanner.scan()));
  }
}
