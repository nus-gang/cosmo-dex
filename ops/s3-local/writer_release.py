"""One-shot writer reopen observation through the reviewed offline validator.

Internal boundary: the caller supplies the original byte-verified capture and
worker arguments, after control-plane stop. No fresh input capture, new home,
lock deletion, repair, retry, or service command is allowed here. A successful
validator opens the existing C store and drops it before its success report.
This observes reopenability, not continuous exclusion or process-tree exit.
"""
import time
from offline_check import validate_snapshot
from process_check import EXPECTED

ERROR = 'WRITER_RELEASE_UNCONFIRMED'


def check(raw, artifacts, arguments, scratch, timeout=60):
    return _check(raw, artifacts, arguments, scratch, timeout,
                  validate_snapshot, time.monotonic_ns)


def _check(raw, artifacts, arguments, scratch, timeout, validate, clock):
    # Trusted callable seam for tests; never loaded from runtime configuration.
    try:
        if (not isinstance(raw, bytes) or not raw or
                not isinstance(arguments, (list, tuple)) or
                any(not isinstance(arg, str) for arg in arguments)):
            raise ValueError()
        arguments = tuple(arguments)
        started = clock()
        if type(started) is not int or started < 0:
            raise ValueError()
        digest, report = validate(raw, artifacts, arguments, scratch, timeout)
        if (not isinstance(digest, str) or len(digest) != 64 or
                any(c not in '0123456789abcdef' for c in digest) or
                not isinstance(report, dict) or set(report) != set(EXPECTED) or
                any(report[k] is not v for k, v in EXPECTED.items())):
            raise ValueError()
        finished = clock()
        if type(finished) is not int or finished < started:
            raise ValueError()
        return {'schema': 's3-local-writer-probe/1',
                'validator_sha256': digest,
                'started_monotonic_ns': started, 'finished_monotonic_ns': finished,
                'writer_reopen_verified': True,
                'continuous_exclusion_verified': False,
                'process_exit_verified': False, 'port_release_verified': False,
                'commit_unchanged_verified': False,
                'approval_verified': False, 'services_started': False}
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception:
        raise ValueError(ERROR) from None
