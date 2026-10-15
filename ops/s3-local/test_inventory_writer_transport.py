"""Real inventory -> writer -> private subprocess path; synthetic validator only."""
import hashlib
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

import pid_mailbox
import release_inventory
import writer_release
from preflight import verify_input_set
import test_offline_check as offline_fixture
from test_preflight import PreflightFixture
import test_prepared_session as prepared_fixture


class InventoryWriterTransportTest(PreflightFixture, unittest.TestCase):
    prepare = offline_fixture.OfflineCheckTest.prepare

    def setup_transport(self):
        self.prepare()
        self.raw, _ = verify_input_set(self.bundle, self.artifacts, self.pin,
            's3-dev-local/1', True, self.root, 'input.json')
        self.digest = hashlib.sha256(self.validator.read_bytes()).hexdigest()

    def make(self):
        return release_inventory.probes([10001], [('127.0.0.1', 23001)],
            self.raw, self.artifacts, ['--home', '/synthetic'], self.scratch, self.digest)

    def test_real_writer_path_and_single_use(self):
        self.setup_transport()
        probe = self.make()['writer_probe']
        result = probe()
        self.assertTrue(result['writer_reopen_verified'])
        self.assertEqual(result['validator_sha256'], self.digest)
        self.assertEqual(result['capture_sha256'], hashlib.sha256(self.raw).hexdigest())
        self.assertFalse(result['inventory_complete_verified'])
        self.assertEqual(list(self.scratch.iterdir()), [])
        with self.assertRaises(ValueError): probe()

    def test_post_capture_binary_change_denies_and_cleans(self):
        self.setup_transport()
        probe = self.make()['writer_probe']
        self.validator.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, release_inventory.ERROR): probe()
        with self.assertRaises(ValueError): probe()
        self.assertEqual(list(self.scratch.iterdir()), [])

    def test_prepared_session_to_actual_writer_transport_fee_profiles(self):
        self.setup_transport()
        fixture = prepared_fixture.PreparedSessionTest()
        self.addCleanup(fixture.doCleanups)
        original = release_inventory._probes
        for fee in (0, 25):
            f, owner, kw = fixture.fixture(fee)
            kw.update(raw=self.raw, artifacts=self.artifacts,
                      scratch=self.scratch, validator_sha256=self.digest)
            # Only host process/port observations are injected. The writer uses
            # actual descriptor, file SHA, private copy, subprocess and reap.
            def build(pids, endpoints, *args):
                process = Mock(return_value={'schema':'s3-local-process-probe/1',
                    'listed_pids_absent':True, 'pids':list(pids)})
                port = Mock(return_value={'schema':'s3-local-port-probe/1',
                    'simultaneous_bind_verified':True, 'endpoints':list(endpoints)})
                return original(pids, endpoints, *args, process, port, writer_release.check)
            with patch.object(release_inventory, 'probes', side_effect=build):
                with owner.session(**kw) as evidence:
                    pid_mailbox.reporter(owner.evidence_path)(2147483647)
            self.assertTrue(evidence['host_release_observations_complete'])
            self.assertFalse(evidence['cleanup_complete_verified'])
            self.assertEqual(evidence['host_release_observations']['writer']
                ['validator_sha256'], self.digest)
            self.assertTrue((owner.evidence_path/'pids.json').is_file())
            self.assertEqual(list(self.scratch.iterdir()), [])

if __name__ == '__main__': unittest.main()
