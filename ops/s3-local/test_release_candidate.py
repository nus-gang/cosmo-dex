"""Release binding tests use inert fixture bytes and never start services."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release_candidate as r


class ReleaseSpec(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'src').mkdir()
        (self.root / 'src/input').write_bytes(b'input')
        digest = hashlib.sha256(b'input').hexdigest()
        settings = {
            'build_cwd': 'src',
            'build_env_json': '{}',
            'build_inputs_sha256_json': '{"src/input":"' + digest + '"}',
        }
        self.spec = {name: {
            'build_argv': ['["tool","arg"]'], 'toolchain': 'fixture',
            'artifacts': ['fixture/' + name], 'approval_sources': ['fixture'],
            'settings': dict(settings),
        } for name in r.COMPONENTS}
        self.spec['chain']['artifacts'].append('bin/nus-s3-local-chain')
        self.spec['sre']['artifacts'].extend(r.SRE_NATIVE)
        self.spec['sre']['artifacts'].extend(r.ASSETS)
        for name in ('chain', 'exchange', 'settlement', 'wallet'):
            self.spec[name]['settings'].update({'approved_head': 'a'*40,
                                                'approved_tree': 'b'*40})

    def verify(self):
        with patch('subprocess.check_output', return_value=('b'*40+'\n').encode()):
            return r.verify_spec(self.root, self.spec)

    def test_exact_runtime_consumers_and_build_provenance_pass(self):
        report = self.verify()
        self.assertEqual(report['sre']['argv'], [['tool', 'arg']])
        self.assertEqual(report['chain']['cwd'], 'src')

    def test_prior_submission_shapes_are_rejected(self):
        cases = {
            'native': lambda: self.spec['sre']['artifacts'].remove(r.SRE_NATIVE[0]),
            'web': lambda: self.spec['sre']['artifacts'].remove('web/index.html'),
            'cwd': lambda: self.spec['sre']['settings'].__setitem__('build_cwd', 'missing'),
            'input': lambda: self.spec['sre']['settings'].__setitem__(
                'build_inputs_sha256_json', '{"src/input":"' + '0'*64 + '"}'),
            'argv': lambda: self.spec['sre'].__setitem__('build_argv', ['not-json']),
        }
        for name, mutate in cases.items():
            with self.subTest(name=name):
                self.setUp()
                mutate()
                with self.assertRaises(ValueError):
                    self.verify()


if __name__ == '__main__':
    unittest.main()
