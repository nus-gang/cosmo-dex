import { WalletClient } from './client.ts';
import { atoms, display } from './direct.ts';
const $ = (id: string) => document.getElementById(id)!;
let wallet: WalletClient | undefined;
const status = (s: string) => { $('status').textContent = s; };
function history() { $('history').textContent = wallet?.history.map(e => `${e.input.owner}\n${e.input.operation} ${display(e.input.amount_atoms)} DEVQUOTE · ${e.state === 'SUBMISSION_UNKNOWN' ? '확인 불가' : e.state === 'COMMITTED' ? '확정' : e.state === 'REJECTED_FINAL' ? '실패' : '대기'}\n${e.tx_hash} · 높이 ${e.height ?? '미확인'}`).join('\n\n') ?? ''; }
const accountIndex = () => Number(($('account') as HTMLSelectElement).value);
async function refresh() { if (!wallet) throw Error('먼저 세션을 생성하세요'); const a = await wallet.account(accountIndex()); $('balance').textContent = `주소 ${a.owner}\n지갑 ${display(a.bank_atoms)} DEVQUOTE\n거래소 확정 ${display(a.exchange_atoms)} DEVQUOTE\n가스 ${a.gas_atoms} DEVGAS atoms\nepoch ${a.epoch} · sequence ${a.sequence} · 확정 조회 높이 ${a.observed_height}`; }
function run(f: () => void | Promise<void>) { return async () => { try { await f(); } catch (e) { status(e instanceof Error ? e.message : '오류'); } finally { history(); } }; }
$('create').onclick = run(() => {
  if (wallet) throw Error('기존 세션은 reset 후 생성하세요');
  wallet = new WalletClient();
  $('public').textContent = JSON.stringify(wallet.publicKeys());
  status('두 로컬 키를 생성했습니다. 공개키 배열로 새 개발망 genesis를 준비하세요. 이 탭을 유지하세요.');
});
$('bind').onclick = run(() => { if (!wallet) throw Error('먼저 세션을 생성하세요'); wallet.bindGenesis(($('genesis') as HTMLInputElement).value.trim()); status('genesis 고정 완료'); });
$('refresh').onclick = run(refresh);
$('submit').onclick = run(async () => {
  if (!wallet) throw Error('먼저 세션을 생성하세요');
  const operation = ($('operation') as HTMLSelectElement).value as 'DEPOSIT' | 'WITHDRAW';
  const amount = atoms(($('amount') as HTMLInputElement).value);
  status('대기: 등록 계정과 잔고를 확인합니다');
  const pending = wallet.submit(accountIndex(), operation, amount); history();
  await pending; status('제출 후 확인 불가: TX 조회로 확정 결과를 확인하세요. 가스가 소비될 수 있습니다.');
});
$('resolve').onclick = run(async () => { if (!wallet) return; await Promise.all(wallet.history.map(e => wallet!.resolve(e))); await refresh(); status('TX 조회 완료. 미확인은 새 서명 없이 다시 조회하세요.'); });
$('reset').onclick = run(() => { if (wallet?.history.some(e => ['PENDING', 'SUBMISSION_UNKNOWN'].includes(e.state))) throw Error('미확인 TX가 있습니다. 먼저 결과를 확인하세요'); wallet?.close(); wallet = undefined; $('public').textContent = ''; $('balance').textContent = ''; status('세션 키 폐기. 이전 계정은 복구할 수 없습니다. 새 키에는 새 genesis와 새 home이 필요합니다.'); });
addEventListener('pagehide', () => wallet?.close());
