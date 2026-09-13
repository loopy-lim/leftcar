const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;
try {for(const all of [false,true])for(const fail of [true,false]){const page=await browser.newPage();try{
 await page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html`);
 await page.getByText('화면 접근: 1',{exact:true}).waitFor();
 await page.evaluate(fail=>{window.pairingIo.refreshFails=true;window.pairingIo.revokeError=fail?'grant journal write failed; access blocked':null;},fail);
 await page.getByRole('button',{name:all?'모든 기기 연결 삭제':'삭제',exact:true}).click();
 await page.getByRole('dialog').getByRole('button',{name:'삭제',exact:true}).click();
 if(fail)await page.getByText('grant journal write failed; access blocked',{exact:true}).waitFor({timeout:2000});
 await page.waitForFunction(()=>!document.body.textContent.includes('Fixture device'),null,{timeout:2000});
 await page.evaluate(()=>window.mountPairing(false));await page.waitForTimeout(20);await page.evaluate(()=>window.mountPairing(true));
 if(fail)await page.getByText('grant journal write failed; access blocked',{exact:true}).waitFor({timeout:2000});
 assert.equal(await page.getByText('Fixture device',{exact:true}).count(),0,'removed device stays pruned through failed refresh and remount');
 assert.equal(await page.getByText('grant journal write failed; access blocked',{exact:true}).count(),Number(fail));
 console.log(`PASS actual ${all?'all':'single'} revoke ${fail?'failure visible after row removal/remount':'successful typed outcome'} while refresh fails`);
}catch(error){failures++;console.error(`FAIL ${all?'all':'single'} ${fail?'error':'success'}: ${error.stack}`);}finally{await page.close();}}}
finally{await browser.close();}if(failures)process.exitCode=1;})().catch(error=>{console.error(error);process.exitCode=1;});
