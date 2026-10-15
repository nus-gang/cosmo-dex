import copy
import os
import sys
import unittest
from unittest.mock import patch

import runtime_config
import worker_inventory
import test_runtime_config
import test_ready_worker
from ready_worker import ready


class WorkerInventoryTest(unittest.TestCase):
    def test_exact_command_extracts_worker_listener_only(self):
        args = test_runtime_config.RuntimeConfigTest().args()
        for fee in (0, 25):
            config = runtime_config.worker_config(sys.executable, '/candidate', args, fee_bps=fee)
            endpoints = worker_inventory.worker_endpoints(config, sys.executable, '/candidate', fee_bps=fee)
            self.assertEqual(endpoints, (('127.0.0.1', int(args[args.index('--bind')+1].split(':')[1])),))
            self.assertNotEqual(endpoints[0][1], int(args[args.index('--rpc')+1].split(':')[1]))
            for field, value in [('command', config['runtimeConfig']['workspaceRuntime']['commands'][0]['command']+'; exit 0'),
                                 ('port', 65500), ('cwd','/elsewhere'), ('env', {'TOKEN':'untrusted'})]:
                bad = copy.deepcopy(config)
                bad['runtimeConfig']['workspaceRuntime']['commands'][0][field] = value
                with self.assertRaisesRegex(ValueError, 'WORKER_INVENTORY_REJECTED'):
                    worker_inventory.worker_endpoints(bad, sys.executable, '/candidate', fee_bps=fee)

    def test_inventory_one_shot_fail_closed(self):
        for bad in (True, 1, '123', -1):
            inventory = worker_inventory.SpawnInventory()
            with self.assertRaises(ValueError): inventory.record(bad)
            with self.assertRaises(ValueError): inventory.record(123)
        inventory = worker_inventory.SpawnInventory()
        inventory.record(123)
        self.assertEqual(inventory.finish(), (123,))
        with self.assertRaises(ValueError): inventory.finish()
        with self.assertRaises(ValueError): inventory.record(456)
        inventory = worker_inventory.SpawnInventory()
        with self.assertRaises(ValueError): inventory.finish()
        with self.assertRaises(ValueError): inventory.record(123)

    def test_real_ready_child_pid_matches_and_reaps(self):
        fixture = test_ready_worker.ReadyTest()
        fixture.setUp(); self.addCleanup(fixture.doCleanups)
        staged = fixture.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
        inventory = worker_inventory.SpawnInventory()
        with ready(staged, [], lambda: 'same', 2, on_spawn=inventory.record):
            self.assertEqual(inventory._pid, int(fixture.pidfile.read_text()))
        pids = inventory.finish()
        with self.assertRaises(ProcessLookupError): os.kill(pids[0], 0)

    def test_callback_error_interrupt_reaps_before_start(self):
        fixture = test_ready_worker.ReadyTest()
        fixture.setUp(); self.addCleanup(fixture.doCleanups)
        staged = fixture.worker('s.sendall(b"READY\\n")\ns.recv(16)\n')
        for error in (ValueError('inventory'), KeyboardInterrupt()):
            pids = []
            def reject(pid):
                pids.append(pid)
                raise error
            with self.assertRaises(type(error)), ready(staged, [], lambda: 'same', 2, on_spawn=reject):
                self.fail('yielded')
            self.assertEqual(len(pids), 1)
            with self.assertRaises(ProcessLookupError): os.kill(pids[0], 0)

if __name__ == '__main__': unittest.main()
