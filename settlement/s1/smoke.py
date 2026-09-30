#!/usr/bin/env python3
"""Bounded real single-node + HTTP integration test; always stops its processes."""
import argparse
import base64
import hashlib
import json
import pathlib
import subprocess
import threading
import time
import urllib.request
from server import Gateway, serve

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--operators', required=True)
p.add_argument('--output', required=True)
a = p.parse_args()
out = pathlib.Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=False)
binary = str(pathlib.Path(a.binary).resolve())
rpc = 'http://127.0.0.1:29757'
home = out / 'node'

def cli(*args):
    r = subprocess.run([binary, *args], capture_output=True, text=True, timeout=30)
    if r.returncode:
        raise RuntimeError(r.stderr + r.stdout)
    return r.stdout

init = json.loads(cli('init', '--home', str(home), '--rpc', 'tcp://127.0.0.1:29757',
    '--p2p', 'tcp://127.0.0.1:29756', '--operator-accounts', str(pathlib.Path(a.operators).resolve())))
gh = init['genesis_hash']
node = http = thread = log = None

def start_node():
    global node, log
    log = open(out / 'node.log', 'a')
    node = subprocess.Popen([binary, 'start', '--home', str(home), '--genesis-hash', gh], stdout=log, stderr=log)
    for _ in range(100):
        if node.poll() is not None:
            raise RuntimeError('node exited; inspect node.log')
        try:
            return Gateway(rpc, gh, str(out / 'journal.db')).snapshot()
        except Exception:
            time.sleep(.2)
    raise RuntimeError('node readiness timeout')

def stop_node():
    global node
    if node and node.poll() is None:
        node.terminate()
        node.wait(timeout=20)
    if log:
        log.close()

def start_http():
    global http, thread
    http = serve(Gateway(rpc, gh, str(out / 'journal.db')), '127.0.0.1', 0)
    thread = threading.Thread(target=http.serve_forever)
    thread.start()

def stop_http():
    if http:
        http.shutdown()
        http.server_close()
        thread.join(timeout=5)

def request(path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request('http://127.0.0.1:' + str(http.server_port) + path,
                                  data, {'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=15) as r:
        return r.status, json.load(r)

proof = []
try:
    initial = start_node()
    start_http()
    assert request('/s1/network')[1]['genesis_hash'] == gh
    # Resolve each CLI signer from its actual receipt after signing/broadcast.
    for user in range(2):
        for op, amount, rid in [('deposit', '100000000', 1), ('withdraw', '40000000', 2)]:
            rawpath = out / f'{user}-{op}.raw'
            cli('tx', '--rpc', rpc, '--user', str(user), '--op', op, '--amount', amount,
                '--request-id', f'{rid:064x}', '--out', str(rawpath), '--expiry', '1000000')
            raw = rawpath.read_bytes()
            body = {'tx_bytes': base64.b64encode(raw).decode()}
            status, submitted = request('/s1/txs', body)
            assert status == 202 and submitted['state'] == 'SUBMISSION_UNKNOWN'
            digest = hashlib.sha256(raw).hexdigest().upper()
            for _ in range(60):
                _, tx = request('/s1/txs/' + digest)
                if tx['state'] != 'SUBMISSION_UNKNOWN':
                    break
                time.sleep(.2)
            assert tx['state'] == 'COMMITTED', tx
            # Response-loss recovery: resubmit exactly the same bytes and query original hash.
            assert request('/s1/txs', body)[1]['state'] == 'SUBMISSION_UNKNOWN'
            assert request('/s1/txs/' + digest)[1]['state'] == 'COMMITTED'
            proof.append(tx)
    accounts = [request('/s1/accounts/' + x['owner'])[1] for x in initial['accounts']]
    for account in accounts:
        assert account['exchange_atoms'] == '60000000' and account['epoch'] == '1'
    receipts = [request('/s1/accounts/' + x['owner'] + '/requests/' + f'{2:064x}')[1] for x in accounts]
    stop_http()
    stop_node()
    start_node()
    start_http()
    for account, receipt in zip(accounts, receipts):
        after = request('/s1/accounts/' + account['owner'])[1]
        for key in ('bank_atoms', 'exchange_atoms', 'gas_atoms', 'sequence', 'epoch'):
            assert after[key] == account[key], (key, after, account)
        assert request('/s1/accounts/' + account['owner'] + '/requests/' + f'{2:064x}')[1] == receipt
    # Rebuild local journal from nothing; authority and hash queries are unchanged.
    stop_http()
    (out / 'journal.db').unlink()
    start_http()
    assert request('/s1/txs/' + proof[0]['tx_hash'])[1]['state'] == 'COMMITTED'
    assert request('/s1/accounts/' + accounts[0]['owner'])[1]['exchange_atoms'] == '60000000'
    result = dict(status='PASS', scope='real single validator + HTTP, not four-validator/browser acceptance',
        genesis_sha256=gh, binary_sha256=hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
        binary_version=json.loads(cli('version')), transactions=proof, receipts=receipts,
        checks=['two users deposit 100 withdraw 40 balance 60', 'exact-byte duplicate no double payout',
                'chain and REST restart preserve ledger and receipts', 'empty journal rebuild from chain'])
    (out / 'result.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
finally:
    stop_http()
    stop_node()
