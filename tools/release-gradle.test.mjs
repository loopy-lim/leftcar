import {test,expect} from 'vitest';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
test('Gradle inventory resolves arrows, deduplicates constraints and keeps license unknowns explicit',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const cache=await mkdtemp(join(tmpdir(),'leftcar-pom-'));
 try {
  const dir=join(cache,'caches/modules-2/files-2.1/org.example/known/2.0/hash');await mkdir(dir,{recursive:true});await writeFile(join(dir,'known-2.0.pom'),'<project><groupId>org.example</groupId><artifactId>known</artifactId><version>2.0</version><licenses><license><name>Apache License, Version 2.0</name></license></licenses></project>');
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

test('Gradle licenses follow exact cached parent coordinates and retain their provenance',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const cache=await mkdtemp(join(tmpdir(),'leftcar-parent-pom-'));
 const pom=async(group,name,version,body)=>{
  const dir=join(cache,'caches/modules-2/files-2.1',group,name,version,'hash');
  await mkdir(dir,{recursive:true});
  await writeFile(join(dir,`${name}-${version}.pom`),`<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion><groupId>${group}</groupId><artifactId>${name}</artifactId><version>${version}</version>${body}</project>`);
 };
 try {
  await pom('org.example','child','2','<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version></parent>');
  await pom('org.example','parent','1','<licenses><license><name>Apache &amp; MIT</name></license></licenses>');
  const inventory=await collectGradleInventory('/fixture',{gradleCache:cache,run:()=>({status:0,stdout:'+--- org.example:child:2'})});
  expect(inventory.records[0]).toMatchObject({license:'Apache & MIT',licenseStatus:'declared',licenseSource:'cached-parent-pom'});
  expect(inventory.records[0].licenseEvidence).toHaveLength(2);
  expect(inventory.records[0].licenseEvidence[1]).toMatchObject({coordinate:'org.example:parent:1',sha256:expect.stringMatching(/^[a-f0-9]{64}$/)});
 }finally {await rm(cache,{recursive:true,force:true});}
});

test('Gradle licenses reject parent cycles, wrong identities and nested plugin licenses',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const cache=await mkdtemp(join(tmpdir(),'leftcar-invalid-pom-'));
 try {
  for(const [name,body] of [
   ['cycle','<artifactId>cycle</artifactId><parent><groupId>org.example</groupId><artifactId>cycle</artifactId><version>1</version></parent>'],
   ['wrong','<artifactId>different</artifactId><licenses><license><name>MIT</name></license></licenses>'],
   ['nested','<artifactId>nested</artifactId><build><plugins><plugin><licenses><license><name>MIT</name></license></licenses></plugin></plugins></build>'],
   ['entity','<artifactId>entity</artifactId><licenses><license><name>&missing;</name></license></licenses>'],
  ]) {
   const dir=join(cache,'caches/modules-2/files-2.1/org.example',name,'1/hash');await mkdir(dir,{recursive:true});
   await writeFile(join(dir,`${name}-1.pom`),`<project><groupId>org.example</groupId><version>1</version>${body}</project>`);
  }
  const inventory=await collectGradleInventory('/fixture',{gradleCache:cache,run:()=>({status:0,stdout:'+--- org.example:cycle:1\n+--- org.example:wrong:1\n+--- org.example:nested:1\n+--- org.example:entity:1'})});
  expect(inventory.records.every(r=>r.licenseStatus==='unknown'&&r.license===null)).toBe(true);
 }finally {await rm(cache,{recursive:true,force:true});}
});

test('Gradle local Expo licenses require exact publication and package versions',async()=>{
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const root=await mkdtemp(join(tmpdir(),'leftcar-expo-license-'));
 try {
  const dir=join(root,'node_modules/@expo/fixture');await mkdir(dir,{recursive:true});
  await writeFile(join(dir,'package.json'),JSON.stringify({name:'@expo/fixture',version:'3',license:'MIT'}));
  await writeFile(join(dir,'expo-module.config.json'),JSON.stringify({android:{publication:{groupId:'expo.fixture',artifactId:'fixture',version:'3',repository:'local-maven-repo'}}}));
  const run=()=>({status:0,stdout:'+--- expo.fixture:fixture:3\n+--- expo.fixture:fixture:4'});
  const inventory=await collectGradleInventory(root,{gradleCache:join(root,'empty-cache'),run});
  expect(inventory.records[0]).toMatchObject({license:'MIT',licenseStatus:'declared',licenseSource:'npm-local-publication'});
  expect(inventory.records[0].licenseEvidence).toHaveLength(2);
  expect(inventory.records[1]).toMatchObject({license:null,licenseStatus:'unknown'});
 }finally {await rm(root,{recursive:true,force:true});}
});

