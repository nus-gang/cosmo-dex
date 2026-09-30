import { SessionKey, base64, hex, unhex, integer, type Input } from './direct.ts';
export interface Network { chain_id: string; genesis_hash: string; contract_version: string; denom: string; decimals: string; gas_denom: string; observed_height: string }
export interface Account { owner: string; public_key_type: string; public_key_base64: string; account_number: string; sequence: string; epoch: string; bank_atoms: string; exchange_atoms: string; gas_atoms: string; observed_height: string; state: string }
export interface Entry { input: Input; tx_bytes: string; tx_hash: string; state: 'PENDING' | 'SUBMISSION_UNKNOWN' | 'COMMITTED' | 'REJECTED_FINAL'; height?: string }
export class WalletClient {
  readonly keys = [new SessionKey(), new SessionKey()];
  readonly history: Entry[] = [];
  #closed = false;
  genesis = "";
  readonly transport: typeof fetch;
  constructor(transport: typeof fetch = fetch) { this.transport = transport; }
  bindGenesis(hash: string) { unhex(hash); if (this.history.length || this.genesis) throw Error("NETWORK_ALREADY_BOUND"); this.genesis = hash; }
  publicKeys() { return this.keys.map(k => base64.encode(k.publicKey)); }
  close() { this.#closed = true; this.keys.forEach(k => k.destroy()); }
  async #get(path: string) { const r = await this.transport(path, { cache: 'no-store', signal: AbortSignal.timeout(10000) }); if (!r.ok) throw Error(`API_${r.status}`); return r.json(); }
  async account(index: number): Promise<Account> {
    if (this.#closed) throw Error('SESSION_CLOSED');
    if (!this.genesis) throw Error('GENESIS_NOT_PINNED');
    const key = this.keys[index]; if (!key) throw Error('ACCOUNT');
    const network: Network = await this.#get('/s1/network');
    if (network.genesis_hash !== this.genesis || network.chain_id !== 'nus-s1-dev-1' || network.denom !== 'DEVQUOTE' || network.decimals !== '6' || network.gas_denom !== 'DEVGAS') throw Error('NETWORK_MISMATCH');
    const a: Account = await this.#get(`/s1/accounts/${key.owner}`);
    if (a.state !== 'COMMITTED' || a.owner !== key.owner || a.public_key_type !== 'ML-DSA-65' || a.public_key_base64 !== base64.encode(key.publicKey)) throw Error('ACCOUNT_KEY_MISMATCH');
    for (const name of ['account_number', 'sequence', 'epoch', 'observed_height', 'bank_atoms', 'exchange_atoms', 'gas_atoms'] as const) integer(a[name]);
    return a;
  }
  async submit(index: number, operation: Input['operation'], amount: string): Promise<Entry> {
    const key = this.keys[index];
    if (!key || this.history.some(e => e.input.owner === key.owner && ['PENDING', 'SUBMISSION_UNKNOWN'].includes(e.state))) throw Error('미확인 TX를 먼저 조회하세요');
    // Lock before the first await: double clicks cannot sign two transactions.
    const input: Input = { operation, owner: key.owner, amount_atoms: amount, request_id: hex(crypto.getRandomValues(new Uint8Array(32))), expected_epoch: '', expiry_height: '', genesis_hash: this.genesis, chain_id: 'nus-s1-dev-1', account_number: '', sequence: '', gas_limit: '500000', fee_atoms: '1000' };
    const entry: Entry = { input, tx_bytes: '', tx_hash: '', state: 'PENDING' }; this.history.push(entry);
    try {
      const a = await this.account(index);
      if (integer(amount, 1000000000000n) < 1n || integer(amount) > integer(operation === 'DEPOSIT' ? a.bank_atoms : a.exchange_atoms)) throw Error('INSUFFICIENT_BALANCE');
      if (integer(a.gas_atoms) < 1000n) throw Error('INSUFFICIENT_GAS');
      Object.assign(input, { expected_epoch: a.epoch, expiry_height: (integer(a.observed_height) + 100n).toString(), account_number: a.account_number, sequence: a.sequence });
      Object.assign(entry, key.sign(input));
    } catch (error) { this.history.splice(this.history.indexOf(entry), 1); throw error; }
    entry.state = 'SUBMISSION_UNKNOWN';
    try { await this.transport('/s1/txs', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ tx_bytes: entry.tx_bytes }), signal: AbortSignal.timeout(10000) }); } catch { /* Same hash must be queried, never automatically re-sign. */ }
    return entry;
  }
  async resolve(entry: Entry) {
    if (!entry.tx_hash || entry.state === 'PENDING') return;
    try {
      const r = await this.#get(`/s1/txs/${entry.tx_hash}`);
      if (r.tx_hash !== entry.tx_hash) throw Error('HASH_MISMATCH');
      if (['COMMITTED', 'REJECTED_FINAL'].includes(r.state) && integer(r.height) > 0n && integer(r.code) >= 0n) {
        if ((r.state === 'COMMITTED') !== (r.code === '0')) throw Error('RESULT_MISMATCH');
        entry.state = r.state; entry.height = r.height;
      }
    } catch { /* 404/timeout/indexer lag cannot prove failure. */ }
  }
}
