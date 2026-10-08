import copy
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import mailbox_session
import pid_mailbox
from test_session_release import ReleaseSessionTest as Fixture


class MailboxSessionTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.box = pid_mailbox.Mailbox(Path(self.tmp.name).resolve()/'mailbox')

    def setup_case(self, fee=0, mode='ok'):
        self.f = Fixture(); self.f.setup_case(fee, mode)
        argv = list(self.f.args[2])
        argv[argv.index('--pid-mailbox')+1] = str(self.box.root)
        self.f.args = (*self.f.args[:2], argv)
        # Update exact registered command to include this session mailbox.
        from workspace_registration import prepare
        config = prepare(*self.f.args, fee_bps=fee)['body']['runtimeConfig']
        old = list(self.f.client.read_workspaces.side_effect)
        for group in old:
            for workspace in group:
                workspace['runtimeConfig'] = copy.deepcopy(config)
        command = config['workspaceRuntime']['commands'][0]['command']
        for group in old:
            for workspace in group:
                for service in workspace.get('runtimeServices', []):
                    service['command'] = command
        self.f.client.read_workspaces.side_effect = old
        original = self.f.client.request.side_effect
        def request(packet):
            result = original(packet)
            result['operation']['command'] = command
            result['workspace']['runtimeConfig'] = copy.deepcopy(config)
            for service in result['workspace'].get('runtimeServices', []):
                service['command'] = command
            return result
        self.f.client.request.side_effect = request
        self.inputs = dict(mailbox=self.box, raw=b'capture', artifacts=Path('/artifacts'),
            arguments=['arg'], scratch=self.tmp.name, validator_sha256='a'*64)

    def run_case(self, body=None):
        with patch.object(mailbox_session.release_inventory, 'probes',
                          return_value=self.f.probes) as build:
            self.build = build
            with mailbox_session.session(*self.f.args, **self.f.kwargs,
                                         **self.inputs) as evidence:
                self.evidence = evidence
                if body: body()
        return evidence

    def test_collect_after_stop_and_freeze_capture(self):
        for fee in (0,25):
            if fee:
                self.box = pid_mailbox.Mailbox(Path(self.tmp.name).resolve()/'mailbox25')
            self.setup_case(fee)
            def body():
                self.assertFalse(self.box.used)
                pid_mailbox.reporter(self.box.root)(2147483647)
                self.inputs['artifacts'] = Path('/changed')
                self.inputs['arguments'][0] = 'changed'
            result = self.run_case(body)
            self.assertEqual(self.f.events, ['start','stop','process','port','writer'])
            self.assertEqual(self.build.call_args.args[:5],
                ((os.getpid(),2147483647), (('127.0.0.1',18080),), b'capture',
                 Path('/artifacts'), ('arg',)))
            self.assertTrue(result['pid_handoff_collected'])
            self.assertFalse(result['inventory_complete_verified'])
            self.assertFalse(result['cleanup_complete_verified'])
            self.assertTrue((self.box.root/'pids.json').exists())

    def test_missing_report_preserves_challenge_and_skips_probes(self):
        self.setup_case()
        with self.assertRaises(ValueError): self.run_case()
        self.build.assert_not_called()
        self.assertTrue(self.box.used)
        self.assertTrue((self.box.root/'challenge').exists())
        self.assertEqual(self.f.events, ['start','stop'])
        self.assertFalse(self.evidence['pid_handoff_collected'])

    def test_stop_failure_never_collects(self):
        self.setup_case(mode='stop-failed')
        with self.assertRaises(ValueError):
            self.run_case(lambda: pid_mailbox.reporter(self.box.root)(2147483647))
        self.assertFalse(self.box.used)
        self.build.assert_not_called()
        self.assertTrue((self.box.root/'pids.json').exists())

    def test_interrupt_still_collects_after_stop_and_retains_evidence(self):
        self.setup_case()
        def body():
            pid_mailbox.reporter(self.box.root)(2147483647)
            raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt): self.run_case(body)
        self.assertEqual(self.f.events, ['start','stop','process','port','writer'])
        self.assertTrue(self.evidence['pid_handoff_collected'])
        self.assertTrue((self.box.root/'pids.json').exists())

    def test_wrong_mailbox_rejected_before_control(self):
        self.setup_case()
        self.f.args[2][self.f.args[2].index('--pid-mailbox')+1] = '/wrong'
        with self.assertRaisesRegex(ValueError, 'MAILBOX_SESSION_REJECTED'):
            self.run_case()
        self.f.client.request.assert_not_called()
        self.assertFalse(self.box.used)

if __name__ == '__main__': unittest.main()
