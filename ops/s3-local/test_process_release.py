import errno
import unittest
from unittest.mock import Mock, patch
import process_release as release


class ProcessReleaseTest(unittest.TestCase):
    def test_esrch_only_and_signal_zero_once_per_pid(self):
        probe = Mock(side_effect=OSError(errno.ESRCH, 'absent'))
        result = release._check([100, 101], probe, Mock(side_effect=[10, 20]))
        self.assertEqual([call.args for call in probe.call_args_list], [(100, 0), (101, 0)])
        self.assertEqual(result['pids'], [100, 101])
        self.assertTrue(result['listed_pids_absent'])
        for key in ('inventory_complete_verified', 'process_tree_exit_verified',
                    'writer_release_verified', 'port_release_verified'):
            self.assertFalse(result[key])
        with patch.object(release.os, 'kill', probe), patch.object(release.time, 'monotonic_ns', side_effect=[30, 40]):
            self.assertTrue(release.check([102])['listed_pids_absent'])

    def test_invalid_inventory_rejects_before_probe(self):
        for pids in ([], [0], [-1], [1], [True], ['100'], [2**31], [100, 100],
                     list(range(100, 133)), {100}, '100', [100, None]):
            probe, clock = Mock(), Mock()
            with self.assertRaisesRegex(ValueError, '^'+release.ERROR+'$'):
                release._check(pids, probe, clock)
            probe.assert_not_called(); clock.assert_not_called()

    def test_live_reused_zombie_permission_unknown_refuse_without_retry(self):
        for outcome in (None, PermissionError(errno.EPERM, 'private'),
                        OSError(errno.EACCES, 'private'), OSError(errno.EIO, 'private'),
                        ProcessLookupError('missing errno'), RuntimeError('private')):
            probe = Mock(side_effect=outcome)
            with self.assertRaisesRegex(ValueError, '^'+release.ERROR+'$'):
                release._check([100, 101], probe, Mock(return_value=10))
            probe.assert_called_once_with(100, 0)

    def test_partial_failure_interrupt_and_invalid_clock_never_succeed(self):
        for failure in (OSError(errno.EPERM, 'private'), KeyboardInterrupt(), SystemExit()):
            probe = Mock(side_effect=[OSError(errno.ESRCH, 'absent'), failure])
            expected = type(failure) if isinstance(failure, (KeyboardInterrupt, SystemExit)) else ValueError
            with self.assertRaises(expected):
                release._check([100, 101, 102], probe, Mock(return_value=10))
            self.assertEqual(probe.call_count, 2)
        for values in ([20, 10], [10, True], [10, -1], [-1], [True]):
            probe = Mock(side_effect=OSError(errno.ESRCH, 'absent'))
            with self.assertRaisesRegex(ValueError, '^'+release.ERROR+'$'):
                release._check([100], probe, Mock(side_effect=values))
            if len(values) == 1: probe.assert_not_called()


if __name__ == '__main__': unittest.main()
