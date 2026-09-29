"""rc4 full output specification; injected authentication, no product crypto."""
import json
import runpy
from pathlib import Path
r = Path(__file__).resolve().parents[1]
rc3 = runpy.run_path(str(r / 'tools/check-decision-port.py'))

def decide(a):
    auth = a['authentication_result']
    s = a.get('snapshot')
    out = {'authentication': auth.copy(), 'snapshot_policy': {
        'status': 'NOT_RUN', 'code': None, 'source': 'SYNTHETIC',
        'snapshot_id': s.get('id') if isinstance(s, dict) else None},
        'ack': 'NOT_CONNECTED', 'wal_replay': 'NOT_RUN', 'ledger': 'NOT_CONNECTED'}
    if auth['status'] != 'PASS':
        return out
    p = out['snapshot_policy']
    p['status'] = 'NOT_CONNECTED'
    if not isinstance(s, dict) or not rc3['required'] <= s.keys():
        return out
    if any(s[k] is None for k in rc3['required']):
        return out
    if not isinstance(s['id'], str) or not s['id'] or s['source'] != 'SYNTHETIC':
        return out
    if s['id_state'] not in ('NEW', 'CONFLICT') or not all(
        type(s[k]) is bool for k in ('epoch_matches', 'revoked', 'cumulative_ok', 'confirmed_balance_ok')):
        return out
    ctx, order = a.get('context') or {}, a['authenticated_order']
    if any(ctx.get(k) is None for k in ('snapshot_id', 'height', 'epoch')) or not ctx['snapshot_id']:
        return out
    # Binding failure must not erase the original observed ID or become policy PASS.
    if s['id'] != ctx['snapshot_id'] or s['height'] != ctx['height']:
        return out
    if any(s[k] != order[v] for k, v in (
        ('q', 'max_qty_lots'), ('p', 'limit_price_ticks'),
        ('cap', 'max_fee_bps'), ('expiry_height', 'expiry_height'))):
        return out
    if s['epoch_matches'] != (order['owner_epoch'] == ctx['epoch']):
        return out
    code = rc3['outcome'](rc3['policy'], s)
    p.update(status='PASS' if code == 'OK' else 'REJECTED', code=code)
    return out

v = json.loads((r / 'vectors/snapshot-output.json').read_text())
assert len({c['id'] for c in v['cases']}) == len(v['cases'])
for c in v['cases']:
    actual = decide(c['input'])
    assert actual == c['expected'], (c['id'], actual, c['expected'])
    assert actual['ack'] == actual['ledger'] == 'NOT_CONNECTED'
    assert actual['wal_replay'] == 'NOT_RUN'
print(f"PASS rc4 snapshot full outputs: {len(v['cases'])}; product crypto/differential NOT_RUN")
