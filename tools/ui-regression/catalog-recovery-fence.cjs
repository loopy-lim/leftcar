const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true });
  let failures = 0;
  try {
    for (const outcome of ['timeout', 'no such session']) {
      const page = await browser.newPage();
      try {
        page.setDefaultTimeout(4000);
        await page.clock.install();
        await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/catalog.html`);
        await page.waitForFunction(() => model?.displays.length === 1);
        await page.evaluate(() => { void model.openDisplay(model.displays[0]); });
        await page.waitForFunction(() => viewerIo.preparations.length === 1);
        await page.evaluate(() => viewerIo.preparations[0].resolve());
        await page.waitForFunction(() => viewerIo.opened.length === 1);
        await page.evaluate(() => viewerIo.opened[0].resolve('fixture-native'));
        await page.waitForFunction(() => model.streams.length === 1);
        await page.evaluate(() => {
          window.originStore = streamSessionStore(session.controlHost());
          window.active = originStore.getSnapshot()[0];
          viewerIo.statusView = { sessions: [{ session: active.session, state: 'streaming' }] };
          viewerIo.deferredCommands.add('stopStream');
          originStore.recover(active);
        });
        await page.waitForFunction(() => viewerIo.pendingCommands.some(c => c.command === 'stopStream'));
        if (outcome === 'timeout') await page.clock.fastForward(5001);
        else await page.evaluate(() => viewerIo.pendingCommands.find(c => c.command === 'stopStream').reply('no such session 1'));
        // The former caller swallowed stop uncertainty and prepared another
        // native receiver, then opened a fresh control socket and Host session.
        await page.waitForTimeout(40);
        const prepares = await page.evaluate(() => viewerIo.preparations.length);
        if (prepares > 1) {
          await page.evaluate(() => viewerIo.preparations[1].resolve());
          await page.waitForFunction(() => viewerIo.controlCalls.filter(c => c.command === 'startStream').length === 2);
          if (outcome === 'no such session') {
            await page.waitForFunction(() => viewerIo.opened.length === 2);
            await page.evaluate(() => viewerIo.opened[1].resolve('fixture-native'));
          }
        }
        assert.equal(prepares, outcome === 'timeout' ? 1 : 2);
        assert.equal(await page.evaluate(() => viewerIo.controlCalls.filter(c => c.command === 'startStream').length), outcome === 'timeout' ? 1 : 2);
        console.log(`PASS actual Catalog restore ${outcome} mutation fence`);
      } catch (error) {
        failures++;
        console.error(`FAIL actual Catalog restore ${outcome}: ${error.stack}`);
        console.error(await page.evaluate(() => ({ calls: viewerIo.controlCalls.map(call => call.command),
          preparations: viewerIo.preparations.length, opened: viewerIo.opened.length,
          pending: viewerIo.pendingCommands.map(call => call.command), host: session.controlHost(),
          error: originStore?.getRecoveryError() })));
      } finally { await page.close(); }
    }
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
