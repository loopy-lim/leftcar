/**
 * OG 이미지(1200x630) 생성 스크립트 — 일회성 빌드 도구.
 * playwright-core의 캐시된 headless chromium으로 브랜드 HTML을 렌더링해
 * site/public/og-image.png 로 저장한다.
 */
import { chromium } from 'playwright-core';
import { execSync } from 'child_process';
import path from 'path';
import fs from 'fs';

const root = path.resolve(import.meta.dirname, '..');
const outPath = path.join(root, 'public', 'og-image.png');

function findChromium() {
  // 우선 프로젝트 관례 경로, 없으면 ms-playwright 기본 캐시에서 탐색
  const cache = path.join(process.env.HOME ?? '', 'Library', 'Caches', 'ms-playwright');
  const shell = path.join(cache, 'chromium_headless_shell-1243', 'chrome-headless-shell-mac-arm64', 'chrome-headless-shell');
  if (fs.existsSync(shell)) return shell;
  const out = execSync(
    `find ${cache} -type f \\( -name "chrome-headless-shell" -o -name "Chromium" \\) -perm +111 2>/dev/null | head -1`,
  ).toString().trim();
  if (!out) throw new Error('cached chromium not found');
  return out;
}

const html = `<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    width: 1200px; height: 630px;
    background: #0b0e14;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Apple SD Gothic Neo", "Noto Sans KR", sans-serif;
    color: #e6ebf4;
    display: flex; align-items: center; justify-content: center;
    position: relative; overflow: hidden;
  }
  .glow {
    position: absolute; width: 900px; height: 900px; border-radius: 50%;
    background: radial-gradient(circle, rgba(76,194,168,0.14) 0%, rgba(76,194,168,0) 60%);
    top: -350px; right: -250px;
  }
  .grid {
    position: absolute; inset: 0;
    background-image:
      linear-gradient(rgba(151,163,182,0.05) 1px, transparent 1px),
      linear-gradient(90deg, rgba(151,163,182,0.05) 1px, transparent 1px);
    background-size: 48px 48px;
  }
  .wrap { position: relative; display: flex; align-items: center; gap: 72px; padding: 0 96px; }
  .mark {
    width: 220px; height: 220px; border-radius: 48px; background: #11151f;
    border: 1px solid #1f2733; display: grid; place-items: center; flex: none;
  }
  h1 { font-size: 92px; font-weight: 700; letter-spacing: -0.03em; line-height: 1.05; }
  h1 span { color: #4cc2a8; }
  p { margin-top: 24px; font-size: 30px; color: #97a3b6; font-weight: 400; line-height: 1.5; max-width: 560px; }
</style></head>
<body>
  <div class="grid"></div>
  <div class="glow"></div>
  <div class="wrap">
    <div class="mark">
      <svg width="128" height="128" viewBox="0 0 32 32">
        <rect width="32" height="32" rx="7" fill="#0b0e14"/>
        <rect x="6" y="6" width="13" height="9.5" rx="2" fill="#4cc2a8"/>
        <rect x="21.5" y="6" width="4.5" height="9.5" rx="1.8" fill="none" stroke="#97a3b6" stroke-width="1.6"/>
        <rect x="6" y="18" width="9.5" height="8" rx="2" fill="none" stroke="#97a3b6" stroke-width="1.6"/>
        <rect x="18" y="18" width="8" height="8" rx="2" fill="none" stroke="#97a3b6" stroke-width="1.6"/>
      </svg>
    </div>
    <div>
      <h1>Leftcar<br><span>PC 화면을 Android 여러 창으로</span></h1>
      <p>승인한 디스플레이만 신뢰하는 로컬 네트워크 다중 화면 뷰어</p>
    </div>
  </div>
</body></html>`;

const browser = await chromium.launch({ executablePath: findChromium() });
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
await page.setContent(html, { waitUntil: 'load' });
await page.screenshot({ path: outPath, clip: { x: 0, y: 0, width: 1200, height: 630 } });
await browser.close();
console.log('wrote', outPath, fs.statSync(outPath).size, 'bytes');
