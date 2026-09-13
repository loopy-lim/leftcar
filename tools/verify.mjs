import { spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { diagnose, root } from './doctor.mjs';
export function requirePerfectReactDoctor(output) {
  const clean=output.replace(/\x1b\[[0-9;]*m/g,'');
  const scores=[...clean.matchAll(/Score:\s*(\d+)\s*\/\s*100/g)].map(m=>Number(m[1]));
  if (!scores.length || scores.some(s=>s!==100)) throw new Error('React Doctor must report actual Score: 100 / 100; missing or lower score fails');
}
function runCommand(command, args=[], cwd=root, capture=false) {
  console.log(`\n> ${command} ${args.join(' ')}`);
  const result=spawnSync(command,args,{cwd,encoding:'utf8',stdio:capture?'pipe':'inherit',maxBuffer:32*1024*1024});
  if(capture){process.stdout.write(result.stdout??'');process.stderr.write(result.stderr??'');}
  if(result.error || result.status!==0)throw new Error(`${command} failed (${result.status}): ${result.error?.message??''}`);
  return `${result.stdout??''}\n${result.stderr??''}`;
}
export function verify(scope='all', { platform = process.platform, diagnose: doctor = diagnose, run = runCommand } = {}) {
  if(!['all','js','rust'].includes(scope))throw new Error('Usage: bun run verify [all|js|rust]');
  if(!doctor(scope==='js'?'js':scope==='rust'?(platform==='darwin'?'host-macos':'rust'):'all'))throw new Error('Resolve doctor prerequisites first');
  if(scope!=='rust') {
    run('bun',['install','--frozen-lockfile']);
    requirePerfectReactDoctor(run('npx',['-y','react-doctor@latest','.','--verbose'],root,true));
    run('bun',['run','typecheck']);
    run('bun',['run','test']);
    run('bun',['run','test:contract']);
    run('bun',['run','test:architecture']);
    run('bun',['run','test:ui']);
  }
  if(scope!=='js') {
    run('cargo',['fmt','--all','--','--check']);
    run('cargo',['fmt','--manifest-path','apps/host-desktop/src-tauri/Cargo.toml','--','--check']);
    run('cargo',['run','-p','architecture-check','--locked']);
    run('cargo',['test','--workspace','--locked']);
    run('cargo',['test','--manifest-path','apps/host-desktop/src-tauri/Cargo.toml','--locked']);
    run('cargo',['clippy','--workspace','--all-targets','--locked','--','-D','warnings']);
    run('cargo',['clippy','--manifest-path','apps/host-desktop/src-tauri/Cargo.toml','--all-targets','--locked','--','-D','warnings']);
    if(platform==='darwin') {
      const dir=mkdtempSync(join(tmpdir(),'leftcar-verify-'));
      run('zsh',['tools/build-macos-capture-shim.zsh','library',join(dir,'libleftcar_capture.dylib')]);
      for(const mode of ['policy-test','retransmit-ring-test','split-test'])run('zsh',['tools/build-macos-capture-shim.zsh',mode,join(dir,mode)]);
      run('zsh',['tools/build-macos-capture-shim.zsh','split-policy-test',join(dir,'split-policy-test')]);
      run(join(dir,'split-policy-test'));
      run('zsh',['tools/build-macos-capture-shim.zsh','retransmit-policy-test',join(dir,'retransmit-policy-test')]);
      run(join(dir,'retransmit-policy-test'));
      console.log('Swift: full shim and existing adapter test binaries COMPILED ONLY; Pure production split and RTX policies (Foundation/CryptoKit) EXECUTED. Hardware adapters/encoder/capture UNVERIFIED.');
    }
  }
  if(scope==='all') run(platform==='win32'?'gradlew.bat':'./gradlew',[':app:testDebugUnitTest','--no-daemon'],join(root,'apps/viewer-expo/android'));
  console.log(`VERIFY ${scope} passed; app install, hardware/capture and device delivery are not verified.`);
}
if(import.meta.main)verify(process.argv.filter(a=>a!=='--')[2]??'all');
