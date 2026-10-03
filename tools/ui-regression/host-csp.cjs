const { chromium } = require('playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const violations = [];
    await page.exposeFunction('recordViolation', directive => violations.push(directive));
    await page.addInitScript(() => document.addEventListener('securitypolicyviolation', event => window.recordViolation(event.effectiveDirective)));
    await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/host-csp.html?dashboard`);
    await page.getByRole('button', { name: 'Host Settings', exact: true }).click();
    await page.getByRole('dialog', { name: 'Host Settings', exact: true }).waitFor();
    await page.evaluate(() => {
      const script = document.createElement('script');
      script.textContent = 'window.untrustedScriptRan = true';
      document.body.append(script);
    });
    assert.equal(await page.evaluate(() => window.untrustedScriptRan), undefined, 'untrusted inline script must not execute');
    assert.equal(await page.evaluate(async () => {
      try { await fetch('https://example.invalid/untrusted'); return false; }
      catch { return true; }
    }), true, 'remote connection must be blocked');
    await page.waitForFunction(() => window.untrustedScriptRan === undefined);
    await page.waitForTimeout(30);
    assert(violations.includes('script-src-elem'));
    assert(violations.includes('connect-src'));
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: /Generate Pairing Code/ }).click();
    const pairing = page.getByRole('dialog', { name: 'Generate Pairing Code', exact: true });
    await pairing.getByRole('button', { name: /Generate Pairing Code/ }).click();
    await page.evaluate(() => window.settle('begin_pairing', { offer_id: 'csp-fixture-offer', qr_payload: 'leftcar://pair?fixture=1', code: '123456', expires_in_secs: 120 }));
    await pairing.getByRole('img').waitFor();
    assert.equal(await pairing.getByRole('img').evaluate(img => img.naturalWidth > 0 && img.src.startsWith('data:image/')), true, 'local pairing QR data image must remain usable');
    console.log('PASS Host CSP allows local React UI and pairing QR while blocking injected scripts and remote connections');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
