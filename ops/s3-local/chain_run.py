"""Internal foreground Chain supervisor for the later managed L-T command.

Must run within chain_stage.stage with an authenticated exact-candidate audit.
No restart, home deletion, repair, or reusable authorization is provided.
"""
import math
import os
import selectors
import time
from chain_ready import _scope


def run(staged, pin, home, rpc, p2p, peers, audit, *, stopped=lambda: False,
        timeout=30, lifetime=300, output_limit=1024*1024, on_spawn=None):
    if type(lifetime) not in (int, float) or not math.isfinite(lifetime) or not 0 < lifetime <= 3600:
        raise ValueError('CHAIN_RUN_LIFETIME_LIMIT')
    if type(output_limit) is not int or not 0 < output_limit <= 1024*1024:
        raise ValueError('CHAIN_RUN_OUTPUT_LIMIT')
    with _scope(staged, pin, home, rpc, p2p, peers, audit, stopped=stopped,
                timeout=timeout, on_spawn=on_spawn) as (child, baseline, deadline):
        # Fresh authorization after READY, immediately before the sole START.
        if audit() != baseline:
            raise ValueError('APPROVAL_CHANGED_BEFORE_CHAIN_START')
        staged.verify()
        if stopped() or child.poll() is not None or time.monotonic() >= deadline:
            raise ValueError('CHAIN_STOPPED_BEFORE_START')
        os.set_blocking(child.stdin.fileno(), False)
        if os.write(child.stdin.fileno(), b'START\n') != 6:
            raise ValueError('CHAIN_START_WRITE_OUTCOME_UNKNOWN')
        child.stdin.close()  # Exact START plus EOF is the B gate protocol.
        end = time.monotonic() + lifetime
        total = 0
        with selectors.DefaultSelector() as selector:
            for stream in (child.stdout, child.stderr):
                selector.register(stream, selectors.EVENT_READ)
            while True:
                if stopped():
                    return {'start_sent': True, 'stop_requested': True,
                            'child_exit': None, 'output_bytes': total,
                            'approval_verified': False, 'reusable_permit': False,
                            'cleanup_complete_verified': False}
                remaining = end-time.monotonic()
                if remaining <= 0:
                    raise ValueError('CHAIN_RUN_TIMEOUT_OUTCOME_UNKNOWN')
                for key, _ in selector.select(min(.05, remaining)):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    else:
                        total += len(data)
                        if total > output_limit:
                            raise ValueError('CHAIN_RUN_OUTPUT_OUTCOME_UNKNOWN')
                code = child.poll()
                if code is not None and not selector.get_map():
                    if code != 0:
                        raise ValueError('CHAIN_RUN_FAILED_OUTCOME_UNKNOWN')
                    return {'start_sent': True, 'stop_requested': False,
                            'child_exit': code, 'output_bytes': total,
                            'approval_verified': False, 'reusable_permit': False,
                            'cleanup_complete_verified': False}
