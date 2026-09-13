import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync, accessSync, constants, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve, join, delimiter } from 'node:path';
import semver from 'semver';
import { chromium } from 'playwright-core';
export const root = resolve(import.meta.dirname, '..');
const require = createRequire(join(root, 'apps/viewer-expo/package.json'));
export function diagnose(scope = 'all', { platform = process.platform } = {}) {
  if (!['all','js','android','host-macos','windows','rust'].includes(scope)) throw new Error(`Unknown doctor scope: ${scope}`);
  let failures = 0;
  const check = (label, fn) => { try { console.log(`OK ${label}: ${fn()}`); } catch (e) { failures++; console.error(`MISSING ${label}: ${e.message}`); } };
  const version = (cmd, args=['--version']) => execFileSync(cmd,args,{encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
  check('Bun root entry', () => { const pin = JSON.parse(readFileSync(join(root,'package.json'))).packageManager.split('@')[1]; const actual=version('bun'); if(actual!==pin) throw new Error(`need bun ${pin}, found ${actual}`); return actual; });
  check('frozen JavaScript dependencies', () => {
    for (const name of ['react-native','@react-native/gradle-plugin']) {
      const pkg=require(`${name}/package.json`); const node=version('node');
      if (!semver.satisfies(node,pkg.engines.node)) throw new Error(`${name}@${pkg.version} requires Node ${pkg.engines.node}; found ${node}`);
    }
    return `Node ${version('node')} satisfies installed React Native / Gradle plugin engines (Bun remains root entry)`;
  });
  if(scope==='js' || scope==='all')check('isolated browser fixture prerequisite', () => {
    const binary=process.env.CHROMIUM_PATH || chromium.executablePath();
    if(!existsSync(binary))throw new Error('run bun run setup:browser to download the pinned isolated browser runtime');
    return binary;
  });
  if (scope !== 'js') {
    check('Rust locked toolchain', () => {const pin=readFileSync(join(root,'rust-toolchain.toml'),'utf8').match(/channel = "([^"]+)"/)[1];const found=version('rustc');if(!found.startsWith(`rustc ${pin} `))throw new Error(`need rustc ${pin}; found ${found}`);return found;});
  }
  if (scope==='all' || scope==='android') {
    check('Java (JAVA_HOME)', () => version(process.env.JAVA_HOME ? join(process.env.JAVA_HOME,'bin/java') : 'java', ['-version']) || 'java available');
    check('Android SDK + pinned NDK', () => {
      const sdk=process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
      if(!sdk)throw new Error('set ANDROID_HOME to the installed SDK');
      const ndk=join(sdk,'ndk/27.1.12297006');if(!existsSync(ndk))throw new Error(`install ndk;27.1.12297006 under ${sdk}`);
      return `${sdk}; NDK 27.1.12297006; Cargo target ${process.env.CARGO_TARGET_DIR || join(root,'target')}`;
    });
    const debugKey=process.env.LEFTCAR_INTERNAL_DEBUG_KEYSTORE??join(root,'apps/viewer-expo/android/app/debug.keystore');
    let debugKeyReady=false;try {debugKeyReady=statSync(debugKey).isFile();}catch{}
    console.log(debugKeyReady?'INFO existing debug/internal signing file available (certificate checked during packaging)':'NOTE debug/internal APK packaging needs an existing app/debug.keystore or absolute LEFTCAR_INTERNAL_DEBUG_KEYSTORE; no automatic key creation');
    check('Android Rust target', () => {if(!version('rustup',['target','list','--installed']).includes('aarch64-linux-android'))throw new Error('rustup target add aarch64-linux-android');return 'aarch64-linux-android';});
  }
  if (scope==='host-macos' || (scope==='all' && platform==='darwin')) {
    if (platform !== 'darwin') check('macOS Host platform', () => { throw new Error('host-macos checks require macOS'); });
    else check('Swift compiler', () => version('/usr/bin/xcrun',['swiftc','--version']));
  }
  if(scope==='windows') {
    check('Windows Rust target', () => {const target='x86_64-pc-windows-msvc';if(!version('rustup',['target','list','--installed']).includes(target))throw new Error(`rustup target add ${target}`);return target;});
    if(platform!=='win32')check('Windows resource compiler for cross-check', () => {
      for(const directory of (process.env.PATH??'').split(delimiter)) {
        const path=join(directory,'llvm-rc');
        try { accessSync(path,constants.X_OK); return path; } catch {}
      }
      throw new Error('put llvm-rc from the installed LLVM toolchain on PATH');
    });
  }
  console.log('Scopes: Host desktop; installable Viewer = apps/viewer-expo/android; legacy apps/viewer-android = TypeScript/specimen tests only.');
  console.log('Hardware/capture/device delivery remains UNVERIFIED by doctor.');
  return failures===0;
}
if (import.meta.main && !diagnose(process.argv[2] || 'all')) process.exitCode=1;
