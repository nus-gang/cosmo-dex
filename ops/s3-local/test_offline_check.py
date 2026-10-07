import base64
import hashlib
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
from manifest import encode, aggregate
from process_check import EXPECTED
from test_preflight import PreflightFixture
import offline_check


class OfflineCheckTest(PreflightFixture, unittest.TestCase):
    def prepare(self, tail=None):
        self.scratch = self.root / 'scratch'
        self.scratch.mkdir(mode=0o700)
        self.validator = self.artifacts / offline_check.VALIDATOR
        self.validator.parent.mkdir()
        self.validator.write_text('#!' + sys.executable + '\nimport sys,json\n'
            'sys.stdin.buffer.read()\n' + (tail or 'print('+repr(json.dumps(EXPECTED))+')\n'))
        path = offline_check.DESCRIPTOR
        descriptor = json.loads((self.bundle / 'files' / path).read_bytes())
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode({
            offline_check.VALIDATOR: hashlib.sha256(self.validator.read_bytes()).hexdigest()}).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        enc = lambda b: base64.b64encode(b).decode()
        self.input_file = self.root / 'input.json'
        self.input_file.write_bytes(encode({
            'runtime_manifest': enc((self.bundle/'runtime-manifest.json').read_bytes()),
            'files': {p: enc((self.bundle/'files'/p).read_bytes()) for p in self.hashes},
            'guard': enc(b'TEST_ONLY'), 'genesis': enc(b'TEST_ONLY')}))
    def run_check(self, **kw):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
                    profile='s3-dev-local/1', acknowledge=True, inputs=self.root,
                    input_name='input.json', arguments=[], scratch=self.scratch, timeout=2)
        args.update(kw)
        return offline_check.check(**args)
    def test_full_capture_synthetic_validator_and_cleanup(self):
        self.prepare()
        result = self.run_check()
        self.assertFalse(result['approval_verified'])
        self.assertTrue(result['byte_preflight']['input_set_byte_match'])
        self.assertEqual(result['semantic_preflight'], EXPECTED)
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_cli_capture_descriptor_child_and_cleanup(self):
        import subprocess
        import offline_cli
        self.prepare()
        args = [sys.executable, '-B', offline_cli.__file__]
        values = {'bundle':self.bundle, 'artifacts':self.artifacts,
            'input-set':self.input_file, 'effective-profile':self.root/'profile.json',
            'home':self.root/'home', 'key-directory':self.root/'keys',
            'scratch':self.scratch, 'runtime-pin':self.pin,
            'local-demo-profile':'s3-dev-local/1', 'bind':'127.0.0.1:18080',
            'rpc':'127.0.0.1:26657', 'lifetime-seconds':'60',
            'max-requests':'10', 'max-ticks':'10'}
        for key, value in values.items(): args.extend(['--'+key,str(value)])
        args.append('--acknowledge-unproven-space')
        result = subprocess.run(args, capture_output=True, timeout=10)
        self.assertEqual((result.returncode,result.stderr),(0,b''))
        report = json.loads(result.stdout)
        self.assertEqual(report['semantic_preflight'], EXPECTED)
        self.assertFalse(report['approval_verified'])
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_original_replaced_after_capture_does_not_execute_replacement(self):
        self.prepare()
        original = offline_check.validate_captured
        def runner(executable, *args):
            self.validator.write_bytes(b'REPLACEMENT_NOT_EXECUTABLE')
            (self.bundle/'files'/offline_check.DESCRIPTOR).write_bytes(b'REPLACEMENT')
            self.assertEqual(Path(executable).stat().st_mode & 0o777, 0o500)
            self.assertEqual(Path(executable).parent.stat().st_mode & 0o777, 0o700)
            return original(executable, *args)
        with patch.object(offline_check, 'validate_captured', side_effect=runner):
            self.run_check()
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_mutation_between_capture_and_binary_read_rejected_before_spawn(self):
        self.prepare()
        original = offline_check.verify_input_set
        def capture(*args):
            result = original(*args)
            self.validator.write_bytes(b'CHANGED')
            return result
        with patch.object(offline_check, 'verify_input_set', side_effect=capture), \
             patch.object(offline_check, 'validate_captured') as runner:
            with self.assertRaisesRegex(ValueError, 'VALIDATOR_BYTES_CHANGED'): self.run_check()
            runner.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])
    def test_missing_sre_validator_and_optin_rejected_before_spawn(self):
        self.prepare()
        with patch.object(offline_check, 'validate_captured') as runner:
            with self.assertRaises(ValueError): self.run_check(acknowledge=False)
            with patch.object(offline_check, 'VALIDATOR', 'bin/not-in-descriptor'):
                with self.assertRaisesRegex(ValueError, 'VALIDATOR_NOT_IN_SRE_DESCRIPTOR'): self.run_check()
            runner.assert_not_called()
    def test_failed_child_and_interruption_remove_only_private_copy(self):
        self.prepare('sys.exit(2)\n')
        for error in (None, KeyboardInterrupt()):
            with self.assertRaises((ValueError, KeyboardInterrupt)):
                if error is None:
                    self.run_check()
                else:
                    with patch.object(offline_check, 'validate_captured', side_effect=error):
                        self.run_check()
            self.assertEqual(list(self.scratch.iterdir()), [])
            self.assertTrue(self.validator.exists())
            self.assertTrue(self.input_file.exists())

if __name__ == '__main__': unittest.main()
