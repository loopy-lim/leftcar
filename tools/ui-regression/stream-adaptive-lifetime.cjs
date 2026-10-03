const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true });
  let failures = 0;
  try {
    for (const change of ['window', 'selection', 'late failure', 'stale restore', 'stale UDP scope', 'restore selection', 'switch selection', 'switch late failure', 'host uncertainty', 'Host stop', 'Host stop successor']) {
      const page = await browser.newPage();
      try {
        page.setDefaultTimeout(4000);
        await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/catalog.html?adaptive`);
        await page.waitForFunction(() => window.adaptiveController);
        await page.evaluate(async () => {
          const target = { width: 3840, height: 2160, fps: 60 };
          window.active = { session: 6, port: 5007, startedAt: Date.now(), sourceIndex: 0,
            sourceName: 'Fixture display', ...target, sourceTarget: target, activeTarget: target,
            fallbackTarget: null, qualityState: 'native', captureBackend: 'screenCaptureKit',
            contentMode: 'interactive', encoderExperiment: 'auto', mediaTransport: 'usb',
            viewerIps: ['192.168.0.2'], mediaKey: 'fixture' };
          window.originStore = streamSessionStore(session.controlHost());
          viewerIo.statusView = { sessions: [{ session: 6, state: 'streaming', fps: 60,
            encodeOutputFps: 60, receiverFrameGaps: 0, pendingFrameOldestAgeUs: 0 }] };
          originStore.update(() => [active]);
          await queryClient.invalidateQueries({ queryKey: ['host-status', session.controlHost()] });
        });
        if (change.startsWith('Host stop')) {
          await page.evaluate(async () => {
            const reservation = await createFixtureReservation(active);
            window.active = { ...active, reservation, startedAt: Date.now() - 6000 };
            originStore.update(() => [active]);
            window.restoreResult = adaptiveController.applyUdpStability({ preset: 'balanced', profile: 'auto' });
          });
          await page.waitForFunction(() => restoreRequests.length === 1);
          await page.evaluate(async () => {
            viewerIo.statusView = { sessions: [{ session: 6, state: 'stopped', error: 'host operator stopped stream' }] };
            await queryClient.invalidateQueries({ queryKey: ['host-status', session.controlHost()] });
          });
          await page.waitForFunction(() => closeRequests.length === 1);
          const logicalCountBeforeCloseAck = await page.evaluate(() => originStore.getSnapshot().length);
          await page.evaluate(change => {
            if (change === 'Host stop successor') {
              window.replacement = { ...active, session: 7, startedAt: active.startedAt + 1,
                port: 5011, reservation: undefined };
              originStore.update(() => [replacement]);
            }
            restoreRequests[0].resolve({ ...active, session: 7 });
          }, change);
          await page.evaluate(() => restoreResult);
          await page.evaluate(() => closeRequests[0].resolve());
          await page.waitForTimeout(30);
          assert.equal(logicalCountBeforeCloseAck, 0);
          assert.equal(await page.evaluate(change => change === 'Host stop successor'
            ? originStore.getSnapshot()[0] === replacement : originStore.getSnapshot().length === 0, change), true);
          assert.equal(await page.evaluate(() => viewerIo.controlCalls.filter(call => call.command === 'stopStream' && call.args.session === 7).length), change === 'Host stop successor' ? 0 : 1);
          console.log(`PASS actual ${change} retires logical owner before native close ACK`);
          continue;
        }
        if (change === 'host uncertainty') {
          await page.evaluate(async () => {
            window.realDateNow = Date.now.bind(Date);
            window.clockOffset = 0;
            Date.now = () => realDateNow() + clockOffset;
            originStore.update(() => [{ ...active, startedAt: Date.now() - 6000 }]);
            viewerIo.statusView = { sessions: [{ session: 6, state: 'stopped', revision: 1 }] };
            await queryClient.invalidateQueries({ queryKey: ['host-status', session.controlHost()] });
          });
          await page.waitForFunction(() => restoreRequests.length === 1);
          await page.evaluate(() => restoreRequests[0].reject(new AmbiguousControlError('startStream',
            new ControlRequestError('control connection closed', 'transport'))));
          await page.waitForFunction(() => adaptiveController.streamError?.includes('whether the operation completed'));
          await page.evaluate(async () => {
            clockOffset += 120000;
            viewerIo.statusView = { sessions: [{ session: 6, state: 'stopped', revision: 2 }] };
            await queryClient.invalidateQueries({ queryKey: ['host-status', session.controlHost()] });
          });
          await page.waitForTimeout(50);
          assert.equal(await page.evaluate(() => restoreRequests.length), 1);
          console.log('PASS actual unhealthy Host poll cannot bypass uncertain recovery retirement');
          continue;
        }
        if (change.startsWith('stale') || change.startsWith('restore') || change.startsWith('switch')) {
          await page.evaluate(change => {
            window.scopeCurrent = true;
            if (change.startsWith('switch')) {
              viewerIo.usbSubscribers.forEach(listener => listener({ attached: false, controlPort: 0 }));
              return;
            }
            window.restoreResult = adaptiveController.applyUdpStability(
              { preset: 'balanced', profile: 'auto' },
              change === 'stale UDP scope' ? { activeStreams: [active], isCurrent: () => scopeCurrent } : undefined,
            );
          }, change);
          await page.waitForFunction(() => restoreRequests.length === 1);
          await page.evaluate(async change => {
            const selectionChanged = change.includes('selection') || change.startsWith('switch');
            window.replacement = selectionChanged ? active : { ...active, startedAt: active.startedAt + 1 };
            if (selectionChanged) await session.connectHost('192.168.0.43');
            else originStore.update(() => [replacement]);
            window.scopeCurrent = false;
            if (change === 'switch late failure') restoreRequests[0].reject(new Error('Old restore failure'));
            else restoreRequests[0].resolve({ ...active, session: active.session, width: 2560 });
          }, change);
          if (!change.startsWith('switch')) await page.evaluate(() => restoreResult);
          else await page.waitForTimeout(50);
          assert.equal(await page.evaluate(() => originStore.getSnapshot()[0] === replacement), true);
          assert.equal(await page.evaluate(() => viewerIo.controlCalls.filter(call => call.command === 'stopStream').length), 0);
          assert.equal(await page.evaluate(() => adaptiveController.streamError), null);
          console.log(`PASS actual controller ${change} preserves owner and successor`);
          continue;
        }
        for (const gaps of [1, 2]) {
          await page.evaluate(async gaps => {
            viewerIo.statusView = { sessions: [{ session: 6, state: 'streaming', fps: 60,
              encodeOutputFps: 60, receiverFrameGaps: gaps, pendingFrameOldestAgeUs: 200001 }] };
            await queryClient.invalidateQueries({ queryKey: ['host-status', session.controlHost()] });
          }, gaps);
          // Allow the real React effect to observe each distinct status sample.
          await page.waitForFunction(gaps => viewerIo.controlCalls.filter(c => c.command === 'getStatus').length >= gaps + 2, gaps);
          await page.waitForTimeout(30);
        }
        await page.waitForFunction(() => adaptiveRequests.length === 1);
        await page.evaluate(async change => {
          window.replacement = { ...active, startedAt: active.startedAt + 1, localAudio: true };
          if (change === 'selection') await session.connectHost('192.168.0.43');
          else originStore.update(() => [replacement]);
          if (change === 'late failure') adaptiveRequests[0].reject(new Error('Old native resize failure'));
          else adaptiveRequests[0].resolve({ ...active, width: 2560, height: 1440, qualityState: 'fallback' });
        }, change);
        await page.waitForTimeout(50);
        assert.equal(await page.evaluate(change => originStore.getSnapshot()[0] === (change === 'selection' ? active : replacement), change), true);
        assert.equal(await page.evaluate(() => adaptiveController.streamError), null);
        console.log(`PASS actual adaptive completion retires old ${change}`);
      } catch (error) {
        failures++;
        console.error(`FAIL actual adaptive completion retires old ${change}: ${error.message}`);
      } finally { await page.close(); }
    }
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
