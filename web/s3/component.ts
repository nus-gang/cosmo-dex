import { LocalClient } from './client.ts';
import { ASSURANCE, PUBLIC_RECEIPT_SCHEMA_SHA256, PUBLIC_RECEIPT_VERSION, TRUSTED_RECEIPT_VERSION } from './state.ts';
import { display, atoms } from './direct-codec.ts';
import type { LocalKey } from './key.ts';
export function screen(client: LocalClient) {
  const v=client.projection.view;
  return {
    notice:`개발 전용 · 공개 ${PUBLIC_RECEIPT_VERSION} (${PUBLIC_RECEIPT_SCHEMA_SHA256.slice(0,12)}…) / trusted ${TRUSTED_RECEIPT_VERSION} 분리 · ${ASSURANCE} · G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED`,
    owner:client.projection.owner,
    status:client.projection.open()?(client.canWithdraw()?'조회 정상 · 직접 출금 준비 완료':'조회 정상 · 출금 보류'):'보류 / '+client.projection.reason,
    revision:v?.revision??'—',
    ledger:v?.ledger.map(r=>[r.denom,...[r.C,r.R,r.D,r.P,r.A].map(display)])??[],
    orders:v?.orders.map(o=>`${o.order_id} · ${o.side} · ${o.state} · 잔량 lots ${o.remaining_qty_lots}`)??[],
    fills:v?.fills.map(f=>`${f.fill_id} · ${{PENDING:'잠정',SUBMISSION_UNKNOWN:'제출 결과 불명',COMMITTED:'체인 확정',CORRECTED:'정정'}[f.state as string]} · revision ${f.revision}`)??[],
    batches:v?.batches.map(b=>`${b.batch.batch_id} · seq ${b.batch.batch_seq} · hash ${b.batch.batch_hash} · ${b.state} · H ${b.receipt?.terminal_height??'—'} · TX ${b.receipt?.terminal_tx_hash??'—'}`)??[],
    history:client.history.filter(e=>e.owner===client.projection.owner),
    disabled:!client.canWithdraw(), receipt:client.receipt,receiptConflicts:client.publicReceipts.conflicts.length+client.publicReceipts.queryMismatches.length,
  };
}
// Mount is inert until the launcher explicitly passes both opt-ins and pinned Context.
// Keys are caller-owned tab memory; page teardown destroys them. No secret inputs/exports.
export function mount(root: HTMLElement, client: LocalClient, keys: LocalKey[], origin: string) {
  const d=root.ownerDocument;
  const node=(tag:string,text='')=>{const e=d.createElement(tag);e.textContent=text;return e;};
  const title=node('h1','로컬 정산·사용자 출금'),notice=node('p'),status=node('p'),identity=node('p'),receipt=node('p');
  status.setAttribute('role','status');
  const select=d.createElement('select');select.setAttribute('aria-label','계정');
  for(const [i,key] of keys.entries()){const option=d.createElement('option');option.value=String(i);option.textContent=key.address;select.append(option);}
  const table=node('table'),head=node('tr');
  for(const label of ['자산','확정 C','예약 R','정산 보류 D','잠정 수취 P (사용 불가)','가용 A=C−R−D'])head.append(node('th',label));
  const rows=node('tbody');table.append(head,rows);
  const orders=node('div'),fills=node('div'),batches=node('div'),history=node('div');
  const asset=d.createElement('select');asset.setAttribute('aria-label','출금 자산');
  for(const name of ['DEVBASE','DEVQUOTE']){const o=d.createElement('option');o.value=name;o.textContent=name;asset.append(o);}
  const amount=d.createElement('input');amount.type='text';amount.inputMode='decimal';amount.value='1';amount.setAttribute('aria-label','출금 금액');
  let disposed=false;
  const render=()=>{
    if(disposed)return;const s=screen(client);notice.textContent=s.notice;status.textContent=s.status;identity.textContent=`계정 ${s.owner} · revision ${s.revision}`;receipt.textContent=s.receipt;
    rows.replaceChildren(...s.ledger.map(r=>{const tr=node('tr');for(const c of r)tr.append(node('td',c));return tr;}));
    orders.replaceChildren(...s.orders.map(o=>node('p',o)));
    fills.replaceChildren(...s.fills.map(f=>node('p',f)));batches.replaceChildren(...s.batches.map(b=>node('p',b)));
    history.replaceChildren(...s.history.map(e=>node('p',`${e.tx_hash} · ${e.state} · ${e.height??'확정 높이 없음'}`)));
    withdraw.disabled=s.disabled;
  };
  const button=(text:string,action:()=>Promise<unknown>)=>{const b=d.createElement('button');b.textContent=text;b.addEventListener('click',()=>{const g=client.projection.generation;const pending=action();render();void pending.catch(()=>{if(g===client.projection.generation)status.textContent='요청 실패 또는 보류 — 결과 조회 필요';}).finally(render);});return b;};
  const login=button('인증·조회',()=>client.login(origin)),refresh=button('상태 재조회',()=>client.refresh());
  const prepare=button('출금 준비 (주문 동결·잔량 취소)',()=>client.prepare());
  const abort=button('출금 준비 명시적 해제',()=>client.prepare(true));
  const withdraw=button('직접 서명하여 출금',async()=>client.withdraw(asset.value as 'DEVBASE'|'DEVQUOTE',atoms(amount.value)));
  const resolve=button('출금 결과 조회',async()=>{for(const e of client.history.filter(e=>e.owner===client.projection.owner))await client.resolve(e);await client.refresh();});
  select.addEventListener('change',()=>{client.select(keys[Number(select.value)]);render();});
  root.replaceChildren(title,notice,select,login,refresh,status,identity,table,node('h2','주문·체결·배치'),orders,fills,batches,receipt,node('p','D/P 또는 결과 불명이 남으면 출금 보류. 개발 접수는 확정 자산이 아닙니다.'),prepare,abort,asset,amount,withdraw,resolve,history);
  client.select(keys[0]);render();
  const timer=setInterval(render,250);
  const destroy=()=>{disposed=true;clearInterval(timer);keys.forEach(k=>k.destroy());client.destroy();root.replaceChildren();};
  d.defaultView?.addEventListener('pagehide',destroy,{once:true});
  return {render,destroy};
}
