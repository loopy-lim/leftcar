import { spawnSync, execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir, mkdtemp, readdir, cp, realpath, stat } from 'node:fs/promises';
import { join, resolve, basename, relative, isAbsolute, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { diagnose, root } from './doctor.mjs';
import { cargoTargetDirectory, hostBuildPlan } from './build.host-plan.mjs';
import { benchmarkProfile } from './benchmark-profile.mjs';
import { sourceSnapshot, createManifest, hashPath } from './release-manifest.mjs';
const args=process.argv.slice(2).filter(a=>a!=='--');
const scope=args[0];
if(!['android-debug','android-internal','android-release','host-macos-internal'].includes(scope))throw new Error('Usage: bun run build <android-debug|android-internal|android-release|host-macos-internal>');
const host=scope==='host-macos-internal';
if(!diagnose(host?'host-macos':'android'))throw new Error('Resolve doctor prerequisites first');
const profile=benchmarkProfile(process.env,{requireRoot:host});
if(profile && scope==='android-release')throw new Error('Benchmark package is internal only');
const buildEnv={...process.env};
if(buildEnv.CARGO_TARGET_DIR)buildEnv.CARGO_TARGET_DIR=resolve(root,buildEnv.CARGO_TARGET_DIR);
delete buildEnv.LEFTCAR_BENCHMARK_BUILD_PROFILE;
if(host && profile)buildEnv.LEFTCAR_BENCHMARK_BUILD_PROFILE=profile.slug;
// Local internal packaging never submits notarization, even if the caller's
// shell happens to contain signing/notarization account variables.
for(const name of ['APPLE_ID','APPLE_PASSWORD','APPLE_TEAM_ID','APPLE_API_KEY','APPLE_API_ISSUER','APPLE_API_KEY_PATH','APPLE_SIGNING_IDENTITY','APPLE_CERTIFICATE','APPLE_CERTIFICATE_PASSWORD'])delete buildEnv[name];
const run=(cmd,args,cwd=root,env=buildEnv)=>{
 const result=spawnSync(cmd,args,{cwd,env,encoding:'utf8',stdio:'inherit'});
 if(result.error||result.status!==0)throw new Error(`${cmd} failed (${result.status})`);
};
if(!host && scope!=='android-release') {
  const override=process.env.LEFTCAR_INTERNAL_DEBUG_KEYSTORE;
  const key=override??join(root,'apps/viewer-expo/android/app/debug.keystore');
  if((override!==undefined && !isAbsolute(override)) || !(await stat(key).catch(()=>null))?.isFile())throw new Error('Debug/internal packaging requires the existing app/debug.keystore or LEFTCAR_INTERNAL_DEBUG_KEYSTORE=<existing absolute debug-keystore path>. No key is created.');
}
const before=await sourceSnapshot(root);
const receiptRoot=process.env.LEFTCAR_BUILD_RECEIPTS ? resolve(process.env.LEFTCAR_BUILD_RECEIPTS) : tmpdir();
await mkdir(receiptRoot,{recursive:true});
const relativeReceipt=relative(await realpath(root),await realpath(receiptRoot));
if(relativeReceipt==='' || (relativeReceipt!=='..' && !relativeReceipt.startsWith(`..${sep}`) && !isAbsolute(relativeReceipt)))throw new Error('LEFTCAR_BUILD_RECEIPTS must be outside the repository to keep receipts independent of source inputs');
const receipts=await mkdtemp(join(receiptRoot,`leftcar-${scope}-`));
let artifacts, signing;
let artifactMetadata;
let buildTarget=null;
if(host){
 if(process.platform!=='darwin')throw new Error('host-macos-internal requires macOS');
 const hostCrate=join(root,'apps/host-desktop/src-tauri');
 const hostPlan=hostBuildPlan(cargoTargetDirectory(hostCrate,buildEnv),execFileSync('rustc',['-vV'],{env:buildEnv,encoding:'utf8'}),profile?.hostProductName||'Leftcar Host');
 buildEnv.CARGO_TARGET_DIR=hostPlan.targetDirectory;
 buildTarget=hostPlan.targetTriple;
 const shim=join(root,'native/macos-capture-shim/libleftcar_capture.dylib');
 run('zsh',['tools/build-macos-capture-shim.zsh','library',shim]);
 const config={...(profile?{identifier:profile.hostIdentifier,productName:profile.hostProductName}:{}),bundle:{macOS:{signingIdentity:process.env.LEFTCAR_HOST_SIGNING_IDENTITY || '-'}}};
 const configFile=join(receipts,'host-profile-config.json');await writeFile(configFile,JSON.stringify(config,null,2));
 run('bun',['run','tauri','build','--config','src-tauri/tauri.macos.conf.json','--config',configFile,'--bundles','app','--ci',...hostPlan.targetArguments,'--','--locked'],join(root,'apps/host-desktop'));
 const app=hostPlan.bundlePath;
 run('/usr/bin/codesign',['--verify','--deep','--strict',app]);
 const plistValue=key=>execFileSync('/usr/bin/plutil',['-extract',key,'raw','-o','-',join(app,'Contents/Info.plist')],{encoding:'utf8'}).trim();
 artifactMetadata={identifier:plistValue('CFBundleIdentifier'),version:plistValue('CFBundleShortVersionString')};
 if(artifactMetadata.identifier!==(profile?.hostIdentifier??'leftcar.ll3.kr'))throw new Error('Host bundle identifier does not match requested profile');
 const bundled=join(app,'Contents/Resources/libleftcar_capture.dylib');
 if(!(await readFile(shim)).equals(await readFile(bundled)))throw new Error('Host bundled shim differs from fresh build');
 artifacts=[{role:'host-bundle',path:app},{role:'native-shim',path:bundled}];
 const seal=spawnSync('/usr/bin/codesign',['-dvv',app],{encoding:'utf8'});
 if(seal.status!==0)throw new Error('Cannot inspect Host signing');
 signing={classification:process.env.LEFTCAR_HOST_SIGNING_IDENTITY?'internal-configured-local-signing':'internal-ad-hoc',notarized:false,
   details:(seal.stderr??'').split('\n').filter(line=>/^(Signature=|Authority=|TeamIdentifier=|CDHash=|Identifier=)/.test(line))};
}else{
 buildTarget='aarch64-linux-android';
 const release=scope!=='android-debug';
 const gradleArgs=[release?':app:assembleRelease':':app:assembleDebug','--no-daemon'];
 if(scope==='android-internal'||profile)gradleArgs.push('-PleftcarInternalBuild=true');
 run(process.platform==='win32'?'gradlew.bat':'./gradlew',gradleArgs,join(root,'apps/viewer-expo/android'));
 const variant=release?'release':'debug';
 const apk=join(root,`apps/viewer-expo/android/app/build/outputs/apk/${variant}/app-${variant}.apk`);
 const cargoNative=join(buildEnv.CARGO_TARGET_DIR??join(root,'target'),'aarch64-linux-android/release/libleftcar_viewer.so');
 const copiedNative=join(root,'apps/viewer-expo/android/app/libs/arm64-v8a/libleftcar_viewer.so');
 const capitalized=variant[0].toUpperCase()+variant.slice(1);
 const strippedNative=join(root,`apps/viewer-expo/android/app/build/intermediates/stripped_native_libs/${variant}/strip${capitalized}DebugSymbols/out/lib/arm64-v8a/libleftcar_viewer.so`);
 if(!(await readFile(cargoNative)).equals(await readFile(copiedNative)))throw new Error('Gradle copied library differs from actual Cargo output');
 artifacts=[{role:'viewer-apk',path:apk},{role:'native-cargo-unstripped',path:cargoNative},{role:'native-gradle-copy',path:copiedNative},{role:'native-agp-stripped-apk-input',path:strippedNative}];
 const sdk=process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
 const versions=(await readdir(join(sdk,'build-tools'))).sort((a,b)=>a.localeCompare(b,undefined,{numeric:true}));
 const signer=join(sdk,'build-tools',versions.at(-1),process.platform==='win32'?'apksigner.bat':'apksigner');
 const certificate=execFileSync(signer,['verify','--print-certs',apk],{encoding:'utf8',env:buildEnv});
 const aapt=join(sdk,'build-tools',versions.at(-1),process.platform==='win32'?'aapt2.exe':'aapt2');
 const badging=execFileSync(aapt,['dump','badging',apk],{encoding:'utf8'});
 const packageMatch=badging.match(/^package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'/m);
 if(!packageMatch)throw new Error('Cannot read actual APK package/version metadata');
 artifactMetadata={identifier:packageMatch[1],versionCode:Number(packageMatch[2]),version:packageMatch[3]};
 if(artifactMetadata.identifier!==(profile?.viewerPackage??'leftcar.ll3.kr'))throw new Error('APK identifier does not match requested profile');
 const embedded=execFileSync('unzip',['-p',apk,'lib/arm64-v8a/libleftcar_viewer.so'],{maxBuffer:64*1024*1024});
 if(!embedded.equals(await readFile(strippedNative)))throw new Error('APK native library differs from AGP stripped input');
 signing={classification:scope==='android-release'?'configured-release-key':scope==='android-internal'?'internal-debug-key':'debug-key',
   certificateSha256:certificate.split('\n').filter(line=>line.includes('certificate SHA-256 digest'))};
}
const gradle=await readFile(join(root,'apps/viewer-expo/android/app/build.gradle'),'utf8');
const expo=await import(join(root,'apps/viewer-expo/app.config.ts'));
const versions={bun:Bun.version,android:{versionName:gradle.match(/versionName "([^"]+)"/)[1],versionCode:Number(gradle.match(/versionCode (\d+)/)[1])},expoConfig:expo.default.version,host:JSON.parse(await readFile(join(root,'apps/host-desktop/src-tauri/tauri.conf.json'),'utf8')).version};
versions.node=execFileSync('node',['--version'],{encoding:'utf8'}).trim();
versions.rust=execFileSync('rustc',['--version'],{encoding:'utf8'}).trim();
versions.androidNdk='27.1.12297006';
const archived=[];
const artifactRoot=join(receipts,'artifacts');await mkdir(artifactRoot);
for(const artifact of artifacts) {
  const path=join(artifactRoot,`${artifact.role}-${basename(artifact.path)}`);
  const original=await hashPath(artifact.path);
  await cp(artifact.path,path,{recursive:true,preserveTimestamps:true,errorOnExist:true,force:false});
  if((await hashPath(path)).sha256!==original.sha256)throw new Error(`Artifact archive differs: ${artifact.role}`);
  archived.push({...artifact,path,buildPath:artifact.path});
}
const manifest=await createManifest({root,before,artifacts:archived,versions,signing,profile,target:host ? {platform:'darwin',architecture:buildTarget.startsWith('aarch64')?'arm64':'x64',triple:buildTarget} : {platform:'android',architecture:'arm64',abi:'arm64-v8a',triple:buildTarget}});
manifest.artifactMetadata=artifactMetadata;
manifest.artifactBuildPaths=archived.map(({role,buildPath})=>({role,buildPath}));
manifest.build={scope,targetTriple:buildTarget,cargoTargetDir:buildEnv.CARGO_TARGET_DIR??null,hostBuildProfile:buildEnv.LEFTCAR_BENCHMARK_BUILD_PROFILE??null,
  note:'Signing secrets are never recorded; actual certificate/seal evidence is recorded instead.'};
const output=join(receipts,`${scope}-manifest.json`);await writeFile(output,JSON.stringify(manifest,null,2)+'\n');
console.log(`Build manifest: ${output}`);
