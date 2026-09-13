import {readFileSync,writeFileSync} from 'node:fs';
import {sha256} from '../release-manifest.mjs';
import {finalizeMeasurement,compareMeasurements} from './measurement.ts';
export function saveCollection(prefix,receipt,baselinePath=null) {
 const raw={},rawHashes={},errors=[];
 for(const [name,suffix] of [['host','.host.ndjson'],['android','.android.log']]){
  try{const bytes=readFileSync(prefix+suffix);raw[name]=bytes.toString();rawHashes[name]=sha256(bytes);}
  catch(error){raw[name]='';rawHashes[name]=null;errors.push(`Raw ${name} unavailable: ${error}`);}
 }
 const result=finalizeMeasurement({...receipt,childErrors:[...(receipt.childErrors??[]),...errors],rawHashes},raw.host,raw.android);
 let comparison;
 if(baselinePath){
  try{comparison=compareMeasurements(JSON.parse(readFileSync(baselinePath,'utf8')),result);}
  catch(error){result.collection.status='invalid';result.collection.reasons.push(`Baseline comparison unavailable: ${error}`);result.exitCode=1;comparison={comparable:false,reason:String(error)};}
 }
 // Always preserve the acquisition receipt before attempting the secondary output.
 writeFileSync(prefix+'.collection.json',JSON.stringify(result,null,2)+'\n');
 if(comparison)writeFileSync(prefix+'.comparison.json',JSON.stringify(comparison,null,2)+'\n');
 return result;
}
