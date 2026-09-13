import {expect,test} from 'vitest';
import {command} from './collector-io.mjs';
test('production async command cancellation terminates an owned subprocess promptly',async()=>{
 const controller=new AbortController();const start=performance.now();
 const result=command(process.execPath,['-e','setInterval(()=>{},1000)'],controller.signal);
 setTimeout(()=>controller.abort(new Error('test cancellation')),25);
 await expect(result).rejects.toThrow();expect(performance.now()-start).toBeLessThan(1000);
});
test('production async command keeps argument text out of a shell',async()=>{
 const value='$(not-a-command); literal';
 expect(await command(process.execPath,['-e','process.stdout.write(process.argv[1])',value],new AbortController().signal)).toBe(value);
});
