import {readFile,readdir} from 'node:fs/promises';
import {join} from 'node:path';
import {homedir} from 'node:os';
const failure={ecosystem:'gradle',lockfile:null,lockSha256:null,licenseMetadataStatus:'unavailable',records:[]};
const decodeXml=text=>text.replaceAll('&amp;','&').replaceAll('&lt;','<').replaceAll('&gt;','>').replaceAll('&quot;','"').replaceAll('&apos;',"'");
export async function collectGradleInventory(root,{run,gradleCache=process.env.GRADLE_USER_HOME??join(homedir(),'.gradle')}={}) {
 let result;
 try {result=run(process.platform==='win32'?'gradlew.bat':'./gradlew',['--offline',':app:dependencies','--configuration','releaseRuntimeClasspath','--no-daemon'],join(root,'apps/viewer-expo/android'));}
 catch {result={status:null,error:{code:'TOOL_ERROR'}};}
 if(result.error||result.status!==0)return {...failure,status:result.error?.code==='ENOENT'?'unavailable':'error',reason:'Offline Gradle resolved inventory could not be read; no dependencies or verification trust were installed'};
 const text=result.stdout??'';
 if(/\bFAILED\b/.test(text))return {...failure,status:'error',reason:'Gradle reported unresolved dependencies'};
 const coordinates=new Map();
 for(const line of text.split('\n')) {
  const match=line.match(/(?:\+|\\)--- ([^ ]+)(?: -> ([^ ]+))?/);if(!match)continue;
  let parts=match[1].split(':');if(match[2])parts=match[2].includes(':')?match[2].split(':'):[...parts.slice(0,2),match[2]];
  if(parts.length!==3||parts.some(p=>!p||!/^[A-Za-z0-9_.+\-]+$/.test(p)))continue;
  coordinates.set(parts.join(':'),parts);
 }
 if(!coordinates.size)return {...failure,status:'error',reason:'Gradle did not report resolved external coordinates'};
 const records=[];
 for(const [group,name,version] of [...coordinates.values()].sort((a,b)=>a.join(':').localeCompare(b.join(':'),'en'))) {
  const licenseNames=new Set(),base=join(gradleCache,'caches/modules-2/files-2.1',group,name,version);
  for(const dir of await readdir(base).catch(()=>[]))for(const file of await readdir(join(base,dir)).catch(()=>[]))if(file.endsWith('.pom')) {
   const pom=await readFile(join(base,dir,file),'utf8').catch(()=>'');
   const licenses=pom.match(/<licenses>([\s\S]*?)<\/licenses>/)?.[1]??'';
   for(const match of licenses.matchAll(/<name>([^<]+)<\/name>/g))licenseNames.add(decodeXml(match[1].trim()));
  }
  records.push({name:`${group}:${name}`,version,scope:'releaseRuntimeClasspath',license:licenseNames.size?[...licenseNames].sort().join('; '):null,licenseStatus:licenseNames.size?'declared':'unknown'});
 }
 return {...failure,status:'observed-unlocked',licenseMetadataStatus:records.some(r=>r.license!==null)?'observed':'unavailable',records,reason:'Offline resolved runtime inventory only; no checked-in dependency lock or verification trust policy; not a vulnerability scan'};
}
