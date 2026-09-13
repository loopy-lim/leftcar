const {chromium}=require('playwright-core');const assert=require('node:assert/strict');
(async()=>{const browser=await chromium.launch({headless:true});let failures=0;
const test=async(name,fn)=>{const page=await browser.newPage();try{page.setDefaultTimeout(3000);await fn(page);console.log(`PASS ${name}`);}catch(e){failures++;console.error(`FAIL ${name}: ${e.stack}`);}finally{await page.close();}};
const open=(page,file)=>page.goto(`file://${process.env.UI_TEST_DIR||'/tmp/leftcar-task4-ui'}/${file}`);
try{
await test('actual Host clipboard rejection and deferred success',async page=>{
 await page.addInitScript(()=>{window.copyRequests=[];Object.defineProperty(navigator,'clipboard',{value:{writeText(text){return new Promise((resolve,reject)=>window.copyRequests.push({text,resolve,reject}));}}});});
 await open(page,'index.html?dashboard');await page.getByRole('button',{name:/Computer address/i}).click();
 assert.equal(await page.getByText(/Address .* Copied!/).count(),0);
 await page.evaluate(()=>window.copyRequests.shift().reject(new Error('Synthetic denied')));await page.getByText('Copy failed',{exact:true}).waitFor();
 await page.getByRole('button',{name:/Computer address/i}).click();assert.equal(await page.getByText(/Address .* Copied!/).count(),0);
 await page.evaluate(()=>window.copyRequests.shift().resolve());await page.getByText(/Address .* Copied!/).waitFor();
});
for(const decision of ['허용','거절'])await test(`actual PairingPanel ${decision} failure retains row and retries`,async page=>{
 await open(page,'pairing-grants.html?offer');await page.getByText('Synthetic pending viewer',{exact:true}).waitFor();
 const button=page.getByRole('button',{name:decision,exact:true});await button.click();assert.equal(await button.isDisabled(),true);
 await page.evaluate(()=>window.pairingIo.decisions.shift().reject('Synthetic approval failure'));await page.getByText(/연결 상태를 확인하지 못했습니다/).waitFor();
 assert.equal(await page.getByText('Synthetic pending viewer',{exact:true}).count(),1);await button.click();
 await page.evaluate(()=>{window.pairingIo.pendingOffers=[];window.pairingIo.decisions.shift().resolve();});await page.getByText('Synthetic pending viewer',{exact:true}).waitFor({state:'detached'});
});
await test('actual pairing camera settings failure retry and foreground permission refresh',async page=>{
 await open(page,'camera.html');await page.waitForFunction(()=>window.cameraIo?.setPermission);
 await page.evaluate(()=>window.cameraIo.setPermission({granted:false,canAskAgain:false}));
 const settings=page.getByRole('button',{name:/설정|Settings/});await settings.click();await page.evaluate(()=>window.cameraIo.settings.shift().reject(new Error('설정을 열지 못했습니다')));
 await page.getByText(/설정을 열지 못했습니다/).waitFor({timeout:2000});
 await settings.click();await page.evaluate(()=>window.cameraIo.settings.shift().resolve());
 await page.evaluate(()=>window.cameraIo.listeners.forEach(listener=>listener('active')));await page.waitForFunction(()=>window.cameraIo.requests.length===1);
 await page.evaluate(()=>window.cameraIo.requests.shift().reject(new Error('권한을 확인하지 못했습니다')));
 await page.getByText(/권한을 확인하지 못했습니다/).waitFor({timeout:2000});
 await page.getByRole('button',{name:/다시|Retry/}).click();await page.waitForFunction(()=>window.cameraIo.requests.length===1);
 await page.evaluate(()=>window.cameraIo.requests.shift().resolve({granted:true,canAskAgain:true}));await page.getByTestId('live-camera').waitFor();
 await page.evaluate(()=>window.mountCamera(false));await page.waitForFunction(()=>window.cameraIo.listeners.size===0);
});
}finally{await browser.close();}if(failures)process.exitCode=1;})().catch(e=>{console.error(e);process.exitCode=1;});
