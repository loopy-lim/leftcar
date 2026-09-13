import {expect,test} from 'vitest';
import {runCollection} from './collection-runtime.mjs';
const identity={pid:'42',startTicks:'123',host:'99 Host'};
function fixture(overrides={}){
 const calls=[];let processCalls=0;
 return {calls,io:{processRead:async()=>{calls.push('process');processCalls++;return {...identity};},bracket:async()=>{calls.push('clock');return {hostBeforeMs:Date.now(),hostAfterMs:Date.now(),deviceMs:Date.now()};},startLogs:async fail=>{calls.push('start');},sample:async()=>{calls.push('sample');return {raw:'resource'};},stopLogs:async()=>{calls.push('stop');return [{index:0,code:null,signal:'SIGTERM',expected:true},{index:1,code:null,signal:'SIGTERM',expected:true}];},...overrides}};
}
test('slow sample is cancelled at deadline, not allowed to redefine nominal duration',async()=>{
 let cancelled=false;
 const f=fixture({sample:signal=>new Promise(()=>{signal.addEventListener('abort',()=>{cancelled=true;});})});
 const start=performance.now();const r=await runCollection(f.io,{durationMs:30,operationTimeoutMs:100});
 expect(performance.now()-start).toBeLessThan(250);expect(cancelled).toBe(true);expect(r.timing.elapsedMs).toBeLessThan(200);expect(r.boundaries.end).not.toBeNull();expect(f.calls.at(-1)).toBe('stop');
});
test('interrupt during hung preparation retains acquisition failure and owned cleanup',async()=>{
 const controller=new AbortController();const f=fixture({processRead:()=>new Promise(()=>{})});
 setTimeout(()=>controller.abort(new Error('fixture interrupt')),10);
 const r=await runCollection(f.io,{durationMs:500,signal:controller.signal,operationTimeoutMs:100});
 expect(r.interrupted).toBe(true);expect(r.acquisition.startedAtMs).toBeGreaterThan(0);expect(r.childErrors.join()).toContain('fixture interrupt');expect(r.boundaries.start).toBeNull();expect(f.calls).toEqual(['stop']);
});
test('early log child failure aborts sampling and retains evidence',async()=>{
 const f=fixture({startLogs:async fail=>{setTimeout(()=>fail('child exited early'),10);},sample:()=>new Promise(()=>{})});
 const r=await runCollection(f.io,{durationMs:500,operationTimeoutMs:100});
 expect(r.childErrors.join()).toContain('child exited early');expect(r.timing.elapsedMs).toBeLessThan(250);expect(f.calls.at(-1)).toBe('stop');
});
test('failed clock preparation and partial log launch always stop owned children',async()=>{
 for(const overrides of [{bracket:async()=>{throw new Error('clock broken');}},{startLogs:async()=>{throw new Error('second child launch broken');}}]){
  const f=fixture(overrides);const r=await runCollection(f.io,{durationMs:20,operationTimeoutMs:100});expect(r.childErrors.length).toBeGreaterThan(0);expect(f.calls.at(-1)).toBe('stop');expect(r.acquisition.startedAtMs).toBeGreaterThan(0);
 }
});
test('hung final clock and cleanup are bounded and remain invalid evidence',async()=>{
 let clocks=0;const f=fixture({bracket:async()=>{if(clocks++)return new Promise(()=>{});return {hostBeforeMs:Date.now(),hostAfterMs:Date.now(),deviceMs:Date.now()};},stopLogs:()=>new Promise(()=>{})});
 const start=performance.now();const r=await runCollection(f.io,{durationMs:10,operationTimeoutMs:20});
 expect(performance.now()-start).toBeLessThan(250);expect(r.boundaries.end).toBeNull();expect(r.childErrors.join()).toContain('deadline');
});
