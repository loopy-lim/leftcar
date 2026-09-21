import {createHash} from 'node:crypto';
export const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
export const isSha256=value=>typeof value==='string' && /^[a-f0-9]{64}$/.test(value);
function validateEntries(files, nested=false) {
 if(!Array.isArray(files))throw new Error('Invalid source file inventory');
 let previous=null;
 for(const file of files) {
  if(!file || typeof file.path!=='string' || !file.path || file.path.startsWith('/') || file.path.includes('\\') || file.path.split('/').some(x=>!x || x==='.' || x==='..') || (nested && file.path.includes('/')) || (previous!==null && previous>=file.path))throw new Error('Invalid source file path/order');
  previous=file.path;
  if(file.deleted===true) {if(Object.keys(file).some(k=>!['path','deleted'].includes(k)))throw new Error('Invalid deleted source entry');continue;}
  if(!Number.isInteger(file.mode)||file.mode<0||file.mode>0o777||!isSha256(file.sha256))throw new Error('Invalid source file metadata');
  if(file.kind==='directory') {validateEntries(file.files,true);if(sha256(JSON.stringify(file.files))!==file.sha256)throw new Error('Invalid source directory hash');}
  else if(file.kind==='file') {if(!Number.isSafeInteger(file.bytes)||file.bytes<0)throw new Error('Invalid source file bytes');}
  else if(file.kind!=='symlink')throw new Error('Invalid source entry kind');
 }
}
export function validateSourceSnapshot(source) {
 if(source?.schema!==1 || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(source.commit??'') || typeof source.dirty!=='boolean' || typeof source.scope!=='string' || !source.scope || !Array.isArray(source.exclusions)||!source.exclusions.every(x=>typeof x==='string') || !isSha256(source.sha256))throw new Error('Invalid source snapshot');
 validateEntries(source.files);
 if(sha256(JSON.stringify(source.files))!==source.sha256)throw new Error('Invalid source manifest hash');
 return source;
}
export function validateSourceBinding(...records) {
 if(!records.length)throw new Error('Source evidence required');
 const sources=records.map(record=>validateSourceSnapshot(record?.source ?? record));
 const first=sources[0];
 if(sources.some(s=>s.commit!==first.commit||s.sha256!==first.sha256))throw new Error('Source binding mismatch: commit and build-input digest must match');
 return {commit:first.commit,sha256:first.sha256};
}
