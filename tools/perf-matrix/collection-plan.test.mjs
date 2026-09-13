import {expect,test} from 'vitest';import {collectionCommands,processResources} from './collection-plan.mjs';
test('collection command boundary selects device/process and tails without clearing shared logs',()=>{
 expect(()=>collectionCommands({hostPid:123},'456')).toThrow(/Explicit/);
 expect(collectionCommands({serial:'tablet:5555',hostPid:123},'456')).toEqual([
  ['/usr/bin/log',['stream','--style','ndjson','--level','info','--predicate','processIdentifier == 123 AND eventMessage CONTAINS "LeftcarPerf"']],
  ['adb',['-s','tablet:5555','logcat','-v','epoch','--pid','456','-T','1','-s','LeftcarNative']],
 ]);
});
test('resource observations preserve CPU basis RSS units and missing fields',()=>{
 expect(processResources(' 123 24.5 4096','VmRSS:   8192 kB')).toMatchObject({host:{status:'observed',cpuPercent:24.5,rssKiB:4096},viewer:{status:'observed',rssKiB:8192}});
 expect(processResources('','denied')).toMatchObject({host:{status:'missing'},viewer:{status:'missing',rssKiB:null}});
});
