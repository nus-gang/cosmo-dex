import copy
from contextlib import contextmanager
from pathlib import Path
import sys
import unittest

import managed_session as session
from native_review import COMPANY
from test_runtime_config import RuntimeConfigTest
from workspace_registration import PROJECT, prepare


class SessionTest(unittest.TestCase):
    def setup_case(self, fee=0):
        self.events = []
        self.argv = RuntimeConfigTest.args(self)
        body = prepare(sys.executable, '/candidate', self.argv, fee_bps=fee)['body']
        self.workspace = dict(copy.deepcopy(body), id='11111111-1111-4111-8111-111111111111',
                              companyId=COMPANY, projectId=PROJECT, runtimeServices=[])
        self.kw = dict(fee_bps=fee, broker_root='/private/broker', broker=self.broker,
                       read_workspaces=self.read, request=self.request, audit=self.audit)

    @contextmanager
    def broker(self, root):
        self.events.append('broker-open')
        try:
            yield Path(root) / 's'
        finally:
            self.events.append('broker-close')

    def read(self, path):
        self.assertEqual(path, f'/api/projects/{PROJECT}/workspaces')
        self.events.append('read')
        return [copy.deepcopy(self.workspace)]

    def audit(self):
        self.events.append('audit')

    def request(self, value):
        action = value['path'].rsplit('/', 1)[1]
        self.assertEqual(value['method'], 'POST')
        self.assertEqual(value['body'], {'workspaceCommandId':f"s3-worker-fee{self.kw['fee_bps']}"})
        self.assertIn('/11111111-1111-4111-8111-111111111111/', value['path'])
        self.events.append(action)
        return {'synthetic': action}

    def run_session(self):
        return session._session(sys.executable, '/candidate', self.argv, **self.kw)

    def test_fee_lifecycle_and_evidence_never_claims_exit(self):
        for fee in (0,25):
            self.setup_case(fee)
            with self.run_session() as result:
                self.assertFalse(result['stop_attempted'])
                self.events.append('body')
            self.assertEqual(self.events, ['broker-open','read','audit','read','start','body','stop','broker-close'])
            self.assertTrue(result['stop_acknowledged'])
            self.assertFalse(result['process_exit_verified'])
            self.assertFalse(result['approval_verified'])

    def test_denial_and_drift_do_not_start_or_stop(self):
        for drift in (False, True):
            self.setup_case()
            def audit():
                if drift:
                    self.workspace['runtimeConfig']['desiredState'] = 'running'
                else:
                    raise OSError('secret')
            self.kw['audit'] = audit
            with self.assertRaisesRegex(ValueError, '^'+session.ERROR+'$'):
                with self.run_session(): self.fail()
            self.assertNotIn('start', self.events)
            self.assertNotIn('stop', self.events)
            self.assertEqual(self.events[-1], 'broker-close')

    def test_ambiguous_start_and_body_interrupt_still_stop_once(self):
        for mode in ('start','body','interrupt'):
            self.setup_case()
            def request(value):
                result = self.request(value)
                if mode == 'start' and value['path'].endswith('/start'):
                    raise OSError('response lost: secret')
                return result
            self.kw['request'] = request
            with self.assertRaises(KeyboardInterrupt if mode == 'interrupt' else ValueError):
                with self.run_session():
                    if mode == 'interrupt': raise KeyboardInterrupt()
                    raise OSError('body: secret')
            self.assertEqual(self.events.count('start'), 1)
            self.assertEqual(self.events.count('stop'), 1)
            self.assertEqual(self.events[-2:], ['stop','broker-close'])

    def test_unconfirmed_stop_is_explicit_without_retry(self):
        self.setup_case()
        def request(value):
            result = self.request(value)
            if value['path'].endswith('/stop'): raise OSError('secret')
            return result
        self.kw['request'] = request
        with self.assertRaisesRegex(ValueError, '^'+session.CLEANUP_ERROR+'$'):
            with self.run_session() as result: pass
        self.assertTrue(result['stop_attempted'])
        self.assertFalse(result['stop_acknowledged'])
        self.assertFalse(result['process_exit_verified'])
        self.assertEqual(self.events.count('stop'),1)
        self.assertEqual(self.events[-1], 'broker-close')

    def test_wrong_socket_refused_before_broker(self):
        self.setup_case()
        self.kw['broker_root'] = '/other'
        with self.assertRaisesRegex(ValueError, '^'+session.ERROR+'$'):
            with self.run_session(): self.fail()
        self.assertEqual(self.events, [])


if __name__ == '__main__': unittest.main()
