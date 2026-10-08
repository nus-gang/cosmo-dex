import os
from pathlib import Path
import socket
import struct
import time
import unittest
from unittest.mock import Mock, patch

import private_reader as api

PATH = sorted(api.PATHS)[0]


class PrivateReaderTest(unittest.TestCase):
    def setUp(self):
        self.scratch = os.environ['PAPERCLIP_RUN_SCRATCH_DIR']

    def test_fresh_read_and_cleanup_no_token_transfer(self):
        reader = Mock(side_effect=[{'revision': 1}, {'revision': 2}])
        with api._broker(self.scratch, reader) as endpoint:
            client = api.PrivateReader(endpoint)
            self.assertEqual(client(PATH), {'revision': 1})
            self.assertEqual(client(PATH), {'revision': 2})
            self.assertEqual([p.name for p in endpoint.parent.iterdir()], ['s'])
            self.assertEqual(endpoint.parent.stat().st_mode & 0o777, 0o700)
            self.assertEqual(reader.call_count, 2)
        self.assertFalse(endpoint.parent.exists())
        with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'):
            client(PATH)

    def test_path_modes_links_and_upstream_error_rejected(self):
        reader = Mock(side_effect=OSError('synthetic-private-token'))
        with api._broker(self.scratch, reader) as endpoint:
            client = api.PrivateReader(endpoint)
            with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'):
                client('/api/agents/me')
            reader.assert_not_called()
            for mode in (0o666, 0o644):
                endpoint.chmod(mode)
                with self.assertRaises(ValueError): client(PATH)
            reader.assert_not_called()
            endpoint.chmod(0o600)
            link = endpoint.parent / 'l'; link.symlink_to(endpoint)
            with self.assertRaises(ValueError): api.PrivateReader(link)(PATH)
            link.unlink()
            with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'): client(PATH)
            self.assertEqual(reader.call_count, 1)

    def test_wire_rejects_truncated_large_trailing_and_wrong_paths(self):
        reader = Mock(return_value={})
        for wire in (b'', b'\0', struct.pack('!I', 513),
                     struct.pack('!I', 1)+b'aextra',
                     struct.pack('!I', 1)+b'a'):
            a,b = socket.socketpair()
            with a,b:
                a.sendall(wire); a.shutdown(socket.SHUT_WR)
                with self.assertRaises(ValueError): api._serve(b, reader, time.monotonic()+1)
        reader.assert_not_called()

    def test_response_bounds_and_failed_bind_cleanup(self):
        for result in ([], {'large': 'x'*100}):
            with api._broker(self.scratch, Mock(return_value=result)) as endpoint:
                with patch.object(api, 'MAX_RESPONSE', 32):
                    with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'):
                        api.PrivateReader(endpoint)(PATH)
        before = set(Path(self.scratch).iterdir())
        with patch.object(api.socket.socket, 'bind', side_effect=OSError('denied')):
            with self.assertRaises(OSError):
                with api._broker(self.scratch, Mock()): pass
        self.assertEqual(set(Path(self.scratch).iterdir()), before)

    def test_lease_request_budget_and_interrupt_cleanup(self):
        reader = Mock(return_value={})
        with api._broker(self.scratch, reader, max_requests=1) as endpoint:
            client = api.PrivateReader(endpoint)
            self.assertEqual(client(PATH), {})
            # Budget expiration means no second upstream request. Close the
            # client ourselves instead of waiting for the full response timeout.
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
                conn.connect(str(endpoint))
                api._send(conn, PATH.encode(), 512, time.monotonic()+1)
            self.assertEqual(reader.call_count, 1)
        with self.assertRaises(KeyboardInterrupt):
            with api._broker(self.scratch, reader) as endpoint:
                raise KeyboardInterrupt()
        self.assertFalse(endpoint.parent.exists())
        a,b = socket.socketpair()
        with a,b, self.assertRaises(ValueError):
            api._receive(b, 512, time.monotonic()-1)
        for kwargs in ({'lifetime':0}, {'max_requests':129}, {'lifetime':True}):
            with self.assertRaises(ValueError):
                with api._broker(self.scratch, reader, **kwargs): pass

if __name__ == '__main__': unittest.main()
