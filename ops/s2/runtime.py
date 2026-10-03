#!/usr/bin/env python3
"""Foreground entrypoint for Paperclip managed S2 runtime; never daemonizes."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'ops/s1'))
import devnet
from bounded_log import BoundedLog, LogPump


def get(url):
    with urllib.request.urlopen(url, timeout=3) as r:
        return json.load(r)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('command', choices=['init', 'serve', 'health'])
    p.add_argument('--home', required=True, type=Path)
    p.add_argument('--user-public-keys', type=Path)
    a = p.parse_args()
    a.home = a.home.resolve()
    if a.command == 'init':
        if a.home.exists():
            p.error('existing home refused; no automatic reset')
        args = argparse.Namespace(home=a.home/'chain', binary=ROOT/'chain/app/bin/nusd',
                                  operators=ROOT/'chain/app/config/operator-accounts.json',
                                  base_port=30556, network='s2', user_public_keys=a.user_public_keys)
        m = devnet.init(args)
        pins = {str(x.relative_to(ROOT)): devnet.digest(x) for x in
                [ROOT/'exchange/target/debug/exchange-s2', ROOT/'web/dist/s2/index.html',
                 ROOT/'web/dist/s2/wallet.js']}
        devnet.write(a.home/'runtime.json', dict(chain_genesis=m['genesis_sha256'], files=pins))
        print(json.dumps({'home': str(a.home), 'genesis_hash': m['genesis_sha256']}))
        return
    if a.command == 'health':
        nodes = devnet.health(a.home/'chain')
        status = get('http://127.0.0.1:8788/s2/status')
        with urllib.request.urlopen('http://127.0.0.1:5173/', timeout=3) as r:
            web = r.status
        print(json.dumps(dict(nodes=nodes, api=status, web=web)))
        if any('error' in n or n['catching_up'] or int(n['height']) < 1 for n in nodes) or status['mode'] != 'OPEN' or web != 200:
            raise RuntimeError('runtime is not healthy/open')
        return
    os.umask(0o077)
    lock = (a.home/'runtime.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    pins = json.loads((a.home/'runtime.json').read_text())
    for name, sha in pins['files'].items():
        if devnet.digest(ROOT/name) != sha:
            raise RuntimeError('runtime build changed; explicit reviewed pin update required')
    env = {k:v for k,v in os.environ.items() if k in ('PATH','HOME','TMPDIR','LANG')}
    children = []
    def child(name, argv):
        sink = BoundedLog(a.home/(name+'.log'))
        try:
            proc = subprocess.Popen(argv, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        except BaseException:
            sink.close()
            raise
        children.append((proc, LogPump(proc.stdout, sink)))
        return proc
    def check():
        for proc, pump in children:
            if pump.error:
                raise RuntimeError(str(pump.error))
            if proc.poll() is not None:
                raise RuntimeError('runtime child exited: '+str(proc.returncode))
    def stop(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        child('chain-supervisor', [sys.executable, 'ops/s1/devnet.py', 'serve', '--home', str(a.home/'chain')])
        deadline = time.monotonic()+60
        while True:
            check()
            nodes = devnet.health(a.home/'chain')
            if all('error' not in n and not n['catching_up'] and int(n['height']) > 0 for n in nodes):
                break
            if time.monotonic() > deadline:
                raise RuntimeError('chain readiness deadline')
            time.sleep(.5)
        bootstrap = a.home/'bootstrap'
        genesis = a.home/'chain/node0/config/genesis.json'
        if not bootstrap.exists():
            subprocess.run([sys.executable, 'settlement/s2/bootstrap.py', '--genesis', str(genesis),
                            '--output', str(bootstrap), '--rpc', 'http://127.0.0.1:30557'], cwd=ROOT, env=env, check=True)
        argv = [sys.executable, 'settlement/s2/server.py', '--engine', str(ROOT/'exchange/target/debug/exchange-s2'),
                '--manifest', str(bootstrap/'manifest.json'), '--genesis', str(bootstrap/'genesis.json'),
                '--journal', str(a.home/'journal'), '--evidence', str(a.home/'rpc-evidence'),
                '--rpc', 'http://127.0.0.1:30557', '--port', '8788']
        # Existing or damaged journals always take the open path. Never reset them.
        if not (a.home/'journal').exists():
            argv += ['--bootstrap', str(bootstrap/'bootstrap.json')]
        child('api', argv)
        child('web', ['node', 'web/s2/serve.mjs'])
        while True:
            check()
            time.sleep(.25)
    except KeyboardInterrupt:
        pass
    finally:
        for proc, _ in reversed(children):
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=25)
                except subprocess.TimeoutExpired:
                    proc.kill(); proc.wait(timeout=5)
        errors = []
        for _, pump in children:
            try:
                pump.finish()
            except Exception as error:
                errors.append(str(error))
        lock.close()
        if errors:
            raise RuntimeError('; '.join(errors))

if __name__ == '__main__':
    main()
