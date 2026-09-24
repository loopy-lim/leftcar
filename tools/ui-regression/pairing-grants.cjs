const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
// 신 UI 계약: reviewRequired 행만 "화면 허용"(set_source_grants) 버튼을
// 노출하고, 요약은 승인 대기/화면 N개/화면 권한 없음으로 표현된다.
// 구 UI의 "화면 접근 모두 제거" 버튼과 "화면 접근: N" 카운터는 제거됐다.
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;
try{for(const retry of [false,true]) {const page=await browser.newPage();try{
 await page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html?review=1`);
 await page.getByText('승인 대기',{exact:true}).waitFor();await page.evaluate(()=>window.pairingIo.refreshFails=true);
 if(retry){await page.getByRole('button',{name:'화면 허용',exact:true}).click();await page.evaluate(()=>window.pairingIo.pending.shift().reject('grant save failed'));await page.getByText(/연결 상태를 확인하지 못했습니다/).waitFor();assert.equal(await page.getByText('승인 대기',{exact:true}).count(),1,'failed approval must keep pending review');await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));await page.getByRole('button',{name:'화면 허용',exact:true}).waitFor();}
 await page.getByRole('button',{name:'화면 허용',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.pairingIo.pending[0].args.credentialId),'credential-A','actual save binds expected credential');
 await page.evaluate(()=>{const io=window.pairingIo;io.revision=2;io.devices[0].source_grants={...io.devices[0].source_grants,reviewRequired:false};io.pending.shift().resolve({credentialId:'credential-A',stateRevision:2,sourceIds:['display:A'],revision:2,reviewRequired:false,persistenceError:null});});
 await page.getByText('화면 1개',{exact:true}).waitFor({timeout:2000});
 await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));
 await page.getByText('화면 1개',{exact:true}).waitFor({timeout:2000});
 const before=await page.evaluate(()=>{const io=window.pairingIo;io.refreshFails=false;io.revision=1;io.devices[0].source_grants={...io.devices[0].source_grants,reviewRequired:true};return io.reads;});
 await page.waitForFunction(previous=>window.pairingIo.reads>previous,before);await page.waitForTimeout(30);
 assert.equal(await page.getByText('승인 대기',{exact:true}).count(),0,'stale revision must not restore pending review');
 assert.equal(await page.getByText('화면 1개',{exact:true}).count(),1,'stale revision must not revoke approved screens');
 console.log(`PASS actual PairingPanel ${retry?'failed approval then retry':'direct approval'} authoritative save survives refresh failure/remount/stale snapshot`);
}catch(error){failures++;console.error(`FAIL ${retry?'retry':'direct'}: ${error.stack}`);}finally{await page.close();}}}
finally{await browser.close();}if(failures)process.exitCode=1;})().catch(error=>{console.error(error);process.exitCode=1;});
