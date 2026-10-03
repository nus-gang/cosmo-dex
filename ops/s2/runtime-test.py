#!/usr/bin/env python3
"""Finite subprocess lifecycle test; always reaps its own runtime, never a preview."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import socket
import sys
import time
from runtime import ROOT

p = argparse.ArgumentParser()
p.add_argument('--home', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
a.home = a.home.resolve(); a.output.mkdir(parents=True, exist_ok=False)
argv = [sys.executable, str(ROOT/'ops/s2/runtime.py')]
def command(op):
    return [*argv, op, '--home', str(a.home)]
PORTS = [30556+i*10+offset for i in range(4) for offset in (0,1)]+[8788,5173]
def released():
    for port in PORTS:
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', port))
    for name in ['runtime.lock', 'chain/.supervisor.lock', 'journal/writer.lock']:
        with (a.home/name).open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
def stop(proc):
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=35)
        except subprocess.TimeoutExpired:
            proc.kill(); proc.wait(timeout=5)
    assert proc.returncode == 0, proc.returncode
    released()
def ready(proc):
    deadline = time.monotonic()+90
    while time.monotonic() < deadline:
        assert proc.poll() is None
        r = subprocess.run(command('health'), capture_output=True, text=True)
        if r.returncode == 0:
            assert proc.poll() is None
            return json.loads(r.stdout)
        time.sleep(.5)
    raise AssertionError('readiness deadline')
report = {'result': 'FAIL'}; proc = None
try:
    for port in PORTS:
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', port))
    subprocess.run(command('init'), cwd=ROOT, check=True, capture_output=True)
    before_pins = (a.home/'runtime.json').read_bytes()
    with (a.output/'runtime.log').open('w') as log:
        proc = subprocess.Popen(command('serve'), cwd=ROOT, stdout=log, stderr=log)
        first = ready(proc)
        second = subprocess.run(command('serve'), cwd=ROOT, capture_output=True, text=True, timeout=10)
        assert second.returncode != 0 and 'BlockingIOError' in second.stderr
        (a.output/'second-writer.txt').write_text(second.stderr)
        stop(proc)
        # Stopped supervisor has no running validators or bound control socket.
        assert not (a.home/'chain/.control.sock').exists()
        wal = (a.home/'journal/journal.wal').read_bytes()
        journal_pins = {name: (a.home/'journal'/name).read_bytes() for name in ['context.json', 'bootstrap.json']}
        keys = {str(x.relative_to(a.home)): x.read_bytes() for x in a.home.glob('chain/node*/config/*key.json')}
        proc = subprocess.Popen(command('serve'), cwd=ROOT, stdout=log, stderr=log)
        later = ready(proc)
        assert min(int(n['height']) for n in later['nodes']) >= min(int(n['height']) for n in first['nodes'])
        assert before_pins == (a.home/'runtime.json').read_bytes()
        assert all((a.home/x).read_bytes() == data for x,data in keys.items())
        stop(proc)
        assert (a.home/'journal/journal.wal').read_bytes().startswith(wal)
        assert all((a.home/'journal'/name).read_bytes() == raw for name, raw in journal_pins.items())
        assert not (a.home/'chain/.control.sock').exists()
        logs = list((a.home/'chain').glob('node*.log*'))
        assert len(logs) <= 20 and all(x.stat().st_size <= 104857600 for x in logs)
        report.update(result='PASS', first=first, restarted=later, second_writer_exit=second.returncode,
                      keys_unchanged=True, pins_unchanged=True, clean_stop=True,
                      ports_locks_released=True, journal_prefix_preserved=True,
                      journal_prefix_sha256=hashlib.sha256(wal).hexdigest(),
                      genesis_sha256=json.loads(before_pins)['chain_genesis'],
                      node_logs={x.name:x.stat().st_size for x in logs})
finally:
    if proc and proc.poll() is None:
        stop(proc)
    (a.output/'result.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
