"""Nonce-bound preparation observations, separate from normal v2 PID receipts.

No approval or cleanup claim. Never repair/replace partial evidence. Only fixed
phase names and public hashes are persisted; exception messages are excluded.
"""
import hashlib
import json
import math
import os
import socket
from pathlib import Path
import time

from pid_mailbox import _root, _read, _write, _nonce
from preflight import bounded
from web_pid_mailbox import _report

PHASES = ('handoff', 'audit_first', 'assets', 'audit_final', 'bind',
          'http_listening', 'closed')
CAUSES = ('operator_stop', 'approval', 'files', 'port', 'transport', 'internal')
ERROR = 'LOCAL_PREPARATION_REJECTED'


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def location(box):
    return box.parent / ('preparation-' + _nonce(_read(_root(box), 'challenge')))


class Trace:
    def __init__(self, box):
        self.box = _root(box)
        self.identity = self.box.stat()
        self.nonce = _nonce(_read(self.box, 'challenge'))
        self.pid_raw = _read(self.box, 'pids.json')
        if self.pid_raw != _report(self.nonce, os.getpid()):
            raise ValueError(ERROR)
        self.root = location(self.box)
        self.root.mkdir(mode=0o700)
        self.index = 0
        self.phase = 'handoff'
        self.emit('handoff')

    def emit(self, phase, *, cause=None, html_sha256=None):
        current = _root(self.box).stat()
        if ((current.st_dev, current.st_ino) != (self.identity.st_dev, self.identity.st_ino)
                or _read(self.box, 'challenge').decode() != self.nonce
                or _read(self.box, 'pids.json') != self.pid_raw or self.index >= 16
                or phase not in (*PHASES, 'failed')
                or (phase != 'failed' and (self.index >= len(PHASES) or phase != PHASES[self.index]))
                or (self.index and self.phase in ('failed','closed'))
                or (phase == 'failed') != (cause is not None)
                or (cause is not None and cause not in CAUSES)):
            raise ValueError(ERROR)
        value = dict(schema='s3-local-preparation-event/1', nonce=self.nonce,
                     pid=os.getpid(), index=self.index, phase=phase,
                     at_monotonic_ns=time.monotonic_ns())
        if cause is not None:
            value.update(cause=cause, failed_phase=self.phase)
        if html_sha256 is not None:
            if phase != 'http_listening' or len(html_sha256) != 64 or any(
                    c not in '0123456789abcdef' for c in html_sha256):
                raise ValueError(ERROR)
            value['html_sha256'] = html_sha256
        from workspace_reentry import _rename_exclusive
        pending=f'.{self.index:02d}.pending'
        _write(_root(self.root), pending, encode(value))
        _rename_exclusive(self.root/pending,self.root/f'{self.index:02d}.json')
        self.index += 1
        self.phase = phase


def events(box):
    """Absent trace means preparing, malformed/partial trace means reject."""
    identity=_root(box.root).stat()
    if ((identity.st_dev,identity.st_ino)!=(box.identity.st_dev,box.identity.st_ino) or
            _read(box.root,'challenge').decode()!=box.nonce):
        raise ValueError(ERROR)
    root = location(box.root)
    try:
        root.lstat()
    except FileNotFoundError:
        return []
    _root(root)
    if _read(_root(box.root), 'challenge').decode() != box.nonce:
        raise ValueError(ERROR)
    pid_raw = _read(box.root, 'pids.json')
    pid = json.loads(pid_raw)['web_pid']
    if pid_raw != _report(box.nonce, pid) or type(pid) is not int or not 2 <= pid <= 2147483647:
        raise ValueError(ERROR)
    names = sorted(p.name for p in root.iterdir())
    # A writer publishes each event with a final no-replace rename. Pending
    # bytes never establish readiness; abandoned publication times out.
    if not names:return []
    pending=[n for n in names if n.startswith('.')]
    if pending:
        count=len(names)-len(pending)
        if pending!=[f'.{count:02d}.pending']:raise ValueError(ERROR)
        bounded(root,pending[0],1024)  # Links/FIFOs still reject immediately.
        return []
    if not names or len(names) > 16 or names != [f'{i:02d}.json' for i in range(len(names))]:
        raise ValueError(ERROR)
    result = []
    previous = -1
    for i, name in enumerate(names):
        raw = bounded(root, name, 1024)
        event = json.loads(raw)
        fields = {'schema', 'nonce', 'pid', 'index', 'phase', 'at_monotonic_ns'}
        phase = event.get('phase')
        if phase == 'failed': fields |= {'cause', 'failed_phase'}
        if phase == 'http_listening': fields |= {'html_sha256'}
        at = event.get('at_monotonic_ns')
        if (raw != encode(event) or set(event) != fields or
                event['schema'] != 's3-local-preparation-event/1' or
                event['nonce'] != box.nonce or event['pid'] != pid or event['index'] != i or
                type(at) is not int or at < previous or phase not in (*PHASES, 'failed') or
                (i == 0 and phase != 'handoff') or
                (phase != 'failed' and (i >= len(PHASES) or phase != PHASES[i])) or
                (result and result[-1]['phase'] in ('failed', 'closed'))):
            raise ValueError(ERROR)
        if phase == 'failed' and (event['cause'] not in CAUSES or not result or
                                  event['failed_phase'] != result[-1]['phase']):
            raise ValueError(ERROR)
        if phase == 'http_listening' and (len(event['html_sha256']) != 64 or any(
                c not in '0123456789abcdef' for c in event['html_sha256'])):
            raise ValueError(ERROR)
        previous = at
        result.append(event)
    return result


