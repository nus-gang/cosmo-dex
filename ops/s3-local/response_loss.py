"""Opt-in, single-use L-T direct-broadcast response-loss seam.

Only wraps an existing bounded exchange after WebProxy framing/auth forwarding.
No socket, retry, runtime activation, chain finality inference or stored bearer.
The owning L-T harness must supply the approved exchange and retain its evidence.
"""
import hashlib
import re


class ResponseLoss:
    def __init__(self, exchange, *, destination, body_sha256,
                 enable_local_demo=False, allow_unproven_host_space=False):
        if (enable_local_demo is not True or allow_unproven_host_space is not True or
                not callable(exchange) or type(destination) is not tuple or
                len(destination) != 2 or destination[0] != '127.0.0.1' or
                type(destination[1]) is not int or not 1024 <= destination[1] <= 65535 or
                destination[1] == 5173 or type(body_sha256) is not str or
                re.fullmatch('[0-9a-f]{64}', body_sha256) is None):
            raise ValueError('RESPONSE_LOSS_CONFIG')
        self._exchange = exchange
        self._destination = destination
        self._body_sha256 = body_sha256
        self._used = False
        self._attempted = False
        self._returned = False

    def report(self):
        return dict(schema='s3-local-response-loss/1', used=self._used,
                    upstream_attempted=self._attempted, upstream_returned=self._returned,
                    response_discarded=self._returned, body_sha256=self._body_sha256,
                    chain_effect_verified=False, DEV='NOT_RUN', durable_ack=False)

    def __call__(self, destination, request):
        if self._used:
            raise ValueError('RESPONSE_LOSS_USED')
        # Close before validation/callback: even interruption or reentrancy cannot resend.
        self._used = True
        if (destination != self._destination or type(request) is not bytes or
                not 0 < len(request) <= 150000 or
                not request.startswith(b'POST /dev-local/v1/chain/broadcast HTTP/1.1\r\n')):
            raise ValueError('RESPONSE_LOSS_INPUT')
        _, sep, body = request.partition(b'\r\n\r\n')
        if not sep or not 0 < len(body) <= 16384 or hashlib.sha256(body).hexdigest() != self._body_sha256:
            raise ValueError('RESPONSE_LOSS_INPUT')
        self._attempted = True
        try:
            self._exchange(destination, request)
        except Exception:
            raise OSError('RESPONSE_LOSS_UPSTREAM_UNKNOWN') from None
        self._returned = True
        # Discard *any* complete response; a return does not prove a broadcast/commit.
        raise OSError('RESPONSE_LOSS_INJECTED')


class BroadcastResponseLoss:
    """WebProxy-only router: preserve reads/auth, close all subsequent broadcasts."""
    def __init__(self, exchange, **config):
        self._fault = ResponseLoss(exchange, **config)
        self._exchange = exchange
        self._destination = config['destination']

    def report(self):
        return self._fault.report()

    def __call__(self, destination, request):
        if (destination != self._destination or type(request) is not bytes or
                not 0 < len(request) <= 150000):
            raise ValueError('RESPONSE_LOSS_INPUT')
        line = request.partition(b'\r\n')[0]
        if line == b'POST /dev-local/v1/chain/broadcast HTTP/1.1':
            return self._fault(destination, request)
        # Only requests already framed and validated by WebProxy reach this seam.
        # No transparent fallback for alternate broadcast methods or spellings.
        from web_proxy import GET, POST, PREFIX
        allowed = {f'{method} {PREFIX}{route} HTTP/1.1'.encode('ascii')
                   for method, routes in (('GET', GET), ('POST', POST))
                   for route in routes if route != 'chain/broadcast'}
        if line not in allowed:
            raise ValueError('RESPONSE_LOSS_ROUTE')
        return self._exchange(destination, request)
