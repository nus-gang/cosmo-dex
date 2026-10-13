"""Stopped workspace evidence and fresh resource checks. Never a start permit.

Receipts are local, same-uid evidence. Authenticated current listings and launch
approval audits remain mandatory. No service row is discarded or rewritten.
"""
import copy
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shlex
import stat
import time

from native_review import COMPANY, _uuid
from pid_mailbox import _root, _read, _nonce, _write, _archive_pending, _archive_clear, Mailbox
from web_pid_mailbox import WebMailbox, _report
from workspace_registration import PROJECT, prepare, prepare_chain, prepare_web
from runtime_evidence import operation, _time

ERROR = 'WORKSPACE_REENTRY_REJECTED'
LIMIT = 1024 * 1024


def encode(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'))+'\n').encode()


def command_info(packet):
    """Reconstruct an allowed command, including all argv and config bytes."""
    body = packet['body']
    command, = body['runtimeConfig']['workspaceRuntime']['commands']
    words = shlex.split(command['command'])
    if words[:1] != ['exec'] or words[2] != '-B':
        raise ValueError(ERROR)
    python, candidate, args = words[1], body['cwd'], words[5:]
    script = str(Path(candidate)/'ops/s3-local')
    cid = command['id']
    if cid in ('s3-web-fee0', 's3-web-fee25'):
        fee = int(cid.removeprefix('s3-web-fee'))
        if words[3:5] == [script+'/key_preparation_cli.py', 'prepare-keys-reviewed']:
            from key_preparation_cli import parse
            from registration_render import key_packets
            a = parse(words[4:])
            control = a.pid_mailbox.parent.parent.parent
            spec = {k: str(getattr(a, k)) for k in ('bundle','artifacts','runtime_pin',
                    'native_decision_id','ceo_revision','cto_revision')}
            spec.update(python=python, candidate=candidate, control_root=str(control))
            expected = key_packets(spec)[int(fee == 25)]
            kind = 'keys'
        else:
            from web_cli import parse_web
            a, _, endpoint, origin, *_ = parse_web(words[4:])
            a.approval_socket, a.web_origin = endpoint, origin
            expected = prepare_web(python, candidate, args, fee_bps=fee)
            kind = 'web'
        endpoints = (('127.0.0.1', 5173),)
    elif cid in ('s3-worker-fee0', 's3-worker-fee25'):
        from offline_cli import parse
        fee = int(cid.removeprefix('s3-worker-fee'))
        a, _ = parse(args, reviewed=True, managed=True)
        expected = prepare(python, candidate, args, fee_bps=fee)
        endpoints = (('127.0.0.1', int(a.bind.split(':')[1])),)
        kind = 'worker'
    else:
        from chain_cli import parse
        fee, index = cid.removeprefix('s3-chain-fee').split('-v')
        a = parse(words[4:])
        expected = prepare_chain(python, candidate, args, fee_bps=int(fee), validator_index=int(index))
        endpoints = tuple(('127.0.0.1', int(x.split(':')[1])) for x in (a.rpc, a.p2p))
        kind = 'chain'
    if packet != expected:
        raise ValueError(ERROR)
    return kind, a, endpoints


def service_record(record):
    packet, response = record['packet'], record['stop_response']
    kind, args, endpoints = command_info(packet)
    workspace = response['workspace']; wid = _uuid(workspace['id'])
    command, = packet['body']['runtimeConfig']['workspaceRuntime']['commands']
    operation(response, 'stop', wid, command)
    if any(workspace.get(k) != v for k, v in packet['body'].items()):
        raise ValueError(ERROR)
    op = response['operation']; sid = _uuid(op['metadata']['runtimeServiceId'])
    service, = [r for r in workspace['runtimeServices'] if r['id'] == sid]
    if (service['companyId'] != COMPANY or service['projectId'] != PROJECT or
            service['projectWorkspaceId'] != wid or service['scopeType'] != 'project_workspace' or
            service['scopeId'] != wid or service['executionWorkspaceId'] is not None or
            service['issueId'] is not None or service['provider'] != 'local_process' or
            service['lifecycle'] != 'shared' or service['status'] != 'stopped' or
            service['serviceName'] != command['name'] or service['command'] != command['command'] or
            service['cwd'] != command['cwd'] or service['port'] != command['port'] or
            service['url'] is not None or service.get('exposure') is not None or
            not _time(service['startedAt']) <= _time(service['stoppedAt']) <= _time(op['finishedAt']) or
            not _time(op['startedAt']) <= _time(service['stoppedAt'])):
        raise ValueError(ERROR)
    nonce = _nonce(record['challenge'].encode('ascii'))
    raw = record['pid_raw'].encode('ascii'); p = json.loads(raw)
    if kind in ('keys','web'):
        pids = (p['web_pid'],)
        if raw != _report(nonce, pids[0]): raise ValueError(ERROR)
    else:
        pids = (p['launcher_pid'], p['worker_pid'])
        if p != dict(schema='s3-local-pids/1', nonce=nonce, launcher_pid=pids[0], worker_pid=pids[1]):
            raise ValueError(ERROR)
        if raw != json.dumps(p,sort_keys=True,separators=(',',':')).encode(): raise ValueError(ERROR)
    if (any(type(p) is not int or not 2 <= p <= 2147483647 for p in pids) or
            len(set(pids)) != len(pids) or str(pids[0]) != service['providerRef'] or
            str(args.pid_mailbox) != record['mailbox_root']):
        raise ValueError(ERROR)
    return service, wid, kind, args, endpoints, pids


def _sync(directory):
    fd = os.open(directory, os.O_RDONLY|os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)


def _rename_exclusive(source, destination):
    # Current approved host: Darwin. Local SDK sys/stdio.h: RENAME_EXCL=4.
    # No overwrite fallback on an unsupported platform.
    rename = ctypes.CDLL(None, use_errno=True).renamex_np
    rename.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(os.fsencode(source), os.fsencode(destination), 4) != 0:
        raise OSError(ctypes.get_errno(), 'archive rename rejected')


def read_receipt(path):
    return _read_receipt(path, Path(path).parent)


def _read_receipt(path, published_root):
    path = Path(path); _root(path.parent)
    if path.name != 'receipt.json': raise ValueError(ERROR)
    fd = os.open(path, os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
    try:
        m = os.fstat(fd)
        if (not stat.S_ISREG(m.st_mode) or stat.S_IMODE(m.st_mode) != 0o600 or
                m.st_uid != os.getuid() or m.st_nlink != 1 or not 0 < m.st_size <= LIMIT):
            raise ValueError(ERROR)
        raw = os.read(fd, LIMIT+1)
        if len(raw) != m.st_size or os.read(fd,1): raise ValueError(ERROR)
    finally: os.close(fd)
    value = json.loads(raw)
    if (encode(value) != raw or value['schema'] != 's3-local-stopped-receipt/2' or
            value['archive_root'] != str(published_root) or
            _read(_root(path.parent/'mailbox'), 'challenge').decode() != value['challenge'] or
            _read(path.parent/'mailbox', 'pids.json').decode() != value['pid_raw']):
        raise ValueError(ERROR)
    if Path(path).parent == published_root:
        _archive_clear(value['mailbox_root'])
    service_record(value)
    return value, hashlib.sha256(raw).hexdigest()


def fresh_resources(record):
    import process_release, port_release, chain_writer_release, writer_release, preflight
    _, _, kind, a, endpoints, pids = service_record(record)
    reports = {'process': process_release.check(pids), 'port': port_release.check(endpoints)}
    if kind == 'chain':
        reports['writer'] = chain_writer_release.check(a.home)
    elif kind == 'worker':
        from offline_cli import parse
        command = record['packet']['body']['runtimeConfig']['workspaceRuntime']['commands'][0]
        _, arguments = parse(shlex.split(command['command'])[5:], reviewed=True, managed=True)
        raw, _ = preflight.verify_input_set(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,
            a.acknowledge_unproven_space,a.input_set.parent,a.input_set.name)
        reports['writer'] = writer_release.check(raw,a.artifacts,arguments,a.scratch)
    return reports


class Reentry:
    def __init__(self, packet, receipt_paths, *, transition=None):
        try:
            self.packet = copy.deepcopy(packet); command_info(self.packet)
            self.paths = tuple(Path(p) for p in receipt_paths)
            if not 1 <= len(self.paths) <= 32 or len(set(self.paths)) != len(self.paths): raise ValueError()
            self.records = [read_receipt(p) for p in self.paths]
            self.transition = copy.deepcopy(transition)
            self._checks = 0; self._closed = False; self.observations = []
            services = [service_record(r)[0]['id'] for r, _ in self.records]
            if len(set(services)) != len(services): raise ValueError()
            transitions = [] if transition is None else transition if isinstance(transition,list) else [transition]
            # Preserve the actual RC4 + RC6 stopped rows through RC7 preparation
            # and the final web command. Every edge still binds exact packets.
            if len(transitions)>3: raise ValueError()
            self.transitions=copy.deepcopy(transitions)
            for edge in self.transitions: self._edge(edge)
            if self.transitions:
                for left,right in zip(self.transitions,self.transitions[1:]):
                    if left['next'] != right['previous']: raise ValueError()
                if self.transitions[-1]['next'] != self.packet: raise ValueError()
            for r, _ in self.records: self._allowed(r['packet'])
        except Exception: raise ValueError(ERROR) from None

    def _edge(self, edge):
        if set(edge) != {'previous','next'}: raise ValueError(ERROR)
        previous,following=edge['previous'],edge['next']
        oldkind, old, _ = command_info(previous); kind, new, _ = command_info(following)
        if oldkind != 'keys' or kind not in ('keys','web'): raise ValueError(ERROR)
        for key in ('name','sourceType','isPrimary'):
            if previous['body'][key] != following['body'][key]: raise ValueError(ERROR)
        for key in ('web_origin','local_demo_profile','acknowledge_unproven_space'):
            if getattr(old,key) != getattr(new,key): raise ValueError(ERROR)
        if shlex.split(previous['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command'])[1] != shlex.split(following['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command'])[1]:
            raise ValueError(ERROR)
        if kind == 'web':
            if previous['body']['cwd'] != following['body']['cwd']: raise ValueError(ERROR)
            for key in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision',
                        'approval_socket','pid_mailbox'):
                if getattr(old,key) != getattr(new,key): raise ValueError(ERROR)
        else:
            # An explicitly reconfigured preparation workspace can move from a
            # revoked candidate to a new reviewed candidate. Never reuse its
            # home/key/mailbox, and never upgrade a trading worker implicitly.
            if previous['body']['cwd'] == following['body']['cwd']: raise ValueError(ERROR)
            for key in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision',
                        'approval_socket','pid_mailbox'):
                if getattr(old,key) == getattr(new,key): raise ValueError(ERROR)

    def _allowed(self, previous):
        if previous == self.packet: return
        for edge in self.transitions:
            if edge['previous'] == previous:
                previous=edge['next']
        if previous != self.packet: raise ValueError(ERROR)

    def _snapshot(self, packet, workspace):
        if self._closed or packet != self.packet: raise ValueError(ERROR)
        expected = {}
        for path, (record, digest) in zip(self.paths,self.records):
            if read_receipt(path) != (record,digest): raise ValueError(ERROR)
            service, wid, *_ = service_record(record)
            if workspace['id'] != wid: raise ValueError(ERROR)
            expected[service['id']] = service
        rows = workspace['runtimeServices']
        if len(rows) != len(expected) or {r['id']:r for r in rows} != expected: raise ValueError(ERROR)

    def check(self, packet, workspace):
        self._check(packet, workspace, fresh_resources, time.monotonic_ns)

    def _check(self, packet, workspace, probe, clock):
        try:
            if self._checks >= 2: raise ValueError()
            self._checks += 1
            self._snapshot(packet,workspace)
            started = clock(); observed = []
            for record,_ in self.records:
                _,_,kind,a,endpoints,pids = service_record(record)
                r = probe(record)
                if (r['process'].get('schema') != 's3-local-process-probe/1' or
                        r['port'].get('schema') != 's3-local-port-probe/1' or
                        r['process'].get('listed_pids_absent') is not True or
                        r['process'].get('pids') != list(pids) or
                        r['port'].get('simultaneous_bind_verified') is not True or
                        r['port'].get('endpoints') != list(endpoints)):
                    raise ValueError()
                if kind == 'chain' and (r['writer'].get('writer_lock_reacquired') is not True or
                        r['writer'].get('home') != str(a.home)): raise ValueError()
                if kind == 'worker' and r['writer'].get('writer_reopen_verified') is not True: raise ValueError()
                observed.append(copy.deepcopy(r))
            finished = clock()
            if not (type(started) is int and type(finished) is int and 0 <= started <= finished <= started+60_000_000_000): raise ValueError()
            self._snapshot(packet,workspace)
            self.observations.append(observed); self._last = finished
        except BaseException:
            self._closed = True
            raise

    def confirm(self, packet, listing, workspace_id):
        """Fresh authenticated GET after the final resource probe, before START."""
        try:
            row, = [w for w in listing if w.get('name') == packet['body']['name']]
            if sum(w.get('id') == workspace_id for w in listing) != 1 or row['id'] != workspace_id or any(row.get(k) is not None for k in (
                    'setupCommand','cleanupCommand','sharedWorkspaceKey','remoteProvider','remoteWorkspaceRef','repoUrl')):
                raise ValueError()
            if self._checks != 2 or not 0 <= time.monotonic_ns()-self._last <= 2_000_000_000: raise ValueError()
            # Validate all workspace properties without erasing service rows.
            self._snapshot(packet,row)
            if any(row.get(k) != v for k,v in packet['body'].items()): raise ValueError()
            if row['companyId'] != COMPANY or row['projectId'] != PROJECT: raise ValueError()
            self._closed = True
        except BaseException:
            self._closed = True
            raise

    def bind_mailbox(self, box):
        _, args, _ = command_info(self.packet)
        if (box.used or box.root != args.pid_mailbox or
                _nonce(_read(_root(box.root),'challenge')) != box.nonce or
                box.nonce in [r['challenge'] for r,_ in self.records] or
                set(p.name for p in box.root.iterdir()) != {'challenge'}):
            raise ValueError(ERROR)

    def prior_services(self):
        return {service_record(r)[0]['id']:service_record(r)[0] for r,_ in self.records}