def http_ready(origin, digest, timeout, *, clock=time.monotonic, connect=socket.create_connection):
    """One direct bounded GET, no proxy, redirect, retry, credential or upstream."""
    if origin not in ('http://127.0.0.1:5173', 'http://localhost:5173'):
        raise ValueError(ERROR)
    started=clock();previous=started;deadline=started+min(1.0,timeout)
    def remaining():
        nonlocal previous
        now=clock()
        if not math.isfinite(now) or not previous<=now<deadline:raise ValueError(ERROR)
        previous=now;return deadline-now
    connection = connect(('127.0.0.1',5173),timeout=remaining())
    try:
        connection.settimeout(remaining())
        connection.sendall(('GET / HTTP/1.1\r\nHost: '+origin[7:]+'\r\nConnection: close\r\n\r\n').encode())
        raw=bytearray();size=None;offset=None
        while True:
            connection.settimeout(remaining());chunk=connection.recv(4096)
            if not chunk or len(raw)+len(chunk)>81920:raise ValueError(ERROR)
            raw.extend(chunk);remaining()
            if offset is None and b'\r\n\r\n' in raw:
                header,_,body=raw.partition(b'\r\n\r\n');offset=len(header)+4
                if len(header)>16384:raise ValueError(ERROR)
                lines=header.decode('ascii').split('\r\n')
                if lines[0]!='HTTP/1.1 200 Response':raise ValueError(ERROR)
                fields=[line.split(':',1) for line in lines[1:]]
                if any(len(x)!=2 for x in fields):raise ValueError(ERROR)
                lengths=[v.strip() for k,v in fields if k.lower()=='content-length']
                if (len(lengths)!=1 or not lengths[0].isdigit() or not 0<int(lengths[0])<=65536 or
                        any(k.lower()=='transfer-encoding' for k,v in fields)):raise ValueError(ERROR)
                size=int(lengths[0])
            if offset is None and len(raw)>16384:raise ValueError(ERROR)
            if offset is not None and len(raw)>=offset+size:
                if len(raw)!=offset+size or hashlib.sha256(raw[offset:]).hexdigest()!=digest:
                    raise ValueError(ERROR)
                return
    finally:
        connection.close()


def wait_ready(box, origin, stop, seconds=60, *, clock=time.monotonic,
               sleep=time.sleep, probe=http_ready):
    if type(seconds) is not int or not 1 <= seconds <= 60:
        raise ValueError(ERROR)
    started = previous = clock()
    while True:
        now = clock()
        if (stop() or not all(math.isfinite(x) for x in (now, previous)) or
                now < previous or now >= started + seconds):
            raise ValueError('LOCAL_PREPARATION_STOP_OR_TIMEOUT')
        previous = now
        observed = events(box)
        if observed and observed[-1]['phase'] in ('failed', 'closed'):
            raise ValueError('LOCAL_PREPARATION_CHILD_TERMINATED')
        ready = [e for e in observed if e['phase'] == 'http_listening']
        if ready:
            if len(ready) != 1: raise ValueError(ERROR)
            probe(origin, ready[0]['html_sha256'], min(1.0, started + seconds - now))
            finished = clock()
            if stop() or not now <= finished < started + seconds or events(box) != observed:
                raise ValueError(ERROR)
            return dict(schema='s3-local-http-readiness/1', nonce=box.nonce,
                        pid=ready[0]['pid'], http_status=200,
                        html_sha256=ready[0]['html_sha256'],
                        preparation_seconds=finished-started, approval_verified=False)
        sleep(min(0.05, started + seconds - now))
