#!/usr/bin/env python3
"""L-T foreground Chain command; configure only through managed runtime."""
import re
import sys
from pathlib import Path
import approval_gate
import chain_stage
import chain_run
import native_review
from offline_cli import Parser
from launcher_signal import stop_latch
from pid_mailbox import reporter


def parse(argv):
    argv = list(argv)
    if not argv or argv.pop(0) != 'serve-chain-reviewed':
        raise ValueError('COMMAND')
    opts = [v for v in argv if v.startswith('--')]
    if len(opts) != len(set(opts)) or any('=' in v for v in opts):
        raise ValueError('ARGUMENTS')
    p = Parser(allow_abbrev=False, add_help=False)
    paths = ('bundle', 'artifacts', 'input-set', 'effective-profile', 'home',
             'scratch', 'approval-socket', 'pid-mailbox')
    for name in paths:
        p.add_argument('--'+name, required=True, type=Path)
    for name in ('runtime-pin', 'local-demo-profile', 'native-decision-id',
                 'ceo-revision', 'cto-revision', 'rpc', 'p2p', 'lifetime-seconds'):
        p.add_argument('--'+name, required=True)
    topology=p.add_mutually_exclusive_group(required=True)
    topology.add_argument('--peers')
    topology.add_argument('--topology-root',type=Path)
    p.add_argument('--acknowledge-unproven-space', action='store_true')
    a = p.parse_args(argv)
    for name in paths:
        value = getattr(a, name.replace('-', '_'))
        if not value.is_absolute() or '..' in value.parts:
            raise ValueError('PATH')
    if a.local_demo_profile != 's3-dev-local/1' or not a.acknowledge_unproven_space:
        raise ValueError('OPT_IN')
    if re.fullmatch('[0-9a-f]{64}', a.runtime_pin) is None:
        raise ValueError('PIN')
    a.decision = native_review._uuid(a.native_decision_id)
    a.revisions = {r: native_review._uuid(getattr(a, r+'_revision')) for r in ('ceo', 'cto')}
    if re.fullmatch('[1-9][0-9]{0,2}', a.lifetime_seconds) is None or int(a.lifetime_seconds) > 300:
        raise ValueError('LIFETIME')
    if a.topology_root is not None:
        if (not a.topology_root.is_absolute() or '..' in a.topology_root.parts or
                a.home.parent!=a.topology_root or a.home.name not in [f'validator-{i}' for i in range(4)] or
                a.input_set!=a.topology_root/'input.json' or a.effective_profile!=a.topology_root/'effective-profile.json'):
            raise ValueError('TOPOLOGY_ROOT')
        for addr in (a.rpc,a.p2p):
            if re.fullmatch(r'127\.0\.0\.1:[1-9][0-9]{3,4}',addr) is None or not 1024<=int(addr.split(':')[1])<=65535:
                raise ValueError('LOOPBACK')
        if a.rpc==a.p2p:raise ValueError('TOPOLOGY')
        return a
    # The managed topology deliberately uses literal IPv4 loopback only.
    addresses, ids = {a.rpc, a.p2p}, set()
    if len(addresses) != 2 or len(a.peers.split(',')) != 3:
        raise ValueError('TOPOLOGY')
    for peer in a.peers.split(','):
        ident, sep, addr = peer.partition('@')
        if not sep or re.fullmatch('[0-9a-f]{40}', ident) is None or ident in ids or addr in addresses:
            raise ValueError('PEER')
        ids.add(ident); addresses.add(addr)
    for addr in addresses:
        if re.fullmatch(r'127\.0\.0\.1:[1-9][0-9]{3,4}', addr) is None or not 1024 <= int(addr.split(':')[1]) <= 65535:
            raise ValueError('LOOPBACK')
    return a


def resolve_peers(a):
    """Read public node IDs only after approved initialization; B still checks
    the real topology/home before any START. No key-dependent host command."""
    if a.topology_root is None:return
    from manifest import decode
    from preflight import bounded,checked_root
    r=decode(bounded(checked_root(a.topology_root),'launch-topology.json',16384))
    if set(r)!={'schema','root','runtime_pin','fee_bps','node_ids'} or r['schema']!='s3-local-launch-topology/1' or r['root']!=str(a.topology_root) or r['runtime_pin']!=a.runtime_pin or r['fee_bps'] not in (0,25):
        raise ValueError('TOPOLOGY_REPORT')
    ids=r['node_ids'];i=int(a.home.name[-1]);base=26656 if r['fee_bps']==0 else 26756
    if (not isinstance(ids,list) or len(ids)!=4 or len(set(ids))!=4 or any(not isinstance(x,str) or not re.fullmatch('[0-9a-f]{40}',x) for x in ids) or
            a.rpc!=f'127.0.0.1:{base+2*i+1}' or a.p2p!=f'127.0.0.1:{base+2*i}'):
        raise ValueError('TOPOLOGY_REPORT')
    a.peers=','.join(f'{node}@127.0.0.1:{base+2*j}' for j,node in enumerate(ids) if j!=i)
    return ids


def main(argv=None):
    try:
        a = parse(sys.argv[1:] if argv is None else argv)
        resolve_peers(a)
        record = reporter(a.pid_mailbox)
        def audit():
            return approval_gate.inspect(a.bundle, a.artifacts, a.runtime_pin,
                a.local_demo_profile, a.acknowledge_unproven_space, a.decision, a.revisions)
        with stop_latch() as stop, approval_gate.private_transport(a.approval_socket):
            if stop():
                raise ValueError('STOPPED')
            with chain_stage.stage(a.bundle, a.artifacts, a.runtime_pin,
                    a.local_demo_profile, a.acknowledge_unproven_space, a.decision,
                    a.revisions, a.input_set.parent, a.input_set.name,
                    a.effective_profile, a.scratch) as staged:
                result = chain_run.run(staged, a.runtime_pin, str(a.home), a.rpc,
                    a.p2p, a.peers, audit, stopped=stop,
                    lifetime=int(a.lifetime_seconds), on_spawn=record)
        if (type(result) is not dict or result.get('start_sent') is not True or
                type(result.get('stop_requested')) is not bool or
                result.get('approval_verified') is not False or
                result.get('reusable_permit') is not False or
                result.get('cleanup_complete_verified') is not False or
                type(result.get('output_bytes')) is not int or
                not 0 <= result['output_bytes'] <= 1024*1024 or
                (result['child_exit'] is not None if result['stop_requested'] else
                 type(result['child_exit']) is not int or result['child_exit'] != 0)):
            raise ValueError('RESULT')
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_MANAGED_CHAIN_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
