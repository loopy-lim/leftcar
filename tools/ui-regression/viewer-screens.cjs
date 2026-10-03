const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
const { getTranslation } = require('../../packages/ui-tokens/src');
(async () => {
  const browser = await chromium.launch({ headless: true });
  let failures = 0;
  const test = async (name, setup, run) => {
    const page = await browser.newPage({ viewport: { width: 320, height: 640 }, locale: 'en-US', ...setup });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    try { page.setDefaultTimeout(4000); await run(page); assert.deepEqual(errors, []); console.log(`PASS ${name}`); }
    catch (error) { failures++; console.error(`FAIL ${name}: ${error.stack}`); }
    finally { await page.close(); }
  };
  const open = (page, file) => page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/${file}`);
  const bounds = async page => {
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'screen overflows horizontally');
    for (const node of await page.locator('button:not([aria-hidden="true"]),input').all()) {
      if (!await node.isVisible()) continue;
      const rect = await node.boundingBox();
      assert.ok(rect.width >= 44 && rect.height >= 44, `target ${await node.getAttribute('aria-label')} is ${rect.width}x${rect.height}`);
    }
  };
  try {
    for (const theme of ['light', 'dark']) for (const language of ['en', 'ko']) await test(`actual Catalog ${theme}/${language} compact layout and one settings entry`, { colorScheme: theme }, async page => {
      await page.addInitScript(language => { window.__viewerStoredValues = { 'leftcar.language': language }; }, language);
      await open(page, 'viewer-screens.html');
      const settings = page.getByRole('button', { name: language === 'en' ? 'Settings' : '설정', exact: true });
      await settings.waitFor();
      assert.equal(await settings.count(), 1);
      assert.equal(await page.getByRole('button', { name: /^(Open|열기):/ }).count(), 1);
      await bounds(page);
      if (theme === "dark" && language === "ko") await page.screenshot({ path: `${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/viewer-catalog.png` });
      await page.setViewportSize({width:1024,height:600}); await bounds(page);
      await settings.click();
      const sheet = page.getByRole('dialog'); await sheet.waitFor();
      assert.equal(await sheet.getByRole('button', { name: getTranslation(language).viewer.qualityAutoLabel, exact: true }).count(), 1);
      await bounds(page);
    });
    await test('actual Settings sheet shows a native failure and retries the same audio intent', {}, async page => {
      await open(page, 'viewer-screens.html');
      await page.getByRole('button', { name: /^Open:/ }).click();
      await page.waitForFunction(() => viewerIo.preparations.length === 1);
      await page.evaluate(() => viewerIo.preparations[0].resolve());
      await page.waitForFunction(() => viewerIo.opened.length === 1);
      await page.evaluate(() => viewerIo.opened[0].resolve('fixture-native'));
      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      const sheet = page.getByRole('dialog');
      await sheet.getByRole('switch', { name: 'System Audio', exact: true }).click();
      await page.waitForFunction(() => viewerIo.audioRequests.length === 1);
      await page.evaluate(() => viewerIo.audioRequests[0].reject(new Error('Device audio update failed')));
      const error = sheet.getByRole('alert').filter({ hasText: 'Device audio update failed' });
      await error.waitFor();
      await error.getByRole('button', { name: 'Retry', exact: true }).click();
      await page.waitForFunction(() => viewerIo.audioRequests.length === 2);
      assert.deepEqual(await page.evaluate(() => viewerIo.audioRequests.map(request => request.enabled)), [false, false]);
      await page.evaluate(() => viewerIo.audioRequests[1].resolve());
      await error.waitFor({ state: 'detached' });
      assert.equal(await sheet.getByRole('switch', { name: 'System Audio', exact: true }).getAttribute('aria-checked'), 'false');
    });
    await test('actual PIN fits compact width, focuses and paste error is actionable', {}, async page => {
      await page.addInitScript(() => { window.__viewerSearchParams = { endpoint: '192.168.0.42:7777' }; window.__viewerStoredValues = { 'leftcar.language': 'en' }; window.__viewerClipboard = 'only 12'; });
      await open(page, 'camera.html');
      const input = page.getByRole('textbox', { name: 'Enter 6-Digit Pairing Code', exact: true });
      await input.waitFor(); await input.focus(); await input.fill('ab12-34');
      assert.equal(await input.inputValue(), '1234');
      assert.equal(await input.getAttribute('inputmode'), 'numeric');
      await bounds(page);
      await page.getByRole('button', { name: 'Paste connection code', exact: true }).click();
      await page.getByRole('alert').waitFor();
      assert.equal(await input.inputValue(), '1234');
      await page.evaluate(() => window.cameraIo.blur());
      await page.getByRole('textbox').waitFor({ state: 'detached' });
      await page.evaluate(() => window.cameraIo.focus());
      await input.waitFor(); assert.equal(await input.inputValue(), '1234');
    });
    await test('actual QR camera releases on blur and mode change', {}, async page => {
      await open(page, 'camera.html'); await page.waitForFunction(() => window.cameraIo?.setPermission);
      await page.evaluate(() => window.cameraIo.setPermission({ granted: true, canAskAgain: true }));
      await page.getByTestId('live-camera').waitFor();
      await page.evaluate(() => window.cameraIo.blur()); await page.getByTestId('live-camera').waitFor({ state: 'detached' });
      await page.evaluate(() => window.cameraIo.focus()); await page.getByTestId('live-camera').waitFor();
      await page.getByRole('tab', { name: 'Enter Pairing Code' }).click(); await page.getByTestId('live-camera').waitFor({ state: 'detached' });
      await page.evaluate(() => window.mountCamera(false)); await page.waitForFunction(()=>window.cameraIo.listeners.size===0); assert.equal(await page.evaluate(() => window.cameraIo.listeners.size), 0);
    });
    await test('actual pending file picker cannot send to another Host', {}, async page => {
      await open(page, 'viewer-screens.html');
      await page.getByRole('button', { name: 'File Transfer', exact: true }).click();
      const sheet = page.getByRole('dialog');
      await sheet.getByRole('button', { name: getTranslation('en').viewer.fileSend, exact: true }).click();
      await page.waitForFunction(() => fileIo.picks.length === 1);
      await page.evaluate(async () => { await session.connectHost('192.168.0.43'); fileIo.picks[0].resolve({ name: 'test.txt', size: 1, readBase64: async () => 'YQ==' }); });
      await sheet.getByRole('alert').waitFor();
      assert.equal(await page.evaluate(() => viewerIo.controlCalls.filter(call => call.command === 'sendFileBegin').length), 0);
      assert.equal(await sheet.getByRole('button', { name: getTranslation('en').viewer.fileSend, exact: true }).isDisabled(), false);
    });
    await test('actual Host discovery failure provides retry and cleans up', {}, async page => {
      await page.addInitScript(() => { window.__viewerNsdEnabled = true; window.__viewerStoredValues = { 'leftcar.language': 'en' }; });
      await open(page, 'host.html');
      await page.waitForFunction(() => viewerIo.discoveryStarts === 1);
      await page.evaluate(() => viewerIo.nativeListeners.get('leftcar:discovery-failed').forEach(listener => listener()));
      await page.getByText(getTranslation('en').viewer.discoveryFailed,{exact:true}).waitFor();
      await page.getByRole('button', { name: 'Retry', exact: true }).click();
      await page.waitForFunction(() => viewerIo.discoveryStarts === 2);
      await page.evaluate(() => window.mountHost(false));
      await page.waitForFunction(()=>[...viewerIo.nativeListeners.values()].every(listeners=>listeners.size===0));
      assert.equal(await page.evaluate(() => [...viewerIo.nativeListeners.values()].reduce((total, listeners) => total + listeners.size, 0)), 0);
      assert.ok(await page.evaluate(() => viewerIo.discoveryStops) >= 1);
    });
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
