import { test, expect } from 'vitest';
import { mkdtemp, mkdir, writeFile, readFile, rm, readdir, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { replaceBundle } from './build.bundle.mjs';

test('bundle staging failure preserves the previous installed bundle', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'leftcar-bundle-'));
  try {
    const old = join(dir, 'app'); await mkdir(old); await writeFile(join(old, 'seal'), 'old');
    await expect(replaceBundle(join(dir, 'missing-source'), old, async p => { await readFile(join(p, 'seal')); })).rejects.toThrow();
    expect(await readFile(join(old, 'seal'), 'utf8')).toBe('old');
    expect(await readdir(dir)).toEqual(['app']);
  } finally { await rm(dir, {recursive:true, force:true}); }
});
test('post-replacement verification failure rolls back and successful update retains backup', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'leftcar-bundle-'));
  try {
    const old = join(dir, 'app'); const fresh = join(dir, 'fresh');
    for (const [p, seal] of [[old, 'old'], [fresh, 'new']]) { await mkdir(p); await writeFile(join(p, 'seal'), seal); }
    await expect(replaceBundle(fresh, old, async p => {
      if (p === old && await readFile(join(p, 'seal'), 'utf8') === 'new') throw new Error('post-copy verification failed');
    })).rejects.toThrow('post-copy verification failed');
    expect(await readFile(join(old, 'seal'), 'utf8')).toBe('old');
    const result = await replaceBundle(fresh, old, async p => { await readFile(join(p, 'seal')); });
    expect(await readFile(join(old, 'seal'), 'utf8')).toBe('new');
    expect(await readFile(join(result.backup, 'seal'), 'utf8')).toBe('old');
  } finally { await rm(dir, {recursive:true, force:true}); }
});
import { createManifest, sourceSnapshot, verifyManifest } from './release-manifest.mjs';
import { execFileSync } from 'node:child_process';
test('manifest binds dirty build inputs, ignores result docs and detects artifact tampering', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'leftcar-manifest-'));
  try {
    execFileSync('git', ['init', dir], {stdio:'ignore'});
    await writeFile(join(dir, 'source.ts'), 'export const x=1');
    execFileSync('git', ['-C', dir, 'add', 'source.ts']);
    execFileSync('git', ['-C', dir, '-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','fixture'], {stdio:'ignore'});
    const artifact = join(dir, 'output.bin'); await writeFile(artifact, 'artifact');
    const before = await sourceSnapshot(dir);
    await mkdir(join(dir, 'docs')); await writeFile(join(dir, 'docs', 'results.md'), 'result');
    const manifest = await createManifest({root:dir, before, artifacts:[{role:'fixture',path:artifact}], versions:{fixture:'1'}, signing:{classification:'fixture-unsigned'}});
    expect(manifest.source.dirty).toBe(true);
    expect(await verifyManifest(JSON.parse(JSON.stringify(manifest)))).toBe(true);
    await writeFile(artifact, 'tampered');
    await expect(verifyManifest(manifest)).rejects.toThrow('Artifact hash mismatch');
    await writeFile(join(dir, 'source.ts'), 'changed');
    await expect(createManifest({root:dir, before, artifacts:[], versions:{}, signing:{classification:'fixture'}})).rejects.toThrow('Build source changed');
  } finally { await rm(dir, {recursive:true, force:true}); }
});
import { requirePerfectReactDoctor } from './verify.mjs';
test('React Doctor exit success cannot hide a lower or missing score', () => {
  expect(() => requirePerfectReactDoctor('Score: 99 / 100 Great')).toThrow();
  expect(() => requirePerfectReactDoctor('success')).toThrow();
  expect(() => requirePerfectReactDoctor('Score: 100 / 100 Great')).not.toThrow();
  expect(() => requirePerfectReactDoctor('Score: 100 / 100\nScore: 70 / 100')).toThrow();
});
import { benchmarkProfile } from './benchmark-profile.mjs';
test('benchmark tooling preserves default identity and rejects partial Host profiles', () => {
  expect(benchmarkProfile({})).toBeNull();
  expect(() => benchmarkProfile({LEFTCAR_BENCHMARK_ROOT:'/tmp/test'})).toThrow();
  expect(() => benchmarkProfile({LEFTCAR_BENCHMARK_PROFILE:'baseline068'}, {requireRoot:true})).toThrow();
  expect(() => benchmarkProfile({LEFTCAR_BENCHMARK_PROFILE:'../bad'})).toThrow();
  const profile=benchmarkProfile({LEFTCAR_BENCHMARK_PROFILE:'baseline-068',LEFTCAR_BENCHMARK_ROOT:'/tmp/dedicated'},{requireRoot:true});
  expect(profile.viewerPackage).toBe('leftcar.ll3.kr.benchmark.baseline_068');
  expect(profile.credentialService).toBe('leftcar-host.benchmark.baseline-068');
});
test('source snapshot includes tracked build-named source folders and manifest rejects missing outputs', async () => {
  const dir=await mkdtemp(join(tmpdir(),'leftcar-source-scope-'));
  try {
    execFileSync('git',['init',dir],{stdio:'ignore'});
    await mkdir(join(dir,'tools/build'),{recursive:true});
    await writeFile(join(dir,'tools/build/logic.mjs'),'source');
    execFileSync('git',['-C',dir,'add','tools/build/logic.mjs']);
    execFileSync('git',['-C',dir,'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','fixture'],{stdio:'ignore'});
    const source=await sourceSnapshot(dir);
    expect(source.files.some(file=>file.path==='tools/build/logic.mjs')).toBe(true);
    await expect(verifyManifest({schema:1,source,artifacts:[]})).rejects.toThrow();
  } finally {await rm(dir,{recursive:true,force:true});}
});
import { cargoTargetDirectory, hostBuildPlan } from './build.host-plan.mjs';
test('Host packaging resolves Cargo-config output and explicitly binds its native target subtree', async () => {
  const dir=await mkdtemp(join(tmpdir(),'leftcar-host-output-'));
  try {
    await mkdir(join(dir,'src'));await mkdir(join(dir,'.cargo'));
    await writeFile(join(dir,'Cargo.toml'),'[package]\nname="host-output-fixture"\nversion="0.0.0"\nedition="2021"\n');
    await writeFile(join(dir,'src/lib.rs'),'');
    await writeFile(join(dir,'.cargo/config.toml'),'[build]\ntarget-dir="configured-output"\ntarget="x86_64-unknown-linux-gnu"\n');
    const env={...process.env,CARGO_BUILD_TARGET:'x86_64-unknown-linux-gnu'};delete env.CARGO_TARGET_DIR;
    execFileSync('cargo',['generate-lockfile','--offline'],{cwd:dir,env,stdio:'pipe'});
    const target=cargoTargetDirectory(dir,env);
    expect(target).toBe(join(await realpath(dir),'configured-output'));
    const plan=hostBuildPlan(target,'rustc fixture\nhost: aarch64-apple-darwin\n','Leftcar Benchmark task5');
    expect(plan.targetArguments).toEqual(['--target','aarch64-apple-darwin']);
    expect(plan.bundlePath).toBe(join(target,'aarch64-apple-darwin/release/bundle/macos/Leftcar Benchmark task5.app'));
  } finally {await rm(dir,{recursive:true,force:true});}
});
test('non-macOS compiler receipts remain rejected by the pure Host plan', () => {
  for (const triple of ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc']) {
    expect(() => hostBuildPlan('/tmp/output', `rustc fixture\nhost: ${triple}\n`, 'Leftcar Host'))
      .toThrow('macOS internal packaging requires a native macOS Rust host compiler');
  }
});

test('benchmark display restriction requires an explicit profile and preserves exact identity', () => {
  expect(() => benchmarkProfile({LEFTCAR_BENCHMARK_SOURCE_ID:'macos:display:synthetic'})).toThrow();
  expect(() => benchmarkProfile({LEFTCAR_BENCHMARK_PROFILE:'candidate',LEFTCAR_BENCHMARK_SOURCE_ID:''})).toThrow();
  expect(benchmarkProfile({LEFTCAR_BENCHMARK_PROFILE:'candidate',LEFTCAR_BENCHMARK_SOURCE_ID:'macos:display:synthetic'}).sourceId).toBe('macos:display:synthetic');
});
