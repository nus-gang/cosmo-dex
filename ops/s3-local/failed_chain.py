"""RC9 BOOTSTRAP-01/STOP-02 archive, explicitly NOT a normal receipt.

The four original mailboxes, empty entity, failed attempts and later observations
remain separate pinned facts. Fresh observations and new approval are mandatory.
"""
import copy
import hashlib
from pathlib import Path
from manifest import decode, read_regular
from preflight import bounded
from pid_mailbox import _root, _read, _write, _archive_clear

INCIDENT_SHA256='b34cc16f5723c13275b5eea259fde11939c8b1dcc7f551e3a145fb512626fc08'
ERROR='FAILED_CHAIN_RC9_REJECTED'
FIXTURE=Path(__file__).parent/'fixtures/failed-chain-rc9'


def incident(raw):
    if hashlib.sha256(raw).hexdigest()!=INCIDENT_SHA256:raise ValueError(ERROR)
    value=decode(raw)
    from workspace_reentry import service_record
    if [n['index'] for n in value['nodes']]!=list(range(4)):raise ValueError(ERROR)
    for n in value['nodes']:
        _,wid,kind,*_=service_record(n)
        if kind!='chain' or wid!=n['workspace_id']:raise ValueError(ERROR)
    return value


def originals(value):
    for n in value['nodes']:
        root=_root(n['mailbox_root']);_archive_clear(root)
        if (set(p.name for p in root.iterdir())!={'challenge','pids.json'} or
                _read(root,'challenge').decode()!=n['challenge'] or
                _read(root,'pids.json').decode()!=n['pid_raw']):raise ValueError(ERROR)


def preserve(destination):
    from workspace_reentry import _sync,_rename_exclusive,encode
    raw=read_regular(FIXTURE,'incident.json');value=incident(raw);originals(value)
    destination=Path(destination);_root(destination.parent)
    if destination.name!='failed-chain-rc9-'+INCIDENT_SHA256[:16] or destination.exists():raise ValueError(ERROR)
    pending=destination.with_name('.'+destination.name+'.pending');pending.mkdir(mode=0o700)
    _sync(pending.parent);_write(pending,'incident.json',raw)
    evidence=pending/'evidence';evidence.mkdir(mode=0o700)
    for name,digest in value['files_sha256'].items():
        raw=read_regular(FIXTURE/'evidence',name)
        if hashlib.sha256(raw).hexdigest()!=digest:raise ValueError(ERROR)
        target=evidence/name;target.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
        _write(_root(target.parent),target.name,raw);_sync(target.parent)
    _write(pending,'failure.json',encode(dict(schema='s3-local-failed-chain-archive/1',
        archive_root=str(destination),incident_sha256=INCIDENT_SHA256,normal_receipt=False,
        historical_failure_preserved=True,cleanup_complete_verified=False)))
    _sync(evidence);_sync(pending);_sync(pending.parent)
    load(pending/'failure.json',destination)
    _rename_exclusive(pending,destination)
    return destination/'failure.json'


def load(path,published):
    from workspace_reentry import encode
    path=Path(path);root=_root(path.parent)
    if path.name!='failure.json':raise ValueError(ERROR)
    raw=bounded(root,'incident.json',131072);value=incident(raw)
    expected=dict(schema='s3-local-failed-chain-archive/1',archive_root=str(published),
        incident_sha256=INCIDENT_SHA256,normal_receipt=False,
        historical_failure_preserved=True,cleanup_complete_verified=False)
    if bounded(root,'failure.json',2048)!=encode(expected):raise ValueError(ERROR)
    actual=set(p.relative_to(root/'evidence').as_posix() for p in (root/'evidence').rglob('*') if not p.is_dir())
    if actual!=set(value['files_sha256']) or set(p.name for p in root.iterdir())!={'evidence','incident.json','failure.json'}:
        raise ValueError(ERROR)
    for name,digest in value['files_sha256'].items():
        # read_regular rejects symlinks/FIFOs in every path segment.
        if hashlib.sha256(read_regular(root/'evidence',name)).hexdigest()!=digest:raise ValueError(ERROR)
    if published.name!='failed-chain-rc9-'+INCIDENT_SHA256[:16]:raise ValueError(ERROR)
    originals(value)
    return value


class Failure:
    def __init__(self,path,packet):
        from workspace_reentry import service_record
        self.path=Path(path);self.all=load(self.path,self.path.parent)
        self.value,= [n for n in self.all['nodes'] if n['packet']['body']['name']==packet['body']['name']]
        self.row,_,_,self.args,*_=service_record(self.value)
        self._pending()

    def _pending(self):
        try:self.path.parent.with_name('.'+self.path.parent.name+'.pending').lstat()
        except FileNotFoundError:return
        raise ValueError(ERROR)

    def check(self,normal_records,transitions):
        self._pending()
        if load(self.path,self.path.parent)!=self.all or normal_records or len(transitions)!=1 or transitions[0]['previous']!=self.value['packet']:
            raise ValueError(ERROR)

    def probe(self):
        from workspace_reentry import fresh_resources,service_record
        r=fresh_resources(self.value)
        _,_,_,a,endpoints,pids=service_record(self.value)
        if (r['process'].get('pids')!=list(pids) or r['process'].get('listed_pids_absent') is not True or
                r['port'].get('endpoints')!=list(endpoints) or r['port'].get('simultaneous_bind_verified') is not True or
                r['writer'].get('home')!=str(a.home) or r['writer'].get('writer_lock_reacquired') is not True):
            raise ValueError(ERROR)
        return dict(observations=copy.deepcopy(r),normal_receipt=False,
                    historical_failure_preserved=True,cleanup_complete_verified=False)
