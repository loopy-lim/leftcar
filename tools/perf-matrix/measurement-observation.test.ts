import {expect,test} from 'vitest';
import {finalizeMeasurement,summarizeMeasurement} from './measurement';
import {summarizeCounterSeries,summarizePerformance} from './analyze-performance';

const bounds={host:{startMs:1000,endMs:4000},android:{startMs:1000,endMs:4000}};
const host=(t:number,n:number)=>JSON.stringify({timestamp:new Date(t).toISOString(),eventMessage:`LeftcarPerf schema=2 process=h stream=s incarnation=i encodeOutputCallbacks=${n}`});
const canonical=(t:number,n:number,epoch=0,extra='')=>`${t} 42 70 I LeftcarNative: LeftcarViewerPerf schema=2 process=42 stream=5001 incarnation=i decoderEpoch=${epoch} kind=single released=${n} ${extra}`;
const legacy=(t:number,n:number)=>`${t} 42 70 I LeftcarNative: Rendered ${n} frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None`;
const paired=[canonical(1,30),legacy(1.001,30),canonical(2,90),legacy(2.002,90),canonical(3,150,1),legacy(3.003,30),canonical(4,210,1),legacy(4.01,90)];

function receipt() {
 const identity={pid:'42',startTicks:'123',host:'99 fixed Host process'};
 return {context:{serial:'fixture'},boundaries:{start:{hostBeforeMs:999,hostAfterMs:1000,deviceMs:1000},end:{hostBeforeMs:4000,hostAfterMs:4001,deviceMs:4000}},timing:{requestedMs:3000,elapsedMs:3000},processes:{initial:identity,final:{...identity},unchanged:true},interrupted:false,childErrors:[],childOutcomes:[{index:0,code:null,signal:'SIGTERM',expected:true},{index:1,code:null,signal:'SIGTERM',expected:true}],rawHashes:{host:'unchanged-host',android:'unchanged-android'}};
}
const validHost=[host(1000,0),host(2000,60),host(3000,120),host(4000,180)].join('\n');

test('same-emitter companions preserve decoder epoch offsets and one canonical series',()=>{
 const result=summarizeMeasurement(validHost,paired.join('\n'),bounds);
 expect(result.android).toHaveLength(1);
 const stream=result.android[0];
 expect(stream.identity.status).toBe('observed');
 expect(stream.counter.sampleCount).toBe(4);
 expect(stream.counter.averageFps).toBe(60);
 expect(stream.complete).toBe(true);
 expect(stream.samples.map(row=>row.legacyCompanion?.counterOffset)).toEqual([0,0,120,120]);
 expect(stream.samples.map(row=>row.legacyCompanion?.decoderFrames)).toEqual([30,90,30,90]);
 expect(stream.samples[3].legacyCompanion?.raw).toBe(legacy(4.01,90));
 expect(result.physicalPresentation.status).toBe('unmeasured');
 expect(result.final4kCriteriaMet).toBe(false);
});

test('a companion just past the end stays linked without changing collection boundaries',()=>{
 const result=summarizeMeasurement('',[canonical(3,150,1),legacy(3.001,30),canonical(4,210,1),legacy(4.01,90)].join('\n'),bounds);
 expect(result.android).toHaveLength(1);
 expect(result.android[0].counter.sampleCount).toBe(2);
 expect(result.android[0].samples[1].timestampMs).toBe(4000);
 expect(result.android[0].samples[1].legacyCompanion?.timestampMs).toBe(4010);
 expect(result.android[0].coverage.missingHeadMs).toBe(2000);
 expect(result.android[0].complete).toBe(false);
});

test('other-thread interleaving is allowed but an intervening same-emitter event is not skipped',()=>{
 const other='1.0005 42 71 I LeftcarNative: unrelated';
 const same='1.0005 42 70 I LeftcarNative: unrelated';
 const linked=summarizeMeasurement('',[canonical(1,30),other,legacy(1.001,30)].join('\n'),bounds);
 expect(linked.android).toHaveLength(1);
 const orphan=summarizeMeasurement('',[canonical(1,30),same,legacy(1.001,30)].join('\n'),bounds);
 expect(orphan.android.some(row=>row.identity.status==='unknown')).toBe(true);
});

