import { build } from 'esbuild';
import { chromium } from 'playwright-core';
import { mkdir,writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const out=process.argv[2];if(!out)throw Error('evidence directory required');await mkdir(out,{recursive:true});
const code=await build({entryPoints:['s3/browser-fixture.ts'],bundle:true,write:false,platform:'browser',format:'iife',target:'es2022'});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--disable-background-networking']});
try {
  const page=await browser.newPage();await page.route('**/*',r=>r.abort());
  await page.setContent('<html lang="ko"><meta charset="utf-8"><style>body{font:16px system-ui;background:#101522;color:#eef2ff;padding:24px}button,select,input{padding:10px;margin:8px}td,th{padding:12px;border-bottom:1px solid #667}</style><p>COMPONENT SYNTHETIC FIXTURE — 실제 서비스·자산 아님</p><main id="app"></main></html>');
  await page.addScriptTag({content:code.outputFiles[0].text});
  const withdraw=page.getByRole('button',{name:'직접 서명하여 출금'}),login=page.getByRole('button',{name:'인증·조회'});
  assert.equal(await withdraw.isDisabled(),true);
  await login.click();await page.waitForFunction(()=>fixture.client.canWithdraw());
  assert.match(await page.locator('body').textContent(),/durable_ack=false/);
  assert.match(await page.locator('body').textContent(),/공개 s3-dev-local-account\/1 .* trusted s3-dev-local\/1 분리/);
  await page.getByRole('button',{name:'출금 준비 (주문 동결·잔량 취소)'}).click();await page.waitForFunction(()=>fixture.client.receipt.includes('공개 계정 영수증'));
  assert.match(await page.locator('body').textContent(),/LOCAL_ACCEPTED .* 체인 COMMITTED·durable ACK 아님/);
  assert.deepEqual(await page.evaluate(()=>fixture.receiptMismatch()),{error:'CLIENT_RECEIPT_MISMATCH',canWithdraw:false,queryMismatches:1});
  assert.equal(await withdraw.isDisabled(),true);assert.match(await page.getByRole('status').textContent(),/CLIENT_RECEIPT_MISMATCH/);assert.equal(await page.evaluate(()=>fixture.posts()),0);
  await page.screenshot({path:out+'/receipt-mismatch-held.png',fullPage:true});await login.click();await page.waitForFunction(()=>fixture.client.canWithdraw());
  for(const fault of ['recovery','503','disconnect','late-recovery','late-abort','late-DP']) {
    await page.evaluate(fault=>fixture.reorder(fault),fault);
    assert.equal(await withdraw.isDisabled(),true);
    assert.match(await page.getByRole('status').textContent(),/보류/);
    await withdraw.evaluate(button=>button.click());
    assert.equal(await page.evaluate(()=>fixture.posts()),0);
    assert.equal(await page.evaluate(()=>fixture.client.history.length),0);
    if(fault==='late-abort'||fault==='late-DP')await page.screenshot({path:out+'/'+fault+'-held.png',fullPage:true});
    if(fault==='recovery')await page.screenshot({path:out+'/delayed-open-held.png',fullPage:true});
    await page.getByRole('button',{name:'상태 재조회'}).click();
    await page.waitForFunction(()=>fixture.client.canWithdraw());
  }
  await withdraw.click();await page.waitForFunction(()=>fixture.posts()===1);
  assert.equal(await withdraw.isDisabled(),true);
  await page.getByRole('button',{name:'출금 결과 조회'}).click();
  assert.equal(await page.evaluate(()=>fixture.posts()),1);
  assert.deepEqual(await page.evaluate(()=>fixture.chainRoutes),['/dev-local/v1/chain/account','/dev-local/v1/chain/broadcast','/dev-local/v1/chain/result']);
  await page.getByLabel('계정',{exact:true}).selectOption('1');
  assert.equal(await page.locator('tbody tr').count(),0);assert.equal(await withdraw.isDisabled(),true);
  assert.doesNotMatch(await page.locator('body').textContent(),/SUBMISSION_UNKNOWN/);
  await page.evaluate(()=>fixture.held());await login.click();await page.waitForFunction(()=>fixture.client.projection.view!==undefined);
  assert.equal(await withdraw.isDisabled(),true);assert.match(await page.locator('body').textContent(),/제출 결과 불명/);
  await page.screenshot({path:out+'/component.png',fullPage:true});
  await writeFile(out+'/browser.json',JSON.stringify({scope:'BROWSER_COMPONENT_SYNTHETIC_NO_SERVICE',browser:browser.version(),checks:['initial-disabled','guarantee-label','public-trusted-version-split','public-receipt-not-committed','receipt-query-mismatch-held','direct-click-one-TX','unknown-query-no-retry','account-switch-empty','DP-held','recovery-delayed-OPEN-disabled','503-delayed-OPEN-disabled','disconnect-delayed-OPEN-disabled','earlier-request-latest-recovery-disabled','OPEN-abort-held','OPEN-DP-held','authenticated-chain-three-routes'],pass:16,fail:0,DEV12:'NOT_RUN'},null,2)+'\n');
  await page.evaluate(()=>fixture.component.destroy());assert.equal(await page.locator('#app').textContent(),'');
  console.log('browser component: 16 PASS / 0 FAIL; DEV12 NOT_RUN');
}finally{await browser.close();}
