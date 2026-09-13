// Durable, immutable profile-only adaptation for exact baseline068. This never
// derives new hunks from the evolving candidate or an ephemeral source snapshot.
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname, resolve } from 'node:path';
import { sha256 } from './release-manifest.mjs';
import { root } from './doctor.mjs';
const output=process.argv[2];
if(!output)throw new Error('Usage: bun tools/benchmark-profile.adapt.mjs <output-directory>');
const patch=await readFile(join(root,'tools/benchmark-profile.baseline068.patch'));
const metadata=JSON.parse(await readFile(join(root,'tools/benchmark-profile.baseline068.json'),'utf8'));
if(metadata.baseline!=='068b6628df5dc57f264446f8ec51215b37c51b6f' || sha256(patch)!==metadata.patchSha256)throw new Error('Frozen baseline adaptation hash mismatch');
const fixture=await mkdtemp(join(tmpdir(),'leftcar-adapted-baseline-'));
for(const file of metadata.files) {
 if(file.beforeSha256===null)continue;
 const before=execFileSync('git',['-C',root,'show',`${metadata.baseline}:${file.path}`]);
 if(sha256(before)!==file.beforeSha256)throw new Error(`Exact baseline source mismatch: ${file.path}`);
 await mkdir(dirname(join(fixture,file.path)),{recursive:true});await writeFile(join(fixture,file.path),before);
}
execFileSync('git',['apply','-'],{cwd:fixture,input:patch,stdio:['pipe','pipe','pipe']});
for(const file of metadata.files)if(sha256(await readFile(join(fixture,file.path)))!==file.afterSha256)throw new Error(`Adapted source mismatch: ${file.path}`);
await mkdir(output,{recursive:true});
await writeFile(join(output,'profile-only-baseline068.patch'),patch);
await writeFile(join(output,'profile-only-baseline068.json'),JSON.stringify(metadata,null,2)+'\n');
console.log(JSON.stringify({label:metadata.label,patch:resolve(output,'profile-only-baseline068.patch'),sha256:metadata.patchSha256,files:metadata.files.length,verifiedFixture:fixture}));
