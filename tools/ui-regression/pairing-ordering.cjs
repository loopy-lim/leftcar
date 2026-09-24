const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
// 신 UI 계약 재사상: 구 UI의 "화면 접근 모두 제거"(set_source_grants로 화면
// 권한 제거)는 사라지고, reviewRequired 행의 "화면 허용" 승인 저장이 같은
// 명령·저장 경로를 쓴다. 요약 텍스트 대응: 화면 접근: 1 → 승인 대기,
// 화면 접근: 0 → 화면 권한 없음. 저장 지연·실패 재시도·stale 스냅샷 펜스
// 시맨틱은 paired-device-state의 revision 규칙으로 그대로 검증한다.
const root=`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html`;
const row=(page,name='Fixture device')=>page.locator('.device-row-item').filter({has:page.getByText(name,{exact:true})});
const approve=(page,name)=>row(page,name).getByRole('button',{name:'화면 허용',exact:true}).click();
const revokeViaDialog=async(page,name)=>{await row(page,name).getByRole('button',{name:'삭제',exact:true}).click();await page.getByRole('dialog').getByRole('button',{name:'삭제',exact:true}).click();};
async function remount(page){await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));}
async function settle(page,deviceId,stateRevision,revision=2,sourceIds=[]){await page.evaluate(({deviceId,stateRevision,revision,sourceIds})=>{const io=window.pairingIo;const index=io.pending.findIndex(p=>p.args.deviceId===deviceId);io.pending.splice(index,1)[0].resolve({credentialId:deviceId==='fixture-b'?'credential-B':'credential-A',stateRevision,revision,sourceIds,reviewRequired:false,persistenceError:null});},{deviceId,stateRevision,revision,sourceIds});}
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;async function test(name,body){const page=await browser.newPage();try{await body(page);console.log('PASS '+name);}catch(error){failures++;console.error('FAIL '+name+': '+error.stack);}finally{await page.close();}}
try{
for(const revoke of [false,true])await test(`actual parent reversed two-device ${revoke?'revoke':'approval'} results, failed refresh/remount/stale snapshot`,async page=>{
 await page.goto(root+'?two=1&review=1');await row(page).getByText('승인 대기',{exact:true}).waitFor();
 await page.evaluate(()=>{window.pairingIo.refreshFails=true;window.pairingIo.deferRevoke=true;});
 if(revoke)await revokeViaDialog(page,'Fixture device');else await approve(page);
 await approve(page,'Fixture B');await settle(page,'fixture-b',3);
 await row(page,'Fixture B').getByText('화면 권한 없음',{exact:true}).waitFor();
 if(revoke)await page.evaluate(()=>window.pairingIo.pendingRevokes.shift().resolve({removedDevices:[{deviceId:'fixture-device',credentialId:'credential-A'}],stateRevision:2,persistenceErrors:[]}));else await settle(page,'fixture-device',2);
 if(revoke)await row(page).waitFor({state:'detached',timeout:2000});else await row(page).getByText('화면 권한 없음',{exact:true}).waitFor({timeout:2000});
 await remount(page);await row(page,'Fixture B').getByText('화면 권한 없음',{exact:true}).waitFor();
 if(revoke)assert.equal(await row(page).count(),0);else assert.equal(await row(page).getByText('화면 권한 없음',{exact:true}).count(),1);
 // 마지막 pre-mutation 완전 스냅샷은 승인을 되돌리거나 멤버십을 되살릴 수 없다.
 const reads=await page.evaluate(()=>{window.pairingIo.refreshFails=false;return window.pairingIo.reads;});await page.waitForFunction(before=>window.pairingIo.reads>before,reads);await page.waitForTimeout(30);
 if(revoke)assert.equal(await row(page).count(),0);else assert.equal(await row(page).getByText('화면 권한 없음',{exact:true}).count(),1);
 assert.equal(await row(page,'Fixture B').getByText('화면 권한 없음',{exact:true}).count(),1);
 // 완전 스냅샷이 A의 제거/변경을 ack해도 B의 더 새 partial 상태가 이긴다.
 const acknowledged=await page.evaluate(revoke=>{const io=window.pairingIo;io.revision=2;if(revoke)io.devices=io.devices.filter(d=>d.device_id!=='fixture-device');else io.devices[0].source_grants={...io.devices[0].source_grants,stateRevision:2,revision:2,sourceIds:[]};return io.reads;},revoke);
 await page.waitForFunction(before=>window.pairingIo.reads>before,acknowledged);await page.waitForTimeout(20);assert.equal(await row(page,'Fixture B').getByText('화면 권한 없음',{exact:true}).count(),1);
 if(revoke){const reads=await page.evaluate(()=>{const io=window.pairingIo;io.revision=1;io.devices.unshift({device_id:'fixture-device',name:'Fixture device',paired_at:'2026-09-13',source_grants:{credentialId:'credential-A',stateRevision:1,revision:1,sourceIds:['display:A'],reviewRequired:false}});return io.reads;});await page.waitForFunction(before=>window.pairingIo.reads>before,reads);await remount(page);assert.equal(await row(page).count(),0,'acknowledged tombstone pruning must retain complete-snapshot fence');}
});
await test('actual parent unrelated delayed snapshot cannot clear failed approval across remount; later retry confirms',async page=>{
 await page.goto(root+'?review=1');await row(page).getByText('승인 대기',{exact:true}).waitFor();
 await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=true;io.revision=2;io.devices.push({...io.devices[0],device_id:'fixture-b',name:'Fixture B',source_grants:{...io.devices[0].source_grants,credentialId:'credential-B'}});io.deferReads=true;});
 await remount(page);
 await page.waitForFunction(()=>window.pairingIo.pendingReads.length===1);
 await page.evaluate(()=>{window.pairingIo.deferReads=false;});
 await approve(page);await page.evaluate(()=>window.pairingIo.pending.shift().reject('failed approval unknown'));await page.getByText(/연결 상태를 확인하지 못했습니다/).waitFor();
 await page.evaluate(()=>window.pairingIo.pendingReads.shift()());await row(page).getByText('승인 대기',{exact:true}).waitFor();await page.waitForTimeout(30);await remount(page);
 await row(page).getByText('승인 대기',{exact:true}).waitFor({timeout:2000});
 await approve(page);await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=false;io.revision=4;io.devices[0].source_grants={...io.devices[0].source_grants,reviewRequired:false};const index=io.pending.findIndex(p=>p.args.deviceId==='fixture-device');io.pending.splice(index,1)[0].resolve({credentialId:'credential-A',stateRevision:4,revision:3,sourceIds:['display:A'],reviewRequired:false,persistenceError:null});});await row(page).getByText('화면 1개',{exact:true}).waitFor();await remount(page);await row(page).getByText('화면 1개',{exact:true}).waitFor();
});
await test('actual parent success before a later failure keeps acknowledged state until a newer retry confirms',async page=>{
 await page.goto(root+'?review=1');await row(page).getByText('승인 대기',{exact:true}).waitFor();await page.evaluate(()=>{window.pairingIo.refreshFails=true;});
 await approve(page);await page.evaluate(()=>{const io=window.pairingIo;io.revision=2;io.devices[0].source_grants={...io.devices[0].source_grants,reviewRequired:false};const index=io.pending.findIndex(p=>p.args.deviceId==='fixture-device');io.pending.splice(index,1)[0].resolve({credentialId:'credential-A',stateRevision:2,revision:2,sourceIds:['display:A'],reviewRequired:false,persistenceError:null});});await row(page).getByText('화면 1개',{exact:true}).waitFor();
 await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=false;io.revision=3;io.devices[0].source_grants={...io.devices[0].source_grants,revision:3,stateRevision:3,reviewRequired:true};});await row(page).getByText('승인 대기',{exact:true}).waitFor();
 await page.evaluate(()=>{window.pairingIo.refreshFails=true;});
 await approve(page);await page.evaluate(()=>window.pairingIo.pending.pop().reject('newer approval failed'));await page.getByText(/연결 상태를 확인하지 못했습니다/).waitFor();
 await remount(page);await row(page).getByText('승인 대기',{exact:true}).waitFor({timeout:2000});
 await approve(page);await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=false;io.revision=4;io.devices[0].source_grants={...io.devices[0].source_grants,revision:4,stateRevision:4,reviewRequired:false};const index=io.pending.findIndex(p=>p.args.deviceId==='fixture-device');io.pending.splice(index,1)[0].resolve({credentialId:'credential-A',stateRevision:4,revision:4,sourceIds:['display:A'],reviewRequired:false,persistenceError:null});});await row(page).getByText('화면 1개',{exact:true}).waitFor();await remount(page);await row(page).getByText('화면 1개',{exact:true}).waitFor();
});
await test('actual parent same-credential older grant revision loses to newer approved revision',async page=>{
 await page.goto(root+'?review=1');await row(page).getByText('승인 대기',{exact:true}).waitFor();await page.evaluate(()=>{window.pairingIo.refreshFails=true;});
 await approve(page);await page.evaluate(()=>{const io=window.pairingIo;io.revision=3;io.devices[0].source_grants={...io.devices[0].source_grants,revision:3,stateRevision:3,reviewRequired:false};const index=io.pending.findIndex(p=>p.args.deviceId==='fixture-device');io.pending.splice(index,1)[0].resolve({credentialId:'credential-A',stateRevision:3,revision:3,sourceIds:['display:A'],reviewRequired:false,persistenceError:null});});await row(page).getByText('화면 1개',{exact:true}).waitFor();
 await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=false;io.revision=4;io.devices[0].source_grants={...io.devices[0].source_grants,revision:3,stateRevision:4,reviewRequired:true};});await row(page).getByText('승인 대기',{exact:true}).waitFor();
 await page.evaluate(()=>{window.pairingIo.refreshFails=true;});
 await approve(page);await page.evaluate(()=>window.pairingIo.pending.pop().resolve({credentialId:'credential-A',stateRevision:2,revision:2,sourceIds:['display:B'],reviewRequired:false,persistenceError:null}));await page.waitForTimeout(30);
 assert.equal(await row(page).getByText('화면 1개',{exact:true}).count(),0,'older revision must not overwrite newer approved revision');
 await remount(page);await row(page).getByText('승인 대기',{exact:true}).waitFor({timeout:2000});
});
}finally{await browser.close();}if(failures)process.exitCode=1;})().catch(error=>{console.error(error);process.exitCode=1;});
