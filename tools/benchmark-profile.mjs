import { isAbsolute } from 'node:path';
export function benchmarkProfile(env=process.env, {requireRoot=false}={}) {
  const slug=env.LEFTCAR_BENCHMARK_PROFILE;
  const root=env.LEFTCAR_BENCHMARK_ROOT;
  const audio=env.LEFTCAR_BENCHMARK_SYSTEM_AUDIO;
  const sourceId=env.LEFTCAR_BENCHMARK_SOURCE_ID;
  if(sourceId!==undefined && (slug===undefined || !sourceId.startsWith("macos:display:") || sourceId.length<=14))throw new Error("Exact native display restriction requires a benchmark profile and stable display sourceId");
  if(audio!==undefined && (audio!=="off" || slug===undefined))throw new Error("System audio restriction requires an explicit benchmark profile and value off");
  if(slug===undefined){if(root!==undefined)throw new Error('Benchmark root without profile');return null;}
  if(!/^[a-z][a-z0-9-]{0,31}$/.test(slug))throw new Error('Invalid LEFTCAR_BENCHMARK_PROFILE');
  if(requireRoot && (!root || !isAbsolute(root)))throw new Error('Host benchmark build requires absolute LEFTCAR_BENCHMARK_ROOT (runtime also validates dedicated state paths)');
  return {slug, sourceId:sourceId??null, systemAudioCapture: audio!=="off", root:root??null, viewerPackage:`leftcar.ll3.kr.benchmark.${slug.replaceAll('-','_')}`,
    hostIdentifier:`leftcar.ll3.kr.benchmark.${slug}`, hostProductName:`Leftcar Benchmark ${slug}`,
    credentialService:`leftcar-host.benchmark.${slug}`, stateChild:`host-${slug}`};
}
if(import.meta.main)console.log(JSON.stringify(benchmarkProfile(process.env,{requireRoot:true}),null,2));