test.each([
 legacy(1.001,30).replace('42 70','43 70'),
 legacy(1.001,30).replace('42 70','42 71'),
])('foreign PID/TID legacy remains unknown: %s',line=>{
 const result=summarizeMeasurement('',[canonical(1,30),line].join('\n'),bounds);
 expect(result.android.some(row=>row.identity.status==='unknown')).toBe(true);
});

test.each([
 canonical(1,30).replace('schema=2','schema=999'),
 canonical(1,30).replace('schema=2 ',''),
 canonical(1,30).replace('kind=single','kind=future'),
])('unsupported or incomplete canonical cannot hide a legacy companion: %s',line=>{
 const result=summarizeMeasurement('',[line,legacy(1.001,30)].join('\n'),bounds);
 expect(result.android.some(row=>row.identity.status==='unknown')).toBe(true);
 expect(result.android.every(row=>!row.complete)).toBe(true);
});

test('standalone legacy is preserved even when valid canonical data exists elsewhere',()=>{
 const text=[...paired,legacy(3.5,999).replace('42 70','99 70')].join('\n');
 const result=finalizeMeasurement(receipt(),validHost,text);
 expect(result.collection.status).toBe('invalid');
 expect(result.summary.android.some((row:any)=>row.identity.status==='unknown')).toBe(true);
 const historical=summarizeMeasurement('',[legacy(1,30),legacy(2,90)].join('\n'),bounds);
 expect(historical.android[0].identity.status).toBe('unknown');
 expect(historical.android[0].complete).toBe(false);
});

test('an epoch-local companion counter mismatch is explicit corruption',()=>{
 const result=finalizeMeasurement(receipt(),validHost,[canonical(1,30),legacy(1.001,30),canonical(2,90),legacy(2.001,89)].join('\n'));
 expect(result.collection.status).toBe('invalid');
 expect(result.summary.status).toBe('unavailable');
 expect(result.collection.reasons.join(' ')).toContain('Inconsistent Viewer companion counter offset');
 expect(result.rawHashes).toEqual(receipt().rawHashes);
});

test.each([
 legacy(1.001,30).replace('Rendered 30','Rendered NaN'),
 legacy(1.001,30).replace('Rendered 30','Rendered -1'),
 legacy(1.001,30).replace('outputDrops=0','outputDrops=0junk'),
 legacy(1.001,30)+' released=30',
 legacy(1.001,30)+' schema=999',
 canonical(1,30).replace('LeftcarNative:','OtherTag:'),
 canonical(1,30)+' released=31',
 canonical(1,30).replace('released=30','released=1junk'),
 canonical(1,30).replace('released=30','released=9007199254740992'),
])('malformed companion/canonical rows are never silently deduplicated: %s',line=>{
 const result=finalizeMeasurement(receipt(),validHost,[...paired,line].join('\n'));
 expect(result.collection.status).toBe('invalid');
 expect(result.summary.status).toBe('unavailable');
 expect(result.rawHashes).toEqual(receipt().rawHashes);
});

test('two-second Host cadence remains incomplete without inventing zero FPS',()=>{
 const result=finalizeMeasurement(receipt(),[host(1000,0),host(3001,120)].join('\n'),paired.join('\n'));
 const counter=result.summary.host[0].counter;
 expect(counter.averageFps).toBeCloseTo(120000/2001);
 expect(counter.maxSampleGapMs).toBe(2001);
 expect(counter.zeroFpsStallDetected).toBe(false);
 expect(counter.observedZeroCounterIntervals).toEqual([]);
 expect(counter.rollingOneSecondP5Fps).toBeNull();
 expect(counter.observationComplete).toBe(false);
 expect(result.collection.status).toBe('invalid');
});

