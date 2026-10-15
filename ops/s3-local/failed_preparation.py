"""Explicit, review-scoped L-T-RC8-01 failure preservation; never a v2 receipt.

Only this independently investigated, public-assets-only preparation incident
is supported. No general providerRef -> child PID conversion or cleanup grant.
Fresh authenticated listing, host observations and NEW approvals remain needed.
"""
import copy
import hashlib
from pathlib import Path

from manifest import decode, read_regular
from pid_mailbox import _root, _read, _write, _archive_clear
from preflight import bounded

INCIDENT_SHA256 = '2b64ebc89b3a0808f373f2e4f196aadd5c5cea87e750cb0979ebd9061c8cc2e5'
ERROR = 'FAILED_PREPARATION_REJECTED'
SID = '89cf8ef6-e801-48b4-bc81-95da712047f5'


def incident(raw):
    if hashlib.sha256(raw).hexdigest() != INCIDENT_SHA256:
        raise ValueError(ERROR)
    from workspace_reentry import command_info, service_record
    value = decode(raw)
    kind, args, _ = command_info(value['packet'])
    if kind != 'keys': raise ValueError(ERROR)
    row, = [r for r in value['workspace']['runtimeServices'] if r['id'] == SID]
    prior = {service_record(r)[0]['id']: service_record(r)[0] for r in value['prior_receipts']}
    changed = {k for k in row if row[k] != prior[SID].get(k)}
    if changed != {'providerRef', 'startedAt', 'stoppedAt', 'updatedAt', 'lastUsedAt'}:
        raise ValueError(ERROR)
    rest = [r for r in value['workspace']['runtimeServices'] if r['id'] != SID]
    if len(rest) != 2 or {r['id']:r for r in rest} != {k:v for k,v in prior.items() if k != SID}:
        raise ValueError(ERROR)
    if row['providerRef'] != '47145' or value['challenge'] in [r['challenge'] for r in value['prior_receipts']]:
        raise ValueError(ERROR)
    return value, row, args


def original(value, args):
    """Original failed mailbox is always read-only, including on reentry."""
    root = _root(args.pid_mailbox)
    _archive_clear(root)
    if (set(p.name for p in root.iterdir()) != {'challenge'} or
            _read(root, 'challenge').decode() != value['challenge']):
        raise ValueError(ERROR)


def preserve(destination):
    """Copy pinned facts into a NEW archive. Never rename the failed mailbox."""
    from workspace_reentry import _sync, _rename_exclusive, encode, read_receipt
    raw = read_regular(Path(__file__).parent, 'fixtures/failed-preparation-rc8/incident.json')
    value, _, args = incident(raw)
    original(value, args)
    # Preserve all three normal receipts, including the superseded service row.
    for expected in value['prior_receipts']:
        if read_receipt(Path(expected['archive_root'])/'receipt.json')[0] != expected:
            raise ValueError(ERROR)
    destination = Path(destination)
    _root(destination.parent)
    if destination.name != 'failed-' + value['challenge'] or destination.exists():
        raise ValueError(ERROR)
    pending = destination.with_name('.' + destination.name + '.pending')
    pending.mkdir(mode=0o700)
    _sync(pending.parent)
    _write(pending, 'incident.json', raw)
    _write(pending, 'challenge', value['challenge'].encode())
    record = dict(schema='s3-local-failed-preparation-archive/1', archive_root=str(destination),
                  incident_sha256=INCIDENT_SHA256, normal_receipt=False,
                  pid_handoff='MISSING', cleanup_complete_verified=False)
    _write(pending, 'failure.json', encode(record))
    _sync(pending)
    _sync(pending.parent)
    original(value, args)
    _load(pending/'failure.json', destination)
    _rename_exclusive(pending, destination)  # Final publication; no post-rename IO.
    return destination/'failure.json'


def _load(path, published):
    from workspace_reentry import encode
    path = Path(path); root = _root(path.parent)
    raw = bounded(root, 'incident.json', 131072)
    value, row, args = incident(raw)
    record_raw = bounded(root, 'failure.json', 2048)
    record = decode(record_raw)
    expected = dict(schema='s3-local-failed-preparation-archive/1', archive_root=str(published),
                    incident_sha256=INCIDENT_SHA256, normal_receipt=False,
                    pid_handoff='MISSING', cleanup_complete_verified=False)
    if (path.name != 'failure.json' or record != expected or record_raw != encode(record) or
            set(p.name for p in root.iterdir()) != {'incident.json','failure.json','challenge'} or
            _read(root, 'challenge').decode() != value['challenge'] or
            published.name != 'failed-'+value['challenge']):
        raise ValueError(ERROR)
    original(value, args)
    return value, row, args


class Failure:
    def __init__(self, path):
        self.path = Path(path)
        self.value, self.row, self.args = _load(self.path, self.path.parent)
        try: self.path.parent.with_name('.'+self.path.parent.name+'.pending').lstat()
        except FileNotFoundError: pass
        else: raise ValueError(ERROR)

    def check(self, normal_records, transitions):
        try:self.path.parent.with_name('.'+self.path.parent.name+'.pending').lstat()
        except FileNotFoundError:pass
        else:raise ValueError(ERROR)
        if _load(self.path, self.path.parent) != (self.value,self.row,self.args):
            raise ValueError(ERROR)
        # argparse Namespace equality also binds the exact parsed command.
        by_sid = {}
        from workspace_reentry import service_record, command_info
        for r, _ in normal_records: by_sid[service_record(r)[0]['id']] = r
        for previous in self.value['prior_receipts']:
            sid = service_record(previous)[0]['id']
            # Explicit imports may relocate a NORMAL completed archive. All
            # original packet/mailbox/nonce/PID/STOP fields must remain exact.
            observed=by_sid.get(sid,{})
            if {k:v for k,v in observed.items() if k!='archive_root'} != {
                    k:v for k,v in previous.items() if k!='archive_root'}:
                raise ValueError(ERROR)
        # A fresh reviewed keys->keys candidate is mandatory. Same pin/path,
        # direct keys->web, empty history and duplicate workspace cannot pass.
        if not any(edge['previous'] == self.value['packet'] and
                   command_info(edge['next'])[0] == 'keys'
                   for edge in transitions):
            raise ValueError(ERROR)

    def probe(self):
        import process_release, port_release
        # These are CONTROL-PLANE PID observations, explicitly not child handoff.
        pids = [15632, 47145]
        ports = [('127.0.0.1',5173)]
        process = process_release.check(pids)
        port = port_release.check(ports)
        if (process.get('schema') != 's3-local-process-probe/1' or
                process.get('pids') != pids or process.get('listed_pids_absent') is not True or
                port.get('schema') != 's3-local-port-probe/1' or
                port.get('endpoints') != ports or port.get('simultaneous_bind_verified') is not True):
            raise ValueError(ERROR)
        return dict(process=copy.deepcopy(process), port=copy.deepcopy(port),
                    pid_source='reviewed_control_plane_history', pid_handoff='MISSING',
                    normal_receipt=False, cleanup_complete_verified=False)
