#!/usr/bin/env python3
"""Bounded real consensus test. --managed uses an existing managed supervisor."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from devnet import ROOT, cli, control, digest, init, load, rpc, write


def eventually(fn, timeout=60):
    deadline = time.monotonic() + timeout
    error = None
    while time.monotonic() < deadline:
        try:
            result = fn()
            if result:
                return result
        except Exception as e:
            error = e
        time.sleep(.5)
    raise RuntimeError(f'condition timeout ({timeout}s), last error: {error}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--home', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=ROOT/'chain/app/bin/nusd')
    p.add_argument('--operators', type=Path, default=ROOT/'chain/app/config/operator-accounts.json')
    p.add_argument('--base-port', type=int, default=28656)
    p.add_argument('--managed', action='store_true')
    a = p.parse_args()
    a.home, a.output = a.home.resolve(), a.output.resolve()
    a.output.mkdir(parents=True, exist_ok=False)
    process = None
    log = None
    report = {'status': 'FAIL', 'scope': 'real local four-validator consensus; one physical host', 'checks': []}
    try:
        if not a.managed:
            init(a)
            log = open(a.output/'supervisor.log', 'w')
            # CI test child, always reaped below; not a persistent development server.
            process = subprocess.Popen([sys.executable, str(ROOT/'ops/s1/devnet.py'), 'serve', '--home', str(a.home)], stdout=log, stderr=log)
        m = load(a.home)
        nodes, binary = m['nodes'], m['binary']
        report['manifest'] = m
        for n in nodes:
            out = a.output/f"node{n['index']}-config.toml"
            out.write_bytes((Path(n['home'])/'config/config.toml').read_bytes())
        (a.output/'genesis.json').write_bytes((Path(nodes[0]['home'])/'config/genesis.json').read_bytes())
        def height(i):
            return int(rpc(nodes[i], '/status')['sync_info']['latest_block_height'])
        def advance(indices, baseline, count=2):
            return eventually(lambda: min(height(i) for i in indices) >= baseline+count)
        eventually(lambda: control(a.home, 'status')['running'] == [0, 1, 2, 3])
        advance(range(4), 0, 3)
        def same_height(label):
            h = min(height(i) for i in range(4))
            blocks = [rpc(n, f'/block?height={h}') for n in nodes]
            commits = [rpc(n, f'/commit?height={h}') for n in nodes]
            hashes = [b['block']['header']['app_hash'] for b in blocks]
            ids = [b['block_id']['hash'] for b in blocks]
            assert hashes[0] and len(set(hashes)) == len(set(ids)) == 1
            assert all(sum(s['block_id_flag'] == 2 for s in c['signed_header']['commit']['signatures']) >= 3 for c in commits)
            evidence = {'height': h, 'app_hashes': hashes, 'block_ids': ids, 'commits': commits}
            write(a.output/(label+'.json'), evidence)
            return evidence
        vals = [rpc(n, '/validators')['validators'] for n in nodes]
        assert all(len(v) == 4 and {x['voting_power'] for x in v} == {'10'} for v in vals)
        assert all({x['address'] for x in v} == {n['validator_address'] for n in nodes} for v in vals)
        assert len({n['node_id'] for n in nodes}) == 4
        assert len({n['validator_address'] for n in nodes}) == 4
        report['initial'] = same_height('initial-consensus')
        report['checks'].append('AT01: four unique validators, equal power 10, identical genesis/block/app hash and commit quorum')
        def snapshot(i=0):
            return cli(binary, 'snapshot', '--rpc', nodes[i]['rpc'])
        def conserved(s):
            assert sum(int(x['exchange_atoms']) for x in s['accounts']) == int(s['module_atoms'])
            assert sum(int(x['bank_atoms']) for x in s['accounts']) + int(s['module_atoms']) == 2*10**12
            assert sum(int(x['gas_atoms']) for x in s['accounts']+s['operator_accounts'])+int(s['gas_collector_atoms']) == int(s['gas_supply']) == int(s['genesis_gas_supply']) == 6*10**9
            assert len(s['operator_accounts']) == 4
            assert all(x['gas_atoms']=='1000000000' and x['bank_atoms']=='0' and not x['exchange_signer'] for x in s['operator_accounts'])
        def tx(user, op, amount, rid):
            d = cli(binary, 'tx', '--rpc', nodes[0]['rpc'], '--user', user, '--op', op,
                    '--amount', amount, '--request-id', f'{rid:064x}')
            assert int(d['height']) > 0 and d['check_tx']['code'] == d['tx_result']['code'] == 0
            write(a.output/f'tx-{user}-{rid}.json', d)
            conserved(snapshot())
            return d
        conserved(snapshot())
        for u in range(2):
            tx(u, 'deposit', '1000000', 1)
            tx(u, 'withdraw', '400000', 2)
        assert all(x['exchange_atoms']=='600000' for x in snapshot()['accounts'])
        report['checks'].append('two users: real signed deposit 1000000 / withdraw 400000; separate DEVQUOTE/DEVGAS conservation')
        control(a.home, 'stop', 3)
        baseline = height(0)
        advance([0, 1, 2], baseline)
        tx(0, 'deposit', '1', 3)
        report['one_stopped'] = {'before': baseline, 'after': height(0), 'running': control(a.home, 'status')['running']}
        assert report['one_stopped']['running'] == [0, 1, 2]
        report['checks'].append('AT05: 3/4 validators continue actual TX finalization')
        control(a.home, 'stop', 2)
        # Let any already assembled quorum finish before the observation window.
        time.sleep(5)
        frozen = [height(0), height(1)]
        before_halt = snapshot()
        raw = a.output/'halt-tx.raw'
        cli(binary, 'tx', '--rpc', nodes[0]['rpc'], '--user', '1', '--op', 'deposit', '--amount', '1',
            '--request-id', f'{3:064x}', '--out', raw)
        admission = rpc(nodes[0], '/broadcast_tx_sync?tx=0x'+raw.read_bytes().hex())
        assert admission['code'] == 0
        samples = []
        start = time.monotonic()
        for _ in range(15):
            time.sleep(1)
            samples.append([height(0), height(1)])
        assert all(s == frozen for s in samples), (frozen, samples)
        assert snapshot() == before_halt
        report['two_stopped'] = {'frozen_heights': frozen, 'samples': samples,
                                  'observed_seconds': time.monotonic()-start,
                                  'checktx_only_not_finalized': admission}
        report['checks'].append('AT05: 2/4 no height advance for 15 samples; accepted CheckTx remains uncommitted, ledger unchanged')
        resumed = time.monotonic()
        control(a.home, 'start', 2)
        control(a.home, 'start', 3)
        advance(range(4), max(frozen), 3)
        receipt = eventually(lambda: cli(binary, 'receipt', '--rpc', nodes[0]['rpc'], '--user', '1', '--request-id', f'{3:064x}'))
        report['resume_seconds'] = time.monotonic()-resumed
        write(a.output/'halt-tx-receipt.json', receipt)
        conserved(snapshot())
        report['recovered'] = same_height('recovered-consensus')
        report['checks'].append('AT05: original queued signed TX confirmed after recovery; all four agree at same height')
        pre = snapshot()
        conserved(pre)
        receipts = [cli(binary, 'receipt', '--rpc', nodes[0]['rpc'], '--user', u, '--request-id', f'{2:064x}') for u in range(2)]
        control(a.home, 'stop')
        assert control(a.home, 'status')['running'] == []
        control(a.home, 'start')
        advance(range(4), int(pre['observed_height']), 2)
        post = snapshot()
        conserved(post)
        for s in (pre, post):
            s.pop('observed_height')
        assert pre == post
        assert receipts == [cli(binary, 'receipt', '--rpc', nodes[0]['rpc'], '--user', u, '--request-id', f'{2:064x}') for u in range(2)]
        write(a.output/'persistent-ledger.json', post)
        report['restart'] = same_height('restart-consensus')
        report['checks'].append('full stop/start preserves committed ledger/receipts and validator signing state')
        report['status'] = 'PASS'
        print(json.dumps({'status': report['status'], 'checks': report['checks'], 'resume_seconds': report['resume_seconds']}, indent=2))
    except Exception as error:
        report['error'] = str(error)
        raise
    finally:
        if process:
            process.terminate()
            try:
                process.wait(timeout=40)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if log:
            log.close()
        for i in range(4):
            path = a.home/f'node{i}.log'
            if path.exists():
                (a.output/path.name).write_bytes(path.read_bytes())
        write(a.output/'report.json', report)


if __name__ == '__main__':
    main()
