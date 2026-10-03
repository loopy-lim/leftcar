const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true });
  const test = async (name, run) => {
    const page = await browser.newPage(); page.setDefaultTimeout(4000);
    try {
      await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/catalog.html`);
      await page.waitForFunction(() => model?.displays.length === 1);
      await page.evaluate(() => { void model.openDisplay(model.displays[0]); });
      await page.waitForFunction(() => viewerIo.preparations.length === 1);
      await page.evaluate(() => viewerIo.preparations[0].resolve());
      await page.waitForFunction(() => viewerIo.opened.length === 1);
      await page.evaluate(() => viewerIo.opened[0].resolve('fixture-native'));
      await page.waitForFunction(() => model.streams.length === 1);
      await page.evaluate(() => {
        const store = streamSessionStore(session.controlHost());
        const active = { ...store.getSnapshot()[0], reservation: undefined };
        store.update(() => [active]);
        window.originStore = store; window.originActive = active;
      });
      await page.waitForFunction(() => model.streams[0].reservation === undefined);
      await page.evaluate(() => window.originModel = model);
      await run(page); console.log(`PASS ${name}`);
    } finally { await page.close(); }
  };
  try {
    await test('actual old Catalog stop callback cannot target a newly selected Host', async page => {
      const stopped = await page.evaluate(async () => {
        await session.connectHost('192.168.0.43');
        return originModel.stopStream(originActive);
      });
      assert.equal(stopped, false);
      assert.equal(await page.evaluate(() => viewerIo.controlCalls.filter(call => call.command === 'stopStream').length), 0);
      assert.equal(await page.evaluate(() => originStore.getSnapshot().length), 1);
    });
    await test('actual delayed stop ack preserves a replacement stream with the same session id', async page => {
      await page.evaluate(() => { viewerIo.deferredCommands.add('stopStream'); window.stopResult = originModel.stopStream(originActive); });
      await page.waitForFunction(() => viewerIo.pendingCommands.length === 1);
      await page.evaluate(() => {
        window.replacement = { ...originActive, startedAt: originActive.startedAt + 1, port: originActive.port + 1 };
        originStore.update(() => [replacement]);
        viewerIo.pendingCommands.shift().reply();
      });
      assert.equal(await page.evaluate(() => window.stopResult), false);
      assert.equal(await page.evaluate(() => originStore.getSnapshot()[0] === replacement), true);
      assert.equal(await page.evaluate(() => originStore.getSnapshot().length), 1);
    });
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
