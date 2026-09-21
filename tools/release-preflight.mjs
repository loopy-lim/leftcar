import {fileURLToPath} from 'node:url';
import {benchmarkProfile} from './benchmark-profile.mjs';
import {collectReleaseInputs,releaseScopes} from './release-inputs.mjs';
const args=process.argv.slice(2).filter(x=>x!=='--');
const usage='bun run release:preflight -- --scope <android-debug|android-internal|android-release|host-macos-internal> [--json]';
if(args.includes('--help'))console.log(usage);
else {
 try {
  if(args[0]!=='--scope'||!releaseScopes.includes(args[1])||args.slice(2).some(x=>x!=='--json')||args.length>3)throw new Error(usage);
  const profile=benchmarkProfile(process.env,{requireRoot:args[1]==='host-macos-internal'});
  if(profile&&args[1]==='android-release')throw new Error('Benchmark package is internal only');
  const inputs=await collectReleaseInputs(fileURLToPath(new URL('../',import.meta.url)),args[1],{cargoAuditDb:process.env.LEFTCAR_CARGO_AUDIT_DB??null});
  if(args.includes('--json'))console.log(JSON.stringify(inputs,null,2));
  else console.log(JSON.stringify({scope:inputs.scope,sourceCommit:inputs.sourceCommit,sourceSha256:inputs.sourceSha256,internalBuildable:inputs.internalBuildable,distributionReady:inputs.distributionReady,status:inputs.status,blockers:inputs.blockers,vulnerabilityChecks:inputs.vulnerabilityChecks.map(({tool,input,status,findings,warnings})=>({tool,input,status,findings:findings.length,warnings:warnings.length})),evidence:inputs.evidence},null,2));
  process.exitCode=args[1]==='android-release'&&!inputs.distributionReady?1:0;
 }catch(error) {
  // Never print scanner stderr, environment values or credential configuration.
  console.error(JSON.stringify({status:'invalid',internalBuildable:false,distributionReady:false,error:error.code?'Required local release input could not be read':error.message}));process.exitCode=1;
 }
}
