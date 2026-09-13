import {readFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {join,dirname} from 'node:path';
import ts from 'typescript';
import {sha256} from './release-source.mjs';
import {collectGradleInventory} from './release-gradle.mjs';
export const lockfiles=['bun.lock','Cargo.lock','apps/host-desktop/src-tauri/Cargo.lock'];
export const runReadOnly=(command,args,cwd)=>spawnSync(command,args,{cwd,encoding:'utf8',timeout:30000,maxBuffer:32*1024*1024});
const sorted=records=>records.sort((a,b)=>JSON.stringify([a.name,a.version,a.source??'',a.scope]).localeCompare(JSON.stringify([b.name,b.version,b.source??'',b.scope]),'en'));
const outcome=result=>result?.error?.code==='ENOENT'?'unavailable':'error';
const json=result=>{try{return JSON.parse(result.stdout);}catch{return null;}};
function bunPackages(text) {
 const parsed=ts.parseConfigFileTextToJson('bun.lock',text);
 if(parsed.error||!parsed.config?.packages||parsed.config.lockfileVersion!==1)throw new Error('Invalid bun.lock');
 const records=[];
 for(const [key,value] of Object.entries(parsed.config.packages)) {
  if(!Array.isArray(value)||typeof value[0]!=='string')throw new Error('Invalid Bun package record');
  const split=value[0].lastIndexOf('@'),name=value[0].slice(0,split),version=value[0].slice(split+1);
  if(!name||!version)throw new Error('Invalid Bun package identity');
  records.push({name,version,source:key,scope:'lockfile-all',license:null,licenseStatus:'unknown'});
 }
 if(!records.length)throw new Error('Empty Bun dependency inventory');
 return sorted(records);
}
function cargoPackages(text) {
 const sections=text.split(/^\[\[package\]\]\s*$/m).slice(1);
 if(!sections.length)throw new Error('Invalid Cargo.lock');
 return sorted(sections.map(section=>{
  const get=key=>section.match(new RegExp(`^${key} = "([^"\\n]+)"$`,'m'))?.[1]??null;
  const name=get('name'),version=get('version');if(!name||!version)throw new Error('Invalid Cargo lock package');
  return {name,version,source:get('source')??'workspace',scope:'lockfile-all-targets',license:null,licenseStatus:'unknown'};
 }));
}
export function parseVulnerabilityResult(tool,result) {
 const data=json(result),base={status:outcome(result),findings:[],warnings:[],database:null};
 if(result?.error||![0,1].includes(result.status)||!data)return base;
 if(tool==='bun') {
  if(typeof data!=='object'||Array.isArray(data)||Object.values(data).some(v=>!Array.isArray(v)))return base;
  for(const [name,advisories] of Object.entries(data))for(const a of advisories) {
   if(typeof a.url!=='string'||typeof a.title!=='string'||!['low','moderate','high','critical'].includes(a.severity))return {...base,status:'error'};
   base.findings.push({package:name,id:String(a.id??a.url),url:a.url,title:a.title,severity:a.severity,range:a.vulnerable_versions??null});
  }
 } else {
  if(!Array.isArray(data.vulnerabilities?.list)||!Number.isInteger(data.vulnerabilities.count)||data.vulnerabilities.count!==data.vulnerabilities.list.length||data.vulnerabilities.found!==(data.vulnerabilities.count>0)||!data.warnings||!data.database)return base;
  const record=x=>({package:x.package.name,version:x.package.version,id:x.advisory.id,title:x.advisory.title,url:x.advisory.url??null,kind:x.kind??'vulnerability',targetReachability:'not-evaluated; complete lockfile scan'});
  try {base.findings=data.vulnerabilities.list.map(record);base.warnings=Object.values(data.warnings).flat().map(record);}catch{return {...base,status:'error'};}
  base.database={advisoryCount:data.database['advisory-count']??null,lastCommit:data.database['last-commit']??null,lastUpdated:data.database['last-updated']??null};
 }
 if(result.status===1 && !base.findings.length && !base.warnings.length)return {...base,status:'error'};
 return {...base,status:base.findings.length||base.warnings.length?'findings':'pass'};
}
export async function collectDependencyInventory(root,scope,{run=runReadOnly,cargoAuditDb=null,gradleCache}={}) {
 const dependencies=[],vulnerabilityChecks=[];
 const invoke=(cmd,args,cwd=root)=>{try{return run(cmd,args,cwd);}catch{return {status:null,error:{code:'TOOL_ERROR'}};}};
 const version=(cmd,args)=>{const r=invoke(cmd,args);return r.status===0&&!r.error ? (r.stdout??'').trim().split('\n')[0]||null:null;};
 const bunVersion=version('bun',['--version']),cargoVersion=version('cargo',['audit','--version']);
 for(const lockfile of lockfiles) {
  const text=await readFile(join(root,lockfile),'utf8'),lockSha256=sha256(text),ecosystem=lockfile==='bun.lock'?'bun':'cargo';
  const records=ecosystem==='bun'?bunPackages(text):cargoPackages(text);
  const result=ecosystem==='bun'?invoke('bun',['pm','licenses','--json']):invoke('cargo',['metadata','--locked','--offline','--format-version','1'],join(root,dirname(lockfile)));
  const metadata=json(result),licenses=new Map();
  if(result.status===0&&!result.error && metadata) {
   if(ecosystem==='bun')for(const group of Object.values(metadata))if(Array.isArray(group))for(const pkg of group)for(const v of pkg.versions??[])if(typeof pkg.license==='string')licenses.set(`${pkg.name}@${v}`,pkg.license);
   if(ecosystem==='cargo')for(const pkg of metadata.packages??[])if(typeof pkg.license==='string')licenses.set(`${pkg.name}@${pkg.version}`,pkg.license);
  }
  for(const record of records)if(licenses.has(`${record.name}@${record.version}`)) {record.license=licenses.get(`${record.name}@${record.version}`);record.licenseStatus='declared';}
  dependencies.push({ecosystem,lockfile,lockSha256,status:'locked',licenseMetadataStatus:licenses.size?'observed':outcome(result),records});
  const scanArgs=ecosystem==='bun'?['audit','--json']:['audit','--no-fetch',...(cargoAuditDb?['--db',cargoAuditDb]:[]),'--file',lockfile,'--json'];
  const scan=invoke(ecosystem==='bun'?'bun':'cargo',scanArgs);
  vulnerabilityChecks.push({tool:ecosystem==='bun'?'bun':'cargo-audit',toolVersion:ecosystem==='bun'?bunVersion:cargoVersion,input:lockfile,inputSha256:lockSha256,scope:'all-lockfile-dependencies',...parseVulnerabilityResult(ecosystem,scan)});
 }
 dependencies.push(await collectGradleInventory(root,{run:invoke,gradleCache}));
 vulnerabilityChecks.push({tool:'gradle',toolVersion:null,input:null,inputSha256:null,scope:'android-runtime',status:'unavailable',findings:[],warnings:[],database:null,reason:'No configured Gradle vulnerability scanner; dependency resolution is not a vulnerability scan'});
 return {dependencies,vulnerabilityChecks};
}
