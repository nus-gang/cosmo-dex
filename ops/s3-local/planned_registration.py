"""Key-independent 12-command plan. Existing workspace IDs are retained.

All paths, gates, ports, lifetimes and executables are fixed BEFORE key creation.
Only public node IDs are read later; approved B validates them against the homes.
"""
from pathlib import Path
import copy
import hashlib
import json
import re
from workspace_registration import prepare,prepare_chain,PROJECT,registered
from runtime_config import shell_command
from browser_web_cli import parse as parse_browser
from preflight import checked_root,bounded
from manifest import decode
from pid_mailbox import _write


def browser_packet(python,candidate,argv,fee_bps):
    if fee_bps not in (0,25):raise ValueError('FEE')
    a,*_=parse_browser(['serve-browser-reviewed',*argv])
    name=f's3-web-fee{fee_bps}'
    command=shell_command([python,'-B',str(Path(candidate)/'ops/s3-local/browser_web_cli.py'),'serve-browser-reviewed',*argv])
    body=dict(name=f'nus-s3-local-web-fee{fee_bps}',sourceType='local_path',cwd=candidate,isPrimary=False,
        runtimeConfig=dict(desiredState='manual',serviceStates={'0':'manual'},workspaceRuntime={'commands':[
            dict(id=name,name=name,kind='service',command=command,cwd=candidate,port=5173,lifecycle='shared',reuseScope='project_workspace')]}))
    return dict(method='POST',path=f'/api/projects/{PROJECT}/workspaces',body=body,
                requires_board_registration=True,approval_verified=False,starts_service=False)


def plan(spec):
    if set(spec)!={'schema','python','candidate','bundle','artifacts','runtime_pin','native_decision_id',
                  'ceo_revision','cto_revision','control_root','profiles'} or spec['schema']!='s3-local-initializer-registration/1' or set(spec['profiles'])!={'fee0','fee25'}:
        raise ValueError('PLAN_INPUT')
    for p in [spec[k] for k in ('python','candidate','bundle','artifacts','control_root')]+list(spec['profiles'].values()):
        if not Path(p).is_absolute() or '..' in Path(p).parts:raise ValueError('PLAN_PATH')
    roots=[Path(spec['control_root']),*(Path(p) for p in spec['profiles'].values())]
    if any(p==q or p in q.parents or q in p.parents for i,p in enumerate(roots) for q in roots[i+1:]):raise ValueError('PLAN_ISOLATION')
    common=[]
    for k in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision'):
        common+=['--'+k.replace('_','-'),spec[k]]
    common+=['--local-demo-profile','s3-dev-local/1','--acknowledge-unproven-space','--lifetime-seconds','300']
    packets=[]
    for fee,base in ((0,26656),(25,26756)):
        root=Path(spec['profiles'][f'fee{fee}']);control=Path(spec['control_root'])/f'fee{fee}'
        inherited=common+['--input-set',str(root/'input.json'),'--effective-profile',str(root/'effective-profile.json')]
        def ipc(kind):
            p=control/kind
            if len(str(p/'b/s').encode())>103:raise ValueError('UNIX_SOCKET_PATH_TOO_LONG')
            return ['--scratch',str(p/'scratch'),'--approval-socket',str(p/'b/s'),'--pid-mailbox',str(p/'mail')]
        worker=inherited+['--home',str(root/'engine'),'--key-directory',str(root/'authority/operator-0'),
            '--bind',f'127.0.0.1:{18080+int(fee==25)}','--rpc',f'127.0.0.1:{base+1}','--max-requests','10000','--max-ticks','10000']
        packets.append(prepare(spec['python'],spec['candidate'],worker+ipc('worker'),fee_bps=fee))
        packets.append(browser_packet(spec['python'],spec['candidate'],worker+ipc('web')+
            ['--web-origin','http://127.0.0.1:5173','--browser-control',str(control/'web/browser')],fee))
        for i in range(4):
            argv=inherited+ipc(f'v{i}')+['--home',str(root/f'validator-{i}'),'--rpc',f'127.0.0.1:{base+2*i+1}',
                '--p2p',f'127.0.0.1:{base+2*i}','--topology-root',str(root)]
            packets.append(prepare_chain(spec['python'],spec['candidate'],argv,fee_bps=fee,validator_index=i))
    return dict(schema='s3-local-planned-registration/1',packets=packets,packet_count=12,
        key_creation_required=False,board_change_after_keys=False,starts_service=False,
        packet_sha256=hashlib.sha256(json.dumps(packets,sort_keys=True,separators=(',',':')).encode()).hexdigest())


def publish_topology(root):
    """L-T after the approved initializer has published its public report."""
    root=checked_root(root);r=decode(bounded(root,'initialization.json',16384))
    if r['schema']!='s3-local-initialization/1' or r['root']!=str(root) or r['service_started'] is not False or r['c_semantic_validation_verified'] is not True:
        raise ValueError('INITIALIZATION')
    from workspace_reentry import encode
    value=dict(schema='s3-local-launch-topology/1',root=str(root),runtime_pin=r['runtime_pin'],fee_bps=int(r['fee_bps']),node_ids=r['node_ids'])
    # Validate by the normal parser/resolver before any service invocation.
    if (value['fee_bps'] not in (0,25) or not re.fullmatch('[0-9a-f]{64}',value['runtime_pin']) or
        len(value['node_ids'])!=4 or len(set(value['node_ids']))!=4 or
        any(not isinstance(i,str) or not re.fullmatch('[0-9a-f]{40}',i) for i in value['node_ids'])):raise ValueError('TOPOLOGY')
    _write(root,'launch-topology.json',encode(value))
    return value


def board_ready(planned,listing,history_factory):
    """One fresh authenticated project listing; no callback creates any key.

    history_factory returns a fresh Reentry per call. Exact stopped records are
    used for occupied workspaces. No row
    is removed to pretend the 12 completed registrations were empty/new.
    """
    if planned['packet_count']!=12 or len(planned['packets'])!=12:raise ValueError('REGISTRATIONS')
    result=[]
    for packet in planned['packets']:
        name=packet['body']['name']
        result.append(registered(packet,listing,reentry=history_factory(packet)))
    if len({r['workspace_id'] for r in result})!=12:raise ValueError('REGISTRATIONS')
    return result


def diff(planned,listing):
    """Minimal update packets for existing IDs, never duplicate POST requests."""
    updates=[]
    for packet in planned['packets']:
        current,=[w for w in listing if w['name']==packet['body']['name']]
        if current['companyId']!='a479c266-538c-4ed6-bbf0-1f96e1f67b3e' or current['projectId']!=PROJECT or current['isPrimary'] is not False:raise ValueError('WORKSPACE')
        changed={k:copy.deepcopy(v) for k,v in packet['body'].items() if current.get(k)!=v}
        if changed:updates.append(dict(method='PATCH',path=f"/api/projects/{PROJECT}/workspaces/{current['id']}",
            body=changed,previous={k:copy.deepcopy(current.get(k)) for k in changed},
            service_rows_preserved=copy.deepcopy(current['runtimeServices'])))
    return updates
