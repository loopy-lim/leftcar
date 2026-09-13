import {summarizeCounterSeries} from './analyze-performance';
export interface Boundary {startMs:number;endMs:number}
interface Row {time:number; count:number; tokens:Record<string,string>}
const tokens=(text:string):Record<string,string>=>Object.fromEntries([...text.matchAll(/\b([\w]+)=([^\s]+)/g)].map(m=>[m[1],m[2]]));
function partition(rows:Row[], boundary:Boundary, host:boolean) {
 if(!Number.isFinite(boundary.startMs)||!Number.isFinite(boundary.endMs)||boundary.endMs<=boundary.startMs)throw new Error('Actual collection boundaries required');
 const groups=new Map<string,Row[][]>();
 for(const row of rows.sort((a,b)=>a.time-b.time)) {
  if(row.time<boundary.startMs||row.time>boundary.endMs)continue;
  const {process,stream,incarnation,kind}=row.tokens;
  const key=process&&stream&&incarnation?JSON.stringify([process,stream,incarnation,kind??'single',row.tokens.schema??'legacy']):'unknown';
  const segments=groups.get(key)??[[]];let last=segments.at(-1)!;
  if(last.length && (row.count<last.at(-1)!.count || row.time<=last.at(-1)!.time)) {last=[];segments.push(last);}
  last.push(row);groups.set(key,segments);
 }
 return [...groups].flatMap(([key,segments])=>segments.map((rows,segment)=>{
  const first=rows[0],last=rows.at(-1)!;
  const coverage={missingHeadMs:first.time-boundary.startMs,missingTailMs:boundary.endMs-last.time};
  const unsupported=first.tokens.schema!=null&&first.tokens.schema!=='2';
  const counter=summarizeCounterSeries(unsupported?[]:rows.map(r=>({timestampMs:r.time,frames:r.count})),last.time-first.time);
  const identity=unsupported?{status:'unsupported-schema' as const,reason:`Unsupported metric schema ${first.tokens.schema}`} : key==='unknown'||first.tokens.schema!=='2'?{status:'unknown' as const,reason:'Historical metric has no stream/incarnation; cannot attribute or aggregate'}:{status:'observed' as const,process:first.tokens.process,stream:first.tokens.stream,incarnation:first.tokens.incarnation};
  if(unsupported){counter.averageFps=null;counter.errors.push('Unsupported metric schema');}
  return {identity,segment,stage:unsupported?'unknown':host?(first.tokens.encoderMode==='splitVertical'?'paired-encoder-output':'encoder-output'):first.tokens.kind==='split'?'paired-surface-release':'surface-release',counter,coverage,
   complete:identity.status==='observed'&&segments.length===1&&coverage.missingHeadMs<=1500&&coverage.missingTailMs<=1500&&counter.errors.length===0&&!counter.zeroFpsStallDetected,
   samples:rows.map(row=>({timestampMs:row.time,...row.tokens})),
   latency:unsupported?{status:'unknown',reason:'Unsupported metric schema; raw fields retained'}:host?{basis:'host-monotonic',input:'encode-submission (single) or capture callback (split pair)',output:'encoder-callback',distribution:'rolling native encodeOutputP95Us snapshots; not a combined frame distribution',status:'see raw native fields'}:{basis:'estimated-host-wall-offset',input:'capture-wall-timestamp',output:'latest-PTS-matched-Surface-release-call-return',distribution:'latest-released-output sample only; not every released output',status:rows.some(r=>r.tokens.releaseCaptureAgeMs?.startsWith('Some('))?'observed':'unknown'},
  };
 }));
}
export function summarizeMeasurement(hostText:string,androidText:string,bounds:{host:Boundary;android:Boundary}) {
 const host:Row[]=[],android:Row[]=[];
 for(const [index,line] of hostText.split('\n').entries()) {
  // /usr/bin/log stream --style ndjson emits this command header before its
  // JSON records. Match the collector's exact predicate only at the first line;
  // malformed records containing LeftcarPerf must still fail closed.
  if(index===0 && /^Filtering the log data using "processIdentifier == [1-9]\d* AND composedMessage CONTAINS "LeftcarPerf""\r?$/.test(line))continue;
  if(!line.includes('LeftcarPerf'))continue;
  let row;try{row=JSON.parse(line);}catch{throw new Error('Malformed Host metric');}
  if(!row.eventMessage?.includes('LeftcarPerf'))continue;
  const fields=tokens(row.eventMessage),time=Date.parse(row.timestamp),count=Number(fields.encoderMode==='splitVertical'?fields.splitPairs:fields.encodeOutputCallbacks);
  if(!Number.isFinite(time)||!Number.isSafeInteger(count)||count<0)throw new Error('Malformed Host counter');
  host.push({time,count,tokens:fields});
 }
 for(const line of androidText.split('\n')) {
  if(!line.includes('LeftcarViewerPerf')&&!/\bRendered \d+ frames;/.test(line))continue;
  const fields=tokens(line),time=Number(line.trim().split(/\s+/)[0])*1000;
  const count=Number(fields.released??/Rendered (\d+) frames;/.exec(line)?.[1]);
  if(!Number.isFinite(time)||!Number.isSafeInteger(count)||count<0)throw new Error('Malformed Viewer counter');
  android.push({time,count,tokens:fields});
 }
 const budgets=new Map<string,Map<number,number>>();
 for(const row of host) {
  if(row.time<bounds.host.startMs||row.time>bounds.host.endMs||row.tokens.schema!=='2'||row.tokens.rtxScope!=='process-budget'||!row.tokens.rtxOwner)continue;
  const value=Number(row.tokens.rtxRetainedEnvelopeBytes);
  if(!Number.isSafeInteger(value)||value<0)continue;
  const series=budgets.get(row.tokens.rtxOwner)??new Map<number,number>();series.set(row.time,value);budgets.set(row.tokens.rtxOwner,series);
 }
 const rtxProcessBudgets=[...budgets].map(([owner,series])=>({owner,scope:'process-budget',unit:'logical-envelope-bytes-not-RSS',samples:[...series].map(([timestampMs,retainedEnvelopeBytes])=>({timestampMs,retainedEnvelopeBytes}))}));
 return {schema:2,rtxProcessBudgets,boundaries:bounds,host:partition(host,bounds.host,true),android:partition(android,bounds.android,false),
  physicalPresentation:{status:'unmeasured',reason:'Surface release is not physical presentation'},glassToGlass:{status:'unmeasured',reason:'Requires suitable optical measurement'},
  final4kCriteriaMet:false,reason:'Stage throughput alone cannot establish physical presentation or complete matrix acceptance',
  sharedCounterRule:'RTX process-budget snapshots are not summed per stream; logical envelope bytes are not RSS'};
}
export function parseThermal(text:string) {
 let source='unknown';const readings:Array<{source:string;name:string;type:number;value:number;unit:string;status:number;sampleAge:null}>=[];
 for(const line of text.split('\n')) {
  if(line.includes('Cached temperatures:'))source='cached';
  else if(line.includes('Current temperatures from HAL:'))source='current-hal';
  for(const m of line.matchAll(/Temperature\{mValue=([-\d.]+), mType=(-?\d+), mName=([^,]+), mStatus=(\d+)\}/g)) {
   const type=Number(m[2]),value=Number(m[1]);
   if(Number.isFinite(value))readings.push({source,name:m[3],type,value,unit:[0,1,2,3,4].includes(type)?'celsius':'unknown',status:Number(m[4]),sampleAge:null});
  }
 }
 return {status:readings.length?'observed':'missing',readings,attribution:'system sensors, not per-app heat'};
}
// Receipt fields remain optional here so historical or incomplete receipts fail closed.
interface Comparable {
 conditions?:Record<string,unknown>; summary?:any; context?:{serial?:string};
 collection?:{status:string;reasons:string[]}; boundaries?:any; processes?:any;
 timing?:{requestedMs:number;elapsedMs:number}; interrupted?:boolean;
 childErrors?:string[]; childOutcomes?:Array<{index:number;expected:boolean;code:number|null;signal:string|null}>;
}
export const durationToleranceMs=(requestedMs:number)=>Math.min(1500,requestedMs*0.05);
export function collectionInvalidReasons(receipt:Comparable):string[] {
 const reasons:string[]=[];
 if(!receipt.context?.serial)reasons.push('Selected device identity unavailable');
 if(receipt.interrupted!==false)reasons.push('Interrupted or interruption status unavailable');
 if(!receipt.childErrors||receipt.childErrors.length)reasons.push('Collector failure or error status unavailable');
 if(receipt.childOutcomes?.length!==2 || ![0,1].every(index=>receipt.childOutcomes?.some(x=>x.index===index&&x.expected&&(x.code===0||x.signal==='SIGTERM'))))reasons.push('Collectors did not complete under owned termination');
 const {initial,final,unchanged}=receipt.processes??{};
 const identity=(p:any)=>p&&/^\d+$/.test(p.pid)&&/^\d+$/.test(p.startTicks)&&typeof p.host==='string'&&p.host.trim().length>0;
 if(!identity(initial)||!identity(final)||unchanged!==true||JSON.stringify(initial)!==JSON.stringify(final))reasons.push('Process identity missing or replaced');
 const {start,end}=receipt.boundaries??{};
 const bracket=(b:any)=>b&&[b.hostBeforeMs,b.hostAfterMs,b.deviceMs].every(Number.isFinite)&&b.hostAfterMs>=b.hostBeforeMs;
 const requested=receipt.timing?.requestedMs,elapsed=receipt.timing?.elapsedMs;
 if(!Number.isSafeInteger(requested)||requested!<1000||requested!>1800000||!Number.isFinite(elapsed))reasons.push('Requested or monotonic duration unavailable');
 if(!bracket(start)||!bracket(end)||end.hostBeforeMs<=start.hostAfterMs||end.deviceMs<=start.deviceMs)reasons.push('Actual clock boundaries unavailable or reversed');
 else if(requested!=null){
  const tolerance=durationToleranceMs(requested),hostMs=end.hostBeforeMs-start.hostAfterMs,deviceMs=end.deviceMs-start.deviceMs;
  if(Math.abs(hostMs-requested)>tolerance||Math.abs(deviceMs-hostMs)>tolerance||Math.abs(elapsed!-requested)>tolerance||start.hostAfterMs-start.hostBeforeMs>tolerance||end.hostAfterMs-end.hostBeforeMs>tolerance)reasons.push('Actual duration or clock acquisition exceeds declared tolerance');
 }
 return reasons;
}
export function finalizeMeasurement<T extends Comparable>(receipt:T,hostText:string,androidText:string) {
 const reasons=collectionInvalidReasons(receipt);
 let summary:any;
 try {
  const {start,end}=receipt.boundaries??{};
  if(!start||!end)throw new Error('No actual start/end boundary');
  summary=summarizeMeasurement(hostText,androidText,{host:{startMs:start.hostAfterMs,endMs:end.hostBeforeMs},android:{startMs:start.deviceMs,endMs:end.deviceMs}});
  if(!summary.host.length||!summary.android.length||[...summary.host,...summary.android].some(s=>!s.complete))reasons.push('Incomplete or unsupported attributed stage metrics');
 }catch(error){reasons.push(`Summary unavailable: ${String(error)}`);summary={status:'unavailable',reason:String(error),physicalPresentation:{status:'unmeasured'},glassToGlass:{status:'unmeasured'},final4kCriteriaMet:false};}
 return {...receipt,summary,collection:{status:reasons.length?'invalid':'valid',reasons},exitCode:reasons.length?1:0};
}
export function compareMeasurements(baseline:Comparable,candidate:Comparable) {
 const keys=['sourceSha256','workload','transport','effectiveMode','geometry','effectiveVideoCodec','effectiveAudioCodec','sourceCount','sourceViewport','encodedDimensions','surfaceDimensions','physicalPanelDimensions','refreshHz'];
 const differences=keys.filter(key=>baseline.conditions?.[key]==null||candidate.conditions?.[key]==null||JSON.stringify(baseline.conditions[key])!==JSON.stringify(candidate.conditions[key]));
 const invalidRuns=([['baseline',baseline],['candidate',candidate]] as const).flatMap(([name,receipt])=>{
  const reasons=collectionInvalidReasons(receipt);
  if(receipt.collection?.status!=='valid')reasons.push('Explicit valid collection status required');
  if(!receipt.summary?.host?.length||!receipt.summary?.android?.length||[...(receipt.summary?.host??[]),...(receipt.summary?.android??[])].some(s=>!s.complete))reasons.push('Complete attributed stage summary required');
  return reasons.map(reason=>`${name}: ${reason}`);
 });
 if(!baseline.context?.serial||baseline.context.serial!==candidate.context?.serial)differences.push('selectedDevice');
 if(!baseline.timing||!candidate.timing||baseline.timing.requestedMs!==candidate.timing.requestedMs)differences.push('requestedDuration');
 else {
  const tolerance=durationToleranceMs(baseline.timing.requestedMs);
  const actual=(r:Comparable)=>[r.timing?.elapsedMs,(r.boundaries?.end?.hostBeforeMs??NaN)-(r.boundaries?.start?.hostAfterMs??NaN),(r.boundaries?.end?.deviceMs??NaN)-(r.boundaries?.start?.deviceMs??NaN)];
  const left=actual(baseline),right=actual(candidate);
  if(left.some((value,index)=>!Number.isFinite(value)||!Number.isFinite(right[index])||Math.abs(value!-right[index]!)>tolerance))differences.push('actualDuration');
 }
 return {comparable:differences.length===0&&invalidRuns.length===0,differences,invalidRuns,baseline:baseline.summary,candidate:candidate.summary,claim:'Same-condition stage observations only; no competitor or physical-presentation inference'};
}
