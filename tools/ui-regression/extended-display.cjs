const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ headless: true });
  let failures = 0;
  const test = async (name, run, query = '', fixture = 'extended-display') => {
    const page = await browser.newPage();
    try {
      page.on('pageerror', error => console.error('PAGE ERROR', error.message));
      page.setDefaultTimeout(3000);
      await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/${fixture}.html${query}`);
      await page.waitForFunction(() => window.displayIo?.calls.length > 0 || window.extensionModel);
      await run(page);
      console.log(`PASS ${name}`);
    } catch (error) { failures++; console.error(`FAIL ${name}: ${error.message}; UI: ${await page.locator('body').innerText()}`); }
    finally { await page.close(); }
  };
  try {
    await test('panel-derived size outside presets actually creates the requested display', async page => {
      await page.getByRole('button', { name: 'Create', exact: true }).click();
      await page.waitForFunction(() => window.displayIo.calls.some(c => c.command === 'virtual_display_create'));
      assert.deepEqual(await page.evaluate(() => window.displayIo.calls.find(c => c.command === 'virtual_display_create').args),
        { width: 1280, height: 720, scale: 2 });
    });
    await test('choosing a size does not create a monitor until Apply', async page => {
      const selector = page.getByRole('combobox', { name: 'Resolution', exact: true });
      const value = await selector.locator('option').evaluateAll(options => options.find(o => o.textContent.includes('1440'))?.value);
      await selector.selectOption(value);
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      assert.equal(await page.evaluate(() => window.displayIo.calls.filter(c => c.command === 'virtual_display_create').length), 0);
    });
    await test('removal remains visible and blocks creation until the OS completes it', async page => {
      await page.getByText('Removing…', { exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: 'Create', exact: true }).count(), 0);
      await page.evaluate(() => { window.displayIo.status.removalPending = false; });
      await page.getByRole('button', { name: 'Create', exact: true }).waitFor();
    }, '?pending');
    await test('Viewer recovers after delayed removal despite a stale catalog', async page => {
      await page.waitForFunction(() => window.extensionModel.extensionRemovalPending);
      await page.evaluate(() => { window.extensionIo.pending = false; });
      await page.waitForFunction(() => !window.extensionModel.extensionRemovalPending, null, { timeout: 5000 });
    }, '', 'extension-viewer');
    await test('switching Host during creation clears busy and ignores the late result', async page => {
      await page.evaluate(() => { window.extensionIo.pending = false; });
      await page.waitForFunction(() => !window.extensionModel.extensionRemovalPending, null, { timeout: 5000 });
      await page.evaluate(() => { void window.extensionModel.handleCreateExtensionDisplay(); });
      await page.waitForFunction(() => window.extensionIo.creates.length === 1);
      await page.evaluate(() => window.selectExtensionHost('192.168.0.43'));
      await page.waitForFunction(() => document.querySelector('output').textContent.includes('192.168.0.43:9123|false|idle|ok'));
      await page.evaluate(() => { window.extensionIo.creates[0].resolve({ sourceId: 'display:late' }); });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(await page.evaluate(() => window.opened.length), 0);
      assert.equal(await page.evaluate(() => window.extensionModel.extensionOperation), null);
    }, '', 'extension-viewer');
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})();
