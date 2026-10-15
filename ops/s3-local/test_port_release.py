import socket
import unittest
from unittest.mock import Mock
import port_release as probe


class PortReleaseTest(unittest.TestCase):
    def setup_probe(self, fail=None):
        self.events = []
        self.sockets = []
        def factory(family, kind):
            index = len(self.sockets)
            sock = Mock()
            sock.close.side_effect = lambda: self.events.append(('close', index))
            def bind(endpoint):
                self.events.append(('bind', index, endpoint))
                if fail and index == 1: raise fail
            sock.bind.side_effect = bind
            self.sockets.append(sock)
            return sock
        self.factory = Mock(side_effect=factory)
        self.clock = Mock(side_effect=[10,20])

    def test_simultaneous_ipv4_ipv6_bind_then_reverse_close(self):
        self.setup_probe()
        result = probe._check([('127.0.0.1',21001),('::1',21001)],self.factory,self.clock)
        self.assertEqual(self.events,[('bind',0,('127.0.0.1',21001)),
            ('bind',1,('::1',21001)),('close',1),('close',0)])
        self.assertEqual(self.factory.call_args_list[0].args,(socket.AF_INET,socket.SOCK_STREAM))
        self.assertEqual(self.factory.call_args_list[1].args,(socket.AF_INET6,socket.SOCK_STREAM))
        self.sockets[0].setsockopt.assert_not_called()
        self.sockets[1].setsockopt.assert_called_once_with(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1)
        for sock in self.sockets:
            sock.listen.assert_not_called(); sock.connect.assert_not_called()
            sock.set_inheritable.assert_called_once_with(False)
            sock.close.assert_called_once()
        self.assertTrue(result['simultaneous_bind_verified'])
        for key in ('process_exit_verified','writer_release_verified','reservation_retained'):
            self.assertFalse(result[key])

    def test_invalid_or_duplicate_endpoints_reject_before_io(self):
        for endpoints in ([], '127.0.0.1:1234', [('0.0.0.0',1234)], [('localhost',1234)],
                [('127.0.0.1',True)], [('::1',0)], [('::1',65536)],
                [('::1','1234')], [('::1',1234)]*2, [('::1',1234,1)],
                [('::1',2000+i) for i in range(33)]):
            self.setup_probe()
            with self.assertRaisesRegex(ValueError,'^'+probe.ERROR+'$'):
                probe._check(endpoints,self.factory,self.clock)
            self.factory.assert_not_called(); self.clock.assert_not_called()

    def test_busy_interrupt_and_clock_failure_close_all(self):
        for fail in (OSError('private diagnostic'),KeyboardInterrupt()):
            self.setup_probe(fail)
            with self.assertRaises(KeyboardInterrupt if isinstance(fail,KeyboardInterrupt) else ValueError):
                probe._check([('::1',1234),('::1',1235)],self.factory,self.clock)
            self.assertEqual(self.events[-2:],[('close',1),('close',0)])
            self.assertEqual(self.factory.call_count,2)
        self.setup_probe(); self.clock.side_effect = [20,10]
        with self.assertRaisesRegex(ValueError,'^'+probe.ERROR+'$'):
            probe._check([('::1',1234)],self.factory,self.clock)
        self.sockets[0].close.assert_called_once()

    def test_allocation_setup_and_close_errors_never_report_success(self):
        for stage in ('allocate','setup','close'):
            self.setup_probe()
            if stage == 'allocate': self.factory.side_effect = OSError('secret')
            else:
                old = self.factory.side_effect
                def factory(family,kind):
                    sock = old(family,kind)
                    getattr(sock,'set_inheritable' if stage=='setup' else 'close').side_effect = OSError('secret')
                    return sock
                self.factory.side_effect = factory
            with self.assertRaisesRegex(ValueError,'^'+probe.ERROR+'$'):
                probe._check([('::1',1234)],self.factory,self.clock)
            if self.sockets: self.sockets[0].close.assert_called_once()

if __name__ == '__main__': unittest.main()
