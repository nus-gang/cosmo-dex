import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const service = spawn(process.execPath, ['s1/serve.mjs'], { env: { ...process.env, S1_PORT: '18080' }, stdio: ['ignore', 'pipe', 'inherit'] });
let browser;
try {
  await new Promise((resolve, reject) => { service.stdout.once('data', resolve); service.once('error', reject); service.once('exit', code => reject(Error(`server ${code}`))); });
  browser = await chromium.launch({ executablePath: process.env.CHROME_BIN ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
  const page = await browser.newPage(); const errors = []; const requests = [];
  page.on('pageerror', e => errors.push(e.message)); page.on('request', r => requests.push({ method: r.method(), path: new URL(r.url()).pathname }));
  await page.goto('http://127.0.0.1:18080'); await page.click('#create');
  await page.waitForFunction(() => document.querySelector('#public').textContent.startsWith('['));
  const keys = JSON.parse(await page.locator('#public').textContent()); assert.equal(keys.length, 2); assert.notEqual(keys[0], keys[1]); assert.equal(Buffer.from(keys[0], 'base64').length, 1952);
  await page.fill('#genesis', '11'.repeat(32)); await page.click('#bind'); assert.match(await page.locator('#status').textContent(), /고정 완료/);
  await page.click('#reset'); assert.equal(await page.locator('#public').textContent(), '');
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.deepEqual(errors, []); assert.ok(!requests.some(r => r.path.startsWith('/s1/')));
  await mkdir('s1/evidence', { recursive: true });
  await page.screenshot({ path: 's1/evidence/browser-preparation.png', fullPage: true });
  const evidence = { browser: browser.version(), checks: ['two random public keys', 'genesis pin', 'reset', '390px layout', 'no API calls during key preparation'], pageErrors: errors, requests, actualChain: 'NOT_RUN', actualRest: 'NOT_CONNECTED' };
  await writeFile('s1/evidence/browser-preparation.json', JSON.stringify(evidence, null, 2) + '\n'); console.log(JSON.stringify(evidence));
} finally { await browser?.close(); service.kill('SIGTERM'); }
