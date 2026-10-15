import { TabEntry } from './entry.ts';
import { context, type Context } from '../../../web/s3/state.ts';

// Runtime supplies this public context only after its independent approval gate.
// Parsing this document does not grant runtime approval.
export function parseContext(raw: string): Context {
  if (new TextEncoder().encode(raw).length > 16384) throw Error('CONTEXT_SIZE');
  const value: unknown = JSON.parse(raw);
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw Error('CONTEXT_FORMAT');
  const pairs = Object.entries(value);
  const fields=['chain_id','config_hash','contract_hash','genesis_hash','market_config_version','market_id','service_schema'];
  if (pairs.map(([k])=>k).sort().join(',') !== fields.join(',') || pairs.some(([k,v]) =>
    !/^[a-z][a-z0-9_]{0,63}$/.test(k) || ['constructor','prototype','__proto__'].includes(k) ||
    typeof v !== 'string' || v.length < 1 || v.length > 256)) throw Error('CONTEXT_FORMAT');
  const ctx = Object.freeze({...value}) as Context;
  context(ctx, ctx);
  if (ctx.market_id!=='DEVBASE/DEVQUOTE'||ctx.market_config_version!=='1'||['genesis_hash','contract_hash','config_hash'].some(k=>!/^[0-9a-f]{64}$/.test(ctx[k]))) throw Error('CONTEXT_FORMAT');
  return ctx;
}

export function attachPage(doc: Document, transport: typeof fetch) {
  const get = <T extends HTMLElement>(id: string) => {
    const node=doc.getElementById(id); if (!node) throw Error('PAGE_STRUCTURE'); return node as T;
  };
  const enabled=get<HTMLInputElement>('enabled'), ack=get<HTMLInputElement>('acknowledge');
  const prepare=get<HTMLButtonElement>('prepare'), activate=get<HTMLButtonElement>('activate');
  const registration=get<HTMLElement>('registration'), status=get<HTMLElement>('status'), root=get<HTMLElement>('wallet');
  const win=doc.defaultView; if (!win) throw Error('DOCUMENT_REQUIRED');
  let tab: TabEntry | undefined, busy=false, closed=false;
  const destroy=()=>{if(closed)return;closed=true;tab?.destroy();registration.textContent='';activate.disabled=true;prepare.disabled=true;};
  prepare.addEventListener('click',()=>{
    if(closed||tab)return;
    try {
      tab=new TabEntry(win.location.origin,enabled.checked,ack.checked);
      registration.textContent=JSON.stringify(tab.registrations(),null,2);
      prepare.disabled=true;enabled.disabled=true;ack.disabled=true;activate.disabled=false;
      status.textContent='공개 등록 데이터를 초기화 담당자에게 전달하세요. 이 탭을 유지하세요.';
    } catch {status.textContent='로컬 주소와 두 동의 항목을 확인하세요.';}
  });
  activate.addEventListener('click',async()=>{
    if(closed||!tab||busy||activate.disabled)return;
    busy=true;activate.disabled=true;
    const controller=new AbortController();const timeout=setTimeout(()=>controller.abort(),2000);
    const cancel=()=>controller.abort();win.addEventListener('pagehide',cancel,{once:true});
    try {
      const response=await transport('/runtime-context.json',{method:'GET',credentials:'omit',cache:'no-store',redirect:'error',signal:controller.signal});
      if(!response.ok||response.headers.get('content-type')!=='application/json'||!response.body)throw Error('CONTEXT_RESPONSE');
      const reader=response.body.getReader();const chunks:Uint8Array[]=[];let size=0;
      try {for(;;){const {value,done}=await reader.read();if(done)break;size+=value.length;if(size>16384)throw Error('CONTEXT_SIZE');chunks.push(value);}}
      finally {await reader.cancel();reader.releaseLock();}
      const bytes=new Uint8Array(size);let pos=0;for(const chunk of chunks){bytes.set(chunk,pos);pos+=chunk.length;}
      const ctx=parseContext(new TextDecoder('utf-8',{fatal:true}).decode(bytes));
      if(closed||controller.signal.aborted)throw Error('TAB_CLOSED');
      tab.activate(root,ctx,ctx,transport);
      status.textContent='연결 준비 완료. 인증·조회는 아래 버튼으로 시작하세요.';
    } catch {if(!closed){status.textContent='승인된 로컬 Context를 읽지 못했습니다. 초기화·기동 상태를 확인하세요.';activate.disabled=false;}}
    finally {clearTimeout(timeout);win.removeEventListener('pagehide',cancel);busy=false;}
  });
  win.addEventListener('pagehide',destroy,{once:true});
  return {destroy};
}
