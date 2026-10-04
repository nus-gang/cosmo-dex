// Transport only: QA module owns all page interactions and assertions.
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
import {createInterface} from 'node:readline';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
const [product, driver, output] = process.argv.slice(2);
const require = createRequire(path.join(product, 'web/package.json'));
const {chromium} = require('playwright-core');
const lines = createInterface({input: process.stdin});
const responses = lines[Symbol.asyncIterator]();
console.log = (...args) => console.error(...args);
let serial = 0;
async function control(op, args = {}) {
  const id = ++serial;
  process.stdout.write(JSON.stringify({id, op, ...args})+'\n');
  const next = await responses.next();
  if (next.done) throw Error('controller disconnected');
  const reply = JSON.parse(next.value);
  if (reply.id !== id || reply.error) throw Error(reply.error || 'RPC id mismatch');
  return reply.result;
}
let browser;
const report = {result: 'FAIL', scope: 'environment only; document QA NOT_RUN'};
try {
  browser = await chromium.launch({headless: true});
  report.chromium = browser.version();
  const context = await browser.newContext();
  const page = await context.newPage();
  page.setDefaultTimeout(30000);
  const http = [];
  page.on('response', response => {
    const url = new URL(response.url());
    if (['5173','8788'].includes(url.port)) http.push({origin:url.origin, path:url.pathname,
      status:response.status(), allowOrigin:response.headers()['access-control-allow-origin'] ?? null});
  });
  try {
    const module = await import(pathToFileURL(driver));
    report.driverResult = await module.default({page, control, output});
    report.result = 'PASS';
  } finally {
    await writeFile(path.join(output,'http.json'), JSON.stringify(http,null,2)+'\n');
  }
} catch (error) {
  report.error = String(error);
  process.exitCode = 1;
} finally {
  try { await browser?.close(); } catch (error) { report.closeError=String(error); process.exitCode=1; }
  await writeFile(path.join(output,'browser.json'), JSON.stringify(report,null,2)+'\n');
  lines.close();
}
