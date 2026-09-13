import {test,expect} from 'vitest';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
test('Gradle inventory resolves arrows, deduplicates constraints and keeps license unknowns explicit',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const cache=await mkdtemp(join(tmpdir(),'leftcar-pom-'));
 try {
  const dir=join(cache,'caches/modules-2/files-2.1/org.example/known/2.0/hash');await mkdir(dir,{recursive:true});await writeFile(join(dir,'known-2.0.pom'),'<project><licenses><license><name>Apache License, Version 2.0</name></license></licenses></project>');
  const output='+--- org.example:known:1.0 -> 2.0\n|    \\--- org.example:unknown:3.0\n+--- org.example:known:2.0 (c)\n+--- project :expo\n';
  const calls=[];const inventory=await collectGradleInventory('/fixture',{gradleCache:cache,run:(command,args,cwd)=>{calls.push({command,args,cwd});return {status:0,stdout:output};}});
  expect(inventory.status).toBe('observed-unlocked');expect(inventory.records).toHaveLength(2);expect(inventory.records[0]).toMatchObject({name:'org.example:known',version:'2.0',licenseStatus:'declared'});expect(inventory.records[1]).toMatchObject({license:null,licenseStatus:'unknown'});
  expect(calls[0].args).toContain('--offline');expect(calls[0].args).not.toContain('--write-verification-metadata');
  const failed=await collectGradleInventory('/fixture',{gradleCache:cache,run:()=>({status:0,stdout:'+--- org.example:unknown:3.0 FAILED'})});expect(failed.status).toBe('error');
  const missing=await collectGradleInventory('/fixture',{gradleCache:cache,run:()=>({status:null,error:{code:'ENOENT'}})});expect(missing.status).toBe('unavailable');
 }finally {await rm(cache,{recursive:true,force:true});}
});
test('real offline Gradle output handles whole-coordinate substitution and versionless constraints',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const {readFile}=await import('node:fs/promises');
 const stdout=await readFile(new URL('./fixtures/release-gradle-runtime.txt',import.meta.url),'utf8');
 const inventory=await collectGradleInventory('/fixture',{gradleCache:'/nonexistent-fixture-cache',run:()=>({status:0,stdout})});
 expect(inventory.records.map(({name,version})=>({name,version}))).toEqual([
  {name:'com.facebook.react:react-android',version:'0.86.2'},
  {name:'org.jetbrains:annotations',version:'23.0.0'},
  {name:'org.jetbrains.kotlin:kotlin-stdlib',version:'2.2.20'},
 ]);
});
