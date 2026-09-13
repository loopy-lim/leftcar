const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({headless:true});
  let failures = 0;
  const test = async (name, run) => {
    const page = await browser.newPage({locale:'ko-KR'});
    try {
      page.setDefaultTimeout(3000);
      await page.addInitScript(() => { window.__DEV__ = false; });
      await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/host.html`);
      await page.locator('input').waitFor();
      await run(page);
      console.log(`PASS ${name}`);
    } catch (error) { failures++; console.error(`FAIL ${name}: ${error.stack}`); }
    finally { await page.close(); }
  };
  const startA = async page => {
    await page.locator('input').fill('192.168.0.10');
    // Existing tests isolate lifetime from the separately covered keyboard path.
    await page.locator('input').blur();
    await page.getByRole('button', {name: /^(연결하기|Connect)$/}).click();
    await page.waitForFunction(() => transport.attempts.length === 1);
  };
  const selectB = async page => {
    await page.evaluate(() => { window.b = session.connectHost('192.168.0.20'); });
    await page.waitForFunction(() => transport.attempts.some(x => x.host === '192.168.0.20'));
    await page.evaluate(async () => { transport.attempts.find(x => x.host === '192.168.0.20').resolve(); await window.b; });
  };
  const remainsB = async page => {
    await page.waitForTimeout(1100);
    assert.equal(await page.evaluate(() => session.controlTarget()?.host), '192.168.0.20');
    assert.equal(await page.evaluate(() => transport.clients.find(x => x.host === '192.168.0.20').closed), false);
    assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    assert.equal(await page.evaluate(() => transport.attempts.filter(x => x.host === '192.168.0.10').length), 1);
  };
  try {
    await test('actual Host failed socket replacement preserves the previous successful session', async page => {
      await selectB(page);
      await page.locator('input').fill('192.168.0.10');
      await page.locator('input').blur();
      await page.getByRole('button', {name:/^(연결하기|Connect)$/}).click();
      for (let index = 1; index <= 3; index++) {
        await page.waitForFunction(index => transport.attempts.length === index + 1, index);
        await page.evaluate(index => transport.attempts[index].reject(new Error('connection closed')), index);
      }
      await page.getByText('컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.', {exact:true}).waitFor();
      assert.equal(await page.evaluate(() => session.controlTarget()?.host), '192.168.0.20');
      assert.equal(await page.evaluate(() => transport.clients[0].closed), false);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    });
    await test('actual Host failed current catalog closes its session and preserves visible failure', async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      await page.evaluate(() => transport.catalogs[0].reject(new Error('connection closed')));
      await page.getByText('컴퓨터와 연결할 수 없습니다. Leftcar가 실행 중인지 확인해 주세요.', {exact:true}).waitFor();
      assert.equal(await page.evaluate(() => session.controlTarget()), null);
      assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 1);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    });
    for (const bState of ['in-flight', 'connected']) await test(`actual Host late catalog failure preserves ${bState} B`, async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      await page.evaluate(() => { window.b = session.connectHost('192.168.0.20'); });
      await page.waitForFunction(() => transport.attempts.length === 2);
      if (bState === 'connected') await page.evaluate(async () => { transport.attempts[1].resolve(); await window.b; });
      await page.evaluate(() => transport.catalogs[0].reject(new Error('late catalog unavailable')));
      await page.waitForTimeout(50);
      if (bState === 'in-flight') await page.evaluate(async () => { transport.attempts[1].resolve(); await window.b; });
      assert.equal(await page.evaluate(() => session.controlTarget()?.host), '192.168.0.20');
      assert.equal(await page.evaluate(() => transport.clients[1].closed), false);
      assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 1);
      assert.equal(await page.getByText('late catalog unavailable', {exact:true}).count(), 0);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    });
    for (const result of ['success', 'failure']) await test(`actual Host late A ${result} cannot retry over selected B`, async page => {
      await startA(page); await selectB(page);
      await page.evaluate(result => { const a = transport.attempts[0]; result === 'success' ? a.resolve() : a.reject(new Error('late A failure')); }, result);
      await remainsB(page);
    });
    await test('actual Host retry backoff retains its original selection', async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].reject(new Error('retryable')));
      await selectB(page); await remainsB(page);
    });
    for (const departure of ['blur', 'unmount']) await test(`actual Host ${departure} cancels pending connection`, async page => {
      await startA(page);
      await page.evaluate(departure => departure === 'blur' ? hostIo.blur.forEach(fn => fn?.()) : mountHost(false), departure);
      await page.waitForTimeout(30);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForTimeout(1100);
      assert.equal(await page.evaluate(() => session.controlTarget()), null);
      assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
      assert.equal(await page.evaluate(() => transport.attempts.length), 1);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    });
    for (const after of ['selection', 'departure']) await test(`actual Host catalog completion is fenced after ${after}`, async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      if (after === 'selection') await selectB(page);
      else await page.evaluate(() => hostIo.blur.forEach(fn => fn?.()));
      await page.evaluate(() => transport.catalogs[0].resolve({}));
      await page.waitForTimeout(100);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
      if (after === 'departure') {
        assert.equal(await page.evaluate(() => session.controlTarget()), null);
        assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
      }
    });
    await test('actual Host successful retry uses one action generation and navigates once', async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].reject(new Error('retryable')));
      await page.waitForFunction(() => transport.attempts.length === 2);
      await page.evaluate(() => transport.attempts[1].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      assert.equal(await page.evaluate(() => session.captureRequestContext().selectionGeneration), 1);
      await page.evaluate(() => transport.catalogs[0].resolve({}));
      await page.waitForFunction(() => hostIo.navigations.length === 1);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), ['/catalog']);
      assert.equal(await page.evaluate(() => session.controlTarget().host), '192.168.0.10');
      await page.evaluate(() => hostIo.blur.forEach(fn => fn?.()));
      assert.equal(await page.evaluate(() => transport.clients[1].closed), false);
    });
    for (const after of ['selection', 'departure']) await test(`actual Host recent-host read completion is fenced after ${after}`, async page => {
      await startA(page);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      await page.evaluate(() => { storageIo.holdRecentRead = true; transport.catalogs[0].resolve({}); });
      await page.waitForFunction(() => storageIo.reads.length === 1);
      if (after === 'selection') await selectB(page);
      else await page.evaluate(() => hostIo.blur.forEach(fn => fn?.()));
      await page.evaluate(() => storageIo.reads.shift()());
      await page.waitForTimeout(100);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
      assert.equal(await page.evaluate(() => viewerIo.storage.has('leftcar.recent_hosts')), false);
    });
    await test('actual Host manual address starts on one handled keyboard tap', async page => {
      await page.locator('input').fill('192.168.0.10');
      // Dispatch keeps the input focused, as the attached native keyboard does.
      await page.getByRole('button', {name: /^(연결하기|Connect)$/}).dispatchEvent('click');
      await page.waitForFunction(() => transport.attempts.length === 1);
      assert.equal(await page.evaluate(() => hostIo.handledTaps), 'handled');
    });
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
