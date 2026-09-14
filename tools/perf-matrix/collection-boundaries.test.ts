import {expect,test} from 'vitest';
import {compareMeasurements,finalizeMeasurement} from './measurement';
const conditions={sourceSha256:'abc',workload:'scroll',transport:'udp',effectiveMode:'immediate',geometry:'2560x1440',effectiveVideoCodec:'h264',effectiveAudioCodec:'off',sourceCount:1,sourceViewport:'2560x1440',encodedDimensions:'2560x1440',surfaceDimensions:'2560x1440',physicalPanelDimensions:'3200x2000',refreshHz:60};
const identity={pid:'42',startTicks:'123',host:'99 Sat Sep 12 00:00:00 2026 Host'};
export const receipt=()=>({schema:2,context:{serial:'tablet-a'},conditions,provenance:{sourceManifest:{sha256:'abc'}},boundaries:{start:{hostBeforeMs:999,hostAfterMs:1000,deviceMs:1000},end:{hostBeforeMs:2000,hostAfterMs:2001,deviceMs:2000}},timing:{requestedMs:1000,elapsedMs:1000},processes:{initial:identity,final:{...identity},unchanged:true},interrupted:false,childErrors:[],childOutcomes:[{index:0,code:null,signal:'SIGTERM',expected:true},{index:1,code:null,signal:'SIGTERM',expected:true}],resources:[{sample:'retained'}],rawHashes:{host:'raw-host',android:'raw-android'}});
const host=[1000,2000].map((t,i)=>JSON.stringify({timestamp:new Date(t).toISOString(),eventMessage:`LeftcarPerf schema=2 process=p stream=s incarnation=i encodeOutputCallbacks=${i*60}`})).join('\n');
const android=[1,2].map((t,i)=>`${t}.0 42 70 I LeftcarNative: LeftcarViewerPerf schema=2 process=42 stream=s incarnation=i decoderEpoch=0 kind=single released=${i*60}`).join('\n');
const finish=(r:ReturnType<typeof receipt>)=>finalizeMeasurement(r,host,android);
test('production finalization admits a complete same-device actual-duration pair',()=>{const r=finish(receipt());expect(r.collection.status).toBe('valid');expect(r.exitCode).toBe(0);expect(compareMeasurements(r,r).comparable).toBe(true);});
test.each(['interrupted','child','replacement','identity','no-end','duration','clock'] as const)('invalid %s run is retained, unsuccessful and incomparable',kind=>{
 const r:any=receipt();
 if(kind==='interrupted')r.interrupted=true;
 if(kind==='child'){r.childErrors=['early exit'];r.childOutcomes[0].expected=false;}
 if(kind==='replacement'){r.processes.final.pid='43';r.processes.unchanged=false;}
 if(kind==='identity')r.processes.initial=null;
 if(kind==='no-end')r.boundaries.end=null;
 if(kind==='duration')r.timing.requestedMs=1800000;
 if(kind==='clock')r.boundaries.end.deviceMs=500;
 const result=finish(r);expect(result.collection.status).toBe('invalid');expect(result.exitCode).not.toBe(0);expect(result.resources).toEqual(r.resources);expect(result.rawHashes).toEqual(r.rawHashes);expect(compareMeasurements(finish(receipt()),result).comparable).toBe(false);
});
test('parser failure and clock failure preserve complete receipt and unavailable summary',()=>{
 for(const [r,text] of [[receipt(),host+'\n{LeftcarPerf'],[{...receipt(),boundaries:{...receipt().boundaries,end:{hostBeforeMs:500,hostAfterMs:501,deviceMs:500}}},host]] as const){
  const result=finalizeMeasurement(r,text,android);expect(result.collection.status).toBe('invalid');expect(result.exitCode).toBe(1);expect(result.summary.status).toBe('unavailable');expect(result.provenance).toEqual(r.provenance);expect(result.rawHashes).toEqual(r.rawHashes);
 }
});
test('different selected serial and actual/intended durations are not comparable',()=>{
 const valid=finish(receipt());
 expect(compareMeasurements(valid,{...valid,context:{serial:'tablet-b'}}).comparable).toBe(false);
 expect(compareMeasurements(valid,{...valid,timing:{requestedMs:1800000,elapsedMs:1800000}}).comparable).toBe(false);
 expect(compareMeasurements(valid,{...valid,boundaries:{...valid.boundaries,end:null}}).comparable).toBe(false);
 expect(compareMeasurements({conditions,summary:{}},valid).comparable).toBe(false);
});
test('unsupported schemas never produce known counter stages or complete throughput',()=>{
 const result=finalizeMeasurement(receipt(),host.replaceAll('schema=2','schema=999'),android.replaceAll('schema=2','schema=999'));
 expect(result.collection.status).toBe('invalid');
 for(const stream of [...result.summary.host,...result.summary.android]){expect(stream.identity.status).toBe('unsupported-schema');expect(stream.complete).toBe(false);expect(stream.stage).toBe('unknown');expect(stream.counter.averageFps).toBeNull();}
});
test('pair comparison checks actual clock windows, even when nominal and monotonic labels match',()=>{
 const r=receipt(),earlyHost=host.replace(new Date(2000).toISOString(),new Date(1950).toISOString()),earlyAndroid=android.replace('2.0 42','1.95 42');
 const left=finalizeMeasurement({...r,boundaries:{...r.boundaries,end:{hostBeforeMs:1960,hostAfterMs:1961,deviceMs:1960}}},earlyHost,earlyAndroid),right=finalizeMeasurement({...r,boundaries:{...r.boundaries,end:{hostBeforeMs:2040,hostAfterMs:2041,deviceMs:2040}}},earlyHost,earlyAndroid);
 expect(left.collection.status).toBe('valid');expect(right.collection.status).toBe('valid');expect(compareMeasurements(left,right).differences).toContain('actualDuration');expect(compareMeasurements(left,right).comparable).toBe(false);
});
test('missing schema and mixed future schema streams cannot become complete',()=>{
 const r=finalizeMeasurement(receipt(),host.replaceAll('schema=2 ','')+'\n'+host.replaceAll('schema=2','schema=999'),android);
 expect(r.collection.status).toBe('invalid');expect(r.summary.host.map((s:any)=>s.identity.status)).toContain('unsupported-schema');expect(r.summary.host.every((s:any)=>!s.complete)).toBe(true);
});
