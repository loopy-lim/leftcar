const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
const { getTranslation } = require('../../packages/ui-tokens/src');
(async () => {
  const browser = await chromium.launch({ headless: true });
  let failures = 0;
  try {
    for (const language of ['en', 'ko']) for (const colorScheme of ['light', 'dark']) {
      const page = await browser.newPage({ viewport: { width: 780, height: 540 }, colorScheme });
      const errors = []; page.on('pageerror', error => errors.push(String(error))); page.setDefaultTimeout(4000);
      try {
        const t = getTranslation(language);
        await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/index.html?dashboard&lang=${language}`);
        await page.waitForFunction(() => pendingCount('get_status') === 1);
        assert.equal(await page.getByText(t.host.inputPermBannerTitle, { exact: true }).count(), 0);
        assert.equal(await page.getByText(t.host.setupScreenStep, { exact: true }).count(), 0);
        await page.evaluate(() => {
          for (const [command, value] of Object.entries({ get_status: { sessions: [] }, get_input_permission: false, get_screen_permission: false, get_host_platform: 'macos', get_control_port: 7777, get_lan_ip: '192.168.0.42', list_input_requests: [] })) settle(command, value);
        });
        await page.getByText(t.host.inputPermBannerTitle, { exact: true }).waitFor();
        await page.getByText(t.host.setupScreenStep, { exact: true }).waitFor();
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        const settings = page.getByRole('button', { name: t.host.settingsTitle, exact: true });
        await settings.click();
        const dialog = page.getByRole('dialog', { name: t.host.settingsTitle, exact: true });
        await dialog.waitFor();
        const switchNode = dialog.getByRole('switch', { name: t.host.privacyCurtainLabel, exact: true });
        assert.equal(await switchNode.isDisabled(), true);
        await page.evaluate(() => settle('get_privacy_settings', 'status unavailable', true));
        await dialog.getByRole('alert').filter({ hasText: 'status unavailable' }).waitFor();
        await dialog.getByRole('button', { name: t.common.retry, exact: true }).click();
        await page.evaluate(() => settle('get_privacy_settings', false));
        await page.waitForFunction(label => [...document.querySelectorAll('button[role="switch"]')].some(button => button.getAttribute('aria-label') === label && !button.disabled), t.host.privacyCurtainLabel);
        assert.equal(await switchNode.isDisabled(), false);
        for (const button of await dialog.locator('button,input,select').all()) {
          const rect = await button.boundingBox(); if (!rect) continue;
          assert.ok(rect.width >= 44 && rect.height >= 44, `small control ${await button.getAttribute('aria-label')}: ${rect.width}x${rect.height}`);
        }
        const rect = await dialog.boundingBox(); assert.ok(rect.x >= 0 && rect.y >= 0 && rect.width <= 780 && rect.height <= 540);
        await page.keyboard.press('Escape'); await dialog.waitFor({ state: 'detached' });
        assert.equal(await settings.evaluate(element => element === document.activeElement), true);
        if (language === 'en' && colorScheme === 'light') await page.screenshot({ path: `${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/host-dashboard.png` });
        assert.deepEqual(errors, []); console.log(`PASS actual Host ${language}/${colorScheme} permission states, settings retry, targets and keyboard`);
      } catch (error) { failures++; console.error(`FAIL Host ${language}/${colorScheme}: ${error.stack}`); }
      finally { await page.close(); }
    }
  } finally { await browser.close(); }
  if (failures) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