def preserve(mailbox, packet, evidence):
    """Explicit successful-session archive; no deletion, retry, or new nonce here."""
    try:
        if (type(mailbox) not in (Mailbox, WebMailbox) or not mailbox.used or
                getattr(mailbox,'_archive_attempted',False) or
                evidence.get('control_plane_stop_verified') is not True or
                evidence.get('host_release_observations_complete') is not True or
                evidence.get('pid_handoff_collected') is not True): raise ValueError()
        mailbox._archive_attempted = True
        root = _root(mailbox.root); current = root.stat()
        if ((current.st_dev,current.st_ino) != (mailbox.identity.st_dev,mailbox.identity.st_ino) or
                set(p.name for p in root.iterdir()) != {'challenge','pids.json'}): raise ValueError()
        record = dict(schema='s3-local-stopped-receipt/2',packet=copy.deepcopy(packet),
            stop_response=copy.deepcopy(evidence['stop_response']),mailbox_root=str(root),
            challenge=_read(root,'challenge').decode(),pid_raw=_read(root,'pids.json').decode())
        if record['challenge'] != mailbox.nonce: raise ValueError()
        service,_,_,_,_,pids = service_record(record)
        if list(pids) != evidence['listed_pids'] or service['id'] != evidence['service_id']: raise ValueError()
        archive = _root(root.parent)/('completed-'+mailbox.nonce)
        pending = _archive_pending(root)
        receipt = archive/'receipt.json'
        record['archive_root'] = str(archive)
        pending.mkdir(mode=0o700)
        # Persist the reservation BEFORE the original path can be vacated.
        _sync(root.parent)
        raw = encode(record)
        if len(raw)>LIMIT: raise ValueError()
        _write(pending,'receipt.json',raw)
        _rename_exclusive(root,pending/'mailbox')
        _sync(pending)
        _sync(pending.parent)
        _read_receipt(pending/'receipt.json',archive)
        # Publication is the last filesystem operation. All evidence/reservation
        # syncs have succeeded. No fallible post-publication fsync/validation:
        # interruption after this atomic rename is a committed publication.
        # This final name is not fsynced: receipt availability after host power
        # loss is unproven. Missing/pending receipts deny reentry; no durable ACK.
        _rename_exclusive(pending,archive)
        return receipt
    except Exception: raise ValueError(ERROR) from None


