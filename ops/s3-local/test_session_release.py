import copy
import errno
import sys
import unittest
from unittest.mock import Mock

import managed_session
import process_release
import port_release
import writer_release
from process_check import EXPECTED
from test_runtime_evidence import EvidenceTest, WID


class ReleaseSessionTest(unittest.TestCase):
    def setup_case(self, fee=0, mode='ok'):
        f = EvidenceTest(); f.setup_case(fee)
        self.events = []
        self.client = Mock(); self.client._workspace_id = WID
        self.client.read_workspaces.side_effect = [
            [copy.deepcopy(f.case.workspace)], [copy.deepcopy(f.case.workspace)],
            [copy.deepcopy(f.workspace)]]
        def request(packet):
            action = packet['path'].rsplit('/', 1)[1]
            self.events.append(action)
            result = f.response(action)
            if mode == action + '-failed': result['operation']['status'] = 'failed'
            return result
        self.client.request.side_effect = request
        def process():
            self.events.append('process')
            def absent(pid, signal): raise OSError(errno.ESRCH, 'gone')
            return process_release._check([10001,10002], absent, lambda: 1)
        def port():
            self.events.append('port')
            return port_release._check([('127.0.0.1', 23001)], Mock(), lambda: 1)
        def writer():
            self.events.append('writer')
            return writer_release._check(b'captured', {}, ('args',), '/scratch', 60,
                lambda *args: ('a'*64, dict(EXPECTED)), lambda: 1)
        self.probes = dict(process_probe=Mock(side_effect=process),
                          port_probe=Mock(side_effect=port), writer_probe=Mock(side_effect=writer))
        self.args = (sys.executable, '/candidate', f.case.argv)
        self.kwargs = dict(client=self.client, fee_bps=fee, broker_root='/private/broker',
                           broker=f.case.broker, audit=f.case.audit)

    def run_case(self, body=None):
        with managed_session._release_session(*self.args, **self.kwargs, **self.probes) as result:
            self.result = result
            self.assertFalse(result['host_release_observations_complete'])
            if body: body()
        return result

    def test_fee_profiles_order_and_no_overclaim(self):
        for fee in (0,25):
            self.setup_case(fee)
            result = self.run_case()
            self.assertEqual(self.events, ['start','stop','process','port','writer'])
            self.assertTrue(result['host_release_observations_complete'])
            self.assertEqual(set(result['host_release_observations']), {'process','port','writer'})
            for key in ('cleanup_complete_verified','process_exit_verified',
                        'writer_release_verified','port_release_verified','approval_verified'):
                self.assertFalse(result[key])
            for probe in self.probes.values(): probe.assert_called_once_with()

    def test_stop_or_start_failure_never_probes(self):
        for mode in ('start-failed','stop-failed'):
            self.setup_case(mode=mode)
            with self.assertRaises(ValueError): self.run_case()
            self.assertEqual(self.events, ['start','stop'])
            for probe in self.probes.values(): probe.assert_not_called()

    def test_partial_failure_malformed_report_and_interrupt_no_retry(self):
        for index, name in enumerate(('process','port','writer')):
            for failure in (OSError('private detail'), KeyboardInterrupt(),
                            {'schema':'wrong'}, {'schema':'s3-local-'+name+'-probe/1'}):
                self.setup_case()
                probe = self.probes[name+'_probe']
                if isinstance(failure, BaseException): probe.side_effect = failure
                else: probe.side_effect = None; probe.return_value = failure
                with self.assertRaises(KeyboardInterrupt if isinstance(failure, KeyboardInterrupt)
                                       else ValueError): self.run_case()
                self.assertFalse(self.result['host_release_observations_complete'])
                self.assertEqual(len(self.result['host_release_observations']), index)
                for n in ('process','port','writer')[index+1:]:
                    self.probes[n+'_probe'].assert_not_called()
                probe.assert_called_once_with()

    def test_body_error_or_interrupt_still_stops_and_observes(self):
        for failure in (OSError('private detail'), KeyboardInterrupt()):
            self.setup_case()
            def fail(): raise failure
            with self.assertRaises(KeyboardInterrupt if isinstance(failure, KeyboardInterrupt)
                                   else ValueError): self.run_case(fail)
            self.assertEqual(self.events, ['start','stop','process','port','writer'])
            self.assertTrue(self.result['host_release_observations_complete'])
            self.assertFalse(self.result['cleanup_complete_verified'])

if __name__ == '__main__': unittest.main()
