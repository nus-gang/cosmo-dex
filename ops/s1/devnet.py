#!/usr/bin/env python3
"""Four real nusd validators. Local synthetic assets only; no Paperclip dependency."""
import argparse
import base64
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import time
import urllib.request
from bounded_log import BoundedLog, LogPump

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def rpc(node, path):
    with urllib.request.urlopen(node['rpc'] + path, timeout=3) as r:
        data = json.load(r)
    if 'error' in data:
        raise RuntimeError(data['error'])
    return data['result']


def cli(binary, *args):
    r = subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True, timeout=45)
    if r.returncode:
        raise RuntimeError(f'nusd {args[0]} failed: {r.stderr} {r.stdout}')
    return json.loads(r.stdout)


def init(a):
    root = a.home.resolve()
    binary = a.binary.resolve()
    if root.exists():
        raise RuntimeError('refusing existing home; use a new directory, never reset validator state')
    if not 1024 <= a.base_port <= 65000:
        raise ValueError('base port must be 1024..65000')
    os.umask(0o077)
    root.mkdir(parents=True, mode=0o700)
    nodes, validators, genesis = [], [], None
    user_keys = getattr(a, 'user_public_keys', None)
    user_args = [] if user_keys is None else ['--user-public-keys', user_keys.resolve()]
    for i in range(4):
        home = root / f'node{i}'
        port = a.base_port + i * 10
        cli(binary, 'init', '--network', getattr(a, 'network', 's1'), '--home', home, '--operator-accounts', a.operators.resolve(),
            '--rpc', f'tcp://127.0.0.1:{port+1}', '--p2p', f'tcp://127.0.0.1:{port}', *user_args)
        g = json.loads((home / 'config/genesis.json').read_text())
        genesis = genesis or g
        val = g['validators'][0]
        val['name'] = f'validator-{i}'
        validators.append(val)
        nk = json.loads((home / 'config/node_key.json').read_text())
        public = base64.b64decode(nk['priv_key']['value'])[32:]
        node_id = hashlib.sha256(public).hexdigest()[:40]
        nodes.append({'index': i, 'home': str(home), 'node_id': node_id,
                      'rpc': f'http://127.0.0.1:{port+1}', 'p2p_port': port,
                      'validator_address': val['address']})
    genesis['validators'] = validators
    raw = (json.dumps(genesis, indent=2) + '\n').encode()
    gh = hashlib.sha256(raw).hexdigest()
    for node in nodes:
        home = Path(node['home'])
        (home / 'config/genesis.json').write_bytes(raw)
        config = home / 'config/config.toml'
        text = config.read_text()
        peers = ','.join(f"{n['node_id']}@127.0.0.1:{n['p2p_port']}" for n in nodes if n != node)
        replacements = {'persistent_peers': '"'+peers+'"', 'addr_book_strict': 'false',
                        'allow_duplicate_ip': 'true', 'pex': 'false',
                        'moniker': json.dumps(f"nus-s1-validator-{node['index']}")}
        for key, value in replacements.items():
            text, count = re.subn(r'^'+key+r' = .*$', key+' = '+value, text, flags=re.M)
            if count != 1:
                raise RuntimeError(f'config field {key}: expected one, got {count}')
        config.write_text(text)
        node['config_sha256'] = digest(config)
        for private in ('priv_validator_key.json', 'node_key.json'):
            (home / 'config' / private).chmod(0o600)
    manifest = {'schema': 1, 'scope': 'single-host real four-validator synthetic devnet',
                'network': getattr(a, 'network', 's1'),
                'binary': str(binary), 'binary_sha256': digest(binary),
                'version': cli(binary, 'version'), 'genesis_sha256': gh,
                'go_mod_sha256': digest(ROOT/'chain/app/go.mod'),
                'go_sum_sha256': digest(ROOT/'chain/app/go.sum'),
                'operator_accounts_sha256': digest(a.operators), 'nodes': nodes}
    write(root / 'manifest.json', manifest)
    return manifest


def load(home):
    m = json.loads((home / 'manifest.json').read_text())
    if digest(m['binary']) != m['binary_sha256']:
        raise RuntimeError('binary hash changed; new build requires new explicit manifest/home')
    for n in m['nodes']:
        p = Path(n['home'])
        if digest(p/'config/genesis.json') != m['genesis_sha256'] or digest(p/'config/config.toml') != n['config_sha256']:
            raise RuntimeError('pinned genesis/config changed')
    return m


def socket_path(home):
    # macOS AF_UNIX path length is 104 bytes, so keep socket relative to cwd.
    return '.control.sock'


def control(home, action, index='all'):
    old = Path.cwd()
    try:
        os.chdir(home)
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(45)
            s.connect(socket_path(home))
            s.sendall(json.dumps({'action': action, 'node': index}).encode()+b'\n')
            result = json.loads(s.makefile('rb').readline(65536))
            if 'error' in result:
                raise RuntimeError(result['error'])
            return result
    finally:
        os.chdir(old)


