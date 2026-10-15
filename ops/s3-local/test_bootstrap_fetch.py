import base64
import hashlib
import unittest
from unittest.mock import patch
import bootstrap_fetch as target
import bootstrap_check
import offline_check
import test_bootstrap_stage as fixture
from manifest import decode, encode, aggregate

class BootstrapFetchTest(fixture.fixtures.PreflightFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        with patch.object(offline_check, "VALIDATOR", bootstrap_check.CHECKER):
            fixture.fixtures.OfflineCheckTest.prepare(self)
        self.fetcher = self.artifacts/target.FETCHER
        self.fetcher.write_bytes(b'SYNTHETIC_FETCH_BYTES')
        path = target.DESCRIPTOR
        descriptor = decode((self.bundle/'files'/path).read_bytes())
        inventory = decode(descriptor['implementation_settings']['artifacts_sha256_json'])
        inventory[target.FETCHER] = hashlib.sha256(self.fetcher.read_bytes()).hexdigest()
        descriptor['implementation_settings']['artifacts_sha256_json'] = encode(inventory).decode()
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        capture = decode(self.input_file.read_bytes())
        capture['runtime_manifest'] = base64.b64encode((self.bundle/'runtime-manifest.json').read_bytes()).decode()
        capture['files'][path] = base64.b64encode((self.bundle/'files'/path).read_bytes()).decode()
        self.input_file.write_bytes(encode(capture))

    def fetch(self, **overrides):
        args = dict(bundle=self.bundle, artifacts=self.artifacts, pin=self.pin,
            profile='s3-dev-local/1', acknowledge=True, decision_id='fixture', revisions={},
            inputs=self.root, input_name='input.json', effective_profile=self.root/'profile.json',
            address='127.0.0.1:26657', scratch=self.scratch, timeout=2)
        args.update(overrides)
        return target.fetch(**args)

    def test_exact_capture_private_copy_and_order(self):
        raw = self.input_file.read_bytes()
        events = []
        original = target._validate_snapshot
        def validate(captured, *args):
            events.append('check')
            self.assertEqual(captured, raw)
            self.fetcher.write_bytes(b'replaced source')
            self.input_file.write_bytes(b'replaced input')
            return original(captured, *args)
        def child(path, address, pin, profile, ack, captured, timeout, stopped):
            events.append('fetch')
            self.assertEqual(captured, raw)
            self.assertEqual(path.read_bytes(), b'SYNTHETIC_FETCH_BYTES')
            self.assertEqual(path.stat().st_mode & 0o777, 0o500)
            self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
            self.assertEqual(address, '127.0.0.1:26657')
            return b'RPC_RAW_NOT_PROOF'
        def audit(*args): events.append('audit'); return {}
        with patch.object(target.approval_gate,'inspect',side_effect=audit), \
             patch.object(target,'_validate_snapshot',side_effect=validate), \
             patch.object(target,'fetch_captured',side_effect=child) as run:
            self.assertEqual(self.fetch(), b'RPC_RAW_NOT_PROOF')
            run.assert_called_once()
        self.assertEqual(events, ['audit','check','audit','fetch','audit'])
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_precheck_denial_no_fetch_cleanup(self):
        with patch.object(target.approval_gate,'inspect',return_value={}), \
             patch.object(target,'fetch_captured') as run:
            with self.assertRaises(ValueError): self.fetch(acknowledge=False)
            with self.assertRaises(target.FetchFailure): self.fetch(stopped=lambda: True)
            with patch.object(target,'FETCHER','bin/missing'):
                with self.assertRaisesRegex(ValueError,'FETCHER_NOT_IN'): self.fetch()
            for error in [ValueError('SEMANTIC'), KeyboardInterrupt()]:
                with patch.object(target,'_validate_snapshot',side_effect=error):
                    with self.assertRaises(type(error)): self.fetch()
            with patch.object(target.approval_gate,'inspect',side_effect=[{}, {'revoked':True}]):
                with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'): self.fetch()
            run.assert_not_called()
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_postcheck_failure_preserves_raw_no_retry(self):
        for failure in ['revoked','io','mutation']:
            def child(path, *args):
                if failure == 'mutation':
                    path.chmod(0o600); path.write_bytes(b'changed')
                return b'RECEIVED_RAW'
            audit = [{}, {}, {'revoked':True} if failure == 'revoked' else OSError('io') if failure == 'io' else {}]
            with patch.object(target.approval_gate,'inspect',side_effect=audit), \
                 patch.object(target,'fetch_captured',side_effect=child) as run:
                with self.assertRaises(target.FetchFailure) as error: self.fetch()
                self.assertEqual(error.exception.partial_raw,b'RECEIVED_RAW')
                run.assert_called_once()
            self.assertEqual(list(self.scratch.iterdir()), [])

    def test_transport_partial_and_interrupt_cleanup(self):
        for error in [target.FetchFailure('FETCH_TIMEOUT',b'partial'), KeyboardInterrupt()]:
            with patch.object(target.approval_gate,'inspect',return_value={}), \
                 patch.object(target,'fetch_captured',side_effect=error) as run:
                with self.assertRaises(type(error)) as caught: self.fetch()
                self.assertIs(caught.exception,error)
                run.assert_called_once()
            self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
