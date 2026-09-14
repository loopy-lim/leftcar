import {readFile,readdir} from 'node:fs/promises';
import {join} from 'node:path';
import {sha256,isSha256,validateSourceSnapshot} from './release-source.mjs';
import {collectDependencyInventory,lockfiles} from './release-dependencies.mjs';
export const releaseScopes=['android-debug','android-internal','android-release','host-macos-internal'];
export const gradleDistribution={version:'9.3.1',sha256:'b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06',provenance:'https://services.gradle.org/distributions/gradle-9.3.1-bin.zip.sha256'};
const configPaths=['package.json','apps/viewer-expo/package.json','apps/viewer-expo/app.config.ts','apps/host-desktop/src-tauri/Cargo.toml','apps/host-desktop/src-tauri/tauri.conf.json','apps/host-desktop/src-tauri/src/ffi.rs','apps/viewer-expo/android/app/build.gradle','apps/viewer-expo/android/gradle.properties','apps/viewer-expo/android/gradle/wrapper/gradle-wrapper.properties','.cargo/config.toml'];
const requireValue=(value,message)=>{if(!value)throw new Error(message);return value;};
const match=(text,regex,label)=>requireValue(text.match(regex)?.[1],`Missing or invalid ${label}`);
const validVersion=value=>typeof value==='string'&&/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(value);
const stableList=list=>[...new Set(list)].sort();
export function parseShimRequirements(ffi,swift) {
 const body=match(ffi,/fn verify_symbols\(&self\)[\s\S]*?\{([\s\S]*?)\n    fn lib\(/,'Host verify_symbols requirement');
 const requiredSymbols=stableList([...body.matchAll(/b"(leftcar_capture_[a-z0-9_]+)"/g)].map(m=>m[1]));
 const starts=requiredSymbols.filter(s=>/^leftcar_capture_start_v\d+$/.test(s));
 requireValue(starts.length===1&&requiredSymbols.length>1,'Missing or ambiguous Host shim start ABI');
 const exportedSymbols=stableList([...swift.matchAll(/@_cdecl\("(leftcar_capture_[a-z0-9_]+)"\)/g)].map(m=>m[1]));
 return {requiredStartAbi:Number(starts[0].match(/v(\d+)$/)[1]),requiredSymbols,exportedSymbols};
}
export function classifyAndroidSigning(certificate,scope) {
 requireValue(releaseScopes.includes(scope)&&scope.startsWith('android-'),'Invalid Android signing scope');
 const subjects=[...certificate.matchAll(/^(?:Signer #\d+|V\d+ Signer:) certificate DN: (.+)$/gm)].map(m=>m[1]);
 const fingerprints=[...certificate.matchAll(/^(?:Signer #\d+|V\d+ Signer:) certificate SHA-256 digest: ([a-fA-F0-9]{64})$/gm)].map(m=>m[1].toLowerCase());
 requireValue(subjects.length>0&&subjects.length===fingerprints.length,'Verified APK certificate identity required');
 const debug=subjects.some(subject=>/(?:^|,)\s*CN\s*=\s*Android Debug(?:,|$)/i.test(subject));
 return {classification:debug?(scope==='android-internal'?'internal-debug-key':'debug-key'):(scope==='android-release'?'configured-release-key':'internal-configured-key'),verified:true,debugCertificate:debug,certificateSha256:stableList(fingerprints)};
}
export function sealReleaseInputs(inputs) {
 const {sha256:ignored,...body}=inputs;return {...body,sha256:sha256(JSON.stringify(body))};
}
function readiness(inputs,signing=null) {
 const blockers=[];
 for(const check of inputs.vulnerabilityChecks)if(check.status!=='pass')blockers.push({code:'VULNERABILITY_CHECK_'+check.status.toUpperCase(),input:check.input,tool:check.tool});
 if(inputs.dependencies.some(d=>d.ecosystem==='gradle'&&d.status!=='locked'))blockers.push({code:'GRADLE_DEPENDENCIES_UNLOCKED'});
 if(inputs.dependencies.some(d=>d.records.some(r=>r.licenseStatus!=='declared')))blockers.push({code:'LICENSE_METADATA_UNKNOWN'});
 // Local configuration does not establish signing/distribution acceptance.
 if(inputs.scope!=='android-release')blockers.push({code:'INTERNAL_SCOPE'});
 if(inputs.scope==='android-release' && !(signing?.verified===true&&signing.classification==='configured-release-key'&&signing.debugCertificate===false&&signing.certificateSha256?.length))blockers.push({code:'DISTRIBUTION_SIGNING_UNVERIFIED'});
 return {internalBuildable:true,distributionReady:blockers.length===0,status:blockers.length?'blocked':'ready',blockers};
}
export async function collectReleaseInputs(root,scope,options={}) {
 requireValue(releaseScopes.includes(scope),'Unsupported release scope');
 const source=options.source??await (await import('./release-manifest.mjs')).sourceSnapshot(root);validateSourceSnapshot(source);
 const files={};for(const path of configPaths)files[path]=await readFile(join(root,path),'utf8');
 const swiftPaths=(await readdir(join(root,'native/macos-capture-shim/Sources'),{recursive:true})).filter(x=>x.endsWith('.swift')).sort().map(x=>'native/macos-capture-shim/Sources/'+x);
 for(const path of swiftPaths)files[path]=await readFile(join(root,path),'utf8');
 const gradle=files['apps/viewer-expo/android/app/build.gradle'],properties=files['apps/viewer-expo/android/gradle.properties'],wrapper=files['apps/viewer-expo/android/gradle/wrapper/gradle-wrapper.properties'];
 const pkg=JSON.parse(files['package.json']),expoPackage=JSON.parse(files['apps/viewer-expo/package.json']),tauri=JSON.parse(files['apps/host-desktop/src-tauri/tauri.conf.json']);
 const rustCommand=match(gradle,/commandLine "cargo", "build", ([^\n]+)/,'Gradle Cargo command');
 const components={
  root:{name:pkg.name,version:pkg.version??null,versionRole:pkg.version?'workspace-package':'unversioned-private-workspace'},
  expo:{version:match(files['apps/viewer-expo/app.config.ts'],/\bversion:\s*"([^"]+)"/,'Expo config version'),packageVersion:expoPackage.version},
  viewerAndroid:{applicationId:match(gradle,/\bapplicationId '([^']+)'/,'Android package'),versionCode:Number(match(gradle,/\bversionCode (\d+)/,'Android versionCode')),versionName:match(gradle,/\bversionName "([^"]+)"/,'Android versionName'),abi:match(properties,/^reactNativeArchitectures=(.+)$/m,'Android ABI'),rustTarget:match(rustCommand,/"--target", "([^"]+)"/,'Android Rust target'),cargoTargetConfigured:/^\[target\.aarch64-linux-android\]$/m.test(files['.cargo/config.toml'])},
  host:{cargoVersion:match(files['apps/host-desktop/src-tauri/Cargo.toml'],/^version\s*=\s*"([^"]+)"/m,'Host Cargo version'),tauriVersion:tauri.version,identifier:tauri.identifier},
  shim:parseShimRequirements(files['apps/host-desktop/src-tauri/src/ffi.rs'],swiftPaths.map(p=>files[p]).join('\n')),
 };
 const gradleWrapper={version:match(wrapper,/gradle-([\d.]+)-bin.zip/,'Gradle version'),distributionSha256:match(wrapper,/^distributionSha256Sum=([a-f0-9]{64})$/m,'Gradle distribution checksum'),distributionUrl:match(wrapper,/^distributionUrl=(.+)$/m,'Gradle distribution URL').replaceAll('\\:',':'),validateDistributionUrl:/^validateDistributionUrl=true$/m.test(wrapper)};
 const inventory=await collectDependencyInventory(root,scope,options);
 const inputs={schema:1,scope,sourceCommit:source.commit,sourceSha256:source.sha256,components,gradleWrapper,configurationInputs:Object.entries(files).map(([path,text])=>({path,sha256:sha256(text)})).sort((a,b)=>a.path.localeCompare(b.path,'en')),signingRequirement:scope==='android-release'?'configured-release-key':scope==='host-macos-internal'?'internal-local-signing':'debug-or-internal-key',...inventory,evidence:'Configuration, source and dependency observations only; not cryptographic build attestation or installation, device, capture, stream, stability, notarization or publication acceptance.'};
 const result=sealReleaseInputs({...inputs,...readiness(inputs)});validateReleaseInputs(result,source);return result;
}
export function validateReleaseInputs(inputs,source) {
 requireValue(inputs?.schema===1&&releaseScopes.includes(inputs.scope),'Invalid release inputs schema/scope');
 const {sha256:digest,...body}=inputs;requireValue(isSha256(digest)&&sha256(JSON.stringify(body))===digest,'Release inputs hash mismatch');
 validateSourceSnapshot(source);
 requireValue(inputs.sourceCommit===source.commit&&inputs.sourceSha256===source.sha256,'Release inputs source binding mismatch');
 const c=inputs.components;
 requireValue(typeof c?.root?.name==='string'&&(c.root.version===null||validVersion(c.root.version))&&validVersion(c.expo?.version)&&validVersion(c.expo.packageVersion),'Missing component version records');
 requireValue(validVersion(c.host?.cargoVersion)&&c.host.cargoVersion===c.host.tauriVersion&&typeof c.host.identifier==='string'&&c.host.identifier.length>0,'Host component version mismatch');
 const a=c.viewerAndroid;requireValue(typeof a?.applicationId==='string'&&/^[a-z][a-z0-9_.]+$/.test(a.applicationId)&&validVersion(a.versionName)&&Number.isSafeInteger(a.versionCode)&&a.versionCode>0,'Missing Android component identity');
 requireValue(a.abi==='arm64-v8a'&&a.rustTarget==='aarch64-linux-android'&&a.cargoTargetConfigured===true,'Android ABI/target mismatch');
 const s=c.shim;requireValue(Number.isSafeInteger(s?.requiredStartAbi)&&s.requiredStartAbi>0&&Array.isArray(s.requiredSymbols)&&s.requiredSymbols.length>1&&Array.isArray(s.exportedSymbols),'Missing shim ABI requirements');
 requireValue(s.requiredSymbols.includes(`leftcar_capture_start_v${s.requiredStartAbi}`)&&s.requiredSymbols.every(name=>s.exportedSymbols.includes(name)),'Shim required symbols/ABI mismatch');
 const g=inputs.gradleWrapper;requireValue(g?.version===gradleDistribution.version&&g.distributionSha256===gradleDistribution.sha256&&g.distributionUrl===`https://services.gradle.org/distributions/gradle-${g.version}-bin.zip`&&g.validateDistributionUrl===true,'Gradle distribution checksum/version mismatch');
 requireValue(Array.isArray(inputs.configurationInputs),'Missing configuration input hashes');
 for(const path of configPaths)requireValue(inputs.configurationInputs.some(x=>x.path===path),'Missing release configuration record');
 const swiftSourcePaths=source.files.filter(x=>x.path.startsWith('native/macos-capture-shim/Sources/')&&x.path.endsWith('.swift')&&!x.deleted).map(x=>x.path);
 requireValue(swiftSourcePaths.length>0&&swiftSourcePaths.every(path=>inputs.configurationInputs.some(x=>x.path===path)),'Missing shim source records');
 requireValue(new Set(inputs.configurationInputs.map(x=>x.path)).size===inputs.configurationInputs.length,'Duplicate configuration source records');
 for(const input of inputs.configurationInputs)requireValue(isSha256(input.sha256)&&source.files.some(file=>file.path===input.path&&file.sha256===input.sha256),'Release configuration/source hash mismatch');
 requireValue(Array.isArray(inputs.dependencies)&&inputs.dependencies.length===4,'Missing dependency inventories');
 for(const path of lockfiles) {
  const entries=inputs.dependencies.filter(d=>d.lockfile===path);requireValue(entries.length===1,'Missing or duplicate lock inventory');
  const d=entries[0];requireValue(d.ecosystem===(path==='bun.lock'?'bun':'cargo')&&d.status==='locked'&&isSha256(d.lockSha256)&&source.files.some(f=>f.path===path&&f.sha256===d.lockSha256),'Dependency lock/source hash mismatch');
  requireValue(Array.isArray(d.records)&&d.records.length>0,'Missing resolved dependency records');
  for(const r of d.records)requireValue(r.name&&r.version&&r.scope&&['declared','unknown'].includes(r.licenseStatus)&&(r.licenseStatus==='declared'?typeof r.license==='string'&&r.license.length>0:r.license===null),'Invalid dependency license record');
 }
 requireValue(inputs.dependencies.filter(d=>d.ecosystem==='gradle'&&['observed-unlocked','unavailable','error'].includes(d.status)&&d.lockfile===null&&Array.isArray(d.records)).length===1,'Missing Gradle inventory availability record');
 const gradleInventory=inputs.dependencies.find(d=>d.ecosystem==='gradle');
 requireValue(gradleInventory.status==='observed-unlocked'?gradleInventory.records.length>0:gradleInventory.records.length===0,'Invalid Gradle inventory outcome');
 for(const r of gradleInventory.records)requireValue(r.name&&r.version&&r.scope==='releaseRuntimeClasspath'&&['declared','unknown'].includes(r.licenseStatus)&&(r.licenseStatus==='declared'?typeof r.license==='string'&&r.license.length>0:r.license===null),'Invalid Gradle license record');
 requireValue(Array.isArray(inputs.vulnerabilityChecks)&&inputs.vulnerabilityChecks.length===4,'Missing vulnerability checks');
 for(const path of [...lockfiles,null]) {
  const checks=inputs.vulnerabilityChecks.filter(v=>v.input===path);requireValue(checks.length===1,'Missing or duplicate vulnerability check');const v=checks[0];
  requireValue(['pass','findings','unavailable','error'].includes(v.status)&&Array.isArray(v.findings)&&Array.isArray(v.warnings),'Invalid vulnerability outcome');
  requireValue(v.tool===(path===null?'gradle':path==='bun.lock'?'bun':'cargo-audit'),'Invalid vulnerability tool');
  if(path)requireValue(v.inputSha256===inputs.dependencies.find(d=>d.lockfile===path).lockSha256,'Vulnerability input lock hash mismatch');
  if(v.status==='pass')requireValue(v.toolVersion&&v.findings.length===0&&v.warnings.length===0&&path!==null,'Invalid passing vulnerability outcome');
  if(v.status==='findings')requireValue(v.findings.length+v.warnings.length>0,'Empty vulnerability findings');
 }
 requireValue(inputs.signingRequirement===(inputs.scope==='android-release'?'configured-release-key':inputs.scope==='host-macos-internal'?'internal-local-signing':'debug-or-internal-key'),'Signing scope mismatch');
 const expected=readiness(inputs,inputs.artifactSigning??null);
 for(const key of ['internalBuildable','distributionReady','status','blockers'])requireValue(JSON.stringify(inputs[key])===JSON.stringify(expected[key]),'Inconsistent release readiness');
 return expected;
}
export function bindArtifactReleaseInputs(inputs,{source,target,artifactMetadata,signing,profile=null,artifacts,versions}) {
 validateReleaseInputs(inputs,source);
 const host=inputs.scope==='host-macos-internal',a=inputs.components.viewerAndroid,h=inputs.components.host;
 requireValue(target?.platform===(host?'darwin':'android'),'Release artifact target/scope mismatch');
 requireValue(versions?.host===h.cargoVersion&&versions.expoConfig===inputs.components.expo.version&&versions.android?.versionName===a.versionName&&versions.android.versionCode===a.versionCode,'Manifest component versions mismatch');
 requireValue(artifactMetadata?.identifier===(host?(profile?.hostIdentifier??h.identifier):(profile?.viewerPackage??a.applicationId)),'Missing or mismatched artifact identifier');
 requireValue(artifactMetadata.version===(host?h.cargoVersion:a.versionName)&& (host||artifactMetadata.versionCode===a.versionCode),'Artifact component version mismatch');
 requireValue(Array.isArray(artifacts)&&artifacts.some(x=>x.role===(host?'host-bundle':'viewer-apk')),'Required release artifact missing');
 if(host) {
  requireValue(['internal-ad-hoc','internal-configured-local-signing'].includes(signing?.classification)&&signing.notarized===false,'Invalid internal Host signing');
  requireValue(artifacts.some(x=>x.role==='native-shim'),'Bundled shim artifact missing');
 }else {
  requireValue(target.abi===a.abi&&target.triple===a.rustTarget&&target.architecture==='arm64','Release Android target mismatch');
  requireValue(signing?.verified===true&&typeof signing.debugCertificate==='boolean'&&Array.isArray(signing.certificateSha256)&&signing.certificateSha256.length>0&&signing.certificateSha256.every(isSha256),'Verified Android signature metadata required');
  const expected=signing.debugCertificate?(inputs.scope==='android-internal'?'internal-debug-key':'debug-key'):(inputs.scope==='android-release'?'configured-release-key':'internal-configured-key');
  requireValue(signing.classification===expected,'Signature classification/scope mismatch');
 }
 requireValue(!(profile&&inputs.scope==='android-release'),'Benchmark package is internal only');
 const result={...inputs,artifactSigning:signing};return sealReleaseInputs({...result,...readiness(result,signing)});
}
