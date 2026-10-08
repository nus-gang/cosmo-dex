"""Render exact commands from public initializer reports, never secret keys.

Two fee profiles execute sequentially: the approved browser origin is port5173.
Pure rendering makes no filesystem, API, port reservation or approval claim.
"""
from pathlib import Path
import re
from registration_set import compile_set
import native_review
from runtime_config import shell_command
from workspace_registration import PROJECT

ERROR = 'INITIALIZER_REGISTRATION_REJECTED'


def key_packets(spec):
    """Initial configuration of the SAME two final web workspaces.

    Board reconfigures each stopped workspace with the final web packet after
    authoritative C bootstrap. No extra workspace or concurrent port5173 use.
    """
    packets=[]
    from key_preparation_cli import parse
    for fee in (0,25):
        path=Path(spec['control_root'])/('fee'+str(fee))/'web'
        args=[]
        for key in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision'):
            args.extend(['--'+key.replace('_','-'),spec[key]])
        args+=['--approval-socket',str(path/'b'/'s'),'--pid-mailbox',str(path/'mail'),
            '--web-origin','http://127.0.0.1:5173','--local-demo-profile','s3-dev-local/1','--acknowledge-unproven-space']
        parse(['prepare-keys-reviewed',*args])
        command=shell_command([spec['python'],'-B',str(Path(spec['candidate'])/'ops/s3-local/key_preparation_cli.py'),
                               'prepare-keys-reviewed',*args])
        packets.append({'method':'POST','path':f'/api/projects/{PROJECT}/workspaces',
            'body':{'name':f'nus-s3-local-web-fee{fee}','sourceType':'local_path','cwd':spec['candidate'],'isPrimary':False,
                'runtimeConfig':{'desiredState':'manual','serviceStates':{'0':'manual'},
                    'workspaceRuntime':{'commands':[{'id':f's3-web-fee{fee}','name':f's3-web-fee{fee}',
                        'kind':'service','command':command,'cwd':spec['candidate'],'port':5173,
                        'lifecycle':'shared','reuseScope':'project_workspace'}]}}},
            'requires_board_registration':True,'approval_verified':False,'starts_service':False})
    return packets


def render(spec, reports):
    try:
        if set(spec) != {'schema','python','candidate','bundle','artifacts','runtime_pin',
                         'native_decision_id','ceo_revision','cto_revision','control_root','profiles'}:
            raise ValueError()
        if spec['schema'] != 's3-local-initializer-registration/1' or set(spec['profiles']) != {'fee0','fee25'}:
            raise ValueError()
        for key in ('python','candidate','bundle','artifacts','control_root'):
            p=Path(spec[key])
            if not p.is_absolute() or '..' in p.parts:raise ValueError()
        for key in ('native_decision_id','ceo_revision','cto_revision'):
            native_review._uuid(spec[key])
        if not re.fullmatch('[0-9a-f]{64}',spec['runtime_pin']):raise ValueError()
        roots=[]; identities=set(); genesis=set(); profiles={}
        common=[]
        for key in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision'):
            common.extend(['--'+key.replace('_','-'),spec[key]])
        common.extend(['--local-demo-profile','s3-dev-local/1','--acknowledge-unproven-space','--lifetime-seconds','300'])
        control=Path(spec['control_root'])
        for fee,baseport in ((0,26656),(25,26756)):
            name='fee'+str(fee);root=Path(spec['profiles'][name]);r=reports[name]
            if not root.is_absolute() or '..' in root.parts:raise ValueError()
            if any(root==p or root in p.parents or p in root.parents for p in roots+[control]):raise ValueError()
            roots.append(root)
            if (r['schema']!='s3-local-initialization/1' or r['root']!=str(root) or r['fee_bps']!=str(fee)
                    or r['runtime_pin']!=spec['runtime_pin'] or r['service_started'] is not False
                    or r['c_semantic_validation_verified'] is not True
                    or r['authority_root']!=str(root/'authority')
                    or r['homes']!=[str(root/f'validator-{i}') for i in range(4)]
                    or len(r['node_ids'])!=4):raise ValueError()
            for ident in r['node_ids']:
                if not re.fullmatch('[0-9a-f]{40}',ident) or ident in identities:raise ValueError()
                identities.add(ident)
            if not re.fullmatch('[0-9a-f]{64}',r['genesis_sha256']) or r['genesis_sha256'] in genesis:raise ValueError()
            genesis.add(r['genesis_sha256'])
            inherited=common+['--input-set',str(root/'input.json'),'--effective-profile',str(root/'effective-profile.json')]
            def ipc(kind):
                p=control/name/kind
                # Short separate Unix socket path, fresh broker at each run.
                socket=p/'b'/'s'
                if len(str(socket).encode())>103:raise ValueError('UNIX_SOCKET_PATH_TOO_LONG')
                return ['--scratch',str(p/'scratch'),'--approval-socket',str(socket),'--pid-mailbox',str(p/'mail')]
            worker=inherited+['--home',str(root/'engine'),'--key-directory',str(root/'authority'/'operator-0'),
                '--bind',f'127.0.0.1:{18080+int(fee==25)}','--rpc',f'127.0.0.1:{baseport+1}',
                '--max-requests','10000','--max-ticks','10000']
            nodes=[]
            for i,ident in enumerate(r['node_ids']):
                peers=','.join(f'{other}@127.0.0.1:{baseport+j*2}' for j,other in enumerate(r['node_ids']) if j!=i)
                argv=inherited+ipc('v'+str(i))+['--home',r['homes'][i],'--rpc',f'127.0.0.1:{baseport+i*2+1}',
                    '--p2p',f'127.0.0.1:{baseport+i*2}','--peers',peers]
                nodes.append({'node_id':ident,'argv':argv})
            profiles[name]={'worker_argv':worker+ipc('worker'),
                            'web_argv':worker+ipc('web')+['--web-origin','http://127.0.0.1:5173'],'nodes':nodes}
        result=compile_set({'schema':'s3-local-registration-input/1','python':spec['python'],
                            'candidate':spec['candidate'],'profiles':profiles})
        return dict(result, execution_order=['fee0','stop-and-verify-release','fee25'],
                    shared_web_origin='http://127.0.0.1:5173',control_root=spec['control_root'],
                    initial_web_packets=key_packets(spec))
    except (KeyError,TypeError,ValueError,AttributeError):
        raise ValueError(ERROR) from None
