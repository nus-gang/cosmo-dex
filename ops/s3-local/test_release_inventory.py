from pathlib import Path
import unittest
from unittest.mock import Mock
import release_inventory as inventory
import test_session_release as session_tests

class InventoryTest(unittest.TestCase):
    def setup_case(self):
        self.pids = [10001, 10002]
        self.ports = [['127.0.0.1', 23001]]
        self.artifacts = Path('/artifacts')
        self.args = ['--home', '/original']
        self.process = Mock(return_value={'pids': [10001,10002]})
        self.port = Mock(return_value={'endpoints': [('127.0.0.1',23001)]})
        self.writer = Mock(return_value={'validator_sha256': 'a'*64})
        self.inputs = [self.pids, self.ports, b'capture', self.artifacts,
                       self.args, '/scratch', 'a'*64]
    def make(self):
        return inventory._probes(*self.inputs, self.process, self.port, self.writer)
    def test_inputs_frozen_and_each_probe_once(self):
        self.setup_case(); probes = self.make()
        self.pids[0] = 999; self.ports[0][1] = 999
        self.inputs[3] = Path('/changed'); self.args[1] = '/changed'
        for name, probe in probes.items():
            result = probe(); self.assertFalse(result['inventory_complete_verified'])
            self.assertEqual(len(result['capture_sha256']),64)
            with self.assertRaisesRegex(ValueError, inventory.ERROR): probe()
        self.process.assert_called_once_with((10001,10002))
        self.port.assert_called_once_with((('127.0.0.1',23001),))
        self.assertEqual(self.writer.call_args.args[1], Path('/artifacts'))
        self.assertEqual(self.writer.call_args.args[2], ('--home','/original'))
    def test_mismatch_error_interrupt_consumes_probe(self):
        for name in ('process','port','writer'):
            for error in (None, OSError('secret'), KeyboardInterrupt()):
                self.setup_case(); adapter = getattr(self,name)
                adapter.return_value = {}; adapter.side_effect = error
                probe = self.make()[name+'_probe']
                with self.assertRaises(KeyboardInterrupt if isinstance(error,KeyboardInterrupt)
                                       else ValueError): probe()
                with self.assertRaises(ValueError): probe()
                self.assertEqual(adapter.call_count,1)
    def test_invalid_inventory_before_any_io(self):
        for index, values in ((0, [[],[True],[2,2]]), (1,[[],[['0.0.0.0',23001]],
                [['127.0.0.1',True]]]), (2,[b'', 'text']), (3,[None, {}, 'relative', '/x/../y']),
                (4,[[],[1]]), (5,['relative','/x/../y']), (6,['x'*64,'a'*63])):
            for value in values:
                self.setup_case(); self.inputs[index] = value
                with self.assertRaises(ValueError): self.make()
                for adapter in (self.process,self.port,self.writer): adapter.assert_not_called()
    def test_session_uses_bound_inventory_fee_profiles(self):
        for fee in (0,25):
            self.setup_case()
            f = session_tests.ReleaseSessionTest(); f.setup_case(fee)
            adapters = f.probes
            self.process.side_effect = lambda p: adapters['process_probe']()
            self.port.side_effect = lambda e: adapters['port_probe']()
            self.writer.side_effect = lambda *a: adapters['writer_probe']()
            f.probes = self.make()
            result = f.run_case()
            self.assertTrue(result['host_release_observations_complete'])
            self.assertFalse(result['cleanup_complete_verified'])
            self.assertEqual(f.events, ['start','stop','process','port','writer'])

if __name__ == '__main__': unittest.main()
