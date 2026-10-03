#!/usr/bin/env python3
"""Bounded single-validator RPC/bootstrap test using public development keys."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

from bootstrap import prepare
from chain import Collector, RPC
from transport import Engine, decode, encode


def run(chain, engine_binary, operators, output, port):
    output.mkdir(parents=True, exist_ok=False)
    home = output / 'node'
    rpc = f'tcp://127.0.0.1:{port}'
    def cli(command, *args):
        result = subprocess.run([chain, command, '--network', 's2', *map(str, args)],
                                capture_output=True, timeout=35, check=True)
        return decode(result.stdout)
    initial = cli('init', '--home', home, '--rpc', rpc, '--p2p',
                  f'tcp://127.0.0.1:{port-1}', '--operator-accounts', operators)
    (output / 'init.json').write_bytes(encode(initial))
    engine = None
    with (output / 'node.log').open('wb') as log:
        node = subprocess.Popen([chain, 'start', '--network', 's2', '--home', str(home),
                                 '--genesis-hash', initial['genesis_hash']], stdout=log, stderr=log)
        try:
            client = RPC(f'http://127.0.0.1:{port}')
            for _ in range(100):
                if node.poll() is not None:
                    raise RuntimeError('node exited')
                try:
                    result, _ = client.call('status', {})
                    if int(result['sync_info']['latest_block_height']) >= 1:
                        break
                except Exception:
                    pass
                time.sleep(.2)
            else:
                raise RuntimeError('node readiness timeout')
            bundle = output / 'bootstrap'
            manifest = prepare(home / 'config/genesis.json', bundle, client)
            argv = [engine_binary, 'create', str(bundle / 'manifest.json'),
                    str(output / 'journal'), str(bundle / 'bootstrap.json')]
            engine = Engine(argv)
            collector = Collector(client, engine, manifest, output / 'observations')
            def observe():
                for _ in range(20):
                    if not collector.tick():
                        raise RuntimeError(collector.last_error)
                    status, body = engine.request('GET', '/s2/status')
                    if body['observation']['observed_height'] == client.call('status', {})[0]['sync_info']['latest_block_height']:
                        assert status == 200 and body['mode'] == 'OPEN'
                        return body
                raise RuntimeError('catchup timeout')
            observe()
            for user, denom, amount in ((0, 'DEVBASE', '10000000'), (1, 'DEVQUOTE', '100000000')):
                tx = cli('tx', '--rpc', rpc, '--user', user, '--denom', denom,
                         '--op', 'deposit', '--amount', amount, '--request-id', f'{user+1:064x}')
                (output / f'deposit-{user}.json').write_bytes(encode(tx))
                observe()
            before = cli('snapshot', '--rpc', rpc)
            (output / 'deposited-snapshot.json').write_bytes(encode(before))
            tx = cli('tx', '--rpc', rpc, '--user', '0', '--denom', 'DEVBASE',
                     '--op', 'withdraw', '--amount', '1000000', '--request-id', f'{3:064x}')
            (output / 'withdraw.json').write_bytes(encode(tx))
            live = observe()
            after = cli('snapshot', '--rpc', rpc)
            (output / 'withdrawn-snapshot.json').write_bytes(encode(after))
            assert any(a['owner_epoch'] != b['owner_epoch'] for a, b in zip(
                before['body']['accounts'], after['body']['accounts']))
            engine.close()
            engine = Engine([engine_binary, 'open', str(bundle / 'manifest.json'), str(output / 'journal')])
            collector = Collector(client, engine, manifest, output / 'observations')
            restarted = observe()
            assert int(restarted['observation']['observed_height']) >= int(live['observation']['observed_height'])
            (output / 'status-before-restart.json').write_bytes(encode(live))
            (output / 'status-after-restart.json').write_bytes(encode(restarted))
            report = {'result': 'PASS', 'scope': 'single-validator real RPC, two deposits, direct withdrawal epoch, engine restart; signed HTTP orders and bilateral fill correction NOT_RUN',
                      'genesis_hash': manifest['context']['genesis_hash'],
                      'chain_binary_hash': hashlib.sha256(Path(chain).read_bytes()).hexdigest(),
                      'engine_binary_hash': hashlib.sha256(Path(engine_binary).read_bytes()).hexdigest()}
            (output / 'result.json').write_bytes(encode(report))
            print(json.dumps(report))
        finally:
            if engine:
                engine.close()
            if node.poll() is None:
                node.terminate()
                node.wait(timeout=15)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--chain', required=True)
    parser.add_argument('--engine', required=True)
    parser.add_argument('--operators', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--port', type=int, default=29857)
    args = parser.parse_args()
    run(str(Path(args.chain).resolve()), str(Path(args.engine).resolve()),
        str(Path(args.operators).resolve()), Path(args.output).resolve(), args.port)
