import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// CPU-only comparison: no screen capture, network or device access. A full
// commit ID makes the baseline reproducible even after this work is committed.
const baseline = process.argv[2] ?? 'cc504306dee40b6e83570f36aec3af23d445e686';
if (!/^[a-f0-9]{40}$/.test(baseline) || process.argv.length > 3) {
  throw new Error('Usage: bun tools/perf-matrix/fec-benchmark.mjs [full-baseline-commit-sha]');
}
const root = fileURLToPath(new URL('../../', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'leftcar-fec-benchmark-'));
function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8', timeout: 120_000 });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} failed: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout;
}
try {
  const source = 'crates/fec-core/src/lib.rs';
  writeFileSync(join(dir, 'before.rs'), run('git', ['show', `${baseline}:${source}`]));
  for (const [name, path] of [['fec_baseline', join(dir, 'before.rs')], ['fec_updated', resolve(root, source)]]) {
    run('rustc', ['--crate-name', name, '--crate-type', 'rlib', '--edition=2021', '-O', path, '-o', join(dir, `lib${name}.rlib`)]);
  }
  const binary = join(dir, 'fec-benchmark');
  run('rustc', ['--edition=2021', '-O', 'tools/perf-matrix/fec-benchmark.rs',
    '--extern', `fec_baseline=${join(dir, 'libfec_baseline.rlib')}`,
    '--extern', `fec_updated=${join(dir, 'libfec_updated.rlib')}`, '-o', binary]);
  console.log(`baseline=${baseline}; ${run('rustc', ['--version']).trim()}; platform=${process.platform}/${process.arch}`);
  console.log('CPU microbenchmark only; 8 x 1124-byte payloads; 20000 iterations/sample; alternating execution order. Not device FPS.');
  process.stdout.write(run(binary, []));
} finally {
  rmSync(dir, { recursive: true, force: true });
}
