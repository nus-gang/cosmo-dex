#!/usr/bin/env python3
"""Finite subprocess lifecycle test; always reaps its own runtime, never a preview."""
import argparse
import json
import os
from pathlib import Path
import subprocess
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
def stop(proc):
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=35)
        except subprocess.TimeoutExpired:
            proc.kill(); proc.wait(timeout=5)
    assert proc.returncode == 0, proc.returncode
def ready(proc):
    deadline = time.monotonic()+90
    while time.monotonic() < deadline:
        assert proc.poll() is None
        r = subprocess.run(command('health'), capture_output=True, text=True)
        if r.returncode == 0:
            return json.loads(r.stdout)
        time.sleep(.5)
    raise AssertionError('readiness deadline')
report = {'result': 'FAIL'}; proc = None
try:
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
        keys = {str(x.relative_to(a.home)): x.read_bytes() for x in a.home.glob('chain/node*/config/*key.json')}
        proc = subprocess.Popen(command('serve'), cwd=ROOT, stdout=log, stderr=log)
        later = ready(proc)
        assert min(int(n['height']) for n in later['nodes']) >= min(int(n['height']) for n in first['nodes'])
        assert before_pins == (a.home/'runtime.json').read_bytes()
        assert all((a.home/x).read_bytes() == data for x,data in keys.items())
        stop(proc)
        assert not (a.home/'chain/.control.sock').exists()
        logs = list((a.home/'chain').glob('node*.log*'))
        assert len(logs) <= 20 and all(x.stat().st_size <= 104857600 for x in logs)
        report.update(result='PASS', first=first, restarted=later, second_writer_exit=second.returncode,
                      keys_unchanged=True, pins_unchanged=True, clean_stop=True,
                      node_logs={x.name:x.stat().st_size for x in logs})
finally:
    if proc and proc.poll() is None:
        stop(proc)
    (a.output/'result.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
