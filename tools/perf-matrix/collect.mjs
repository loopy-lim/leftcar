import {createCollectorIO} from './collector-io.mjs';
import {runCollection} from './collection-runtime.mjs';
import {saveCollection} from './collection-receipt.mjs';
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {dirname,resolve} from 'node:path';
import {sha256,verifyManifest,validateSourceBinding} from '../release-manifest.mjs';
// Explicit selected process/device only; never clears log buffers or changes settings.
const args=process.argv.slice(2),get=(key)=>args[args.indexOf(key)+1];
if(args.includes('--help')) {console.log('bun tools/perf-matrix/collect.mjs --context <run-context.json> --duration <seconds> --output <new-prefix> [--baseline <collection.json>]');process.exit(0);}
for(const key of ['--context','--duration','--output'])if(!args.includes(key))throw new Error(`${key} required`);
const duration=Number(get('--duration')),prefix=resolve(get('--output'));
if(!Number.isInteger(duration)||duration<1||duration>1800)throw new Error('Duration must be 1..1800 seconds');
const context=JSON.parse(readFileSync(get('--context'),'utf8'));
if(!context.serial||!context.package||!Number.isInteger(context.hostPid)||context.hostPid<1||!context.conditions?.workload||!context.sourceManifest||!context.hostManifest||!context.androidManifest)throw new Error('Explicit serial, package, Host PID, workload and source/Host/Android manifests required');
if(!/^[\w.]+$/.test(context.package))throw new Error('Invalid package identifier');
for(const suffix of ['.host.ndjson','.android.log','.collection.json','.comparison.json'])if(existsSync(prefix+suffix))throw new Error('Output already exists; choose a new prefix');
const provenance={},manifests={};
for(const key of ['hostManifest','androidManifest','sourceManifest']) {
 const bytes=readFileSync(context[key]);const manifest=JSON.parse(bytes.toString());
 manifests[key]=manifest;
 provenance[key]={path:resolve(context[key]),sha256:sha256(bytes),schema:manifest.schema??null,sourceSha256:manifest.source?.sha256??null,target:manifest.target??null,historicalObservedArtifactTarget:manifest.observedArtifactTarget??null};
}
const sourceBinding=validateSourceBinding(manifests.sourceManifest,manifests.hostManifest,manifests.androidManifest);
await verifyManifest(manifests.hostManifest);
await verifyManifest(manifests.androidManifest);
if(context.conditions.sourceSha256!=null && context.conditions.sourceSha256!==provenance.sourceManifest.sha256)throw new Error('Context source hash does not match actual source manifest');
const boundConditions={...context.conditions,sourceSha256:provenance.sourceManifest.sha256,sourceSnapshotSha256:sourceBinding.sha256};
mkdirSync(dirname(prefix),{recursive:true});
// Raw paths exist before acquisition starts, so failed preparation also has hashes.
writeFileSync(prefix+'.host.ndjson','');writeFileSync(prefix+'.android.log','');
const cancellation=new AbortController();
const stop=()=>cancellation.abort(new Error('Collection interrupted by signal'));
process.once('SIGINT',stop);process.once('SIGTERM',stop);
const io=createCollectorIO(context,prefix);
let acquisition;
try{acquisition=await runCollection(io,{durationMs:duration*1000,signal:cancellation.signal});}
finally{process.removeListener('SIGINT',stop);process.removeListener('SIGTERM',stop);}
const result=saveCollection(prefix,{schema:2,context,conditions:boundConditions,provenance,...acquisition},args.includes('--baseline')?get('--baseline'):null);
process.exitCode=result.exitCode;
console.log(`Saved ${prefix}.collection.json (${result.collection.status}); physical presentation/optical latency remain unmeasured.`);
