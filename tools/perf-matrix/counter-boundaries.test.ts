import {test,expect} from 'vitest';
import {summarizeCounterSeries} from './analyze-performance';
test('counter reset never becomes a rate and true collection start exposes missing head',()=>{
 const reset=summarizeCounterSeries([{timestampMs:1000,frames:200},{timestampMs:2000,frames:0},{timestampMs:3000,frames:100}],2000);
 expect(reset.averageFps).toBeNull();expect(reset.errors).toContain('counter reset requires a new segment');
 const late=summarizeCounterSeries([{timestampMs:3000,frames:0},{timestampMs:4000,frames:60}],4000,{startMs:0,endMs:4000});
 expect(late.zeroFpsStallDetected).toBe(false);
 expect(late.observationComplete).toBe(false);
 expect(late.observationGaps).toContainEqual({kind:'head',startMs:0,endMs:3000});
});
