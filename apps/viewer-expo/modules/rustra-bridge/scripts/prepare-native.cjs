// ── rustra generated ────────────────────────────────────────
// File:   scripts/prepare-native.cjs
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

'use strict';
const { existsSync, readFileSync } = require('node:fs');
const { dirname, join, resolve } = require('node:path');
const { spawnSync } = require('node:child_process');
const appRoot = resolve(__dirname, "../../..");

function run(command, args, cwd) {
  console.log('[rustra:native] ' + [command, ...args].join(' '));
  const result = spawnSync(command, args, {
    cwd, stdio: 'inherit',
    shell: process.platform === 'win32' && command.endsWith('.bat'),
  });
  if (result.error || result.status !== 0) {
    const detail = result.error ? result.error.message : 'exit ' + (result.status ?? result.signal);
    throw new Error([command, ...args].join(' ') + ' failed in ' + cwd + ': ' + detail +
      '. Install the required native tools separately, fix this step, then rerun preparation.');
  }
}

function prepareNative(platform) {
  if (platform !== 'ios' && platform !== 'android') {
    throw new Error('Choose ios or android: node scripts/prepare-native.cjs <platform>');
  }
  const manifest = JSON.parse(readFileSync(join(appRoot, 'package.json'), 'utf8'));
  const expo = Boolean(manifest.dependencies?.expo || manifest.devDependencies?.expo);
  const project = join(appRoot, platform);
  if (!existsSync(project)) {
    if (!expo) throw new Error('Missing ' + project + '. Use an existing bare React Native native project, or configure an Expo development build.');
    let cli;
    try {
      cli = join(dirname(require.resolve('expo/package.json', { paths: [appRoot] })), 'bin/cli');
    } catch {
      throw new Error('Expo is declared but not installed. Run your package-manager install in ' + appRoot + ', then rerun preparation.');
    }
    run(process.execPath, [cli, 'prebuild', '--platform', platform, '--no-install'], appRoot);
  }
  if (platform === 'ios') {
    if (!existsSync(join(project, 'Podfile'))) {
      throw new Error('Existing ios project has no Podfile at ' + project + '. Preserve your project and restore its Podfile; automatic prebuild only creates absent native directories.');
    }
    const bundled = existsSync(join(appRoot, 'Gemfile')) || existsSync(join(project, 'Gemfile'));
    run(bundled ? 'bundle' : 'pod', bundled ? ['exec', 'pod', 'install'] : ['install'], project);
  } else {
    const wrapper = process.platform === 'win32' ? 'gradlew.bat' : 'gradlew';
    if (!existsSync(join(project, wrapper))) {
      throw new Error('Existing android project has no ' + wrapper + ' at ' + project + '. Restore its Gradle wrapper, then rerun preparation.');
    }
    run(process.platform === 'win32' ? wrapper : './' + wrapper,
      [':app:assembleDebug', '--no-daemon', '-Pandroid.builder.sdkDownload=false'], project);
  }
  console.log('[rustra:native] Prepared ' + platform + '. Next: rebuild/launch your native app (' +
    (expo ? 'expo run:' + platform : 'react-native run-' + platform) +
    '). Import typed commands from your generated/react-native entry. Expo Go cannot load Rustra JSI.');
}

function prepare(platform) {
  try { prepareNative(platform); }
  catch (error) { console.error('[rustra:native] ' + error.message); process.exitCode = 1; }
}
module.exports = { prepare };
if (require.main === module) prepare(process.argv[2]);
