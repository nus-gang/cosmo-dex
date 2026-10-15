"""Go-generated temporary homes; actual B topology only, no start/listener."""
import json
import sys
from pathlib import Path
from unittest.mock import patch
import chain_stage
import chain_topology
import chain_topology_check
import chain_preflight
from test_chain_cli import ChainCliTest


def main(root, pin, fee):
    root = Path(root)
    scratch = root/'scratch'
    scratch.mkdir(mode=0o700)
    ids = json.loads((root/'ids.json').read_bytes())
    nodes = []
    for i in range(4):
        argv = ChainCliTest.args(None)[1:]
        values = dict(bundle=root/'bundle', artifacts=root/'artifacts',
            input_set=root/'input.json', effective_profile=root/'profile.json',
            runtime_pin=pin, home=root/f'homes/v{i}', scratch=root/f'work/v{i}',
            pid_mailbox=root/f'mail/v{i}', approval_socket=root/f'broker/v{i}/s',
            rpc=f'127.0.0.1:{28000+i*2}', p2p=f'127.0.0.1:{28001+i*2}',
            peers=','.join(ids[j]+f'@127.0.0.1:{28001+j*2}' for j in range(4) if j != i))
        for k, v in values.items():
            argv[argv.index('--'+k.replace('_', '-'))+1] = str(v)
        nodes.append(dict(node_id=ids[i], argv=argv))
    candidate = Path(__file__).resolve().parents[2]
    packet = chain_topology.prepare(sys.executable, candidate, nodes, fee_bps=int(fee))
    def snapshot():
        return {str(p.relative_to(root/'homes')):p.read_bytes()
                for p in (root/'homes').rglob('*') if p.is_file()}
    original = snapshot()
    children = []
    spawn = chain_preflight.subprocess.Popen
    def tracked(argv, **kwargs):
        assert argv[1] == 'topology'
        child = spawn(argv, **kwargs)
        children.append(child)
        return child
    with patch.object(chain_stage.approval_gate, 'inspect', return_value={}), \
            patch.object(chain_preflight.subprocess, 'Popen', side_effect=tracked):
        with chain_stage.stage(root/'bundle', root/'artifacts', pin, 's3-dev-local/1',
                True, 'synthetic', {}, root, 'input.json', root/'profile.json', scratch) as staged:
            def check():
                return chain_topology_check.check(sys.executable, candidate, nodes,
                    packet, staged, fee_bps=int(fee), scratch=scratch)
            result = check()
            assert result['node_ids'] == ids and result['home_identity_verified']
            assert not any(result[k] for k in ('approval_verified',
                'port_availability_verified', 'writer_exclusion_verified', 'service_started'))
            assert snapshot() == original
            for name in ('guard.dev.json', 'config/genesis.json', 'config/node_key.json'):
                target = root/'homes/v2'/name
                saved = target.read_bytes()
                target.write_bytes(b'{}')
                try:
                    check()
                except ValueError as e:
                    assert str(e) == 'CHAIN_PREFLIGHT_REJECTED', str(e)
                else:
                    raise AssertionError('changed home accepted')
                finally:
                    target.write_bytes(saved)
                assert snapshot() == original
            assert check() == result
        assert len(children) == 5
        assert all(p.returncode is not None and p.stdout.closed and p.stderr.closed for p in children)
    assert list(scratch.iterdir()) == [] and snapshot() == original
    print('REAL_CHAIN_TOPOLOGY_PASS')

if __name__ == '__main__':
    main(*sys.argv[1:])
