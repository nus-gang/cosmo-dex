#!/usr/bin/env python3
"""Explicit L-T session/reentry from exact registered packets; never registers.

All modes retain the native/canonical approval checks. Prior receipts are
observations, not permits. Successful sessions archive evidence before another
invocation can create a fresh mailbox at the same path.
"""
import json
from pathlib import Path
import shlex
import sys
import time
from contextlib import contextmanager

import approval_gate
from authenticated_session import session as worker_session
from authenticated_chain_session import session as chain_session
from authenticated_web_session import session as web_session
from launcher_signal import stop_latch
from native_review import _uuid
from pid_mailbox import Mailbox
from prepared_session import PreparedSession
from private_reader import broker_at
from runtime_client import RuntimeClient
from web_pid_mailbox import WebMailbox
from workspace_reentry import command_info, Reentry, preserve, import_evidence

ERROR = 'LOCAL_REENTRY_SESSION_REJECTED'


def load(path):
    import preflight
    from manifest import decode
    path=Path(path)
    raw=preflight.bounded(preflight.checked_root(path.parent),path.name,1024*1024)
    spec=decode(raw)
    if (set(spec) != {'schema','packet','workspace_id','duration_seconds','receipts','transition'} or
            spec['schema'] != 's3-local-reentry-session/1' or
            type(spec['duration_seconds']) is not int or not 1 <= spec['duration_seconds'] <= 240 or
            not isinstance(spec['receipts'],list) or len(spec['receipts'])>32): raise ValueError(ERROR)
    _uuid(spec['workspace_id']);command_info(spec['packet'])
    if not spec['receipts'] and spec['transition'] is not None: raise ValueError(ERROR)
    return spec


@contextmanager
def key_session(python,candidate,argv,workspace,packet,box,fee,reentry,stop):
    import web_session as web
    _,a,_=command_info(packet)
    def inspect():
        if stop():raise ValueError(ERROR)
        return approval_gate.inspect(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,
            a.acknowledge_unproven_space,a.native_decision_id,
            {'ceo':a.ceo_revision,'cto':a.cto_revision})
    before=inspect()
    def audit():
        if inspect()!=before or stop():raise ValueError(ERROR)
    with web.session(python,candidate,argv,fee_bps=fee,mailbox=box,
            broker_root=a.approval_socket.parent,broker=broker_at,
            client=RuntimeClient.from_environment(workspace,'s3-web-fee'+str(fee)),
            audit=audit,reentry=reentry,key_packet=packet) as evidence:
        yield evidence


def run(spec, stop):
    packet=spec['packet'];kind,a,_=command_info(packet)
    command,=packet['body']['runtimeConfig']['workspaceRuntime']['commands']
    words=shlex.split(command['command']);python=words[1];candidate=packet['body']['cwd'];argv=words[5:]
    fee=int(command['id'].split('-fee')[1].split('-v')[0])
    reentry=(Reentry(packet,spec['receipts'],transition=spec['transition']) if spec['receipts'] else None)
    if stop():raise ValueError(ERROR)
    if kind=='worker':
        prepared=PreparedSession(python,candidate,argv,fee_bps=fee);box=prepared._mailbox
        ctx=worker_session(prepared,spec['workspace_id'],stop=stop,reentry=reentry)
    else:
        box=(Mailbox if kind=='chain' else WebMailbox)(a.pid_mailbox)
        if kind=='chain':
            ctx=chain_session(python,candidate,argv,spec['workspace_id'],fee_bps=fee,
                validator_index=int(command['id'].split('-v')[1]),mailbox=box,stop=stop,reentry=reentry)
        elif kind=='web':
            ctx=web_session(python,candidate,argv,spec['workspace_id'],fee_bps=fee,
                mailbox=box,stop=stop,reentry=reentry)
        else:
            ctx=key_session(python,candidate,argv,spec['workspace_id'],packet,box,fee,reentry,stop)
    with ctx as evidence:
        previous=time.monotonic();deadline=previous+spec['duration_seconds']
        while not stop():
            now=time.monotonic()
            if now<previous:raise ValueError(ERROR)
            if now>=deadline:break
            previous=now;time.sleep(min(0.1,deadline-now))
    receipt=preserve(box,packet,evidence)
    return dict(schema='s3-local-reentry-result/1',receipt=str(receipt),
        service_id=evidence['service_id'],approval_verified=False,
        cleanup_complete_verified=False,DEV='NOT_RUN')


def main(argv=None):
    try:
        argv=list(sys.argv[1:] if argv is None else argv)
        if len(argv)==3 and argv[:2]==['import-stopped-evidence','--request']:
            import preflight
            from manifest import decode
            path=Path(argv[2])
            spec=decode(preflight.bounded(preflight.checked_root(path.parent),path.name,1024*1024))
            if set(spec) != {'packet','stop_response','mailbox_root','destination'}:raise ValueError(ERROR)
            receipt=import_evidence(**spec)
            print(json.dumps(dict(receipt=str(receipt),approval_verified=False,service_started=False),sort_keys=True))
            return 0
        if len(argv)!=3 or argv[:2]!=['run-reentry-reviewed','--request']:raise ValueError(ERROR)
        spec=load(argv[2])
        with stop_latch() as stop:result=run(spec,stop)
        print(json.dumps(result,sort_keys=True));return 0
    except (Exception,KeyboardInterrupt):
        print(ERROR,file=sys.stderr);return 2


if __name__=='__main__':sys.exit(main())
