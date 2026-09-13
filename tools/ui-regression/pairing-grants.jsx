import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import PairingPanel from "../../apps/host-desktop/src/PairingPanel";
const original = { device_id:"fixture-device",name:"Fixture device",paired_at:"2026-09-13",credential_generation:1,source_grants:{credentialId:"credential-A",stateRevision:1,sourceIds:["display:A"],revision:1,reviewRequired:false,persistenceError:null} };
const second = {...original, device_id:"fixture-b",name:"Fixture B",source_grants:{...original.source_grants,credentialId:"credential-B",sourceIds:["display:B"]}};
const io={revision:1,devices:new URLSearchParams(location.search).has("two")?[original,second]:[original],deferReads:false,pendingReads:[],deferRevoke:false,pendingRevokes:[],refreshFails:false,reads:0,pending:[],calls:[],removed:[],revokeError:null};
window.pairingIo=io;
io.pendingOffers=new URLSearchParams(location.search).has("offer")?[{offer_id:"offer-1",device_name:"Synthetic pending viewer"}]:[];io.decisions=[];
window.__TAURI_INTERNALS__={invoke(command,args){
 io.calls.push({command,args});
 if(command==="list_paired_devices"||command==="list_paired_device_state") {io.reads++;const devices=io.devices.map(device=>({...device,source_grants:{...device.source_grants,stateRevision:io.revision}}));if(io.deferReads){const snapshot=structuredClone(command==="list_paired_devices"?devices:{revision:io.revision,devices});return new Promise(resolve=>io.pendingReads.push(()=>resolve(snapshot)));}return io.refreshFails?Promise.reject("refresh failed"):Promise.resolve(structuredClone(command==="list_paired_devices"?devices:{revision:io.revision,devices}));}
 if(command==="set_source_grants")return new Promise((resolve,reject)=>io.pending.push({args,resolve,reject}));
 if(command==="get_lan_ip")return Promise.resolve("127.0.0.1");
 if(command==="get_control_port")return Promise.resolve(7777);
 if(command==="list_pending_pairings")return Promise.resolve(io.pendingOffers);
 if(command==="approve_pending_pairing"||command==="reject_pending_pairing")return new Promise((resolve,reject)=>io.decisions.push({resolve,reject}));
 if(command==="list_host_sources")return Promise.resolve([{sourceId:"display:A",index:0,name:"Synthetic A",width:1920,height:1080}]);
 if(command==="revoke_paired_device"||command==="revoke_all_devices") {if(io.deferRevoke)return new Promise(resolve=>io.pendingRevokes.push({args,resolve}));const removed=io.devices.filter(d=>command==="revoke_all_devices"||d.device_id===args.deviceId).map(d=>({deviceId:d.device_id,credentialId:d.source_grants.credentialId}));io.devices=io.devices.filter(d=>!removed.some(r=>r.deviceId===d.device_id));io.revision++;return Promise.resolve({removedDevices:removed,stateRevision:io.revision,persistenceErrors:io.revokeError?[io.revokeError]:[]});}
 return Promise.resolve(null);
}};
function Fixture(){const[mounted,setMounted]=useState(true);window.mountPairing=setMounted;return mounted?<PairingPanel language="ko"/>:null;}
createRoot(document.getElementById("root")).render(<Fixture/>);
