const { chromium } = require("playwright-core");
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); const errors=[]; page.on('pageerror', error=>errors.push(String(error)));
    await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/source-grants.html`);
    await page.getByText('Host에서 화면 접근 검토가 필요합니다').waitFor();
    await page.getByRole('button', {name:'화면 접근 보기 · 변경'}).click();
    assert.equal(await page.getByRole('checkbox').count(),2);
    await page.getByLabel('Synthetic A (1920 × 1080)').check();
    await page.getByRole('button', {name:'선택한 화면 접근 승인'}).click();
    assert.deepEqual(await page.evaluate(()=>window.grantIo.pending[0].args),{deviceId:'synthetic-device',sourceIds:['macos:display:a'],credentialId:'synthetic-credential'});
    await page.evaluate(()=> { const io=window.grantIo; io.grants={credentialId:'synthetic-credential',stateRevision:1,sourceIds:['macos:display:a'],revision:1,reviewRequired:false}; io.pending.shift().resolve(io.grants); });
    await page.getByText('화면 접근: 1', {exact:true}).waitFor();
    await page.getByRole('button',{name:'화면 접근 모두 제거'}).click();
    await page.evaluate(()=>{ const io=window.grantIo; io.refreshFails=true; io.grants={credentialId:'synthetic-credential',stateRevision:1,sourceIds:[],revision:2,reviewRequired:true,persistenceError:'저장 실패, 접근 차단'}; io.pending.shift().reject('source_grant_persistence_failed'); });
    await page.getByRole('alert').waitFor();
    await page.getByText('Host에서 화면 접근 검토가 필요합니다').waitFor({timeout:2000});
    assert.equal(await page.getByRole('button',{name:'화면 접근 모두 제거'}).isEnabled(),true);
    assert.deepEqual(errors,[]);
    console.log('PASS actual Host approval caller selects stable ID, confirms durable reply, shows failure/review and permits retry');
    await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/catalog.html`);
    await page.waitForFunction(()=>window.model?.displays.length===1);
    await page.evaluate(()=> { window.model.openDisplay(window.model.displays[0]); });
    await page.waitForFunction(()=>window.viewerIo.preparations.length===1);
    await page.evaluate(()=>window.viewerIo.preparations[0].resolve());
    await page.waitForFunction(()=>window.viewerIo.controlCalls.some(c=>c.command==='startStream'));
    assert.equal(await page.evaluate(()=>window.viewerIo.controlCalls.find(c=>c.command==='startStream').args.sourceId),'macos:display:synthetic');
    await page.waitForFunction(()=>window.viewerIo.opened.length===1);
    await page.evaluate(()=>window.viewerIo.opened[0].resolve());
    await page.waitForFunction(()=>window.model.streams.length===1);
    // Enumeration index is deliberately unchanged while the stable identity differs.
    await page.evaluate(()=>{ window.switchResult=window.model.handleSwitchSessionSource(window.model.streams[0],{...window.model.displays[0],sourceId:'macos:display:other'}); });
    await page.waitForFunction(()=>window.viewerIo.preparations.length===2, {timeout: 2000});
    await page.evaluate(()=>window.viewerIo.preparations[1].resolve());
    await page.waitForFunction(()=>window.viewerIo.controlCalls.some(c=>c.command==='reconfigureStream'));
    assert.equal(await page.evaluate(()=>window.viewerIo.controlCalls.find(c=>c.command==='reconfigureStream').args.sourceId),'macos:display:other');
    await page.waitForFunction(()=>window.viewerIo.opened.length===2);
    await page.evaluate(()=>window.viewerIo.opened[1].resolve());
    assert.equal(await page.evaluate(()=>window.switchResult),true);
    assert.equal(await page.evaluate(()=>window.model.streams[0].sourceId),'macos:display:other');
    await page.evaluate(()=> { window.viewerIo.emptyCatalog=true; window.model.handleRefresh(); });
    await page.waitForFunction(()=>window.model.displays.length===0);
    assert.match(await page.evaluate(()=>window.model.visibleError),/Host/);
    console.log('PASS actual Viewer catalog and same-index source switch pass stable identities and expose Host approval retry');
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
