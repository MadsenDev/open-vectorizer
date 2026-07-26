// Mobile smoke test for the options sheet.
//
// Below Tailwind's `lg` breakpoint the options panel becomes a bottom sheet, and
// the behaviour that matters is not something a unit test can see:
//
//   1. Parked off-screen when closed, and out of the tab order (`inert`), or
//      focus disappears into a panel the user cannot see.
//   2. Opening it scrolls the *result* into view and keeps it fully visible, so
//      you can watch a slider change the thing it changes. This is the whole
//      point of the sheet, and it is easy to break by adjusting heights.
//   3. Sliders inside the sheet still drive the engine.
//   4. Escape closes it.
//   5. At the bottom of the page the floating button covers no content.
//
//   npm run build
//   npx vite preview --port 4180 &
//   npm i --no-save playwright
//   node scripts/smoke-mobile.mjs ../benchmarks/shootout/cases/badge.png
//
// CHROMIUM overrides the browser path, PREVIEW_URL the address, SCREENSHOT_DIR
// captures the open and closed states.

import { chromium, devices } from 'playwright';

const CASE = process.argv[2];
if (!CASE) {
  console.error('usage: node scripts/smoke-mobile.mjs <image>');
  process.exit(2);
}

const browser = await chromium.launch(
  process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {},
);
const context = await browser.newContext({ ...devices['iPhone 13'] });
const page = await context.newPage();

const errors = [];
page.on('console', (m) => {
  if (m.type() === 'error') errors.push(m.text());
});
page.on('pageerror', (e) => errors.push('pageerror: ' + e.message));
page.on('requestfailed', (r) => errors.push('request failed: ' + r.url()));
page.on('response', (r) => {
  if (r.status() >= 400) errors.push(`${r.status()} ${r.url()}`);
});

const url = process.env.PREVIEW_URL ?? 'http://localhost:4180/open-vectorizer/';
await page.goto(url, { waitUntil: 'networkidle' });
console.log('viewport:', JSON.stringify(page.viewportSize()));

const sheetState = async (label) => {
  const state = await page.evaluate(() => {
    const panel = document.getElementById('options-panel');
    const box = panel.getBoundingClientRect();
    return {
      top: Math.round(box.top),
      onScreen: box.top < window.innerHeight - 1,
      inert: panel.inert === true,
    };
  });
  console.log(label, JSON.stringify(state));
  return state;
};

const closed = await sheetState('closed:');

const fab = page.locator('button[aria-controls="options-panel"]');
console.log('button:', (await fab.textContent()).trim());

await page.setInputFiles('input[type=file]', CASE);
await page.waitForSelector('text=Accuracy', { timeout: 30000 });
await page.waitForFunction(
  () => !document.body.innerText.includes('Vectorizing'),
  { timeout: 30000 },
);

await fab.click();
await page.waitForTimeout(600);
const open = await sheetState('open:  ');
const fabHiddenWhileOpen = !(await fab.isVisible());
console.log('button hidden while open:', fabHiddenWhileOpen);

const preview = await page.evaluate(() => {
  const panel = document.getElementById('options-panel');
  const svg = document.querySelector('section svg[viewBox]');
  const sheetBox = panel.getBoundingClientRect();
  const svgBox = svg.getBoundingClientRect();
  return {
    svgTop: Math.round(svgBox.top),
    svgBottom: Math.round(svgBox.bottom),
    sheetTop: Math.round(sheetBox.top),
    fullyVisible: svgBox.top >= 0 && svgBox.bottom <= sheetBox.top,
    hiddenPx: Math.round(Math.max(0, svgBox.bottom - sheetBox.top)),
  };
});
console.log('preview vs sheet:', JSON.stringify(preview));

// A slider inside the sheet must still reach the engine.
await page.locator('#options-panel input[type=range]').nth(1).focus();
for (let i = 0; i < 4; i += 1) await page.keyboard.press('ArrowLeft');
await page.waitForTimeout(1200);
const nodes = await page.evaluate(() => {
  const cells = [...document.querySelectorAll('div.rounded-md')];
  const cell = cells.find((c) =>
    c.textContent.trim().toUpperCase().startsWith('NODES'),
  );
  return cell ? cell.querySelector('div:last-child').textContent.trim() : null;
});
console.log('nodes after slider change:', nodes);

await page.keyboard.press('Escape');
await page.waitForTimeout(600);
const dismissed = await sheetState('escape:');

// Scrolled to the bottom, the floating button must not cover anything.
await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
await page.waitForTimeout(400);
const clearance = await page.evaluate(() => {
  const button = document.querySelector('button[aria-controls="options-panel"]');
  const buttonBox = button.getBoundingClientRect();
  const cells = [
    ...document.querySelectorAll('div.rounded-md.border.border-slate-800'),
  ];
  const worst = cells.reduce((accumulated, cell) => {
    const box = cell.getBoundingClientRect();
    const down = Math.min(box.bottom, buttonBox.bottom) - Math.max(box.top, buttonBox.top);
    const across = Math.min(box.right, buttonBox.right) - Math.max(box.left, buttonBox.left);
    return down > 0 && across > 0
      ? Math.max(accumulated, Math.round(Math.min(down, across)))
      : accumulated;
  }, 0);
  return worst;
});
console.log('content hidden by button at page bottom:', `${clearance}px`);

if (process.env.SCREENSHOT_DIR) {
  await page.screenshot({ path: `${process.env.SCREENSHOT_DIR}/mobile-closed.png` });
  await fab.click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${process.env.SCREENSHOT_DIR}/mobile-open.png` });
}

console.log('console errors:', errors.length ? errors : 'none');
await browser.close();

const passed =
  !closed.onScreen &&
  closed.inert &&
  open.onScreen &&
  !open.inert &&
  fabHiddenWhileOpen &&
  preview.fullyVisible &&
  nodes !== null &&
  !dismissed.onScreen &&
  clearance === 0 &&
  errors.length === 0;

if (!passed) {
  console.error('mobile smoke test FAILED');
  process.exit(1);
}
console.log('mobile smoke test passed');
