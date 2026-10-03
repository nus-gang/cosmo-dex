#!/usr/bin/env python3
"""Foreground entrypoint for Paperclip managed S2 runtime; never daemonizes."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
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


def group_members(groups):
    """Inspect only groups created by this runtime; zombies own no ports/locks."""
    result = subprocess.run(['ps', '-axo', 'pid=,pgid=,stat='],
                            capture_output=True, text=True, check=True, timeout=3)
    members = []
    for line in result.stdout.splitlines():
        pid, pgid, state = line.split()[:3]
        if int(pgid) in groups and not state.startswith('Z'):
            members.append(int(pid))
    return members


def stop_children(children, home, grace=25, kill_grace=5):
    """Bounded cleanup of owned sessions, including orphaned API/chain children.

    Nested services must not detach into a different process group/session.
    Force or failed cleanup is an error, even when all resources were reclaimed.
    """
    errors = []
    groups = {proc.pid for proc, _ in children}
    for proc, _ in reversed(children):
        if proc.poll() is None:
            try:
                proc.terminate()
            except ProcessLookupError:
                pass
            except OSError as error:
                errors.append(f'child pid={proc.pid} SIGTERM failed: {error}')
    deadline = time.monotonic() + grace
    remaining = []
    try:
        while True:
            for proc, _ in children:
                proc.poll()  # reap direct children before inspecting their groups
            remaining = group_members(groups)
            if not remaining or time.monotonic() >= deadline:
                break
            time.sleep(.05)
    except Exception as error:
        errors.append(f'cannot verify owned processes: {error}')
        remaining = ['unknown']
    if remaining:
        errors.append(f'shutdown deadline exceeded; forcing owned groups; remaining={remaining}')
        for pgid in groups:
            try:
                os.killpg(pgid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError as error:
                errors.append(f'group {pgid} SIGKILL failed: {error}')
    deadline = time.monotonic() + kill_grace
    for proc, _ in children:
        try:
            proc.wait(timeout=max(.01, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            errors.append(f'child pid={proc.pid} did not exit')
        if proc.returncode not in (0, -signal.SIGTERM):
            errors.append(f'child pid={proc.pid} exit={proc.returncode}')
    verified = False
    try:
        while True:
            remaining = group_members(groups)
            if not remaining:
                verified = True
                break
            if time.monotonic() >= deadline:
                errors.append(f'owned processes still running: {remaining}')
                break
            time.sleep(.05)
    except Exception as error:
        errors.append(f'cannot verify final process state: {error}')
    for _, pump in children:
        try:
            pump.finish()
        except Exception as error:
            errors.append(str(error))
    # The supervisor may have died before unlinking. Never unlink until every
    # owned process has stopped; do not touch DB, keys, journal or lock files.
    if verified and (home/'chain').is_dir():
        try:
            with (home/'chain/.supervisor.lock').open('a') as chain_lock:
                fcntl.flock(chain_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                (home/'chain/.control.sock').unlink(missing_ok=True)
        except OSError as error:
            errors.append(f'chain supervisor lock/socket cleanup failed: {error}')
    if errors:
        raise RuntimeError('; '.join(errors))


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
        pins = json.loads((a.home/'runtime.json').read_text())
        if status.get('context', {}).get('genesis_hash') != pins['chain_genesis']:
            raise RuntimeError('API belongs to a different genesis/home')
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
            proc = subprocess.Popen(argv, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                    start_new_session=True)
        except BaseException:
            sink.close()
            raise
        children.append((proc, LogPump(proc.stdout, sink)))
        return proc
    completed = set()
    def check():
        for proc, pump in children:
            if pump.error:
                raise RuntimeError(str(pump.error))
            if proc not in completed and proc.poll() is not None:
                raise RuntimeError('runtime child exited: '+str(proc.returncode))
    def stop(*_):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        # Refuse occupied endpoints before health can accidentally observe a
        # different local runtime. Never stop the process using that endpoint.
        reservations = []
        try:
            for port in [30556 + i*10 + offset for i in range(4) for offset in (0, 1)] + [8788, 5173]:
                sock = socket.socket()
                reservations.append(sock)
                sock.bind(('127.0.0.1', port))
        finally:
            for sock in reservations:
                sock.close()
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
            boot = child('bootstrap', [sys.executable, 'settlement/s2/bootstrap.py', '--genesis', str(genesis),
                                      '--output', str(bootstrap), '--rpc', 'http://127.0.0.1:30557'])
            completed.add(boot)
            while boot.poll() is None:
                check()
                time.sleep(.05)
            if boot.returncode != 0:
                raise RuntimeError(f'bootstrap failed: {boot.returncode}')
            if group_members({boot.pid}):
                raise RuntimeError('bootstrap left owned descendants')
            children[-1][1].finish()
            children.pop()
            completed.remove(boot)
            # Do not retain an exited bootstrap's group ID for a long-lived
            # runtime: the OS may later recycle it for an unrelated process.
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
        # A second TERM must not interrupt cleanup and release the writer lock.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        try:
            stop_children(children, a.home)
        finally:
            lock.close()

if __name__ == '__main__':
    main()
