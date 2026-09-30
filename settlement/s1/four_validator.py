#!/usr/bin/env python3
"""Bounded REST test against four real validators, optionally already managed."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import threading
import time
import urllib.request

from server import Gateway, Unavailable, serve
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'ops/s1'))
from devnet import cli, init, load, rpc
from integration import eventually


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--home', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=ROOT/'chain/app/bin/nusd')
    p.add_argument('--operators', type=Path, default=ROOT/'chain/app/config/operator-accounts.json')
    p.add_argument('--base-port', type=int, default=29656)
    p.add_argument('--managed', action='store_true')
    a = p.parse_args()
    a.home, a.output = a.home.resolve(), a.output.resolve()
    a.output.mkdir(parents=True, exist_ok=False)
    process = log = http = thread = None
    report = {'status': 'FAIL', 'scope': 'real four-validator RPC + REST HTTP; synthetic users; no browser acceptance'}
    try:
        if not a.managed:
            init(a)
            log = open(a.output/'supervisor.log', 'w')
            process = subprocess.Popen([sys.executable, str(ROOT/'ops/s1/devnet.py'), 'serve', '--home', str(a.home)], stdout=log, stderr=log)
        m = load(a.home)
        report['manifest'] = m
        assert hashlib.sha256(Path(m['binary']).read_bytes()).hexdigest() == m['binary_sha256']
        nodes = m['nodes']
        def consensus():
            h = min(int(rpc(n, '/status')['sync_info']['latest_block_height']) for n in nodes)
            if h < 3:
                return False
            blocks = [rpc(n, f'/block?height={h}') for n in nodes]
            assert len(set(b['block_id']['hash'] for b in blocks)) == 1
            validators = rpc(nodes[0], f'/validators?height={h}')['validators']
            assert len(validators) == 4 and {v['voting_power'] for v in validators} == {'10'}
            return {'height': str(h), 'block_id': blocks[0]['block_id']['hash'], 'validators': validators}
        report['consensus'] = eventually(consensus)
        journal = a.output/'journal.sqlite'
        # Drop a genuine broadcast response after the chain receives it.
        class LoseResponse(Gateway):
            def rpc(self, method, params):
                result = super().rpc(method, params)
                if method == 'broadcast_tx_sync':
                    raise Unavailable('deliberately lost real response')
                return result
        def start(cls=Gateway):
            nonlocal http, thread
            http = serve(cls(nodes[0]['rpc'], m['genesis_sha256'], str(journal)), '127.0.0.1', 0)
            thread = threading.Thread(target=http.serve_forever)
            thread.start()
        def stop():
            nonlocal http
            if http:
                http.shutdown()
                http.server_close()
                thread.join(timeout=5)
                http = None
        def request(path, body=None):
            data = None if body is None else json.dumps(body).encode()
            req = urllib.request.Request(f'http://127.0.0.1:{http.server_port}'+path, data, {'Content-Type': 'application/json'})
            with urllib.request.urlopen(req, timeout=15) as response:
                return response.status, json.load(response)
        start(LoseResponse)
        report['network'] = request('/s1/network')[1]
        gateway = Gateway(nodes[0]['rpc'], m['genesis_sha256'], str(journal))
        accounts = gateway.snapshot()['accounts']
        genesis = json.loads((Path(nodes[0]['home'])/'config/genesis.json').read_text())
        before = [next(a for a in accounts if a['public_key'] == key) for key in genesis['app_state']['public_keys']]
        proof = []
        for user, account in enumerate(before):
            for op, amount in [('deposit', 1000000), ('withdraw', 400000)]:
                rid = hashlib.sha256(f'{a.output}:{user}:{op}'.encode()).hexdigest()
                rawpath = a.output/f'{user}-{op}.raw'
                cli(m['binary'], 'tx', '--rpc', nodes[0]['rpc'], '--user', user, '--op', op,
                    '--amount', amount, '--request-id', rid, '--out', rawpath, '--expiry', '1000000000')
                raw = rawpath.read_bytes()
                digest = hashlib.sha256(raw).hexdigest().upper()
                body = {'tx_bytes': base64.b64encode(raw).decode()}
                status, submitted = request('/s1/txs', body)
                assert status == 202 and submitted['state'] == 'SUBMISSION_UNKNOWN' and submitted['check_tx_code'] is None
                def confirmed():
                    tx = request('/s1/txs/'+digest)[1]
                    return tx if tx['state'] == 'COMMITTED' else False
                tx = eventually(confirmed)
                balance = request('/s1/accounts/'+account['owner'])[1]
                # Same signed bytes, original sequence/hash: never rebuild or resign.
                assert request('/s1/txs', body)[1]['tx_hash'] == digest
                assert request('/s1/txs/'+digest)[1]['state'] == 'COMMITTED'
                after = request('/s1/accounts/'+account['owner'])[1]
                for key in ('bank_atoms', 'exchange_atoms', 'gas_atoms', 'sequence', 'epoch'):
                    assert balance[key] == after[key], (key, balance, after)
                receipt = request('/s1/accounts/'+account['owner']+'/requests/'+rid)[1]
                proof.append({'submit': submitted, 'tx': tx, 'receipt': receipt, 'owner': account['owner'], 'request_id': rid})
            balance = request('/s1/accounts/'+account['owner'])[1]
            assert int(balance['exchange_atoms']) == int(account['exchange_atoms']) + 600000
            assert int(balance['bank_atoms']) == int(account['bank_atoms']) - 600000
            assert int(balance['sequence']) == int(account['sequence']) + 2
        balances = [request('/s1/accounts/'+x['owner'])[1] for x in before]
        stop()
        start()
        def reconciled():
            for account in balances:
                after = request('/s1/accounts/'+account['owner'])[1]
                for key in ('bank_atoms', 'exchange_atoms', 'gas_atoms', 'sequence', 'epoch'):
                    assert after[key] == account[key]
                assert after['cursor_height'] == after['observed_height']
            for item in proof:
                assert request('/s1/txs/'+item['tx']['tx_hash'])[1]['state'] == 'COMMITTED'
                assert request('/s1/accounts/'+item['owner']+'/requests/'+item['request_id'])[1] == item['receipt']
        reconciled()
        stop()
        journal.unlink()
        start()
        reconciled()
        report.update(status='PASS', transactions=proof, balances=balances,
                      checks=['four-validator same-height consensus', 'two users real HTTP deposit/withdraw',
                              'actual broadcast response loss; original hash resolves', 'exact signed bytes replay: no double payout or extra sequence',
                              'REST restart reconciliation', 'empty journal rebuild: chain balances/receipts/TX hashes remain authoritative'])
    except Exception as error:
        report['error'] = repr(error)
        raise
    finally:
        if http:
            http.shutdown()
            http.server_close()
            thread.join(timeout=5)
        if process:
            process.terminate()
            try:
                process.wait(timeout=40)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if log:
            log.close()
        (a.output/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({'status': report['status'], 'checks': report['checks']}, indent=2))


if __name__ == '__main__':
    main()
