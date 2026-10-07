"""L-T-only point-in-time TCP bind probe. Never listens or starts a service.

All expected endpoints must come from the pinned runtime configuration. A bind
proves availability at this instant, not process exit, writer release, durable
reservation, or the absence of sockets bound with platform-specific reuse.
"""
from contextlib import ExitStack
import socket
import time

ERROR = 'PORT_RELEASE_UNCONFIRMED'


def check(endpoints):
    return _check(endpoints, socket.socket, time.monotonic_ns)


def _check(endpoints, socket_factory, clock):
    # Private injection seam for pure L-R tests, not user-supplied adapters.
    try:
        if not isinstance(endpoints, (tuple, list)) or not 1 <= len(endpoints) <= 32:
            raise ValueError()
        checked = []
        for item in endpoints:
            if (not isinstance(item, (tuple, list)) or len(item) != 2 or
                    item[0] not in ('127.0.0.1', '::1') or
                    type(item[1]) is not int or not 1024 <= item[1] <= 65535):
                raise ValueError()
            checked.append(tuple(item))
        if len(set(checked)) != len(checked):
            raise ValueError()
        started = clock()
        with ExitStack() as stack:
            for host, port in checked:
                family = socket.AF_INET6 if host == '::1' else socket.AF_INET
                sock = socket_factory(family, socket.SOCK_STREAM)
                stack.callback(sock.close)
                sock.set_inheritable(False)
                # No REUSEADDR/REUSEPORT: a prior live binding must not be hidden.
                if family == socket.AF_INET6:
                    sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                sock.bind((host, port))
            finished = clock()
            if (type(started) is not int or type(finished) is not int or
                    started < 0 or finished < started):
                raise ValueError()
        return {'schema': 's3-local-port-probe/1', 'endpoints': checked,
                'started_monotonic_ns': started, 'finished_monotonic_ns': finished,
                'simultaneous_bind_verified': True, 'reservation_retained': False,
                'process_exit_verified': False, 'writer_release_verified': False}
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
