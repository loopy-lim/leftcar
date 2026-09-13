import { expect, test, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const calls = vi.hoisted(() => []);
vi.mock('node:child_process', async importOriginal => ({
  ...await importOriginal(),
  execFileSync: (command, args) => {
    calls.push([command, args]);
    if (command === 'bun') return JSON.parse(readFileSync(join(root, 'package.json'))).packageManager.split('@')[1];
    if (command === 'node') return 'v22.21.1';
    if (command === 'rustc') return `rustc ${readFileSync(join(root, 'rust-toolchain.toml'), 'utf8').match(/channel = "([^"]+)"/)[1]} fixture`;
    if (command === 'rustup') return 'x86_64-pc-windows-msvc';
    if (command === '/usr/bin/xcrun') throw new Error('Swift absent in platform fixture');
    throw new Error(`Unexpected command ${command}`);
  },
}));
import { diagnose, root } from './doctor.mjs';
import { verify } from './verify.mjs';

for (const platform of ['linux', 'win32']) {
  test(`Rust doctor on ${platform} does not require Swift`, () => {
    calls.length = 0;
    expect(diagnose('rust', { platform })).toBe(true);
    expect(calls.map(([command]) => command)).toContain('rustc');
    expect(calls.map(([command]) => command)).not.toContain('/usr/bin/xcrun');
  });
  test(`verify rust on ${platform} chooses neutral prerequisites and Cargo only`, () => {
    const scopes = [], commands = [];
    verify('rust', {
      platform,
      diagnose: scope => { scopes.push(scope); return diagnose(scope, { platform }); },
      run: (command, args) => { commands.push([command, args]); return ''; },
    });
    expect(scopes).toEqual(['rust']);
    expect(commands.length).toBeGreaterThan(0);
    expect(commands.every(([command]) => command === 'cargo')).toBe(true);
    expect(commands).toContainEqual(['cargo', ['test', '--manifest-path', 'apps/host-desktop/src-tauri/Cargo.toml', '--locked']]);
  });
}
test('macOS Rust verification still checks Swift and preserves pure RTX execution', () => {
  const commands = [], scopes = [];
  verify('rust', { platform: 'darwin', diagnose: scope => { scopes.push(scope); return true; }, run: (command, args) => { commands.push([command, args]); return ''; } });
  expect(scopes).toEqual(['host-macos']);
  const libraryBuild = commands.findIndex(([command, args]) => command === 'zsh' && args[1] === 'library');
  const hostTests = commands.findIndex(([command, args]) => command === 'cargo' && args[0] === 'test' && args.includes('apps/host-desktop/src-tauri/Cargo.toml'));
  expect(libraryBuild).toBeGreaterThanOrEqual(0);
  expect(commands[libraryBuild][1][2]).toBe(join(root, 'native/macos-capture-shim/libleftcar_capture.dylib'));
  expect(libraryBuild).toBeLessThan(hostTests);
  expect(commands.some(([command, args]) => command === 'zsh' && args[1] === 'retransmit-policy-test')).toBe(true);
  expect(commands.some(([command]) => command.endsWith('/retransmit-policy-test'))).toBe(true);
  expect(diagnose('host-macos', { platform: 'darwin' })).toBe(false);
});
test('explicit Windows scope still checks the Windows Rust target', () => {
  calls.length = 0;
  expect(diagnose('windows', { platform: 'win32' })).toBe(true);
  expect(calls).toContainEqual(['rustup', ['target', 'list', '--installed']]);
});
test('explicit Windows cross-check still requires a resource compiler', () => {
  const previous = process.env.PATH;
  try {
    process.env.PATH = '';
    expect(diagnose('windows', { platform: 'linux' })).toBe(false);
  } finally { process.env.PATH = previous; }
});
test('requesting macOS scope on Linux fails by platform without invoking xcrun', () => {
  calls.length = 0;
  expect(diagnose('host-macos', { platform: 'linux' })).toBe(false);
  expect(calls.map(([command]) => command)).not.toContain('/usr/bin/xcrun');
});
