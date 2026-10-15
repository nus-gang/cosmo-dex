import copy
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import registration_set as target
import registration_set_cli as cli
import test_chain_topology
import test_runtime_config
import test_web_cli


def replace(args, name, value):
    args[args.index('--' + name) + 1] = value


def profile(fee):
    prefix = f'/private/fee{fee}'
    worker = test_runtime_config.RuntimeConfigTest.args(None)
    web = test_web_cli.WebCliTest.args(None)[1:]
    for args in (worker, web):
        for option in ('bundle', 'artifacts', 'input-set', 'effective-profile'):
            replace(args, option, '/private/shared/' + option)
        replace(args, 'runtime-pin', 'a' * 64)
        replace(args, 'rpc', f'127.0.0.1:{28000 + fee}')
        replace(args, 'bind', f'127.0.0.1:{18080 + fee}')
    nodes = test_chain_topology.nodes()
    for index, node in enumerate(nodes):
        args = node['argv']
        for option in ('bundle', 'artifacts', 'input-set', 'effective-profile'):
            replace(args, option, '/private/shared/' + option)
        replace(args, 'runtime-pin', 'a' * 64)
        replace(args, 'home', f'{prefix}/home/v{index}')
        replace(args, 'scratch', f'{prefix}/scratch/v{index}')
        replace(args, 'pid-mailbox', f'{prefix}/mail/v{index}')
        replace(args, 'approval-socket', f'{prefix}/broker/v{index}/s')
        replace(args, 'rpc', f'127.0.0.1:{28000 + fee + index * 2}')
        replace(args, 'p2p', f'127.0.0.1:{28001 + fee + index * 2}')
    for index, node in enumerate(nodes):
        peers = ','.join(nodes[j]['node_id'] + f'@127.0.0.1:{28001 + fee + j * 2}'
                         for j in range(4) if j != index)
        replace(node['argv'], 'peers', peers)
    return {'worker_argv': worker, 'web_argv': web, 'nodes': nodes}


def spec():
    return {'schema': 's3-local-registration-input/1', 'python': sys.executable,
            'candidate': '/candidate',
            'profiles': {'fee0': profile(0), 'fee25': profile(25)}}


class RegistrationSetTest(unittest.TestCase):
    def test_exact_twelve_deterministic_inert_packets(self):
        value = target.compile_set(spec())
        self.assertEqual(value, target.compile_set(spec()))
        self.assertEqual(value['packet_count'], 12)
        self.assertEqual(len(value['packets']), 12)
        self.assertEqual(len({x['body']['name'] for x in value['packets']}), 12)
        self.assertFalse(value['starts_service'])
        self.assertFalse(value['approval_verified'])
        self.assertTrue(value['requires_board_registration'])
        self.assertEqual(set(value['profiles']), {'fee0', 'fee25'})

    def test_cross_component_and_shape_changes_rejected(self):
        cases = []
        changed = spec(); replace(changed['profiles']['fee0']['web_argv'], 'bind', '127.0.0.1:19999'); cases.append(changed)
        changed = spec(); replace(changed['profiles']['fee0']['nodes'][0]['argv'], 'runtime-pin', 'b' * 64); cases.append(changed)
        changed = spec(); changed['profiles']['fee0']['nodes'][0]['node_id'] = changed['profiles']['fee0']['nodes'][1]['node_id']; cases.append(changed)
        changed = spec(); changed['profiles']['fee0']['worker_argv'].remove('--acknowledge-unproven-space'); cases.append(changed)
        changed = spec(); changed['extra'] = True; cases.append(changed)
        for value in cases:
            with self.assertRaisesRegex(ValueError, target.ERROR): target.compile_set(value)

    def test_cli_canonical_and_denials(self):
        root = Path(tempfile.mkdtemp()).resolve(); self.addCleanup(lambda: __import__('shutil').rmtree(root))
        root.chmod(0o700); source = root / 'input.json'
        source.write_text(json.dumps(spec(), sort_keys=True, separators=(',', ':'))); source.chmod(0o600)
        def invoke(args):
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err): rc = cli.main(args)
            return rc, out.getvalue(), err.getvalue()
        first = invoke(['packets', '--input', str(source)])
        self.assertEqual(first, invoke(['packets', '--input', str(source)]))
        self.assertEqual(first[0], 0); self.assertEqual(first[2], '')
        self.assertEqual(json.loads(first[1]), target.compile_set(spec()))
        link = root / 'link'; link.symlink_to(source)
        for args in ([], ['packet', '--input', str(source)], ['packets', '--input', 'relative'], ['packets', '--input', str(link)]):
            self.assertEqual(invoke(args), (2, '', target.ERROR + '\n'))
        child = subprocess.Popen([sys.executable, '-B', cli.__file__, 'bad'], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.assertEqual(child.wait(timeout=3), 2); self.assertEqual(child.stdout.read(), b'')
        finally:
            if child.poll() is None: child.kill(); child.wait()
            for stream in (child.stdin, child.stdout, child.stderr): stream.close()


if __name__ == '__main__': unittest.main()