const fixtureIdentity = '<groupId>org.example</groupId><artifactId>child</artifactId><version>1</version>';
const fixtureParent = '<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version></parent>';
const fixtureMit = '<licenses><license><name>MIT</name></license></licenses>';

async function collectPomFixture(body) {
 const {collectGradleInventory}=await import('./release-gradle.mjs');
 const cache=await mkdtemp(join(tmpdir(),'leftcar-license-model-'));
 try {
  for(const [name,xml] of [
   ['child',`<project>${body}</project>`],
   ['parent',`<project><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version>${fixtureMit}</project>`],
  ]) {
   const directory=join(cache,'caches/modules-2/files-2.1/org.example',name,'1/hash');
   await mkdir(directory,{recursive:true});await writeFile(join(directory,`${name}.pom`),xml);
  }
  const result=await collectGradleInventory('/fixture',{gradleCache:cache,run:()=>({status:0,stdout:'+--- org.example:child:1'})});
  return result.records[0];
 }finally {await rm(cache,{recursive:true,force:true});}
}

test.each([
 ['project groupId',`${fixtureIdentity}<groupId>org.wrong</groupId>${fixtureMit}`],
 ['project artifactId',`${fixtureIdentity}<artifactId>wrong</artifactId>${fixtureMit}`],
 ['project version',`${fixtureIdentity}<version>2</version>${fixtureMit}`],
 ['project parent',`${fixtureIdentity}${fixtureParent}${fixtureParent}${fixtureMit}`],
 ['project licenses',`${fixtureIdentity}${fixtureMit}<licenses><license><name>Apache-2.0</name></license></licenses>`],
 ['parent groupId',`${fixtureIdentity}<parent><groupId>org.example</groupId><groupId>org.wrong</groupId><artifactId>parent</artifactId><version>1</version></parent>`],
 ['parent artifactId',`${fixtureIdentity}<parent><groupId>org.example</groupId><artifactId>parent</artifactId><artifactId>wrong</artifactId><version>1</version></parent>`],
 ['parent version',`${fixtureIdentity}<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version><version>2</version></parent>`],
 ['license name',`${fixtureIdentity}<licenses><license><name>MIT</name><name>Apache-2.0</name></license></licenses>`],
 ['license URL',`${fixtureIdentity}<licenses><license><name>MIT</name><url>https://example.invalid/one</url><url>https://example.invalid/two</url></license></licenses>`],
])('Gradle license declaration stays unknown for duplicate %s singletons',async(_name,body)=>{
 const record=await collectPomFixture(body);
 expect(record).toMatchObject({license:null,licenseStatus:'unknown'});
 expect(record.licenseEvidence).toBeUndefined();
});

test.each([
 ['URL-only','<licenses><license><url>https://example.invalid/child-license</url></license></licenses>'],
 ['empty name','<licenses><license><name> </name></license></licenses>'],
 ['partially named list','<licenses><license><name>Apache-2.0</name></license><license><url>https://example.invalid/child-license</url></license></licenses>'],
])('Gradle does not replace a child %s license declaration with its parent license',async(_name,licenses)=>{
 const record=await collectPomFixture(`${fixtureIdentity}${fixtureParent}${licenses}`);
 expect(record).toMatchObject({license:null,licenseStatus:'unknown'});
 expect(record.licenseEvidence).toBeUndefined();
});

test('Gradle inherits licenses only for an empty child list and preserves multiple named child licenses',async()=>{
 const empty=await collectPomFixture(`${fixtureIdentity}${fixtureParent}<licenses/>`);
 expect(empty).toMatchObject({license:'MIT',licenseStatus:'declared',licenseSource:'cached-parent-pom'});
 const multiple=await collectPomFixture(`${fixtureIdentity}${fixtureParent}<licenses><license><name>BSD-2-Clause</name></license><license><name>Apache-2.0</name></license></licenses>`);
 expect(multiple).toMatchObject({license:'Apache-2.0; BSD-2-Clause',licenseStatus:'declared',licenseSource:'cached-pom'});
 expect(multiple.licenseEvidence).toHaveLength(1);
});
