const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
const root=`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html`;
const row=(page,name='Fixture device')=>page.locator('.device-row-item').filter({has:page.getByText(name,{exact:true})});
const remove=(page,name)=>row(page,name).getByRole('button',{name:'화면 접근 모두 제거',exact:true}).click();
async function remount(page){await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));}
async function settle(page,deviceId,stateRevision,revision=2,sourceIds=[]){await page.evaluate(({deviceId,stateRevision,revision,sourceIds})=>{const io=window.pairingIo;const index=io.pending.findIndex(p=>p.args.deviceId===deviceId);io.pending.splice(index,1)[0].resolve({credentialId:deviceId==='fixture-b'?'credential-B':'credential-A',stateRevision,revision,sourceIds,reviewRequired:false,persistenceError:null});},{deviceId,stateRevision,revision,sourceIds});}
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;async function test(name,body){const page=await browser.newPage();try{await body(page);console.log('PASS '+name);}catch(error){failures++;console.error('FAIL '+name+': '+error.stack);}finally{await page.close();}}
try{
for(const revoke of [false,true])await test(`actual parent reversed two-device ${revoke?'revoke':'grant'} results, failed refresh/remount/stale snapshot`,async page=>{
 await page.goto(root+'?two=1');await row(page).getByText('화면 접근: 1',{exact:true}).waitFor();
 await page.evaluate(()=>{window.pairingIo.refreshFails=true;window.pairingIo.deferRevoke=true;});
 if(revoke){await row(page).getByRole('button',{name:'삭제',exact:true}).click();await page.getByRole('dialog').getByRole('button',{name:'삭제',exact:true}).click();}else await remove(page);
 await remove(page,'Fixture B');await settle(page,'fixture-b',3);
 await row(page,'Fixture B').getByText('화면 접근: 0',{exact:true}).waitFor();
 if(revoke)await page.evaluate(()=>window.pairingIo.pendingRevokes.shift().resolve({removedDevices:[{deviceId:'fixture-device',credentialId:'credential-A'}],stateRevision:2,persistenceErrors:[]}));else await settle(page,'fixture-device',2);
 if(revoke)await row(page).waitFor({state:'detached',timeout:2000});else await row(page).getByText('화면 접근: 0',{exact:true}).waitFor({timeout:2000});
 await remount(page);await row(page,'Fixture B').getByText('화면 접근: 0',{exact:true}).waitFor();
 if(revoke)assert.equal(await row(page).count(),0);else assert.equal(await row(page).getByText('화면 접근: 0',{exact:true}).count(),1);
 // Last pre-mutation complete snapshot must neither restore a grant nor resurrect membership.
 const reads=await page.evaluate(()=>{window.pairingIo.refreshFails=false;return window.pairingIo.reads;});await page.waitForFunction(before=>window.pairingIo.reads>before,reads);await page.waitForTimeout(30);
 if(revoke)assert.equal(await row(page).count(),0);else assert.equal(await row(page).getByText('화면 접근: 0',{exact:true}).count(),1);
 assert.equal(await row(page,'Fixture B').getByText('화면 접근: 0',{exact:true}).count(),1);
 // A complete snapshot acknowledges A's removal/change, but B's newer partial state still wins.
 const acknowledged=await page.evaluate(revoke=>{const io=window.pairingIo;io.revision=2;if(revoke)io.devices=io.devices.filter(d=>d.device_id!=='fixture-device');else io.devices[0].source_grants={...io.devices[0].source_grants,stateRevision:2,revision:2,sourceIds:[]};return io.reads;},revoke);
 await page.waitForFunction(before=>window.pairingIo.reads>before,acknowledged);await page.waitForTimeout(20);assert.equal(await row(page,'Fixture B').getByText('화면 접근: 0',{exact:true}).count(),1);
 if(revoke){const reads=await page.evaluate(()=>{const io=window.pairingIo;io.revision=1;io.devices.unshift({device_id:'fixture-device',name:'Fixture device',paired_at:'2026-09-13',source_grants:{credentialId:'credential-A',stateRevision:1,revision:1,sourceIds:['display:A'],reviewRequired:false}});return io.reads;});await page.waitForFunction(before=>window.pairingIo.reads>before,reads);await remount(page);assert.equal(await row(page).count(),0,'acknowledged tombstone pruning must retain complete-snapshot fence');}
});
await test('actual parent unrelated delayed snapshot cannot clear failed removal across remount; later retry confirms',async page=>{
 await page.goto(root);await row(page).getByText('화면 접근: 1',{exact:true}).waitFor();
 await page.evaluate(()=>{const io=window.pairingIo;io.revision=2;io.devices.push({...io.devices[0],device_id:'fixture-b',name:'Fixture B',source_grants:{...io.devices[0].source_grants,credentialId:'credential-B'}});io.deferReads=true;});
 await page.waitForFunction(()=>window.pairingIo.pendingReads.length===1);
 await page.evaluate(()=>{window.pairingIo.deferReads=false;window.pairingIo.refreshFails=true;});
 await remove(page);await page.evaluate(()=>window.pairingIo.pending.shift().reject('failed removal unknown'));await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();
 await page.evaluate(()=>window.pairingIo.pendingReads.shift()());await row(page,'Fixture B').waitFor();await remount(page);
 await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor({timeout:2000});assert.equal(await row(page).getByText('화면 접근: 1',{exact:true}).count(),0);
 await remove(page);await settle(page,'fixture-device',4,3);await row(page).getByText('화면 접근: 0',{exact:true}).waitFor();await remount(page);await row(page).getByText('화면 접근: 0',{exact:true}).waitFor();
});
await test('actual parent success started before a later failure cannot clear that uncertainty',async page=>{
 await page.goto(root);await row(page).getByText('화면 접근: 1',{exact:true}).waitFor();await page.evaluate(()=>window.pairingIo.refreshFails=true);
 await remove(page);await remount(page);await remove(page);
 await page.evaluate(()=>window.pairingIo.pending.pop().reject('newer removal failed'));await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();
 await settle(page,'fixture-device',2,2);await remount(page);await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor({timeout:2000});
 await remove(page);await remount(page);await remove(page);await page.evaluate(()=>window.pairingIo.pending.pop().reject('second later failure'));await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();
 await settle(page,'fixture-device',4,4);await remount(page);await row(page).getByText('Host에서 화면 접근 검토가 필요합니다').waitFor({timeout:2000});
 await remove(page);await settle(page,'fixture-device',6,6);await row(page).getByText('화면 접근: 0',{exact:true}).waitFor();
});
await test('actual parent same-credential latest grant revision wins reversed successful completions',async page=>{
 await page.goto(root);await row(page).getByText('화면 접근: 1',{exact:true}).waitFor();await page.evaluate(()=>window.pairingIo.refreshFails=true);
 await remove(page);await remount(page);await remove(page);
 await page.evaluate(()=>window.pairingIo.pending.pop().resolve({credentialId:'credential-A',stateRevision:3,revision:3,sourceIds:[],reviewRequired:false,persistenceError:null}));await row(page).getByText('화면 접근: 0',{exact:true}).waitFor();
 await settle(page,'fixture-device',2,2,['display:A']);await remount(page);await row(page).getByText('화면 접근: 0',{exact:true}).waitFor({timeout:2000});
});
}finally{await browser.close();}if(failures)process.exitCode=1;})().catch(error=>{console.error(error);process.exitCode=1;});
