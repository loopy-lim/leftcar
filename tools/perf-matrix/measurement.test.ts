import {expect,test} from 'vitest';
import {summarizeMeasurement,parseThermal,compareMeasurements} from './measurement';
const row=(t:number,n:number,extra='')=>JSON.stringify({timestamp:new Date(t).toISOString(),eventMessage:`LeftcarPerf schema=2 process=p stream=s incarnation=i captureCallbacks=${n} encodeOutputCallbacks=${n} ${extra}`});
const bounds={host:{startMs:1000,endMs:6000},android:{startMs:1000,endMs:6000}};
test('partitions actual streams and reset segments; delayed samples do not move the boundary',()=>{
 const result=summarizeMeasurement([row(3000,0),row(4000,60),row(5000,120),row(6000,180),row(4000,100,'stream=other'),row(5000,160,'stream=other'),row(6000,0,'stream=other')].join('\n'),'',bounds);
 expect(result.host).toHaveLength(3);expect(result.host[0].coverage.missingHeadMs).toBe(2000);expect(result.host[0].complete).toBe(false);
 expect(result.host[1].counter.averageFps).toBe(60);expect(result.host[2].counter.averageFps).toBeNull();
});
test('legacy identity stays unknown and split pairs are never summed as independent frames',()=>{
 const android=['1.0 42 70 I LeftcarNative: LeftcarViewerPerf schema=2 process=42 stream=s incarnation=i kind=split released=10 leftReleased=11 rightReleased=12','2.0 42 70 I LeftcarNative: LeftcarViewerPerf schema=2 process=42 stream=s incarnation=i kind=split released=70 leftReleased=71 rightReleased=72'].join('\n');
 const r=summarizeMeasurement(JSON.stringify({timestamp:new Date(1000).toISOString(),eventMessage:'LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0'}),android,{host:{startMs:1000,endMs:2000},android:{startMs:1000,endMs:2000}});
 expect(r.host[0].identity.status).toBe('unknown');expect(r.android[0].counter.averageFps).toBe(60);expect(r.android[0].stage).toBe('paired-surface-release');expect(r.physicalPresentation.status).toBe('unmeasured');
});
test('thermal separates current HAL cached and non-Celsius sensor types',()=>{
 const r=parseThermal('Cached temperatures:\nTemperature{mValue=76.3, mType=0, mName=CPU5, mStatus=0}\nCurrent temperatures from HAL:\nTemperature{mValue=38.7, mType=0, mName=CPU5, mStatus=0}\nTemperature{mValue=4200, mType=6, mName=vbat, mStatus=0}');
 expect(r.readings.map(x=>[x.source,x.unit,x.value])).toEqual([['cached','celsius',76.3],['current-hal','celsius',38.7],['current-hal','unknown',4200]]);
 expect(parseThermal('unavailable').status).toBe('missing');
});
test('comparison fails closed for unlike workloads or unknown effective modes',()=>{
 const base={conditions:{sourceSha256:'abc',workload:'scroll',transport:'udp',effectiveMode:'immediate',geometry:'2560x1440',effectiveVideoCodec:'h264',effectiveAudioCodec:'off',sourceCount:1,sourceViewport:'2560x1440',encodedDimensions:'2560x1440',surfaceDimensions:'2560x1440',physicalPanelDimensions:'3200x2000',refreshHz:60},summary:{}};
 expect(compareMeasurements(base,{...base,conditions:{...base.conditions,workload:'video'}}).comparable).toBe(false);
 expect(compareMeasurements(base,{...base,conditions:{...base.conditions,effectiveMode:null}}).comparable).toBe(false);
 expect(compareMeasurements(base,{...base,conditions:{...base.conditions,sourceCount:2}}).comparable).toBe(false);
 expect(compareMeasurements(base,base).comparable).toBe(false); // Historical receipts lack valid acquisition evidence.
});
test('Host split uses complete pairs; process RTX snapshot is deduplicated across streams',()=>{
 const data=[row(1000,20,'encoderMode=splitVertical splitPairs=10 rtxScope=process-budget rtxOwner=p rtxRetainedEnvelopeBytes=200'),row(2000,140,'encoderMode=splitVertical splitPairs=70 rtxScope=process-budget rtxOwner=p rtxRetainedEnvelopeBytes=400'),row(2000,140,'stream=s2 encoderMode=splitVertical splitPairs=70 rtxScope=process-budget rtxOwner=p rtxRetainedEnvelopeBytes=400')].join('\n');
 const result=summarizeMeasurement(data,'',{host:{startMs:1000,endMs:2000},android:{startMs:1000,endMs:2000}});
 expect(result.host[0].latency.output).toBe('encoder-callback');expect(result.host[0].counter.averageFps).toBe(60);expect(result.host[0].stage).toBe('paired-encoder-output');
 expect(result.rtxProcessBudgets[0].samples.map(s=>s.retainedEnvelopeBytes)).toEqual([200,400]);
});

test('actual macOS log command header is metadata while valid records remain measurable',()=>{
 const header='Filtering the log data using "processIdentifier == 48868 AND composedMessage CONTAINS "LeftcarPerf""';
 const metrics=[row(1000,0),row(2000,60)].join('\n');
 const expected=summarizeMeasurement(metrics,'',{host:{startMs:1000,endMs:2000},android:{startMs:1000,endMs:2000}});
 expect(summarizeMeasurement(header+'\n'+metrics,'',{host:{startMs:1000,endMs:2000},android:{startMs:1000,endMs:2000}})).toEqual(expected);
});
test('command header recognition never hides malformed Host metric records',()=>{
 const header='Filtering the log data using "processIdentifier == 48868 AND composedMessage CONTAINS "LeftcarPerf""';
 for(const malformed of ['{"eventMessage":"LeftcarPerf schema=2', header+' trailing metric corruption', 'Filtering the log data using "LeftcarPerf schema=2"']) {
  expect(()=>summarizeMeasurement(header+'\n'+row(1000,0)+'\n'+malformed,'',bounds)).toThrow('Malformed Host metric');
 }
 expect(()=>summarizeMeasurement(row(1000,0)+'\n'+header,'',bounds)).toThrow('Malformed Host metric');
});
