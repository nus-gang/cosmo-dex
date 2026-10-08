import hashlib
import unittest
from response_loss import ResponseLoss
from static_web import StaticWeb
from web_proxy import WebProxy


class ResponseLossTests(unittest.TestCase):
    body = b'{"tx_bytes":"YWJj"}'
    destination = ('127.0.0.1', 8787)

    def fault(self, exchange, **overrides):
        args = dict(destination=self.destination, body_sha256=hashlib.sha256(self.body).hexdigest(),
                    enable_local_demo=True, allow_unproven_host_space=True)
        args.update(overrides)
        return ResponseLoss(exchange, **args)

    def request(self, fault, suffix='chain/broadcast', body=None):
        body = self.body if body is None else body
        origin = 'http://127.0.0.1:5173'
        proxy = WebProxy(origin=origin, worker_port=8787,
                         static=StaticWeb(origin=origin, html=b'x', javascript=b'y'))
        return proxy.respond('POST', '/dev-local/v1/' + suffix,
            [('Host', '127.0.0.1:5173'), ('Origin', origin), ('Authorization', 'Bearer private'),
             ('Content-Type', 'application/json'), ('Content-Length', str(len(body)))],
            body, '127.0.0.1', fault)

    def test_drop_after_one_exchange_and_reuse_refusal(self):
        calls = []
        def exchange(dest, raw):
            calls.append((dest, raw))
            return 200, [], b'private response'
        fault = self.fault(exchange)
        for _ in range(2):
            status, _, body = self.request(fault)
            self.assertEqual(status, 503)
            self.assertNotIn(b'private', body)
            self.assertNotIn(b'COMMITTED', body)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][0], self.destination)
        self.assertTrue(calls[0][1].endswith(self.body))
        report = fault.report()
        self.assertTrue(report['response_discarded'])
        self.assertFalse(report['chain_effect_verified'])
        self.assertNotIn('private', repr(report))
        report['used'] = False
        self.assertTrue(fault.report()['used'])

    def test_error_interrupt_and_reentrancy_never_retry(self):
        for error in (OSError('private detail'), KeyboardInterrupt(), SystemExit()):
            calls = []
            def exchange(*args):
                calls.append(args)
                raise error
            fault = self.fault(exchange)
            if isinstance(error, Exception):
                self.assertEqual(self.request(fault)[0], 503)
            else:
                with self.assertRaises(type(error)):
                    self.request(fault)
            self.assertEqual(self.request(fault)[0], 503)
            self.assertEqual(len(calls), 1)
            self.assertFalse(fault.report()['upstream_returned'])
        def recursive(dest, raw):
            with self.assertRaisesRegex(ValueError, 'RESPONSE_LOSS_USED'):
                fault(dest, raw)
        fault = self.fault(recursive)
        self.assertEqual(self.request(fault)[0], 503)
        self.assertTrue(fault.report()['response_discarded'])

    def test_wrong_route_body_destination_and_size_no_exchange(self):
        for suffix, body in [('orders', self.body), ('chain/result', self.body),
                             ('chain/broadcast', b'{}')]:
            fault = self.fault(lambda *_: self.fail('unexpected exchange'))
            self.assertEqual(self.request(fault, suffix, body)[0], 503)
            self.assertFalse(fault.report()['upstream_attempted'])
        for dest, raw in [(('127.0.0.1', 8788), b'x'), (self.destination, b'x'*150001),
                          (self.destination, b'POST /dev-local/v1/chain/broadcast HTTP/1.1\r\n')]:
            fault = self.fault(lambda *_: self.fail('unexpected exchange'))
            with self.assertRaisesRegex(ValueError, 'RESPONSE_LOSS_INPUT'):
                fault(dest, raw)
            self.assertTrue(fault.report()['used'])
            self.assertFalse(fault.report()['upstream_attempted'])

    def test_defaults_and_invalid_config_closed(self):
        with self.assertRaises(ValueError):
            ResponseLoss(lambda *_: None, destination=self.destination, body_sha256='a'*64)
        for change in [dict(enable_local_demo=False), dict(allow_unproven_host_space=1),
                       dict(destination=('localhost', 8787)), dict(destination=('127.0.0.1', True)),
                       dict(body_sha256='A'*64), dict(body_sha256='a'*63)]:
            with self.assertRaises(ValueError):
                self.fault(lambda *_: None, **change)