def import_evidence(packet, stop_response, mailbox_root, destination):
    """Copy a completed prior run's raw evidence; never move its original home.

    This imports facts only. Reentry still requires fresh authenticated listing,
    current host probes and new approvals before START.
    """
    root = _root(mailbox_root); before=root.stat()
    record=dict(schema='s3-local-stopped-receipt/1',packet=copy.deepcopy(packet),
        stop_response=copy.deepcopy(stop_response),mailbox_root=str(root),
        challenge=_read(root,'challenge').decode(),pid_raw=_read(root,'pids.json').decode())
    service_record(record)
    if root.stat() != before: raise ValueError(ERROR)
    destination=Path(destination);_root(destination.parent)
    if destination.exists(): raise FileExistsError(destination)
    pending=destination.with_name('.'+destination.name+'.import-pending')
    pending.mkdir(mode=0o700)
    _sync(pending.parent)
    record['schema']='s3-local-stopped-receipt/2'
    record['archive_root']=str(destination)
    box=pending/'mailbox';box.mkdir(mode=0o700)
    _write(box,'challenge',record['challenge'].encode());_write(box,'pids.json',record['pid_raw'].encode())
    _write(pending,'receipt.json',encode(record))
    _sync(pending.parent)
    _read_receipt(pending/'receipt.json',destination)
    receipt=destination/'receipt.json'
    _rename_exclusive(pending,destination)
    return receipt
