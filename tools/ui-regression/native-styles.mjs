import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { actionVariants, actionLabelVariants, inputVariants, textVariants } from "../../packages/ui-tokens/src/recipes.ts";
import { colors, hitTargets, typography } from "../../packages/ui-tokens/src/tokens.ts";

// Exercise the installed production Uniwind converter, not the DOM fixture's
// CSS substitute. Only its bundler alias is resolved here; no styles are mocked.
const root = resolve(import.meta.dir, "../..");
const source = resolve(root, "node_modules/uniwind/src");
const out = process.env.UI_TEST_DIR ?? "/tmp/leftcar-task4-ui";
await mkdir(out, { recursive: true });
const entry = `${out}/native-compiler.ts`;
await writeFile(entry, `export { compileCSS } from ${JSON.stringify(`${source}/bundler/css-compiler/compileCSS.ts`)}; export { UniwindBundlerConfig } from ${JSON.stringify(`${source}/bundler/config.ts`)};`);
const result = await Bun.build({ entrypoints: [entry], outdir: out, naming: "native-compiler-bundle.[ext]", target: "bun", plugins: [{
  name: "uniwind-bundler-alias", setup(build) {
    build.onResolve({ filter: /^@\// }, args => ({ path: Bun.resolveSync(resolve(source, args.path.slice(2)), root) }));
    build.onResolve({ filter: /^(lightningcss|@tailwindcss\/node|@tailwindcss\/oxide)$/ }, args => ({ path: Bun.resolveSync(args.path, source), external: true }));
  },
}] });
if (!result.success) throw new Error(result.logs.join("\n"));
// Loading the emitted module avoids Bun's cached lookup for a path that was
// absent when this process started (the first run must also pass).
const { compileCSS, UniwindBundlerConfig } = await import(`data:text/javascript;base64,${Buffer.from(await result.outputs[0].text()).toString("base64")}`);
const code = await compileCSS(new UniwindBundlerConfig({ cssEntryFile: "apps/viewer-expo/global.css" }, "android"));
await writeFile(`${out}/viewer-native-styles.js`, code);
const rt = { screen: { width: 360, height: 640 }, fontScale: 1, insets: { top: 0, bottom: 0, left: 0, right: 0 }, colorScheme: "light" };
const compiled = new Function("rt", `return ${code}`)(rt);
for (const theme of ["light", "dark"]) {
  rt.colorScheme = theme;
  const functions = { ...compiled.vars, ...compiled.scopedVars[`__uniwind-theme-${theme}`] };
  const vars = functions;
  const style = (classes, focused = false) => Object.fromEntries(classes.split(/\s+/).flatMap(className => {
    assert.ok(compiled.stylesheet[className], `missing native class ${className}`);
    return compiled.stylesheet[className].filter(rule => (rule.theme === null || rule.theme === theme) && (rule.focus === null || rule.focus === focused) && rule.disabled !== true && rule.active !== true)
      .flatMap(rule => rule.entries.map(([property, value]) => [property, value(vars)]));
  }));
  const action = style(actionVariants());
  const label = style(actionLabelVariants());
  const field = style(inputVariants());
  const text = style(textVariants());
  const focus = style("focus:outline-2 focus:outline-offset-2 focus:outline-focus", true);
  assert.equal(action.minHeight, 48);
  assert.equal(action.backgroundColor.toLowerCase(), colors[theme].btnPrimaryBg.toLowerCase());
  assert.equal(label.color.toLowerCase(), colors[theme].btnPrimaryText.toLowerCase());
  assert.equal(label.fontSize, 15);
  assert.equal(text.fontSize, 15);
  assert.equal(field.minHeight, 44);
  assert.equal(style(actionVariants({ size: "compact" })).minHeight, hitTargets.mobileMin);
  assert.equal(style(actionVariants({ size: "icon" })).height, hitTargets.mobileMin);
  assert.equal(style(actionVariants({ size: "icon" })).width, hitTargets.mobileMin);
  assert.equal(style(textVariants({ variant: "caption" })).fontSize, typography.fontSize.xs);
  assert.equal(focus.outlineWidth, 2);
  assert.equal(focus.outlineColor.toLowerCase(), colors[theme].borderFocus.toLowerCase());
  console.log(`PASS Android Uniwind ${theme}: actual native actions, text, fields and focus styles`);
}
