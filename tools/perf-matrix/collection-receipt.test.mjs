import {expect,test} from 'vitest';
import {mkdtempSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {saveCollection} from './collection-receipt.mjs';
import {runCollection} from './collection-runtime.mjs';
test('actual receipt writer preserves malformed raw provenance and emits unsuccessful result',()=>{
 const dir=mkdtempSync(join(tmpdir(),'leftcar-receipt-')),prefix=join(dir,'run');
 try{
  writeFileSync(prefix+'.host.ndjson','{LeftcarPerf truncated');writeFileSync(prefix+'.android.log','raw retained');
  const receipt={context:{serial:'selected'},provenance:{host:{sha256:'actual'}},boundaries:{start:{hostBeforeMs:999,hostAfterMs:1000,deviceMs:1000},end:{hostBeforeMs:2000,hostAfterMs:2001,deviceMs:2000}},resources:[{raw:'retained'}],childErrors:[]};
  const r=saveCollection(prefix,receipt);const saved=JSON.parse(readFileSync(prefix+'.collection.json','utf8'));
  expect(r.exitCode).toBe(1);expect(saved.summary.status).toBe('unavailable');expect(saved.provenance).toEqual(receipt.provenance);expect(saved.rawHashes.host).toMatch(/^[a-f0-9]{64}$/);expect(saved.resources).toEqual(receipt.resources);
  expect(readFileSync(prefix+'.host.ndjson','utf8')).toBe('{LeftcarPerf truncated');
 }finally{rmSync(dir,{recursive:true,force:true});}
});
test('production preparation failure persists structured receipt without any device',async()=>{
 const dir=mkdtempSync(join(tmpdir(),'leftcar-preparation-')),prefix=join(dir,'run');
 try{
  writeFileSync(prefix+'.host.ndjson','');writeFileSync(prefix+'.android.log','');
  const acquisition=await runCollection({processRead:async()=>{throw new Error('selected process missing');},stopLogs:async()=>[]},{durationMs:1000});
  const r=saveCollection(prefix,{...acquisition,context:{serial:'selected'},provenance:{source:'retained'}});
  expect(r.exitCode).toBe(1);expect(r.acquisition.startedAtMs).toBeGreaterThan(0);expect(r.childErrors.join()).toContain('selected process missing');expect(JSON.parse(readFileSync(prefix+'.collection.json','utf8')).provenance.source).toBe('retained');
 }finally{rmSync(dir,{recursive:true,force:true});}
});
