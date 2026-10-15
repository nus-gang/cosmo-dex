"""Exact RC10 pre-key browser incident. Never convert a failure to a receipt.

Original mailbox/events and four prior rows are retained. New candidate authority
and current resources are still checked by Reentry; descendant closure is unproven.
"""
import copy
import hashlib
from pathlib import Path
from manifest import decode, read_regular
from preflight import bounded
from pid_mailbox import _root, _read, _write, _archive_clear

INCIDENT_SHA256='de759c7ae79884e2110741552d3153a9e61b095468d7f8f4e69aabaeaa2bc82d'
ERROR='FAILED_BROWSER_RC10_REJECTED'
FIXTURE=Path(__file__).parent/'fixtures/failed-browser-rc10'


def incident(raw):
    if hashlib.sha256(raw).hexdigest()!=INCIDENT_SHA256:raise ValueError(ERROR)
    from workspace_reentry import service_record
    from browser_inventory import read
    value=decode(raw);record=value['record'];row,wid,kind,args,*_=service_record(record)
    if (kind!='browser' or wid!='ccc69949-0a0f-4bdf-9253-9491b26d9269' or
            row['id']!='99e45ba4-df12-468f-ae99-4c9ce6c79b05' or
            args.runtime_pin!='6b8aa5bebb8cd64ba236459dc088aff599c5d09bba02c4f80a4a2ee786bf0c10' or
            read(args.browser_control)['normal_session'] is not False):raise ValueError(ERROR)
    return value


def originals(value):
    record=value['record'];root=_root(record['mailbox_root']);_archive_clear(root)
    if (set(p.name for p in root.iterdir())!={'challenge','pids.json'} or
            _read(root,'challenge').decode()!=record['challenge'] or _read(root,'pids.json').decode()!=record['pid_raw']):raise ValueError(ERROR)
    browser=_root(value['original_browser_root'])
    if set(p.name for p in browser.iterdir())!=set(value['original_event_files'])|{'commands','results'}:raise ValueError(ERROR)
    for folder in ('commands','results'):
        if list(_root(browser/folder).iterdir()):raise ValueError(ERROR)
    for name in value['original_event_files']:
        if hashlib.sha256(bounded(browser,name,65536)).hexdigest()!=value['files_sha256']['browser-events/'+name]:raise ValueError(ERROR)


def preserve(destination):
    from workspace_reentry import _sync,_rename_exclusive,encode
    raw=read_regular(FIXTURE,'incident.json');value=incident(raw);originals(value)
    destination=Path(destination);_root(destination.parent)
    if destination.name!='failed-browser-rc10-'+INCIDENT_SHA256[:16] or destination.exists():raise ValueError(ERROR)
    pending=destination.with_name('.'+destination.name+'.pending');pending.mkdir(mode=0o700);_sync(pending.parent)
    _write(pending,'incident.json',raw);evidence=pending/'evidence';evidence.mkdir(mode=0o700)
    for name,digest in value['files_sha256'].items():
        raw=read_regular(FIXTURE/'evidence',name)
        if hashlib.sha256(raw).hexdigest()!=digest:raise ValueError(ERROR)
        target=evidence/name;target.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
        _write(_root(target.parent),target.name,raw);_sync(target.parent)
    _write(pending,'failure.json',encode(dict(schema='s3-local-failed-browser-archive/1',archive_root=str(destination),
        incident_sha256=INCIDENT_SHA256,normal_receipt=False,original_cause='UNPROVEN',cleanup_complete_verified=False)))
    _sync(evidence);_sync(pending);load(pending/'failure.json',destination);_rename_exclusive(pending,destination)
    return destination/'failure.json'


def load(path,published):
    from workspace_reentry import encode
    path=Path(path);root=_root(path.parent)
    value=incident(bounded(root,'incident.json',131072));originals(value)
    expected=dict(schema='s3-local-failed-browser-archive/1',archive_root=str(published),
        incident_sha256=INCIDENT_SHA256,normal_receipt=False,original_cause='UNPROVEN',cleanup_complete_verified=False)
    if (path.name!='failure.json' or bounded(root,'failure.json',2048)!=encode(expected) or
            published.name!='failed-browser-rc10-'+INCIDENT_SHA256[:16] or
            set(p.name for p in root.iterdir())!={'evidence','incident.json','failure.json'}):raise ValueError(ERROR)
    actual={p.relative_to(root/'evidence').as_posix() for p in (root/'evidence').rglob('*') if not p.is_dir()}
    if actual!=set(value['files_sha256']):raise ValueError(ERROR)
    for name,digest in value['files_sha256'].items():
        if hashlib.sha256(read_regular(root/'evidence',name)).hexdigest()!=digest:raise ValueError(ERROR)
    return value


class Failure:
    def __init__(self,path):
        from workspace_reentry import service_record
        self.path=Path(path);self.all=load(self.path,self.path.parent);self.value=self.all['record']
        self.row,_,_,self.args,*_=service_record(self.value);self._pending()

    def _pending(self):
        try:self.path.parent.with_name('.'+self.path.parent.name+'.pending').lstat()
        except FileNotFoundError:return
        raise ValueError(ERROR)

    def check(self,normal_records,transitions):
        self._pending()
        if (load(self.path,self.path.parent)!=self.all or not transitions or
                transitions[-1]['previous']!=self.value['packet']):raise ValueError(ERROR)

    def prior(self,rows):
        expected={r['id']:r for r in self.all['prior_services']}
        if len(expected)!=4 or rows!=expected or self.row['id'] in rows:raise ValueError(ERROR)

    def probe(self):
        from workspace_reentry import fresh_resources,service_record
        r=fresh_resources(self.value);*_,endpoints,pids=service_record(self.value)
        if (r['process'].get('schema')!='s3-local-process-probe/1' or r['process'].get('pids')!=list(pids) or
                r['process'].get('listed_pids_absent') is not True or r['port'].get('schema')!='s3-local-port-probe/1' or
                r['port'].get('endpoints')!=list(endpoints) or r['port'].get('simultaneous_bind_verified') is not True):raise ValueError(ERROR)
        return dict(observations=copy.deepcopy(r),normal_receipt=False,original_cause='UNPROVEN',cleanup_complete_verified=False)
