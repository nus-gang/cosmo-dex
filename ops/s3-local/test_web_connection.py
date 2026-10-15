import unittest
from static_web import StaticWeb
from web_proxy import WebProxy
from web_connection import handle_connection


class Stream:
    def __init__(self, raw):
        self.raw = raw
        self.out = bytearray()
        self.closed = 0
        self.timeouts = []
        self.fail_send = False
    def settimeout(self, value):
        self.timeouts.append(value)
    def recv(self, n):
        result, self.raw = self.raw[:min(n, 3)], self.raw[min(n, 3):]
        return result
    def send(self, raw):
        if self.fail_send:
            raise OSError('private detail')
        self.out.extend(raw[:7])
        return len(raw[:7])
    def close(self):
        self.closed += 1


class ConnectionTests(unittest.TestCase):
    def setUp(self):
        self.proxy = WebProxy(origin='http://127.0.0.1:5173', worker_port=8787,
            static=StaticWeb(origin='http://127.0.0.1:5173', html=b'html', javascript=b'js'))
        self.calls = []
    def exchange(self, dest, raw):
        self.calls.append((dest, raw))
        return 200, [('Content-Type', 'application/json'), ('Content-Length', '2')], b'{}'
    def request(self, extra=b'', body=b'{}'):
        return (b'POST /dev-local/v1/chain/broadcast HTTP/1.1\r\nHost: 127.0.0.1:5173\r\n'
                b'Origin: http://127.0.0.1:5173\r\nAuthorization: Bearer private\r\n'
                b'Content-Type: application/json\r\nContent-Length: '+str(len(body)).encode()+b'\r\n'+extra+b'\r\n'+body)
    def run_stream(self, raw, **kw):
        stream = Stream(raw)
        result = handle_connection(stream, '127.0.0.1', self.proxy, self.exchange,
            clock=kw.pop('clock', lambda: 0.0), **kw)
        self.assertEqual(stream.closed, 1)
        return result, stream
    def test_exact_one_action_partial_io_pipeline_and_head(self):
        body = b'{"tx_bytes":"YWJj"}'
        result, stream = self.run_stream(self.request(body=body)+self.request())
        self.assertTrue(result)
        self.assertEqual(len(self.calls), 1)
        self.assertTrue(self.calls[0][1].endswith(b'\r\n\r\n'+body))
        self.assertIn(b'Authorization: Bearer private\r\n', self.calls[0][1])
        self.assertEqual(stream.raw, self.request())
        self.assertTrue(stream.out.endswith(b'\r\n\r\n{}'))
        self.assertIn(b'Connection: close\r\n', stream.out)
        self.assertTrue(all(0 < value <= 5 for value in stream.timeouts))
        result, stream = self.run_stream(b'HEAD / HTTP/1.1\r\nHost: 127.0.0.1:5173\r\n\r\n')
        self.assertTrue(result)
        self.assertIn(b'Content-Length: 4\r\n', stream.out)
        self.assertTrue(stream.out.endswith(b'\r\n\r\n'))
        self.assertEqual(len(self.calls), 1)
    def test_ambiguous_framing_and_caps_no_effect(self):
        for extra in [b'Content-Length: 2\r\n', b'Transfer-Encoding: chunked\r\n',
                      b'Expect: 100-continue\r\n', b' Upgrade: x\r\n', b'X: bad\tvalue\r\n',
                      b'X: '+b'x'*4096+b'\r\n', b'X: v\r\n'*33,
                      (b'X: '+b'x'*4000+b'\r\n')*5]:
            with self.subTest(extra=extra[:30]):
                ok, stream = self.run_stream(self.request(extra))
                self.assertFalse(ok)
                self.assertEqual(stream.out, b'')
        for raw in [self.request().replace(b'Length: 2', b'Length: 02'),
                    self.request().replace(b'Length: 2', b'Length: 16385'),
                    self.request()[:-1], self.request().replace(b'\r\n', b'\n'),
                    self.request().replace(b'HTTP/1.1', b'HTTP/1.0'),
                    self.request().replace(b'POST /dev', b'POST http://evil/dev'),
                    self.request().replace(b'Content-Length: 2\r\n', b'')]:
            self.assertFalse(self.run_stream(raw)[0])
        self.assertEqual(self.calls, [])
    def test_duplicate_security_headers_preserved_and_rejected(self):
        for extra in [b'Host: 127.0.0.1:5173\r\n', b'Authorization: Bearer other\r\n',
                      b'Origin: http://127.0.0.1:5173\r\n']:
            ok, stream = self.run_stream(self.request(extra))
            self.assertTrue(ok)  # Error response transmitted, not accepted effect.
            self.assertNotIn(b'HTTP/1.1 200', stream.out)
        self.assertEqual(self.calls, [])
    def test_deadline_clock_stop_interrupt_and_peer_close(self):
        for values in [[0, 5], [1, 0], [0, float('nan')]]:
            iterator = iter(values)
            self.assertFalse(self.run_stream(self.request(), clock=lambda: next(iterator))[0])
        self.assertFalse(self.run_stream(self.request(), stop=lambda: True)[0])
        stream = Stream(self.request())
        with self.assertRaises(KeyboardInterrupt):
            handle_connection(stream, '127.0.0.1', self.proxy, self.exchange,
                stop=lambda: (_ for _ in ()).throw(KeyboardInterrupt()))
        self.assertEqual(stream.closed, 1)
        stream = Stream(self.request())
        self.assertFalse(handle_connection(stream, '192.0.2.1', self.proxy, self.exchange))
        self.assertEqual(stream.closed, 1)
        self.assertEqual(self.calls, [])
    def test_response_loss_never_retries(self):
        stream = Stream(self.request())
        stream.fail_send = True
        self.assertFalse(handle_connection(stream, '127.0.0.1', self.proxy, self.exchange))
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(stream.closed, 1)
        self.assertEqual(stream.out, b'')
        def fail(*args):
            self.calls.append(args)
            raise OSError('private effect may have happened')
        stream = Stream(self.request())
        self.assertTrue(handle_connection(stream, '127.0.0.1', self.proxy, fail))
        self.assertEqual(len(self.calls), 2)
        self.assertIn(b'HTTP/1.1 503', stream.out)
        self.assertNotIn(b'private', stream.out)
        self.assertEqual(stream.closed, 1)


