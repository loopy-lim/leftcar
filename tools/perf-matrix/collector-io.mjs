import {spawn,execFile} from 'node:child_process';
import {appendFileSync} from 'node:fs';
import {collectionCommands,processResources} from './collection-plan.mjs';
import {parseThermal} from './measurement.ts';
// execFile cancellation kills only the command we started, never a device process.
export const command=(file,args,signal)=>new Promise((resolve,reject)=>{
 execFile(file,args,{encoding:'utf8',signal,timeout:10000,killSignal:'SIGKILL',maxBuffer:4*1024*1024},(error,stdout)=>error?reject(error):resolve(stdout.trim()));
});
export function createCollectorIO(context,prefix) {
 const adb=(args,signal)=>command('adb',['-s',context.serial,...args],signal);
 const children=[];let stopping=false,viewerPid;
 return {
  async processRead(signal){
   const pids=(await adb(['shell','pidof',context.package],signal)).split(/\s+/).filter(Boolean);
   if(pids.length!==1||!/^\d+$/.test(pids[0]))throw new Error('Expected exactly one selected Viewer process');
   const stat=await adb(['shell','cat',`/proc/${pids[0]}/stat`],signal);
   const startTicks=stat.slice(stat.lastIndexOf(')')+2).split(/\s+/)[19];
   const host=await command('/bin/ps',['-p',String(context.hostPid),'-o','pid=,lstart=,command='],signal);
   if(!/^\d+$/.test(startTicks??'')||!host)throw new Error('Selected process identity unavailable');
   return {pid:pids[0],startTicks,host};
  },
  async bracket(signal){
   const before=Date.now(),raw=await adb(['shell','date','+%s.%N'],signal),after=Date.now(),deviceMs=Number(raw)*1000;
   if(!Number.isFinite(deviceMs))throw new Error('Device epoch clock unavailable');
   return {hostBeforeMs:before,hostAfterMs:after,deviceMs,raw,basis:'device Unix wall; Host acquisition bracket, not synchronization proof'};
  },
  async startLogs(fail,signal,initial){
   viewerPid=initial.pid;
   for(const [index,[file,args]] of collectionCommands(context,viewerPid).entries()){
    signal.throwIfAborted();const child=spawn(file,args),entry={child,outcome:null,closed:null};children.push(entry);
    entry.closed=new Promise(resolve=>{
     child.stdout.on('data',data=>{try{appendFileSync(prefix+(index?'.android.log':'.host.ndjson'),data);}catch(error){fail(`Raw log write failed: ${error}`);}});
     child.stderr.on('data',data=>fail(`Collector ${index} stderr: ${String(data)}`));
     child.on('error',error=>fail(`Collector ${index}: ${error}`));
     child.on('exit',(code,signal)=>{entry.outcome={index,code,signal,expected:stopping};if(!stopping)fail(`Collector ${index} exited early: ${code}/${signal}`);});
     child.on('close',()=>resolve());
    });
   }
  },
  async sample(signal){
   const acquired={startMs:Date.now(),endMs:0};
   const read=async run=>{try{return {status:'observed',raw:await run()};}catch(error){signal.throwIfAborted();throw error;}};
   const [host,viewer,thermal,battery]=await Promise.all([
    read(()=>command('/bin/ps',['-p',String(context.hostPid),'-o','pid=,%cpu=,rss='],signal)),
    read(()=>adb(['shell','cat',`/proc/${viewerPid}/stat`,`/proc/${viewerPid}/status`],signal)),
    read(()=>adb(['shell','dumpsys','thermalservice'],signal)),read(()=>adb(['shell','dumpsys','battery'],signal)),
   ]);acquired.endMs=Date.now();
   return {acquired,host,viewer,parsed:processResources(host.raw,viewer.raw),thermal:{...thermal,parsed:parseThermal(thermal.raw)},battery,power:{status:'unmeasured',reason:'No calibrated energy source; charging invalidates battery-drain estimate'},audio:{status:'unknown',reason:'Requires exact active owner/codec epoch runtime observation'},buffers:{status:'unknown',reason:'No per-owner buffer observation; raw stream snapshots retained'}};
  },
  async stopLogs(){
   stopping=true;
   await Promise.all(children.map(async entry=>{
    const {child}=entry;if(child.exitCode===null&&child.signalCode===null)child.kill('SIGTERM');
    const timer=setTimeout(()=>{if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL');},1000);
    try{await entry.closed;}finally{clearTimeout(timer);}
   }));
   return children.map((entry,index)=>entry.outcome??{index,code:null,signal:null,expected:false});
  },
 };
}
