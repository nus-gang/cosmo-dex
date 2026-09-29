import { chromium } from 'playwright-core';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { writeFile, mkdir } from 'node:fs/promises';
const executablePath = process.env.CHROME_BIN || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1000, height: 850 } });
  const errors = [], requests = []; page.on('pageerror', e => errors.push(e.message));
  page.on('request', r => { if (/^https?:/.test(r.url())) requests.push(r.url()); });
  await page.goto(pathToFileURL(resolve('dist/demo.html')).href);
  await page.waitForFunction(() => globalThis.conformance);
  await page.click('#create'); await page.waitForFunction(() => document.querySelector('#status').textContent.includes('nus1'));
  await page.click('#sign'); await page.waitForFunction(() => document.querySelector('#status').textContent.includes('3309'));
  await page.click('#recover'); await page.waitForFunction(() => document.querySelector('#status').textContent.includes('복구 검증 성공'));
  const suite = await page.evaluate(() => globalThis.conformance);
  await mkdir('evidence', { recursive: true });
  await page.screenshot({ path: 'evidence/browser.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  const evidence = { runtime: 'real headless Chrome', browser: browser.version(), node: process.version, suite, uiChecks: ['key generation', 'OrderV1 signature', 'memory recovery', '390px no overflow'], pageErrors: errors, externalRequests: requests.length, overflow, restWs: 'NOT_CONNECTED', chainTx: 'NOT_CONNECTED' };
  await writeFile('evidence/browser.json', JSON.stringify(evidence, null, 2) + '\n');
  if (errors.length || requests.length || overflow) throw new Error('Browser boundary failure');
  console.log(JSON.stringify({ ...evidence, suite: { passed: suite.passed } }));
} finally { await browser.close(); }
