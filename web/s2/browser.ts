import { TradingClient } from './client.ts';
import { TradingKey, base64 } from './session.ts';
const $ = (id: string) => document.getElementById(id)!;
const value = (id: string) => ($(id) as HTMLInputElement).value;
let keys: TradingKey[] = [], client: TradingClient | undefined, busy = false;
const status = (message: string) => { $('status').textContent = message; };
function action(f: () => void | Promise<void>) { return async () => { try { await f(); } catch (e) { status(e instanceof Error ? e.message : '오류'); } finally { render(); } }; }
function button(label: string, f: () => Promise<void>) { const b=document.createElement('button'); b.textContent=label; b.onclick=action(f); return b; }
function render() {
  const c=client, v=c?.views.view;
  ($('order') as HTMLButtonElement).disabled = !c?.authenticated || !c.views.open(Date.now());
  $('ledger').replaceChildren(); $('orders').replaceChildren(); $('receipts').replaceChildren();
  if (!c) return;
  $('freshness').textContent = `${c.views.open(Date.now()) ? '접수 가능' : '접수 닫힘 · 마지막 관측값'} · ${c.views.reason} · 높이 ${v?.observed_height ?? '미확인'} · seq ${v?.stream_seq ?? '미확인'} · 지연 ${v?.status.observation.query_latency_ms ?? '?'}ms`;
  for (const row of v?.ledger ?? []) { const tr=document.createElement('tr'); for (const k of ['denom','C','R','D','P','A'] as const) { const td=document.createElement('td'); td.textContent=row[k]; tr.append(td); } $('ledger').append(tr); }
  const book=c.views.book;
  $('book').textContent=book ? `seq ${book.stream_seq}\n매도 (ticks / lots)\n${book.asks.map(l=>`${l.price_ticks} / ${l.qty_lots}`).join('\n')}\n매수 (ticks / lots)\n${book.bids.map(l=>`${l.price_ticks} / ${l.qty_lots}`).join('\n')}` : '조회 전';
  for (const o of v?.orders ?? []) { const p=document.createElement('p'); p.textContent=`${o.order_id} · ${o.side} ${o.order_type} · ${o.state} · 원 수량 ${o.max_qty_lots} / 잔량 ${o.remaining_qty_lots} / 체결 ${o.filled_qty_lots} / 정정 ${o.corrected_qty_lots} lots · seq ${o.admission_seq} · revision ${o.revision}`; if (['OPEN','PARTIALLY_FILLED'].includes(o.state)) p.append(button('잔량 취소',async()=>{await c.cancel(o.order_id);})); $('orders').append(p); }
  $('fills').textContent=v?.fills.map(f=>`${f.fill_id} · ${f.state==='PENDING'?'잠정':'정정'} · ${f.quantity_lots} lots @ ${f.execution_price_ticks} ticks · ${f.reason} · seq ${f.command_seq} · revision ${f.revision}`).join('\n') ?? '';
  for (const e of c.submissions.filter(e=>e.owner===c.key.owner)) { const p=document.createElement('p'); p.textContent=`${e.kind} ${e.id} · ${e.state} · ${e.receipt?.code ?? '결과 미확인'}`; if (e.state==='SUBMISSION_UNKNOWN') { p.append(button('영수증 조회',()=>c.resolve(e))); p.append(button('조회 후 동일 원문 재시도',()=>c.retry(e))); } $('receipts').append(p); }
}
$('create').onclick=action(()=>{if(keys.length) throw Error('기존 탭 키를 유지하세요'); keys=[new TradingKey(),new TradingKey()]; $('public').textContent=JSON.stringify(keys.map(k=>base64.encode(k.publicKey))); status('공개키로 새 S2 genesis를 준비한 후 hash를 고정하세요.');});
$('bind').onclick=action(()=>{if(client||keys.length!==2)throw Error('두 계정 생성 후 한 번만 고정할 수 있습니다'); client=new TradingClient(value('genesis').trim(),location.origin,(path,init)=>fetch(new URL(String(path),'http://127.0.0.1:8788'),init),keys); client.select(Number(value('account'))); status('genesis 고정 완료');});
$('account').onchange=action(()=>{client?.select(Number(value('account'))); status('계정 전환 · 이전 세션 폐기 · 다시 로그인하세요');});
$('login').onclick=action(async()=>{if(!client)throw Error('먼저 genesis를 고정하세요');await client.login();await client.refresh();status('계정 인증·조회 완료');});
$('order').onclick=action(async()=>{if(!client)throw Error('연결 전');await client.order(value('side') as 'BUY'|'SELL',value('tif') as 'GTC'|'IOC',value('qty'),value('price'));});
setInterval(()=>{render();if(busy||!client?.authenticated)return;busy=true;void client.refresh().catch(e=>status(e instanceof Error?e.message:'연결 단절')).finally(()=>{busy=false;render();});},1000);
addEventListener('pagehide',()=>{client?.close();keys.forEach(k=>k.destroy());});
