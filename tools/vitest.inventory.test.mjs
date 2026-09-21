import { expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
it('discovers this checkout and contracts without nested worktrees or generated/default-excluded files', async () => {
  const fixture = await mkdtemp(join(tmpdir(), 'leftcar-vitest-inventory-'));
  try {
    const expected = ['apps/viewer/current.test.ts', 'packages/control-generated/contract.test.ts'];
    const excluded = ['.worktrees/other/duplicate.test.ts', 'vendor/library/dist/generated.test.js',
      'cypress/browser.test.ts', '.cache/stale.test.ts', 'node_modules/library/vendor.test.ts'];
    for (const name of [...expected, ...excluded]) {
      await mkdir(dirname(join(fixture, name)), { recursive: true });
      await writeFile(join(fixture, name), 'throw new Error("file inventory must not execute tests");\n');
    }
    const config = join(root, 'vitest.config.mts');
    const output = execFileSync(process.execPath, [join(root, 'node_modules/vitest/vitest.mjs'),
      'list', '--root', fixture, '--filesOnly', '--json',
      ...(existsSync(config) ? ['--config', config] : [])], { cwd: root, encoding: 'utf8', timeout: 15_000 });
    const discovered = JSON.parse(output).map(({ file }) => relative(fixture, file).replaceAll('\\', '/')).sort();
    expect(discovered).toEqual(expected.sort());
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
});
