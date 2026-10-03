const { chromium } = require(process.env.PLAYWRIGHT_CORE_PATH || 'playwright-core');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const renderer of ['host', 'viewer']) {
      for (const theme of ['light', 'dark']) {
        const page = await browser.newPage({ viewport: { width: 360, height: 640 }, colorScheme: theme });
        const errors = [];
        page.on('pageerror', error => errors.push(String(error)));
        await page.goto(`file://${process.env.UI_TEST_DIR || '/tmp/leftcar-task4-ui'}/design-system-${renderer}.html?${renderer}`);
        const save = page.getByRole('button', { name: 'Save', exact: true });
        await save.waitFor();
        const actual = await save.evaluate(button => {
          const toRgb = hex => `rgb(${parseInt(hex.slice(1,3),16)}, ${parseInt(hex.slice(3,5),16)}, ${parseInt(hex.slice(5,7),16)})`;
          const palette = window.designPalette[matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'];
          const style = getComputedStyle(button);
          const label = button.querySelector('span') ?? button;
          return { width: button.getBoundingClientRect().width, height: button.getBoundingClientRect().height,
            background: style.backgroundColor, expectedBackground: toRgb(palette.btnPrimaryBg),
            color: getComputedStyle(label).color, expectedColor: toRgb(palette.btnPrimaryText),
            font: getComputedStyle(label).fontSize };
        });
        assert.ok(actual.width >= 44 && actual.height >= 44, `${renderer} target is too small`);
        assert.equal(actual.background, actual.expectedBackground, `${renderer}/${theme} action background must resolve from shared tokens`);
        assert.equal(actual.color, actual.expectedColor, `${renderer}/${theme} action text must resolve from shared tokens`);
        assert.equal(actual.font, '15px');
        if (renderer === 'viewer') {
          await save.focus();
          assert.equal(await save.evaluate(button => getComputedStyle(button).outlineWidth), '2px', 'native action focus must be visible');
        }
        await save.click();
        assert.equal(await save.isDisabled(), true);
        assert.equal(await save.getAttribute('aria-busy'), 'true');
        await save.evaluate(button => button.click());
        assert.equal(await page.evaluate(() => window.designClicks), 1, 'busy action admitted another write');
        if (renderer === 'viewer') assert.equal(await page.getByRole('tab', { name: 'Tab' }).getAttribute('aria-selected'), 'true');
        else {
          const toggle = page.getByRole('switch', { name: 'Share' });
          assert.ok((await toggle.boundingBox()).height >= 44);
          await toggle.click();
          assert.equal(await toggle.getAttribute('aria-checked'), 'true');
          assert.equal(await page.getByRole('textbox', { name: 'Width' }).getAttribute('aria-invalid'), 'true');
        }
        assert.deepEqual(errors, []);
        console.log(`PASS ${renderer} ${theme} compiled actions, targets, busy admission and semantics`);
        await page.close();
      }
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
