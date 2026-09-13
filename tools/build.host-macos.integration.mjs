// Explicit macOS CI integration, intentionally outside Vitest's *.test.*
// discovery. Portable unit tests never invoke the real macOS compiler/updater.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { realpathSync } from 'node:fs';
assert.equal(process.platform, 'darwin', 'Run this integration only on the macOS CI lane');
const env={...process.env,CARGO_TARGET_DIR:join(realpathSync(tmpdir()),'leftcar-updater-plan-fixture'),CARGO_BUILD_TARGET:'x86_64-unknown-linux-gnu'};
const parts=execFileSync('bun',['tools/build.host-plan.mjs','apps/host-desktop/src-tauri','Leftcar Host'],{env,encoding:'utf8'}).split('\0');
const triple=execFileSync('rustc',['-vV'],{encoding:'utf8'}).match(/^host: (.+)$/m)[1];
// Supply a canonical absolute environment override and require the exact path.
assert.equal(parts[0],env.CARGO_TARGET_DIR);
assert.equal(parts[1],join(parts[0],triple,'release/bundle/macos/Leftcar Host.app'));
assert.deepEqual(parts.slice(2),['--config','src-tauri/tauri.macos.conf.json','--bundles','app','--target',triple,'--','--locked']);
delete env.LEFTCAR_BENCHMARK_PROFILE;delete env.LEFTCAR_BENCHMARK_ROOT;
const shell=execFileSync('zsh',['tools/dev-host-macos.zsh','--print-build-plan'],{env,encoding:'utf8'}).split('\0');
assert.deepEqual(shell,[...parts,'']);
console.log(`PASS macOS Host plan CLI and updater shell argument integration (${triple}); no build, signing, install, launch or capture`);