class Listener:
    import socket
    family = socket.AF_INET
    type = socket.SOCK_STREAM
    def __init__(self, streams):
        self.streams, self.closed, self.accepts = streams, 0, 0
    def getsockname(self):
        return ('127.0.0.1', 5173)
    def settimeout(self, value):
        pass
    def accept(self):
        self.accepts += 1
        value = self.streams.pop(0)
        if isinstance(value, BaseException):
            raise value
        return value, ('127.0.0.1', 12345)
    def close(self):
        self.closed += 1


class ListenerTests(unittest.TestCase):
    def setUp(self):
        self.proxy = WebProxy(origin='http://127.0.0.1:5173', worker_port=8787,
            static=StaticWeb(origin='http://127.0.0.1:5173', html=b'html', javascript=b'js'))
        self.raw = b'GET / HTTP/1.1\r\nHost: 127.0.0.1:5173\r\n\r\n'
    def run_listener(self, listener, **kw):
        from web_connection import serve_listener
        args = dict(max_requests=2, lifetime_seconds=10, stop=lambda: False, clock=lambda: 0.0)
        args.update(kw)
        return serve_listener(listener, self.proxy, lambda *_: self.fail('upstream'), **args)
    def test_request_limit_and_idle_accept(self):
        streams = [Stream(self.raw) for _ in range(3)]
        listener = Listener([TimeoutError(), *streams])
        result = self.run_listener(listener)
        self.assertEqual(result['handled_connections'], 2)
        self.assertFalse(result['approval_verified'])
        self.assertEqual(listener.accepts, 3)
        self.assertEqual([s.closed for s in streams], [1, 1, 0])
        self.assertEqual(listener.closed, 1)
    def test_error_interrupt_no_reaccept_and_input_reject(self):
        for first in [Stream(b'bad\r\n'), KeyboardInterrupt(), OSError('secret')]:
            listener = Listener([first, Stream(self.raw)])
            with self.assertRaises(BaseException):
                self.run_listener(listener)
            self.assertEqual(listener.accepts, 1)
            self.assertEqual(listener.closed, 1)
            if isinstance(first, Stream):
                self.assertEqual(first.closed, 1)
        for options in [dict(max_requests=True), dict(max_requests=10001), dict(lifetime_seconds=3601)]:
            listener = Listener([])
            with self.assertRaises(ValueError):
                self.run_listener(listener, **options)
            self.assertEqual(listener.accepts, 0)
            self.assertEqual(listener.closed, 1)
    def test_stop_after_accept_and_lifetime(self):
        stream = Stream(self.raw)
        listener = Listener([stream])
        calls = iter([False, True])
        self.assertEqual(self.run_listener(listener, stop=lambda: next(calls))['handled_connections'], 0)
        self.assertEqual(stream.closed, 1)
        self.assertEqual(stream.out, b'')
        listener = Listener([])
        times = iter([0, 10])
        self.assertEqual(self.run_listener(listener, clock=lambda: next(times))['handled_connections'], 0)
        self.assertEqual(listener.accepts, 0)
        self.assertEqual(listener.closed, 1)
