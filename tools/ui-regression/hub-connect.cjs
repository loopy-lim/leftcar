const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({headless:true});
  let failures = 0;
  const test = async (name, run) => {
    const page = await browser.newPage({locale:'ko-KR'});
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    try {
      page.setDefaultTimeout(3000);
      await page.addInitScript(() => { window.__DEV__ = false; });
      await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/hub-connect.html`);
      await page.waitForFunction(() => typeof showScreen === 'function');
      await run(page);
      assert.deepEqual(errors, [], 'no unhandled screen/session errors');
      console.log(`PASS ${name}`);
    } catch (error) { failures++; console.error(`FAIL ${name}: ${error.stack}`); }
    finally { await page.close(); }
  };
  const connect = async (page, host = '127.0.0.1', port = 44711) => {
    const count = await page.evaluate(() => transport.attempts.length);
    await page.evaluate(({host, port}) => { window.connecting = session.connectHost(host, port); }, {host, port});
    await page.waitForFunction(count => transport.attempts.length === count + 1, count);
    await page.evaluate(async count => { transport.attempts[count].resolve(); await window.connecting; }, count);
  };
  const start = async (page, mode) => {
    await page.evaluate(() => {
      viewerIo.storage.set('leftcar.token.v2.127.0.0.1.44711', 'synthetic-A');
      viewerIo.storage.set('leftcar.token.v2.192.168.0.20.7777', 'synthetic-B');
      viewerIo.storage.set('leftcar.recent_hosts', JSON.stringify([{host:'127.0.0.1', port:44711, name:'Synthetic A', lastConnected:Date.now()}]));
    });
    if (mode === 'focus') await connect(page);
    if (mode === 'quick') await page.evaluate(() => gate.markUserDisconnected());
    await page.evaluate(() => showScreen('hub'));
    if (mode === 'quick') await page.getByRole('button', {name:/Synthetic A/}).click();
    if (mode !== 'focus') {
      await page.waitForFunction(() => transport.attempts.length === 1);
      await page.evaluate(() => transport.attempts[0].resolve());
    }
    await page.waitForFunction(() => transport.catalogs.length === 1);
  };
  const settleCatalog = (page, index, kind) => page.evaluate(({index,kind}) => {
    const request = transport.catalogs[index];
    if (kind === 'success') request.resolve({displays:[], windows:[]});
    else request.reject(new ControlRequestError('catalog unavailable', kind));
  }, {index,kind});
  const disconnected = async page => {
    await page.getByText('연결 안 됨', {exact:true}).waitFor();
    assert.equal(await page.getByText('컴퓨터 연결됨', {exact:true}).count(), 0);
    assert.equal(await page.getByRole('button', {name:'화면 목록 보기 →'}).count(), 0);
    assert.equal(await page.evaluate(() => session.controlTarget()), null);
    assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 1);
  };
  const connected = async (page, host = '127.0.0.1') => {
    await page.getByText('컴퓨터 연결됨', {exact:true}).waitFor();
    await page.getByRole('button', {name:'화면 목록 보기 →'}).waitFor();
    assert.equal(await page.evaluate(() => session.controlTarget()?.host), host);
    assert.equal(await page.evaluate(() => session.controlClient().closed), false);
  };
  try {
    await test('actual failed Host to Hub transition never advertises the dead loopback endpoint', async page => {
      await page.evaluate(() => showScreen('host'));
      await page.locator('input').fill('127.0.0.1:44711');
      await page.getByRole('button', {name:/^(연결하기|Connect)$/}).dispatchEvent('click');
      await page.waitForFunction(() => transport.attempts.length === 1);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      await settleCatalog(page, 0, 'transport');
      await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).some(x => x.disabled));
      await page.evaluate(() => showScreen('hub'));
      await disconnected(page);
      assert.equal(await page.evaluate(() => transport.catalogs.length), 1);
    });
    for (const mode of ['auto', 'focus', 'quick']) {
      await test(`actual Hub ${mode} catalog transport failure clears current session and real connected UI`, async page => {
        await start(page, mode);
        await settleCatalog(page, 0, 'transport');
        await disconnected(page);
        assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.127.0.0.1.44711')), 'synthetic-A');
        assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
        await page.evaluate(() => refocus());
        // A fresh focus may legitimately retry; it must not resurrect the dead
        // client's connected card. Finish that controlled attempt if it starts.
        await page.waitForTimeout(50);
        assert.equal(await page.getByText('컴퓨터 연결됨', {exact:true}).count(), 0);
        assert.equal(await page.getByRole('button', {name:'화면 목록 보기 →'}).count(), 0);
        if (await page.evaluate(() => transport.attempts.length === 2)) {
          await page.evaluate(() => transport.attempts[1].resolve());
          await page.waitForFunction(() => transport.catalogs.length === 2);
          await settleCatalog(page, 1, 'transport');
        }
        await disconnected(page);
        assert.equal(await page.evaluate(() => transport.clients.every(x => x.closed)), true);
      });
      await test(`actual Hub ${mode} success retains usable connection`, async page => {
        await start(page, mode);
        await settleCatalog(page, 0, 'success');
        await connected(page);
        assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 0);
        assert.deepEqual(await page.evaluate(() => hostIo.navigations), mode === 'quick' ? ['/catalog'] : []);
      });
      await test(`actual Hub ${mode} 401 retains credential retirement behavior`, async page => {
        await start(page, mode);
        assert.equal(await page.evaluate(() => session.captureRequestContext().credential.token), 'synthetic-A');
        await settleCatalog(page, 0, 'unauthorized');
        await disconnected(page);
        assert.equal(await page.evaluate(() => viewerIo.storage.has('leftcar.token.v2.127.0.0.1.44711')), false);
        assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.192.168.0.20.7777')), 'synthetic-B');
        assert.equal(await page.evaluate(() => hostIo.navigations.length), mode === 'auto' ? 0 : 1);
      });
      for (const bState of ['in-flight', 'connected']) for (const result of ['transport', 'unauthorized', 'success']) {
        await test(`actual Hub ${mode} late A ${result} preserves ${bState} B and its UI`, async page => {
          await start(page, mode);
          await page.evaluate(() => { window.b = session.connectHost('192.168.0.20'); });
          await page.waitForFunction(() => transport.attempts.length === 2);
          if (bState === 'connected') {
            await page.evaluate(async () => { transport.attempts[1].resolve(); await window.b; refocus(); });
            await page.waitForFunction(() => transport.catalogs.length === 2);
            await settleCatalog(page, 1, 'success');
            await connected(page, '192.168.0.20');
          }
          await settleCatalog(page, 0, result);
          await page.waitForTimeout(50);
          if (bState === 'in-flight') {
            await page.evaluate(async () => { transport.attempts[1].resolve(); await window.b; refocus(); });
            await page.waitForFunction(() => transport.catalogs.length === 2);
            await settleCatalog(page, 1, 'success');
          }
          await connected(page, '192.168.0.20');
          assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 1);
          assert.equal(await page.evaluate(() => transport.clients[1].closeCount), 0);
          assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.192.168.0.20.7777')), 'synthetic-B');
          assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
        });
      }
    }
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
