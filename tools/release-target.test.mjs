import {test,expect} from 'vitest';
import {mkdtempSync,writeFileSync,rmSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createManifest,sourceSnapshot,verifyManifest} from './release-manifest.mjs';
test('Android target is proved from archive ELF and embedded JS separately from builder',async()=>{
 const root=mkdtempSync(join(tmpdir(),'leftcar-target-'));
 try {
  execFileSync('git',['init',root]); writeFileSync(join(root,'source'),'input');
  execFileSync('git',['-C',root,'add','source']);execFileSync('git',['-C',root,'-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','-m','fixture']);
  mkdirSync(join(root,'lib/arm64-v8a'),{recursive:true});mkdirSync(join(root,'assets'));
  const elf=Buffer.alloc(64);elf.write('\x7fELF');elf[4]=2;elf[5]=1;elf.writeUInt16LE(183,18);
  writeFileSync(join(root,'lib/arm64-v8a/libleftcar_viewer.so'),elf);writeFileSync(join(root,'assets/index.android.bundle'),'synthetic JS fixture');
  const apk=join(root,'app.apk');execFileSync('zip',['-qr',apk,'lib','assets'],{cwd:root});
  const before=await sourceSnapshot(root);
  const args={root,before,artifacts:[{role:'viewer-apk',path:apk}],versions:{},signing:{classification:'fixture'},target:{platform:'android',architecture:'arm64',abi:'arm64-v8a',triple:'aarch64-linux-android'}};
  const manifest=await createManifest(args);
  expect(manifest.schema).toBe(2);expect(manifest.builder.platform).toBe(process.platform);expect(manifest.target).toEqual(args.target);
  expect(manifest.artifacts[0].inspection.native.elfMachine).toBe(183);expect(manifest.artifacts[0].inspection.javascript.status).toBe('observed');
  expect(await verifyManifest(manifest)).toBe(true);
  await expect(verifyManifest({...manifest,target:{...manifest.target,abi:'x86_64'}})).rejects.toThrow(/target/);
  await expect(createManifest({...args,target:{...args.target,architecture:'x64'}})).rejects.toThrow(/target/);
  // Historical schema1 has only builder labels and remains verifiable as such.
  expect(await verifyManifest({...manifest,schema:1,target:undefined,builder:undefined})).toBe(true);
 }finally {rmSync(root,{recursive:true,force:true});}
});
