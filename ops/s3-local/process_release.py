"""One-shot absence observation for explicit PIDs recorded by the launcher.

Signal 0 never terminates a process. ESRCH is the only success; PID reuse,
permission denial and zombies refuse completion. This does not establish that
an inventory is complete, descendants exited, or writer/ports were released.
"""
import errno
import os
import time

ERROR = 'PROCESS_RELEASE_UNCONFIRMED'


def check(pids):
    return _check(pids, os.kill, time.monotonic_ns)


def _check(pids, probe, clock):
    # Trusted injection seam for pure tests, not serialized runtime options.
    try:
        if (not isinstance(pids, (tuple, list)) or not 1 <= len(pids) <= 32 or
                any(type(pid) is not int or not 2 <= pid <= 2147483647 for pid in pids) or
                len(set(pids)) != len(pids)):
            raise ValueError()
        pids = tuple(pids)
        started = clock()
        if type(started) is not int or started < 0:
            raise ValueError()
        for pid in pids:
            try:
                probe(pid, 0)
            except OSError as exc:
                if exc.errno != errno.ESRCH:
                    raise ValueError() from None
            else:
                raise ValueError()
        finished = clock()
        if type(finished) is not int or finished < started:
            raise ValueError()
        return {'schema': 's3-local-process-probe/1', 'pids': list(pids),
                'started_monotonic_ns': started, 'finished_monotonic_ns': finished,
                'listed_pids_absent': True, 'inventory_complete_verified': False,
                'process_tree_exit_verified': False, 'writer_release_verified': False,
                'port_release_verified': False}
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
