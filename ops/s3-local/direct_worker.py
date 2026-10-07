"""Private helper lifetime for checked READY and L-T managed worker launch.

The validated capture supplies the helper digest. Fresh audits and byte checks
run before READY and START; private copies survive until the child is reaped.
"""
from contextlib import contextmanager
import hashlib
from ready_worker import check_bytes, ready, run_managed
from staged_direct import stage_snapshot

@contextmanager
def _scope(staged, arguments, artifacts, scratch, audit):
    arguments = list(arguments)
    if any(not isinstance(arg, str) or arg.split('=', 1)[0] in
           ('--direct-helper', '--direct-helper-sha256') for arg in arguments):
        raise ValueError('DIRECT_HELPER_OVERRIDE')
    check_bytes(staged)
    baseline = audit()
    with stage_snapshot(staged.capture, artifacts, scratch) as helper:
        if helper.capture_sha256 != hashlib.sha256(staged.capture).hexdigest():
            raise ValueError('DIRECT_CAPTURE_CHANGED')
        def recheck():
            current = audit()
            if current != baseline:
                raise ValueError('DIRECT_APPROVAL_CHANGED')
            check_bytes(staged)
            helper.recheck()
            return current
        recheck()
        argv = ['--direct-helper', str(helper.executable),
                '--direct-helper-sha256', helper.executable_sha256, *arguments]
        yield argv, recheck
        # No post-exit check that could hide an original child exception. Private
        # helper cleanup occurs only after ready/run_managed has reaped the child.

@contextmanager
def ready_with_direct(staged, arguments, artifacts, scratch, audit, timeout=60, *, on_spawn=None):
    """Offline READY only; never sends START."""
    with _scope(staged, arguments, artifacts, scratch, audit) as (argv, recheck):
        with ready(staged, argv, recheck, timeout, on_spawn=on_spawn) as report:
            yield report

def run_with_direct(staged, arguments, artifacts, scratch, audit, stop,
                    timeout=60, lifetime=300, *, on_spawn=None):
    """L-T-only foreground composition; used by checked_run."""
    with _scope(staged, arguments, artifacts, scratch, audit) as (argv, recheck):
        return run_managed(staged, argv, recheck, stop, timeout, lifetime,
                           on_spawn=on_spawn)
