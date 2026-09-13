import { createHash } from 'node:crypto';
import { readFile, lstat, readdir, readlink, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join, resolve, isAbsolute } from 'node:path';
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export async function hashPath(path) {
  const stat = await lstat(path);
  if (stat.isSymbolicLink()) return { kind: 'symlink', sha256: sha256(await readlink(path)) };
  if (stat.isFile()) return { kind: 'file', sha256: sha256(await readFile(path)), bytes: stat.size };
  if (!stat.isDirectory()) throw new Error(`Unsupported artifact: ${path}`);
  const files = [];
  for (const name of (await readdir(path)).sort()) files.push({ path: name, mode: (await lstat(join(path,name))).mode & 0o777, ...await hashPath(join(path, name)) });
  return { kind: 'directory', sha256: sha256(JSON.stringify(files)), files };
}
// Enumerate every git-tracked/nonignored input, not an extension allowlist.
// Only non-build documentation/receipts and known generated outputs are excluded.
// The exact file list and exclusions are part of the receipt; .env/local.properties
// are ignored local configuration, represented by toolchain/profile diagnostics.
export const sourceExclusions = [
  /^(docs\/|\.superpowers\/)/,
  /(^|\/)(README[^/]*|AGENTS\.md)$/,
  /(^|\/)node_modules\//,
  /^target\//,
  /^apps\/host-desktop\/(?:dist|src-tauri\/target)\//,
  /^apps\/viewer-expo\/(?:dist|\.expo|android\/(?:build|\.gradle|app\/build))\//,
  /^apps\/viewer-expo\/android\/app\/libs\//,
  /^native\/macos-capture-shim\/libleftcar_capture\.dylib$/,
];
export async function sourceSnapshot(root) {
  const git = args => execFileSync('git', ['-C', root, ...args], { encoding:'utf8' });
  const names = [...new Set(git(['ls-files', '-z', '--cached', '--others', '--exclude-standard']).split('\0').filter(Boolean))].sort();
  const files = [];
  for (const name of names.filter(n => !sourceExclusions.some(re => re.test(n)))) {
    const path = join(root, name);
    try { files.push({ path: name, mode: (await lstat(path)).mode & 0o777, ...await hashPath(path) }); }
    catch (e) { if (e.code === 'ENOENT') files.push({path: name, deleted: true}); else throw e; }
  }
  return { schema:1, commit:git(['rev-parse','HEAD']).trim(), dirty:git(['status','--porcelain']).length > 0,
    scope:'all tracked and nonignored build inputs except listed documentation and generated outputs',
    exclusions:sourceExclusions.map(String), files, sha256:sha256(JSON.stringify(files)) };
}
// Target claims are independent of the machine executing the build.
export async function inspectTargetArtifact(artifact, target) {
  if (!target?.platform || !target?.architecture || !target?.triple) throw new Error('Explicit target platform/architecture/triple required');
  if (target.platform === 'android') {
    if (target.architecture !== 'arm64' || target.abi !== 'arm64-v8a' || target.triple !== 'aarch64-linux-android') throw new Error('Unsupported or inconsistent Android target');
    if (artifact.role !== 'viewer-apk' && !artifact.role.startsWith('native-')) return null;
    const entry = `lib/${target.abi}/libleftcar_viewer.so`;
    const bytes = artifact.role === 'viewer-apk' ? execFileSync('unzip', ['-p', artifact.path, entry], {maxBuffer:128*1024*1024}) : await readFile(artifact.path);
    if (bytes.length < 20 || bytes.subarray(0,4).toString('hex') !== '7f454c46' || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== 183) throw new Error('Artifact ELF does not match Android target');
    const inspection = {native:{entry:artifact.role === 'viewer-apk' ? entry : null, elfClass:64, elfMachine:183, sha256:sha256(bytes), bytes:bytes.length}};
    if (artifact.role === 'viewer-apk') {
      const entries = execFileSync('unzip',['-Z1',artifact.path],{encoding:'utf8'}).split('\n');
      const jsEntry = 'assets/index.android.bundle';
      if (entries.includes(jsEntry)) {
        const js = execFileSync('unzip',['-p',artifact.path,jsEntry],{maxBuffer:128*1024*1024});
        inspection.javascript = {status:'observed',entry:jsEntry,sha256:sha256(js),bytes:js.length,format:js.subarray(0,8).toString('hex') === 'c61fbc03c103191f' ? 'hermes-bytecode' : 'javascript-or-unknown'};
      } else inspection.javascript = {status:'missing',reason:'No embedded index.android.bundle; requires external development server'};
    }
    return inspection;
  }
  if (target.platform !== 'darwin' || !['arm64','x64'].includes(target.architecture) || target.triple !== (target.architecture === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin')) throw new Error('Inconsistent macOS target');
  let executable = artifact.path;
  if (artifact.role === 'host-bundle') {
    const name = execFileSync('/usr/bin/plutil',['-extract','CFBundleExecutable','raw','-o','-',join(artifact.path,'Contents/Info.plist')],{encoding:'utf8'}).trim();
    executable = join(artifact.path,'Contents/MacOS',name);
  }
  const archs = execFileSync('/usr/bin/lipo',['-archs',executable],{encoding:'utf8'}).trim().split(/\s+/);
  if (!archs.includes(target.architecture === 'x64' ? 'x86_64' : 'arm64')) throw new Error('Mach-O does not match target');
  return {machO:{architectures:archs}};
}
export async function createManifest({ root, before, artifacts, versions, signing, profile = null, adaptation = null, target = null }) {
  const after = await sourceSnapshot(root);
  if (before.sha256 !== after.sha256 || before.commit !== after.commit) throw new Error('Build source changed during the build; refusing manifest');
  if (!artifacts.length || !signing?.classification) throw new Error('Artifacts and signing classification required');
  const outputs = [];
  for (const artifact of artifacts) outputs.push({ role:artifact.role, path:resolve(artifact.path), ...await hashPath(artifact.path), ...(target ? {inspection:await inspectTargetArtifact(artifact,target)} : {}) });
  return { schema:target ? 2 : 1, ...(target ? {builder:{platform:process.platform,architecture:process.arch},target} : {}), createdAt:new Date().toISOString(), platform:process.platform, architecture:process.arch,
    source:before, versions, signing, profile, adaptation, artifacts:outputs,
    evidence:'local build and packaging only; no installation, capture, device, notarization or publication claim' };
}
export async function verifyManifest(manifest) {
  if(!Array.isArray(manifest.artifacts) || !manifest.artifacts.length)throw new Error('Manifest must contain actual artifacts');
  for(const artifact of manifest.artifacts) {
    if(!artifact.role || !isAbsolute(artifact.path??'') || !/^[a-f0-9]{64}$/.test(artifact.sha256??''))throw new Error('Invalid artifact record');
  }
  if (![1,2].includes(manifest.schema) || sha256(JSON.stringify(manifest.source.files)) !== manifest.source.sha256) throw new Error('Invalid source manifest hash');
  for (const artifact of manifest.artifacts) {
    if (manifest.schema === 2 && JSON.stringify(await inspectTargetArtifact(artifact,manifest.target)) !== JSON.stringify(artifact.inspection)) throw new Error('Artifact target inspection mismatch');
    if ((await hashPath(artifact.path)).sha256 !== artifact.sha256) throw new Error(`Artifact hash mismatch: ${artifact.role}`);
  }
  return true;
}
if (import.meta.main) {
  const [mode, path] = process.argv.slice(2);
  if (mode !== 'verify' || !path) throw new Error('Usage: bun run release:manifest -- verify <build-manifest.json>. Creation is bound to bun run build.');
  await verifyManifest(JSON.parse(await readFile(path, 'utf8')));
  console.log('Manifest source hash and all current artifact hashes verified');
}
