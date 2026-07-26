// Browser smoke test: load the built site, vectorize a real image, and check the
// result is sane. Verifies the parts a unit test cannot — that the wasm actually
// loads at its hashed asset path, that the worker starts, and that the page
// renders the SVG.
//
//   npx vite preview --port 4180 &
//   node scripts/smoke.mjs ../benchmarks/shootout/cases/ring.png
//
// Requires playwright (`npm i --no-save playwright`) and a Chromium build. Set
// CHROMIUM to override the browser path, PREVIEW_URL to override the address.

import { chromium } from 'playwright';

const CASE = process.argv[2];
if (!CASE) {
  console.error('usage: node scripts/smoke.mjs <image>');
  process.exit(2);
}

const browser = await chromium.launch(
  process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {},
);
const page = await browser.newPage();
const errors = [];
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', e => errors.push('pageerror: ' + e.message));
page.on('requestfailed', r => errors.push('request failed: ' + r.url()));
page.on('response', r => {
  if (r.status() >= 400) errors.push(`${r.status()} ${r.url()}`);
});

const url = process.env.PREVIEW_URL ?? 'http://localhost:4180/open-vectorizer/';
await page.goto(url, { waitUntil: 'networkidle' });
console.log('title:', await page.title());

await page.setInputFiles('input[type=file]', CASE);

// Wait for the stats grid to appear, which only renders once a report arrives.
await page.waitForSelector('text=Accuracy', { timeout: 30000 });
await page.waitForFunction(() => !document.body.innerText.includes('Vectorizing'), { timeout: 30000 });

const stats = await page.evaluate(() => {
  const out = {};
  document.querySelectorAll('div.rounded-md.border.border-slate-800').forEach(el => {
    const label = el.querySelector('div:first-child')?.textContent?.trim();
    const value = el.querySelector('div:last-child')?.textContent?.trim();
    if (label && value) out[label] = value;
  });
  return out;
});
console.log('stats:', JSON.stringify(stats));

const svgInfo = await page.evaluate(() => {
  const panes = [...document.querySelectorAll('svg')];
  const rendered = panes.find(s => s.getAttribute('viewBox'));
  return rendered ? { viewBox: rendered.getAttribute('viewBox'), children: rendered.children.length,
                      circles: rendered.querySelectorAll('circle').length,
                      rects: rendered.querySelectorAll('rect').length,
                      paths: rendered.querySelectorAll('path').length } : null;
});
console.log('svg:', JSON.stringify(svgInfo));

const downloadEnabled = await page.isEnabled('button:has-text("Download SVG")');
console.log('download enabled:', downloadEnabled);
console.log('console errors:', errors.length ? errors : 'none');

if (process.env.SCREENSHOT) {
  await page.screenshot({ path: process.env.SCREENSHOT, fullPage: true });
}
await browser.close();

if (!svgInfo || !downloadEnabled || errors.length) {
  console.error('smoke test FAILED');
  process.exit(1);
}
console.log('smoke test passed');