test('head phase is retained as missing observation, not a proven stage stall',()=>{
 const result=summarizeMeasurement([host(2791,30),host(3791,90)].join('\n'),'',bounds);
 const stream=result.host[0];
 expect(stream.coverage.missingHeadMs).toBe(1791);
 expect(stream.counter.zeroFpsStallDetected).toBe(false);
 expect(stream.counter.observationGaps).toContainEqual({kind:'head',startMs:1000,endMs:2791});
 expect(stream.complete).toBe(false);
});

test('progress-triggered Viewer gaps preserve low throughput without claiming zero',()=>{
 const counter=summarizeCounterSeries([{timestampMs:0,frames:0},{timestampMs:2551,frames:30}],2551);
 expect(counter.averageFps).toBeCloseTo(30000/2551);
 expect(counter.zeroFpsStallDetected).toBe(false);
 expect(counter.observedZeroCounterIntervals).toEqual([]);
 expect(counter.observationComplete).toBe(false);
 expect(counter.rollingOneSecondWindowCount).toBe(0);
});

test('positive observed p5 cannot turn a missing interval into a 55 FPS pass',()=>{
 const times=[0,1000,4000,5000];
 const hostSamples=times.map(t=>({timestampMs:t,captureCallbacks:t*0.06,encodeOutputCallbacks:t*0.06,queueOldestUs:0}));
 const androidSamples=times.map(t=>({timestampMs:t,frames:t*0.06,outputDrops:0,decoderInputDrops:0,frameGaps:0,captureAgeMs:25}));
 const result=summarizePerformance(hostSamples,androidSamples,5000);
 expect(result.host.encodeOutput.averageFps).toBe(60);
 expect(result.host.encodeOutput.rollingOneSecondP5Fps).toBe(60);
 expect(result.host.encodeOutput.zeroFpsStallDetected).toBe(false);
 expect(result.host.encodeOutput.observationComplete).toBe(false);
 expect(result.candidate55Fps).toBe(false);
 expect(result.final4kCriteriaMet).toBe(false);
});


test.each([
 ['missing PID/TID',(line:string)=>line.replace('42 70 ','')],
 ['PID mismatch',(line:string)=>line.replace('process=42','process=43')],
 ['missing epoch',(line:string)=>line.replace('decoderEpoch=0 ','')],
 ['invalid epoch',(line:string)=>line.replace('decoderEpoch=0','decoderEpoch=NaN')],
 ['wrong tag',(line:string)=>line.replace('LeftcarNative:','OtherTag:')],
] as const)('%s cannot become complete without legacy or through a healthy prefix',(_label,corrupt)=>{
 const good=[1,2,3,4].map((time,index)=>canonical(time,30+index*60));
 for(const rows of [good.map(corrupt),[good[0],corrupt(good[1]),good[2],good[3]]]) {
  const result=finalizeMeasurement(receipt(),validHost,rows.join('\n'));
  expect(result.collection.status).toBe('invalid');
  expect(result.summary.status).toBe('unavailable');
  expect(result.rawHashes).toEqual(receipt().rawHashes);
 }
});

test('raw canonical timestamp reversal cannot be repaired silently by sorting',()=>{
 const rows=[canonical(2,90),legacy(2.001,90),canonical(1,30),legacy(1.001,30),canonical(3,150),legacy(3.001,150),canonical(4,210),legacy(4.001,210)];
 const result=finalizeMeasurement(receipt(),validHost,rows.join('\n'));
 expect(result.collection.status).toBe('invalid');
 expect(result.summary.status).toBe('unavailable');
 expect(result.collection.reasons.join(' ')).toContain('Non-advancing canonical Viewer timestamp');
});

test('supported split identity has a real PID/header but requires no decoder epoch',()=>{
 const rows=[1,2,3,4].map((time,index)=>`${time} 42 70 I LeftcarNative: LeftcarViewerPerf schema=2 process=42 stream=5001 incarnation=i kind=split released=${30+index*60} leftReleased=${31+index*60} rightReleased=${32+index*60}`);
 const result=finalizeMeasurement(receipt(),validHost,rows.join('\n'));
 expect(result.collection.status).toBe('valid');
 expect(result.summary.android[0].stage).toBe('paired-surface-release');
 expect(result.summary.android[0].counter.averageFps).toBe(60);
});
