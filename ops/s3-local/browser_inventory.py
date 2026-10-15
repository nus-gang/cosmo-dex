"""Immutable browser events and PID observations; no full descendant guarantee."""
import hashlib
from pathlib import Path
from preflight import bounded
from pid_mailbox import _root
from manifest import decode


def read(root):
    root=_root(root);parent=decode(bounded(root,'parent.json',4096))
    closed=decode(bounded(root,'parent-closed.json',4096))
    if any(p.name.endswith('.pending') for p in root.iterdir()):raise ValueError('BROWSER_PUBLICATION_INCOMPLETE')
    names=sorted(p.name for p in root.iterdir() if p.name[:1].isdigit())
    if not 1<=len(names)<=10000 or names!=[f'{i:05d}.json' for i in range(len(names))]:raise ValueError('BROWSER_EVENTS')
    raw=[bounded(root,n,65536) for n in names];events=[decode(r) for r in raw]
    if parent.get('schema')!='s3-local-browser-parent/1':raise ValueError('BROWSER_PARENT')
    previous=-1
    for e in events:
        at=e.get('monotonic_ns')
        if (e.get('schema')!='s3-local-browser-lifetime/1' or e.get('phase') not in
            ('spawned','tab_ready','observed','http','keys_held','active','failed','closed') or
            not isinstance(at,str) or not at.isdecimal() or int(at)<previous or
            e.get('keys_exported') is not False or e.get('private_key_storage') is not False):raise ValueError('BROWSER_EVENT')
        previous=int(at)
    if events[-1]['phase']!='closed' or events[-1]['keys_discarded'] is not True or closed['node_pid']!=parent['node_pid']:
        raise ValueError('BROWSER_NOT_CLOSED')
    pids=[parent['parent_pid'],parent['node_pid'],events[-1]['browser_pid']]
    if len(set(pids))!=3 or any(type(p)is not int or not 2<=p<=2147483647 for p in pids):raise ValueError('BROWSER_PID')
    if any(e['pid']!=pids[1] or e['parent_pid']!=pids[0] or e['browser_pid']!=pids[2] for e in events):raise ValueError('BROWSER_PID')
    digest=hashlib.sha256(bounded(root,'parent.json',4096)+bounded(root,'parent-closed.json',4096)+b''.join(raw)).hexdigest()
    return dict(pids=pids,trace_sha256=digest,normal_session=closed['parent_error'] is False and closed['exit_code']==0 and not any(e['phase']=='failed' for e in events),
                cleanup_complete_verified=False)
