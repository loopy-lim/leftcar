import { chromium } from 'playwright-core';
import { spawn } from 'child_process';
import path from 'path';
import fs from 'fs';
import os from 'os';

async function run() {
  const browserPath = fs.readFileSync(path.join(os.homedir(), '.cache', 'playwright-core', 'chromium-path.txt'), 'utf8').trim();
  
  const server = spawn('bun', ['run', 'preview', '--port', '4173'], { cwd: process.cwd() });
  
  await new Promise(resolve => setTimeout(resolve, 2000));

  const browser = await chromium.launch({ executablePath: browserPath });
  const page = await browser.newPage();
  await page.goto('http://localhost:4173');

  const errors = [];

  // 6개 섹션 id 존재
  const sections = ['hero', 'features', 'security', 'stack', 'status', 'footer'];
  for (const id of sections) {
    const el = await page.locator(`#${id}`);
    if (await el.count() === 0) errors.push(`Section #${id} not found`);
  }

  // h1 텍스트
  const h1 = await page.locator('h1').textContent();
  if (!h1 || !h1.includes('승인한 PC 화면을')) errors.push('h1 text mismatch');

  // CTA 2개 href
  const ctas = await page.locator('.hero .btn').evaluateAll(els => els.map(e => e.getAttribute('href')));
  if (ctas.length !== 2) errors.push('Should have 2 CTAs');
  if (ctas[0] !== 'https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4') errors.push('CTA 1 href mismatch');
  if (ctas[1] !== 'https://github.com/loopy-lim/leftcar') errors.push('CTA 2 href mismatch');

  // 다이어그램 aria-label
  const diagram = await page.locator('figure').getAttribute('aria-label');
  if (!diagram || !diagram.includes('모식도')) errors.push('Diagram aria-label mismatch');

  // 본문 배경 토큰
  const bgColor = await page.evaluate(() => window.getComputedStyle(document.body).backgroundColor);
  if (bgColor !== 'rgb(11, 14, 20)') errors.push('Background color mismatch');

  await browser.close();
  server.kill();

  if (errors.length > 0) {
    console.error('Render test failed:', errors);
    process.exit(1);
  }
  console.log('Render test passed');
}

run().catch(err => {
  console.error(err);
  process.exit(1);
});
