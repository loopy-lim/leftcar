import {test,expect,afterEach} from 'vitest';
import {mkdtemp,mkdir,writeFile,readFile,rm,cp} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {execFileSync,spawnSync} from 'node:child_process';
import * as manifest from './release-manifest.mjs';
const roots=[];
afterEach(async()=>{for(const root of roots.splice(0))await rm(root,{recursive:true,force:true});});
async function fixture() {
 const root=await mkdtemp(join(tmpdir(),'leftcar-preflight-'));roots.push(root);
 for(const path of ['package.json','bun.lock','Cargo.lock','apps/host-desktop/src-tauri/Cargo.toml','apps/host-desktop/src-tauri/Cargo.lock','apps/host-desktop/src-tauri/tauri.conf.json','apps/host-desktop/src-tauri/src/ffi.rs','apps/viewer-expo/package.json','apps/viewer-expo/app.config.ts','apps/viewer-expo/android/app/build.gradle','apps/viewer-expo/android/gradle.properties','apps/viewer-expo/android/gradle/wrapper/gradle-wrapper.properties','.cargo/config.toml','native/macos-capture-shim/Sources']) {
  await mkdir(join(root,path,'..'),{recursive:true});await cp(resolve(path),join(root,path),{recursive:true});
 }
 execFileSync('git',['init',root],{stdio:'ignore'});execFileSync('git',['-C',root,'add','.']);execFileSync('git',['-C',root,'-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','-m','fixture'],{stdio:'ignore'});
 return root;
}
const unavailable=()=>({status:null,error:{code:'ENOENT'},stdout:''});
const collect=async(root,scope='android-internal')=>(await import('./release-inputs.mjs')).collectReleaseInputs(root,scope,{run:unavailable});
test('source binding accepts snapshot and wrapper but rejects missing, mixed and altered evidence',async()=>{
 const root=await fixture(),source=await manifest.sourceSnapshot(root);
 expect(manifest.validateSourceBinding(source,{source},{source})).toEqual({commit:source.commit,sha256:source.sha256});
 const mixed=structuredClone(source);mixed.files[0].sha256='0'.repeat(64);mixed.sha256=manifest.sha256(JSON.stringify(mixed.files));expect(()=>manifest.validateSourceBinding(source,mixed)).toThrow(/binding mismatch/i);
 for(const bad of [{}, {...source,commit:'0'.repeat(40)}, {...source,files:[]}, {...source,files:[{path:'../escape',deleted:true}]}])expect(()=>manifest.validateSourceBinding(source,bad)).toThrow(/source/i);
});
test('local preflight records distinct version roles, exact locks and unavailable checks without claiming distribution readiness',async()=>{
 const root=await fixture(),inputs=await collect(root);
 expect(inputs.components.host.cargoVersion).toBe(inputs.components.host.tauriVersion);
 expect(inputs.components.shim.requiredStartAbi).toBe(9);
 expect(inputs.components.expo.packageVersion).toBe('0.1.6');
 expect(inputs.components.viewerAndroid.versionName).toBe('0.1.6');
 expect(inputs.internalBuildable).toBe(true);expect(inputs.distributionReady).toBe(false);
 expect(inputs.dependencies.map(x=>x.lockfile)).toContain('bun.lock');
 expect(inputs.dependencies.find(x=>x.ecosystem==='bun').records.length).toBeGreaterThan(100);
 expect(inputs.vulnerabilityChecks.some(x=>x.status==='unavailable')).toBe(true);
 expect(inputs.components.root.version).toBeNull();
});
test.each([
 ['apps/host-desktop/src-tauri/tauri.conf.json','"version": "0.1.3"','"version": "9.9.9"'],
 ['apps/viewer-expo/android/gradle.properties','reactNativeArchitectures=arm64-v8a','reactNativeArchitectures=x86_64'],
 ['apps/viewer-expo/android/app/build.gradle','"--target", "aarch64-linux-android"','"--target", "x86_64-linux-android"'],
 ['apps/viewer-expo/android/gradle/wrapper/gradle-wrapper.properties','b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06','0'.repeat(64)],
 ['apps/host-desktop/src-tauri/src/ffi.rs','b"leftcar_capture_start_v9"','b"leftcar_capture_start_v99"'],
])('preflight rejects structural mismatch in %s',async(path,from,to)=>{
 const root=await fixture(),file=join(root,path),text=await readFile(file,'utf8');expect(text).toContain(from);await writeFile(file,text.replace(from,to));
 await expect(collect(root)).rejects.toThrow();
});
test('release inputs reject altered records, rehashed missing dependencies and source-lock disagreement',async()=>{
 const {validateReleaseInputs,sealReleaseInputs}=await import('./release-inputs.mjs');
 const root=await fixture(),inputs=await collect(root),source=await manifest.sourceSnapshot(root);
 expect(()=>validateReleaseInputs(inputs,source)).not.toThrow();
 const changed=structuredClone(inputs);changed.components.host.cargoVersion='9';expect(()=>validateReleaseInputs(changed,source)).toThrow();
 const missing=structuredClone(inputs);missing.dependencies=[];expect(()=>validateReleaseInputs(sealReleaseInputs(missing),source)).toThrow();
 const lock=structuredClone(inputs);lock.dependencies[0].lockSha256='0'.repeat(64);expect(()=>validateReleaseInputs(sealReleaseInputs(lock),source)).toThrow();
});
test('Android signing classification comes from verified certificate subject, never requested scope',async()=>{
 const {classifyAndroidSigning}=await import('./release-inputs.mjs');
 const debug='Signer #1 certificate DN: CN=Android Debug, O=Android, C=US\nSigner #1 certificate SHA-256 digest: '+ 'a'.repeat(64);
 expect(classifyAndroidSigning(debug,'android-release').classification).toBe('debug-key');
 expect(classifyAndroidSigning(debug,'android-internal').classification).toBe('internal-debug-key');
 expect(()=>classifyAndroidSigning('', 'android-release')).toThrow();
 const release=classifyAndroidSigning(debug.replace('Android Debug','Leftcar Release'),'android-release');expect(release.classification).toBe('configured-release-key');
});
test('collector refuses mixed snapshots before creating output directory or starting acquisition',async()=>{
 const root=await fixture(),source=await manifest.sourceSnapshot(root);
 const context={serial:'fixture',package:'leftcar.ll3.kr',hostPid:1,conditions:{workload:'fixture'},sourceManifest:join(root,'source.json'),hostManifest:join(root,'host.json'),androidManifest:join(root,'android.json')};
 await writeFile(context.sourceManifest,JSON.stringify(source));
 for(const key of ['hostManifest','androidManifest'])await writeFile(context[key],JSON.stringify({schema:1,source:{...source,commit:'0'.repeat(40)},artifacts:[]}));
 await writeFile(join(root,'context.json'),JSON.stringify(context));
 const output=join(root,'must-not-exist/run');
 const result=spawnSync('bun',['tools/perf-matrix/collect.mjs','--context',join(root,'context.json'),'--duration','1800','--output',output],{encoding:'utf8'});
 expect(result.status).not.toBe(0);expect(result.stderr).toMatch(/source/i);
 await expect(readFile(join(root,'must-not-exist/run.host.ndjson'))).rejects.toThrow();
 const {existsSync}=await import('node:fs');expect(existsSync(join(root,'must-not-exist'))).toBe(false);
});
test('scan outcomes preserve findings/warnings and fail closed on errors and malformed success',async()=>{
 const {parseVulnerabilityResult}=await import('./release-dependencies.mjs');
 expect(parseVulnerabilityResult('bun',unavailable()).status).toBe('unavailable');
 for(const output of [{status:2,stdout:'{}'},{status:0,stdout:'not JSON'},{status:1,stdout:'{}'},{status:0,stdout:'{"error":"bad"}'}])expect(parseVulnerabilityResult('bun',output).status).toBe('error');
 expect(parseVulnerabilityResult('bun',{status:0,stdout:'{}'}).status).toBe('pass');
 const finding={status:1,stdout:JSON.stringify({dependency:[{id:1,url:'https://github.com/advisories/GHSA-fixture',title:'fixture',severity:'high',vulnerable_versions:'<1'}]})};
 expect(parseVulnerabilityResult('bun',finding).findings).toHaveLength(1);
 const warning={package:{name:'fixture',version:'1.0.0'},advisory:{id:'RUSTSEC-fixture',title:'Unmaintained'},kind:'unmaintained'};
 const cargo={database:{'advisory-count':1},vulnerabilities:{found:false,count:0,list:[]},warnings:{unmaintained:[warning]}};
 const parsed=parseVulnerabilityResult('cargo',{status:0,stdout:JSON.stringify(cargo)});expect(parsed.status).toBe('findings');expect(parsed.warnings[0].targetReachability).toContain('not-evaluated');
 cargo.vulnerabilities.found=true;expect(parseVulnerabilityResult('cargo',{status:0,stdout:JSON.stringify(cargo)}).status).toBe('error');
});
test('artifact release binding requires metadata, actual signing and matching scope/version/target',async()=>{
 const {bindArtifactReleaseInputs,classifyAndroidSigning,validateReleaseInputs}=await import('./release-inputs.mjs');
 const root=await fixture(),inputs=await collect(root),source=await manifest.sourceSnapshot(root);
 const signing=classifyAndroidSigning('Signer #1 certificate DN: CN=Android Debug, O=Android, C=US\nSigner #1 certificate SHA-256 digest: '+'a'.repeat(64),'android-internal');
 const args={source,versions:{android:{versionName:'0.1.6',versionCode:5},host:'0.1.3',expoConfig:'0.1.6'},target:{platform:'android',architecture:'arm64',abi:'arm64-v8a',triple:'aarch64-linux-android'},artifactMetadata:{identifier:'leftcar.ll3.kr',version:'0.1.6',versionCode:5},signing,artifacts:[{role:'viewer-apk'}]};
 const bound=bindArtifactReleaseInputs(inputs,args);expect(bound.distributionReady).toBe(false);expect(()=>validateReleaseInputs(bound,source)).not.toThrow();
 for(const change of [{artifactMetadata:null},{artifactMetadata:{...args.artifactMetadata,version:'0.1.4'}},{target:{...args.target,abi:'x86_64'}},{signing:{...signing,classification:'configured-release-key'}},{artifacts:[]}])expect(()=>bindArtifactReleaseInputs(inputs,{...args,...change})).toThrow();
});
test('rehashed invalid component types and removed Swift source records still fail closed',async()=>{
 const {sealReleaseInputs,validateReleaseInputs}=await import('./release-inputs.mjs');
 const root=await fixture(),inputs=await collect(root),source=await manifest.sourceSnapshot(root);
 const bad=structuredClone(inputs);bad.components.expo.version={unexpected:true};expect(()=>validateReleaseInputs(sealReleaseInputs(bad),source)).toThrow();
 const missing=structuredClone(inputs);const path=missing.configurationInputs.find(x=>x.path.endsWith('.swift')).path;missing.configurationInputs=missing.configurationInputs.filter(x=>x.path!==path);expect(()=>validateReleaseInputs(sealReleaseInputs(missing),source)).toThrow();
});
test('schema2 release manifests bind artifact versions, source inputs, signing and current artifact hashes',async()=>{
 const {classifyAndroidSigning}=await import('./release-inputs.mjs');
 const root=await fixture();
 const output=await mkdtemp(join(tmpdir(),'leftcar-release-output-'));roots.push(output);
 await mkdir(join(output,'lib/arm64-v8a'),{recursive:true});
 const elf=Buffer.alloc(64);elf.write('\x7fELF');elf[4]=2;elf[5]=1;elf.writeUInt16LE(183,18);await writeFile(join(output,'lib/arm64-v8a/libleftcar_viewer.so'),elf);
 execFileSync('zip',['-qr','app.apk','lib'],{cwd:output});
 const before=await manifest.sourceSnapshot(root),releaseInputs=await collect(root);
 const signing=classifyAndroidSigning('Signer #1 certificate DN: CN=Android Debug, O=Android, C=US\nSigner #1 certificate SHA-256 digest: '+'a'.repeat(64),'android-internal');
 const args={root,before,releaseInputs,signing,versions:{android:{versionName:'0.1.6',versionCode:5},host:'0.1.3',expoConfig:'0.1.6'},artifactMetadata:{identifier:'leftcar.ll3.kr',version:'0.1.6',versionCode:5},target:{platform:'android',architecture:'arm64',abi:'arm64-v8a',triple:'aarch64-linux-android'},artifacts:[{role:'viewer-apk',path:join(output,'app.apk')}]};
 const receipt=await manifest.createManifest(args);expect(await manifest.verifyManifest(receipt)).toBe(true);
 const missingInputs={...receipt};delete missingInputs.releaseInputs;await expect(manifest.verifyManifest(missingInputs)).rejects.toThrow();
 await expect(manifest.verifyManifest({...receipt,artifactMetadata:{...receipt.artifactMetadata,versionCode:3}})).rejects.toThrow();
 await expect(manifest.verifyManifest({...receipt,versions:{...receipt.versions,host:'9.9.9'}})).rejects.toThrow();
 await expect(manifest.verifyManifest({...receipt,signing:{...signing,classification:'configured-release-key'}})).rejects.toThrow();
 const tampered=structuredClone(receipt);tampered.releaseInputs.dependencies[0].records=[];await expect(manifest.verifyManifest(tampered)).rejects.toThrow();
 await writeFile(join(output,'app.apk'),'changed');await expect(manifest.verifyManifest(receipt)).rejects.toThrow();
});
test('Android build-tools V2 Signer certificate output is recognized without recording key configuration',async()=>{
 const {classifyAndroidSigning}=await import('./release-inputs.mjs');
 const result=classifyAndroidSigning('V2 Signer: certificate DN: CN=Android Debug, O=Android, C=US\nV2 Signer: certificate SHA-256 digest: dd0f47dc0791450eac89ac2d304930d642ff32ad888a618a1992a6225f3b9655','android-release');
 expect(result.classification).toBe('debug-key');expect(result.debugCertificate).toBe(true);
});
test('public preflight rejects benchmark profile before any scans and does not reveal environment values',()=>{
 const result=spawnSync('bun',['tools/release-preflight.mjs','--scope','android-release','--json'],{encoding:'utf8',env:{...process.env,LEFTCAR_BENCHMARK_PROFILE:'private-fixture'}});
 expect(result.status).toBe(1);expect(result.stderr).toContain('Benchmark package is internal only');expect(result.stderr).not.toContain('private-fixture');
});
