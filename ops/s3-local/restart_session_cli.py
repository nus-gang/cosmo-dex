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
from preparation_status import wait_ready

ERROR = 'LOCAL_REENTRY_SESSION_REJECTED'


def load(path):
    import preflight
    from manifest import decode
    path=Path(path)
    raw=preflight.bounded(preflight.checked_root(path.parent),path.name,1024*1024)
    spec=decode(raw)
    fields={'schema','packet','workspace_id','duration_seconds','receipts','transition'}
    if spec.get('schema')=='s3-local-reentry-session/2':fields.add('failed_preparations')
    if (set(spec) != fields or
            spec['schema'] not in ('s3-local-reentry-session/1','s3-local-reentry-session/2') or
            type(spec['duration_seconds']) is not int or not 1 <= spec['duration_seconds'] <= 240 or
            not isinstance(spec['receipts'],list) or len(spec['receipts'])>32): raise ValueError(ERROR)
    _uuid(spec['workspace_id']);command_info(spec['packet'])
    failures=spec.get('failed_preparations',[])
    if not isinstance(failures,list) or len(failures)>1 or (failures and not spec['receipts']):raise ValueError(ERROR)
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
            audit=audit,reentry=reentry,key_packet=packet,
            final_authorize=approval_gate.final_authorizer(before,stop)) as evidence:
        yield evidence


def run(spec, stop):
    packet=spec['packet'];kind,a,_=command_info(packet)
    command,=packet['body']['runtimeConfig']['workspaceRuntime']['commands']
    words=shlex.split(command['command']);python=words[1];candidate=packet['body']['cwd'];argv=words[5:]
    fee=int(command['id'].split('-fee')[1].split('-v')[0])
    reentry=(Reentry(packet,spec['receipts'],transition=spec['transition'],
        failed_preparations=spec.get('failed_preparations',())) if spec['receipts'] else None)
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
    outcome='preparing'
    destination=None
    if kind in ('keys','web'):
        from pid_mailbox import _write, _root
        from workspace_reentry import encode
        destination=box.root.parent/('session-'+box.nonce)
        destination.mkdir(mode=0o700)
    try:
        with ctx as evidence:
            if kind in ('keys','web'):
                evidence['http_readiness']=wait_ready(box,a.web_origin,stop,60)
                _write(_root(destination),'readiness.json',encode(evidence['http_readiness']))
            outcome='holding'
            previous=time.monotonic();deadline=previous+spec['duration_seconds']
            while not stop():
                now=time.monotonic()
                if not previous <= now < float('inf'):raise ValueError(ERROR)
                if now>=deadline:break
                previous=now;time.sleep(min(0.1,deadline-now))
            if stop():outcome='operator_stop'
            else:outcome='hold_completed'
    finally:
        # A failed run remains a failed run, even if STOP/release succeeded.
        # Preserve exact control responses separately; never manufacture a v2 receipt.
        if kind in ('keys','web'):
            _write(_root(destination),'outcome.json',encode(dict(
                schema='s3-local-session-observation/1',phase=outcome,nonce=box.nonce,
                packet=packet,workspace_id=spec['workspace_id'],
                evidence=getattr(box,'session_evidence',{}),
                normal_receipt=False,cleanup_complete_verified=False)))
    receipt=preserve(box,packet,evidence)
    return dict(schema='s3-local-reentry-result/1',receipt=str(receipt),
        service_id=evidence['service_id'],approval_verified=False,
        cleanup_complete_verified=False,DEV='NOT_RUN')


def main(argv=None):
    try:
        argv=list(sys.argv[1:] if argv is None else argv)
        if len(argv)==3 and argv[:2]==['preserve-reviewed-preparation-failure','--destination']:
            from failed_preparation import preserve as preserve_failure
            path=preserve_failure(Path(argv[2]))
            print(json.dumps(dict(failure=str(path),normal_receipt=False,
                approval_verified=False,service_started=False),sort_keys=True));return 0
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
