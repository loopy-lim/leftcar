import { spawnSync } from 'node:child_process';
// Each suite owns and closes its isolated browser fixtures. Any exit fails the gate.
for (const suite of ['run.cjs', 'source-grants.cjs', 'pairing-grants.cjs', 'pairing-revoke.cjs', 'pairing-incarnation.cjs', 'pairing-ordering.cjs', 'retained.cjs', 'host-lifetime.cjs', 'hub-connect.cjs']) {
  console.log(`UI suite: ${suite}`);
  const result = spawnSync('bun', [`${import.meta.dir}/${suite}`], {stdio:'inherit', env:process.env});
  if(result.error || result.status !== 0) throw new Error(`${suite} failed (${result.status}): ${result.error?.message ?? ''}`);
}
