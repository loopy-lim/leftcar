const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
const { getTranslation } = require('../../packages/ui-tokens/src');
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
    if (kind === 'success') { request.resolve({displays:[], windows:[]}); return; }
    request.reject(new ControlRequestError('catalog unavailable', kind));
    // transport 실패는 실제 스트림(control.ts fail)에서 socket.destroy를 동반한다 —
    // 요청 거절과 소켓 사망(whenClosed)은 한 세트다.
    if (kind === 'transport') request.socket.transportFail();
  }, {index,kind});
  const disconnected = async page => {
    // 대기 카드는 최근 호스트·자동 재연결 진행 여부에 따라 접속 가능(빠른
    // 재연결)·연결하는 중·연결 안 됨으로 렌더링된다. 공통 계약은 연결 카드의
    // 소멸(컴퓨터 연결됨·화면 목록 보기)과 세션 클라이언트의 비움이다.
    await page.getByText(/^(연결 안 됨|접속 가능|연결하는 중)/).first().waitFor();
    assert.equal(await page.getByText('컴퓨터 연결됨', {exact:true}).count(), 0);
    assert.equal(await page.getByRole('button', {name:'화면 목록 보기 →'}).count(), 0);
    assert.equal(await page.evaluate(() => session.controlClient()), null);
  };
  const connected = async (page, host = '127.0.0.1') => {
    await page.getByText('컴퓨터 연결됨', {exact:true}).waitFor();
    await page.getByRole('button', {name:'화면 목록 보기 →'}).waitFor();
    assert.equal(await page.evaluate(() => session.controlTarget()?.host), host);
    assert.equal(await page.evaluate(() => session.controlClient().closed), false);
  };
  try {
    await test('actual Hub credential retirement failure stays visible without an unhandled error', async page => {
      await start(page, 'focus');
      await page.evaluate(() => { viewerIo.failStorageDeletes = new Set(['leftcar.token.v2.127.0.0.1.44711']); });
      await settleCatalog(page, 0, 'unauthorized');
      await page.getByText(/연결할 수 없습니다|다시 시도|설정을|저장/).first().waitFor();
    });
    for (const mode of ['auto', 'quick']) await test(`actual Hub ${mode} departure cancels pending catalog without navigation`, async page => {
      await start(page, mode);
      await page.evaluate(() => showScreen('host'));
      await page.getByRole('button',{name:getTranslation('ko').viewer.manualTitle,exact:true}).click();
      await page.locator('input').waitFor();
      await settleCatalog(page, 0, 'success');
      await page.waitForTimeout(500);
      assert.equal(await page.evaluate(() => session.controlClient()), null);
      assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
      assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
    });
    await test('actual failed Host to Hub transition never advertises the dead loopback endpoint', async page => {
      await page.evaluate(() => showScreen('host'));
      await page.getByRole('button',{name:getTranslation('ko').viewer.manualTitle,exact:true}).click();
      await page.locator('input').fill('127.0.0.1:44711');
      await page.getByRole('button', {name:/^(연결하기|Connect)$/}).dispatchEvent('click');
      await page.waitForFunction(() => transport.attempts.length === 1);
      await page.evaluate(() => transport.attempts[0].resolve());
      await page.waitForFunction(() => transport.catalogs.length === 1);
      await settleCatalog(page, 0, 'transport');
      await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).some(x => x.disabled));
      await page.evaluate(() => showScreen('hub'));
      await disconnected(page);
      // 호스트 화면 연결 흐름의 카탈로그 실패는 명시적 연결 실패다 — 세션을
      // 해제(disconnectHost)하고 대상도 비운다.
      assert.equal(await page.evaluate(() => session.controlTarget()), null);
      assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
      assert.equal(await page.evaluate(() => transport.catalogs.length), 1);
    });
    for (const mode of ['auto', 'focus', 'quick']) {
      await test(`actual Hub ${mode} catalog transport failure clears current session and real connected UI`, async page => {
        await start(page, mode);
        await settleCatalog(page, 0, 'transport');
        await disconnected(page);
        // 연결 실패 경로(auto·focus는 연결 검증 probe, quick는 카탈로그 쿼리의
        // requestWithReconnect)가 세션을 해제하고 대상을 비운다.
        assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
        assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.127.0.0.1.44711')), 'synthetic-A');
        assert.equal(await page.evaluate(() => hostIo.navigations.some(x => String(x).includes('/pairing'))), false);
        await page.evaluate(() => refocus());
        // A fresh focus may legitimately retry; it must not resurrect the dead
        // client's connected card. Finish that controlled attempt if it starts.
        await page.waitForTimeout(50);
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
        assert.deepEqual(await page.evaluate(() => hostIo.navigations), mode === 'focus' ? [] : ['/catalog']);
      });
      await test(`actual Hub ${mode} 401 retains credential retirement behavior`, async page => {
        await start(page, mode);
        assert.equal(await page.evaluate(() => session.captureRequestContext().credential.token), 'synthetic-A');
        await settleCatalog(page, 0, 'unauthorized');
        await disconnected(page);
        assert.equal(await page.evaluate(() => viewerIo.storage.has('leftcar.token.v2.127.0.0.1.44711')), false);
        assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.192.168.0.20.7777')), 'synthetic-B');
        // 401 귀결은 시작 경로가 결정한다 — 연결 검증 probe(auto)는 조용히
        // 멈추고(markStale), 카탈로그 쿼리 경로(focus·quick)는 페어링으로 안내한다.
        const navigations = await page.evaluate(() => hostIo.navigations);
        assert.equal(navigations.filter(x => x && x.pathname === '/pairing').length, mode === 'auto' ? 0 : 1);
        assert.equal(await page.evaluate(() => transport.clients[0].closeCount), 1);
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
          assert.equal(await page.evaluate(() => transport.clients[0].closed), true);
          assert.equal(await page.evaluate(() => transport.clients[1].closeCount), 0);
          assert.equal(await page.evaluate(() => viewerIo.storage.get('leftcar.token.v2.192.168.0.20.7777')), 'synthetic-B');
          assert.deepEqual(await page.evaluate(() => hostIo.navigations), []);
        });
      }
    }
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
