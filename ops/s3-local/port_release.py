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


class PortFailure(ValueError):
    def __init__(self, report):
        super().__init__(ERROR)
        self.report = report


def bounded_check(endpoints, stop=lambda: False):
    return _bounded_check(endpoints, socket.socket, time.monotonic_ns, time.sleep, stop)


def _bounded_check(endpoints, factory, clock, sleep, stop):
    """One 60-second budget for the whole set; only EADDRINUSE is transient."""
    import errno
    endpoints = tuple(tuple(x) for x in endpoints)
    # Validate without opening a real socket, using the same strict grammar.
    if (not 1 <= len(endpoints) <= 32 or len(set(endpoints)) != len(endpoints) or
            any(len(x)!=2 or x[0] not in ('127.0.0.1','::1') or type(x[1]) is not int or
                not 1024 <= x[1] <= 65535 for x in endpoints)):
        raise ValueError(ERROR)
    started = clock(); previous = started
    if type(started) is not int or started<0:raise ValueError(ERROR)
    report = dict(schema='s3-local-bounded-port-probe/1', endpoints=list(endpoints),
        attempts=[], simultaneous_bind_verified=False, reservation_retained=False,
        cleanup_complete_verified=False)
    try:
        for index in range(241):
            now = clock()
            if type(now) is not int or not 0 <= previous <= now <= started+60_000_000_000:
                raise ValueError()
            previous = now
            if stop(): raise KeyboardInterrupt()
            row = dict(index=index, at_monotonic_ns=now, endpoint=None, errno=None, code='OK')
            report['attempts'].append(row)
            held = []; busy = False
            try:
                for endpoint in endpoints:
                    row['endpoint'] = endpoint
                    family = socket.AF_INET6 if endpoint[0]=='::1' else socket.AF_INET
                    sock = factory(family,socket.SOCK_STREAM);held.append(sock)
                    sock.set_inheritable(False)
                    if family==socket.AF_INET6: sock.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1)
                    try: sock.bind(endpoint)
                    except OSError as error:
                        row.update(errno=error.errno,code='BIND_ERROR')
                        if error.errno != errno.EADDRINUSE: raise
                        busy=True;break
                if not busy:
                    row['endpoint']=None
                    report['simultaneous_bind_verified']=True
            finally:
                # Every handle is closed even when another close fails.
                close_error=None
                for sock in reversed(held):
                    try:sock.close()
                    except BaseException as error:close_error=error
                if close_error is not None:raise close_error
            row['finished_monotonic_ns']=clock()
            if row['finished_monotonic_ns'] < now or row['finished_monotonic_ns'] > started+60_000_000_000:
                raise ValueError()
            if stop():raise KeyboardInterrupt()
            if report['simultaneous_bind_verified']:return report
            if index==240 or row['finished_monotonic_ns']>=started+60_000_000_000:break
            sleep(min(.25,(started+60_000_000_000-row['finished_monotonic_ns'])/1e9))
        report['error']='PORT_BUSY_DEADLINE'
    except BaseException as error:
        report['simultaneous_bind_verified']=False
        report['error']='INTERRUPTED' if isinstance(error,(KeyboardInterrupt,SystemExit)) else 'PORT_PROBE_FAILED'
    raise PortFailure(report)
