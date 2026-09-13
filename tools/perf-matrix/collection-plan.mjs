export function collectionCommands(context, viewerPid) {
  if(!context.serial || !Number.isSafeInteger(context.hostPid) || context.hostPid<1 || !/^\d+$/.test(viewerPid))throw new Error('Explicit selected device and process identities required');
  return [
    ['/usr/bin/log',['stream','--style','ndjson','--level','info','--predicate',`processIdentifier == ${context.hostPid} AND eventMessage CONTAINS "LeftcarPerf"`]],
    ['adb',['-s',context.serial,'logcat','-v','epoch','--pid',viewerPid,'-T','1','-s','LeftcarNative']],
  ];
}
export function processResources(hostRaw,viewerRaw) {
 const host=/^\s*(\d+)\s+([\d.]+)\s+(\d+)/.exec(hostRaw??'');
 const rss=/^VmRSS:\s+(\d+)\s+kB/m.exec(viewerRaw??'');
 return {
  host:host?{status:'observed',pid:Number(host[1]),cpuPercent:Number(host[2]),cpuBasis:'ps reported CPU percentage; not a per-sample CPU-time delta',rssKiB:Number(host[3])}:{status:'missing'},
  viewer:{status:rss?'observed':'missing',rssKiB:rss?Number(rss[1]):null,cpu:{status:'raw-only',reason:'proc stat ticks retained; no assumed clock-tick frequency or interval CPU percentage'}},
 };
}