def serve(home):
    os.umask(0o077)
    m = load(home)
    os.chdir(home)
    lock = open('.supervisor.lock', 'a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    processes, logs = {}, {}
    ending = False

    def stop(i):
        proc = processes.pop(i, None)
        if proc and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
        if i in logs:
            logs.pop(i).finish()

    def start(i):
        if i in processes and processes[i].poll() is None:
            return
        stop(i)
        load(home)
        n = m['nodes'][i]
        sink = BoundedLog(home/f'node{i}.log')
        # Validator processes receive no Paperclip/GitHub credentials.
        env = {k: v for k, v in os.environ.items() if k in ('PATH', 'HOME', 'TMPDIR', 'LANG')}
        try:
            processes[i] = subprocess.Popen([m['binary'], 'start', '--network', m.get('network', 's1'), '--home', n['home'],
                                '--genesis-hash', m['genesis_sha256']], env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            logs[i] = LogPump(processes[i].stdout, sink)
        except Exception:
            sink.close()
            raise

    def shutdown(*_):
        nonlocal ending
        ending = True

    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)
    path = Path(socket_path(home))
    path.unlink(missing_ok=True)
    try:
        with socket.socket(socket.AF_UNIX) as server:
            server.bind(str(path))
            path.chmod(0o600)
            server.listen(4)
            server.settimeout(.5)
            for i in range(4):
                start(i)
            print('four-validator supervisor started', flush=True)
            while not ending:
                for i, pump in logs.items():
                    if pump.error:
                        raise RuntimeError(f'node{i} log write/rotation failed: {pump.error}')
                failed = [i for i, p in processes.items() if p.poll() is not None]
                if failed:
                    raise RuntimeError(f'validator exited unexpectedly: {failed}; inspect node logs')
                try:
                    conn, _ = server.accept()
                except socket.timeout:
                    continue
                with conn:
                    conn.settimeout(5)
                    action = None
                    try:
                        req = json.loads(conn.makefile('rb').readline(4096))
                        action, node = req['action'], req.get('node', 'all')
                        indices = list(range(4)) if node == 'all' else [int(node)]
                        if any(i not in range(4) for i in indices):
                            raise ValueError('node must be 0..3 or all')
                        if action not in ('stop', 'start', 'restart', 'status'):
                            raise ValueError('invalid action')
                        for i in indices:
                            if action in ('stop', 'restart'):
                                stop(i)
                            if action in ('start', 'restart'):
                                start(i)
                        result = {'running': sorted(processes), 'pids': {i: p.pid for i,p in processes.items()}}
                    except Exception as e:
                        result = {'error': str(e)}
                    conn.sendall(json.dumps(result).encode()+b'\n')
                    if 'error' in result and action in ('start', 'restart', 'stop'):
                        raise RuntimeError(result['error'])
    finally:
        errors = []
        for i in range(4):
            try:
                stop(i)
            except Exception as error:
                errors.append(f'node{i}: {error}')
        path.unlink(missing_ok=True)
        lock.close()
        if errors:
            raise RuntimeError('; '.join(errors))


def health(home):
    m = load(home)
    out = []
    for n in m['nodes']:
        try:
            status = rpc(n, '/status')
            out.append({'node': n['index'], 'height': status['sync_info']['latest_block_height'],
                        'app_hash': status['sync_info']['latest_app_hash'],
                        'catching_up': status['sync_info']['catching_up'],
                        'peers': rpc(n, '/net_info')['n_peers']})
        except Exception as e:
            out.append({'node': n['index'], 'error': str(e)})
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('command', choices=['init', 'serve', 'start', 'stop', 'restart', 'status', 'health', 'log'])
    p.add_argument('--home', type=Path, default=ROOT/'.runtime/s1')
    p.add_argument('--binary', type=Path, default=ROOT/'chain/app/bin/nusd')
    p.add_argument('--operators', type=Path, default=ROOT/'chain/app/config/operator-accounts.json')
    p.add_argument('--network', choices=['s1', 's2'], default='s1')
    p.add_argument('--base-port', type=int, default=28656)
    p.add_argument('--user-public-keys', type=Path, help='JSON array of two public keys; init only')
    p.add_argument('--node', default='all')
    a = p.parse_args()
    a.home = a.home.resolve()
    if a.command == 'init':
        result = init(a)
    elif a.command == 'serve':
        return serve(a.home)
    elif a.command == 'health':
        result = health(a.home)
        print(json.dumps(result, indent=2))
        return int(any('error' in n or n.get('catching_up') or int(n.get('height', 0)) < 1 for n in result))
    elif a.command == 'log':
        if a.node not in ('0', '1', '2', '3'):
            raise ValueError('log requires --node 0..3')
        with (a.home/f'node{a.node}.log').open('rb') as log:
            log.seek(0, os.SEEK_END)
            log.seek(max(0, log.tell() - 65536))
            print('\n'.join(log.read().decode(errors='replace').splitlines()[-100:]))
        return
    else:
        result = control(a.home, a.command, a.node)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
