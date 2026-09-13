// Production orchestration, with injected OS reads for retained no-device execution.
// Every operation races a monotonic deadline and cancellation, even if an adapter stalls.
export async function bounded(run, timeoutMs, parentSignal) {
 const controller=new AbortController();
 const forward=()=>controller.abort(parentSignal.reason);
 if(parentSignal?.aborted)forward();else parentSignal?.addEventListener('abort',forward,{once:true});
 const timer=setTimeout(()=>controller.abort(Object.assign(new Error('Operation deadline reached'),{code:'COLLECTION_DEADLINE'})),Math.max(0,Math.ceil(timeoutMs)));
 let aborted;
 try {
  return await Promise.race([
   new Promise((_,reject)=>{aborted=()=>reject(controller.signal.reason??new Error('Cancelled'));if(controller.signal.aborted)aborted();else controller.signal.addEventListener('abort',aborted,{once:true});}),
   Promise.resolve().then(()=>{controller.signal.throwIfAborted();return run(controller.signal);}),
  ]);
 }finally{controller.abort(new Error('Operation completed'));clearTimeout(timer);controller.signal.removeEventListener('abort',aborted);parentSignal?.removeEventListener('abort',forward);}
}
export async function runCollection(io,{durationMs,signal,operationTimeoutMs=10000}) {
 const acquisition={startedAtMs:Date.now(),finishedAtMs:null};
 const childErrors=[],resources=[],controller=new AbortController();
 let interrupted=false,initial=null,final=null,start=null,end=null,childOutcomes=[],started=null,elapsedMs=null;
 const interrupt=()=>{interrupted=true;controller.abort(signal.reason??new Error('Interrupted'));};
 if(signal?.aborted)interrupt();else signal?.addEventListener('abort',interrupt,{once:true});
 const fail=reason=>{childErrors.push(String(reason));controller.abort(new Error(String(reason)));};
 const operation=run=>bounded(run,operationTimeoutMs,controller.signal);
 try {
  initial=await operation(io.processRead);
  start=await operation(io.bracket);started=performance.now();
  // Log launch shares the measurement deadline: launch overhead is not silently excluded.
  const deadline=started+durationMs;
  await bounded(s=>io.startLogs(fail,s,initial),Math.min(operationTimeoutMs,deadline-performance.now()),controller.signal);
  while(performance.now()<deadline){
   const sampleBudget=Math.min(operationTimeoutMs,deadline-performance.now());
   try {resources.push(await bounded(io.sample,sampleBudget,controller.signal));}
   catch(error){if(controller.signal.aborted||error.code!=='COLLECTION_DEADLINE'||sampleBudget===operationTimeoutMs)throw error;break;}
   const remaining=deadline-performance.now();
   if(remaining>0)await bounded(s=>new Promise(resolve=>{const timer=setTimeout(resolve,Math.min(1000,remaining));s.addEventListener('abort',()=>clearTimeout(timer),{once:true});}),remaining+10,controller.signal);
  }
  elapsedMs=performance.now()-started;
  end=await operation(io.bracket);
  final=await operation(io.processRead);
 }catch(error){childErrors.push(`Collection incomplete: ${String(error)}`);}
 finally {
  elapsedMs??=started==null?null:performance.now()-started;
  // Cleanup has its own finite budget; interruption cannot skip stopping owned children.
  try{childOutcomes=await bounded(io.stopLogs,operationTimeoutMs);}catch(error){childErrors.push(`Cleanup incomplete: ${String(error)}`);}
  signal?.removeEventListener('abort',interrupt);acquisition.finishedAtMs=Date.now();
 }
 return {acquisition,boundaries:{start,end},timing:{requestedMs:durationMs,elapsedMs},processes:{initial,final,unchanged:initial!=null&&final!=null&&JSON.stringify(initial)===JSON.stringify(final)},interrupted,childErrors,childOutcomes,resources};
}
