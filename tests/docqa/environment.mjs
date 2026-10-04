// SRE environment acceptance only. QA replaces/reviews this module independently.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
export default async function ({page, control, output}) {
  await control('temporary_start');
  await page.goto('http://127.0.0.1:5173');
  let navigations=0;
  page.on('framenavigated', frame => { if (frame===page.mainFrame()) navigations++; });
  await page.locator('#create').click();
  const publicKeys=JSON.parse(await page.locator('#public').textContent());
  assert.equal(publicKeys.length,2);
  await writeFile(path.join(output,'public-keys.json'),JSON.stringify(publicKeys,null,2)+'\n');
  const pins=await control('init',{publicKeys});
  await control('temporary_stop');
  const first=await control('start');
  await page.locator('#genesis').fill(pins.chain_genesis);
  await page.locator('#bind').click();
  async function login(account) {
    await page.locator('#account').selectOption(String(account));
    await page.locator('#login').click();
    await page.waitForFunction(()=>document.querySelector('#status').textContent==='계정 인증·조회 완료');
    await page.locator('#chain-account').click();
    await page.waitForFunction(()=>document.querySelector('#chain-balances').textContent.includes('DEVGAS'));
  }
  await login(0);
  await login(1);
  await page.screenshot({path:path.join(output,'first.png')});
  await control('stop');
  const restarted=await control('start');
  await login(0);
  await login(1);
  assert.deepEqual(JSON.parse(await page.locator('#public').textContent()),publicKeys);
  assert.equal(await page.locator('#genesis').inputValue(),pins.chain_genesis);
  assert.equal(navigations,0);
  await page.screenshot({path:path.join(output,'restarted.png')});
  await control('stop');
  return {scope:'ENVIRONMENT_ONLY', documentQA:'NOT_RUN', samePage:true, navigationsAfterKeyCreation:navigations,
    first,restarted, genesis:pins.chain_genesis, twoAccountsSignedLogin:true};
}
