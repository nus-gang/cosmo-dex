"""Same-origin REST forwarding boundary. No listener or authorization grant.

The caller supplies parsed, duplicate-preserving headers and a bounded upstream
exchange. The exchange receives only a fixed literal loopback destination and
one immutable HTTP request; it must never retry, follow redirects, or log bytes.
Worker Rest remains the authentication and economic authority.
"""
import re
from static_web import StaticWeb

PREFIX = '/dev-local/v1/'
GET = frozenset(('capabilities', 'account', 'chain/account'))
POST = frozenset(('auth/challenge', 'auth/session', 'auth/logout', 'orders',
    'cancels', 'receipts/orders', 'receipts/cancels', 'withdraw/prepare',
    'withdraw/abort', 'chain/broadcast', 'chain/result'))
TOKEN = re.compile(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+\Z")
STATUSES = frozenset((200, 204, 400, 401, 403, 404, 405, 409, 413, 503))


class WebProxy:
    def __init__(self, *, origin, worker_port, static):
        if origin not in ('http://127.0.0.1:5173', 'http://localhost:5173'):
            raise ValueError('ORIGIN_REJECTED')
        if type(worker_port) is not int or not 1024 <= worker_port <= 65535 or worker_port == 5173:
            raise ValueError('WORKER_PORT_REJECTED')
        if not isinstance(static, StaticWeb) or static._host != origin[7:]:
            raise ValueError('STATIC_ORIGIN_REJECTED')
        self.origin, self.host = origin, origin[7:]
        self.destination = ('127.0.0.1', worker_port)
        self.static = static

    @staticmethod
    def failure(status=400):
        return StaticWeb._response(status,
            b'{"code":"WEB_TRANSPORT_UNAVAILABLE","durable_ack":false}', 'application/json')

    def respond(self, method, target, headers, body, peer, exchange):
        if (peer not in ('127.0.0.1', '::1') or type(body) is not bytes or len(body) > 16384 or
            type(headers) is not list or len(headers) > 32):
            return self.failure()
        for row in headers:
            if (type(row) is not tuple or len(row) != 2 or
                any(type(v) is not str for v in row) or not TOKEN.fullmatch(row[0]) or
                len(row[0]) + len(row[1]) > 4094 or
                any(ord(c) < 32 or ord(c) >= 127 for c in row[1])):
                return self.failure()
        values = lambda key: [v for k, v in headers if k.lower() == key]
        if values('host') != [self.host]:
            return self.failure()
        # Refuse ambiguous framing and proxy metadata; never erase duplicates.
        if any(values(k) for k in ('transfer-encoding', 'expect', 'upgrade',
                'proxy-connection', 'proxy-authorization', 'forwarded',
                'x-forwarded-for', 'x-forwarded-host', 'x-forwarded-proto')):
            return self.failure()
        lengths = values('content-length')
        if lengths and lengths != [str(len(body))]:
            return self.failure()
        if method != 'POST' and body:
            return self.failure()
        if not isinstance(target, str):
            return self.failure()
        if not target.startswith(PREFIX):
            return self.static.respond(method, target, headers, peer)
        suffix = target[len(PREFIX):]
        if (method == 'GET' and suffix not in GET) or (method == 'POST' and suffix not in POST) or method not in ('GET', 'POST'):
            return self.failure(404)
        origins = values('origin')
        # Same-origin browser GET can omit Origin. Exact Host+loopback was checked;
        # worker still requires a bearer. Never repair an explicit wrong Origin.
        if origins != [self.origin] and not (method == 'GET' and origins == []):
            return self.failure(403)
        auth = values('authorization')
        if len(auth) > 1:
            return self.failure(401)
        if method == 'POST' and (lengths != [str(len(body))] or values('content-type') != ['application/json']):
            return self.failure()
        out = [f'{method} {target} HTTP/1.1',
               f'Host: 127.0.0.1:{self.destination[1]}', f'Origin: {self.origin}',
               'Connection: close', f'Content-Length: {len(body)}']
        if auth:
            out.append('Authorization: ' + auth[0])
        if method == 'POST':
            out.append('Content-Type: application/json')
        request = ('\r\n'.join(out) + '\r\n\r\n').encode('ascii') + body
        try:
            # Exactly one call even on ambiguous response loss after an effect.
            status, response_headers, response = exchange(self.destination, request)
            if type(status) is not int or status not in STATUSES or type(response) is not bytes or len(response) > 2*1024*1024:
                return self.failure(503)
            if type(response_headers) is not list or any(type(row) is not tuple or len(row) != 2 or any(type(v) is not str for v in row) for row in response_headers):
                return self.failure(503)
            fields = lambda key: [v for k, v in response_headers if k.lower() == key]
            if (fields('content-length') != [str(len(response))] or
                fields('content-type') != ['application/json'] or fields('transfer-encoding') or
                fields('location') or fields('set-cookie') or (status == 204 and response)):
                return self.failure(503)
            # Regenerate headers; never pass worker strings into browser headers.
            return StaticWeb._response(status, response, 'application/json')
        except Exception:
            return self.failure(503)
