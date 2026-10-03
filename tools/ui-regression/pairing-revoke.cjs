const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
// 신 UI 계약: 행 요약(승인 대기/화면 N개/화면 권한 없음)으로 대기를 확인하고,
// 단일 삭제·전체 삭제 모두 RevokeConfirmDialog의 "삭제" 확인을 거친다.
// revoke의 persistenceErrors는 role=alert 배너에 그대로 노출된다.
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;
try {for(const all of [false,true])for(const fail of [true,false]){const page=await browser.newPage();try{
 await page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/pairing-grants.html`);
 await page.getByText('Fixture device',{exact:true}).waitFor();
 await page.evaluate(fail=>{window.pairingIo.refreshFails=true;window.pairingIo.revokeError=fail?'grant journal write failed; access blocked':null;},fail);
 if(all)await page.getByRole('button',{name:'모든 기기 연결 삭제',exact:true}).click();
 else await page.getByRole('listitem').filter({has:page.getByText('Fixture device',{exact:true})}).getByRole('button',{name:'Fixture device: 삭제',exact:true}).click();
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
