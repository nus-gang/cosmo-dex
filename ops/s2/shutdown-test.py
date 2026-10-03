#!/usr/bin/env python3
"""Finite real-process regression for CTO-S2F-01; synthetic data, no preview."""
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
from unittest.mock import patch

from runtime import ROOT, stop_children


def alive(pid):
    r = subprocess.run(['ps', '-p', str(pid), '-o', 'stat='], capture_output=True, text=True)
    return bool(r.stdout.strip()) and not r.stdout.strip().startswith('Z')


def wait_for(predicate, proc, timeout=15):
    deadline = time.monotonic() + timeout
    while not predicate():
        if proc.poll() is not None:
            raise AssertionError(f'early exit {proc.returncode}')
        if time.monotonic() >= deadline:
            raise AssertionError('readiness deadline')
        time.sleep(.05)


def scenario(root, mode):
    root.mkdir()
    for part in ['ops/s1', 'settlement/s2', 'web/s2', 'home/chain', 'home/bootstrap']:
        (root/part).mkdir(parents=True, exist_ok=True)
    (root/'home/runtime.json').write_text('{"files":{}}')
    leaf = root/'leaf.py'
    leaf.write_text('''#!/usr/bin/env python3
import fcntl,json,os,pathlib,signal,socket,sys,time
home=pathlib.Path(sys.argv[sys.argv.index('--home')+1]);home.mkdir(exist_ok=True)
mode=MODE
lock=(home/'leaf.lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
s=socket.socket();s.bind(('127.0.0.1',0));s.listen()
def stop(*_):
 if mode=='delayed':time.sleep(1)
 raise SystemExit(0)
signal.signal(signal.SIGTERM,signal.SIG_IGN if mode in ('ignore','supervisor_crash') else stop)
(home/'leaf.json').write_text(json.dumps({'pid':os.getpid(),'port':s.getsockname()[1]}))
while True:time.sleep(.05)
'''.replace('MODE', repr(mode)))
    leaf.chmod(0o700)
    (root/'ops/s1/devnet.py').write_text(f'''import sys,pathlib,os
sys.path.insert(0,{str(ROOT/'ops/s1')!r})
import devnet
home=pathlib.Path(sys.argv[sys.argv.index('--home')+1])
(home/'supervisor.pid').write_text(str(os.getpid()))
devnet.load=lambda _:{{'binary':{str(leaf)!r},'network':'s2','genesis_sha256':'fixture','nodes':[{{'home':str(home/('node'+str(i)))}} for i in range(4)]}}
devnet.serve(home)
''')
    (root/'settlement/s2/server.py').write_text(f'''import pathlib,signal,subprocess,sys,time
p=subprocess.Popen([sys.executable,{str(leaf)!r},'--home','engine'])
def stop(*_):
 p.terminate()
 try:p.wait(timeout=2)
 except subprocess.TimeoutExpired:pass
 raise SystemExit(0)
signal.signal(signal.SIGTERM,stop)
pathlib.Path('api-ready').touch()
while True:time.sleep(.05)
''')
    (root/'web/s2/serve.mjs').write_text('import {writeFileSync} from "node:fs";writeFileSync("web-ready","");setInterval(()=>{},1000);')
    (root/'driver.py').write_text(f'''import sys,pathlib
sys.path.insert(0,{str(ROOT/'ops/s2')!r})
import runtime
runtime.ROOT=pathlib.Path({str(root)!r})
def health(home):
 ready=all((home/('node'+str(i))/'leaf.json').exists() for i in range(4))
 return [{{'height':'1' if ready else '0','catching_up':False}} for _ in range(4)]
runtime.devnet.health=health
real_socket=runtime.socket.socket
class Reservation:
 def __init__(self):self.sock=real_socket()
 def setsockopt(self,*args):self.sock.setsockopt(*args)
 def bind(self,address):self.sock.bind(('127.0.0.1',0))
 def close(self):self.sock.close()
runtime.socket.socket=Reservation
runtime.main()
''')
    env = {k:v for k,v in os.environ.items() if k in ('PATH','HOME','TMPDIR','LANG')}
    proc = None
    records = []
    try:
        with (root/'driver.log').open('w') as log:
            proc = subprocess.Popen([sys.executable, str(root/'driver.py'), 'serve', '--home', str(root/'home')],
                                    stdout=log, stderr=log, env=env)
            wait_for(lambda: (root/'web-ready').exists() and (root/'engine/leaf.json').exists(), proc)
            records = [(p, json.loads(p.read_text())) for p in root.glob('**/leaf.json')]
            assert len(records) == 5
            start = time.monotonic()
            if mode == 'supervisor_crash':
                os.kill(int((root/'home/chain/supervisor.pid').read_text()), signal.SIGKILL)
            else:
                proc.terminate()
            proc.wait(timeout=40)
            elapsed = time.monotonic() - start
        expected = 0 if mode in ('normal', 'delayed') else 1
        assert proc.returncode == expected, (proc.returncode, (root/'driver.log').read_text())
        assert elapsed < 35
        for path, record in records:
            assert not alive(record['pid']), record
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', record['port']))
            with (path.parent/'leaf.lock').open('a') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for name in ['runtime.lock', 'chain/.supervisor.lock']:
            with (root/'home'/name).open('a') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert not (root/'home/chain/.control.sock').exists()
        if expected:
            logs = (root/'driver.log').read_text() + (root/'home/chain-supervisor.log').read_text()
            assert 'forced SIGKILL' in logs or 'forcing owned groups' in logs
        return dict(mode=mode, result='PASS', exit=proc.returncode, seconds=round(elapsed,3),
                    owned_leaf_count=5, pids_ports_locks_released=True, socket_removed=True,
                    leaves=[r for _, r in records])
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            try: proc.wait(timeout=35)
            except subprocess.TimeoutExpired: proc.kill(); proc.wait()
        # Test failure cleanup is limited to the synthetic fixture's recorded PIDs.
        for path in root.glob('**/leaf.json'):
            pid = json.loads(path.read_text())['pid']
            if alive(pid): os.kill(pid, signal.SIGKILL)


def failed_cleanup(root):
    """Real owned child; injected permission failure must not report clean exit."""
    class Pump:
        def finish(self): pass
    proc = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'], start_new_session=True)
    try:
        with patch.object(proc, 'terminate'), patch('runtime.os.killpg', side_effect=PermissionError('injected kill denied')):
            try:
                stop_children([(proc, Pump())], root, grace=.1, kill_grace=.1)
                raise AssertionError('cleanup failure silently accepted')
            except RuntimeError as error:
                assert 'still running' in str(error) and 'injected kill denied' in str(error)
                message = str(error)
        assert proc.poll() is None
    finally:
        os.killpg(proc.pid, signal.SIGKILL); proc.wait(timeout=5)
    return dict(mode='cleanup_denied', result='PASS', error=message, fixture_reaped=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', required=True, type=Path)
    a = p.parse_args(); a.output = a.output.resolve(); a.output.mkdir(parents=True, exist_ok=False)
    results = []
    try:
        for mode in ['normal', 'delayed', 'ignore', 'supervisor_crash']:
            result = scenario(a.output/mode, mode); results.append(result)
            print(json.dumps(result), flush=True)
        results.append(failed_cleanup(a.output))
    finally:
        (a.output/'result.json').write_text(json.dumps(results, indent=2)+'\n')
    print('5 shutdown regressions PASS', flush=True)
