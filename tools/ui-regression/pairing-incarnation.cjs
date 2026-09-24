const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
// 신 UI 계약 재사상: 구 UI의 "화면 접근 모두 제거" 저장은 "화면 허용" 승인
// 저장으로 치환됐고, 지연 저장·스테일 스냅샷 배제는 paired-device-state의
// complete-snapshot revision 펜스가 담당한다. 따라서 먼저 더 새 revision의
// 완전 스냅샷을 승인(ack)시킨 뒤 늦은 저장과 구 snapshot을 주입한다.
(async()=>{const browser=await chromium.launch({headless:true});try{for(const remove of [false,true]){const page=await browser.newPage();try{
 await page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html?review=1`);
 await page.getByText('승인 대기',{exact:true}).waitFor();
 await page.evaluate(()=>{window.pairingIo.refreshFails=true;});
 await page.getByRole('button',{name:'화면 허용',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.pairingIo.pending[0].args.credentialId),'credential-A','actual save binds expected credential');
 await page.evaluate(remove=>{const io=window.pairingIo;io.old=structuredClone(io.devices);io.revision=3;io.devices=remove?[]:[{...io.devices[0],name:'Repaired fixture',source_grants:{credentialId:'credential-B',stateRevision:3,sourceIds:[],revision:0,reviewRequired:true,persistenceError:null}}];},remove);
 await page.evaluate(()=>window.pairingIo.pending.shift().resolve({credentialId:'credential-A',stateRevision:2,sourceIds:['display:A'],revision:2,reviewRequired:false,persistenceError:null}));
 await page.waitForTimeout(20);
 await page.evaluate(()=>{window.pairingIo.refreshFails=false;});
 const reads=await page.evaluate(()=>window.pairingIo.reads);
 await page.waitForFunction(reads=>window.pairingIo.reads>reads,reads);
 if(!remove)await page.getByText('Repaired fixture',{exact:true}).waitFor();
 await page.evaluate(()=>{const io=window.pairingIo;io.revision=1;io.devices=io.old;return io.reads;});
 await page.waitForFunction(reads=>window.pairingIo.reads>reads,reads);await page.waitForTimeout(30);
 await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));
 await page.waitForTimeout(50);
 assert.equal(await page.getByText('Fixture device',{exact:true}).count(),0,'late snapshot cannot resurrect retired credential');
 if(remove)assert.equal(await page.locator('.device-row-item').count(),0,'late save cannot approve retired credential');
 else{assert.equal(await page.getByText('Repaired fixture',{exact:true}).count(),1,'repaired credential stays acknowledged');assert.equal(await page.getByText('승인 대기',{exact:true}).count(),1,'late save cannot approve repaired credential');}
 console.log(`PASS actual parent ${remove?'removed':'repaired'} credential rejects late old save and stale snapshot across remount`);
}finally{await page.close();}}}finally{await browser.close();}})().catch(error=>{console.error(error);process.exitCode=1;});
