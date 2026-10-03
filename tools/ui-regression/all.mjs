import { spawnSync } from 'node:child_process';
// Each suite owns and closes its isolated browser fixtures. Any exit fails the gate.
for (const suite of ['native-styles.mjs', 'host-screens.cjs', 'catalog-lifetime.cjs', 'stream-adaptive-lifetime.cjs', 'catalog-recovery-fence.cjs', 'viewer-screens.cjs', 'design-system.cjs', 'run.cjs', 'host-csp.cjs', 'pairing-grants.cjs', 'pairing-revoke.cjs', 'pairing-incarnation.cjs', 'pairing-ordering.cjs', 'retained.cjs', 'host-lifetime.cjs', 'hub-connect.cjs']) {
  console.log(`UI suite: ${suite}`);
  const result = spawnSync('bun', [`${import.meta.dir}/${suite}`], {stdio:'inherit', env:process.env});
  if(result.error || result.status !== 0) throw new Error(`${suite} failed (${result.status}): ${result.error?.message ?? ''}`);
}
