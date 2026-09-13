const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;
try{for(const retry of [false,true]) {const page=await browser.newPage();try{
 await page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html`);
 await page.getByText('화면 접근: 1',{exact:true}).waitFor();await page.evaluate(()=>window.pairingIo.refreshFails=true);
 if(retry){await page.getByRole('button',{name:'화면 접근 모두 제거',exact:true}).click();await page.evaluate(()=>window.pairingIo.pending.shift().reject('grant persistence failed'));await page.getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));await page.getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();}
 await page.getByRole('button',{name:'화면 접근 모두 제거',exact:true}).click();
 await page.evaluate(()=>window.pairingIo.pending.shift().resolve({credentialId:'credential-A',stateRevision:2,sourceIds:[],revision:2,reviewRequired:false,persistenceError:null}));
 await page.getByText('화면 접근: 0',{exact:true}).waitFor({timeout:2000});
 await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));
 await page.getByText('화면 접근: 0',{exact:true}).waitFor({timeout:2000});
 const before=await page.evaluate(()=>{window.pairingIo.refreshFails=false;return window.pairingIo.reads;});
 await page.waitForFunction(previous=>window.pairingIo.reads>previous,before);await page.waitForTimeout(30);
 assert.equal(await page.getByText('화면 접근: 0',{exact:true}).count(),1,'stale revision must not restore old count');
 console.log(`PASS actual PairingPanel ${retry?'failed removal then retry':'direct removal'} authoritative save survives refresh failure/remount/stale snapshot`);
}catch(error){failures++;console.error(`FAIL ${retry?'retry':'direct'}: ${error.stack}`);}finally{await page.close();}}}
finally{await browser.close();}if(failures)process.exitCode=1;})().catch(error=>{console.error(error);process.exitCode=1;});
