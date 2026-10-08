"""F05 before-send READY probe; never sends START or creates evidence."""
from contextlib import contextmanager
import os
from ready_worker import _ready_scope


def before_send_arguments(tx_hash, evidence_root, enable):
    # Match before_send_options.rs before Path normalization can hide bad input.
    try:
        raw = os.fspath(evidence_root)
    except TypeError:
        raise ValueError('F05_OPTIONS') from None
    if enable is not True or not isinstance(tx_hash, str) or len(tx_hash) != 64 or any(c not in '0123456789abcdef' for c in tx_hash) or \
       not isinstance(raw, str) or not raw.startswith('/') or '\0' in raw or \
       any(part in ('', '.', '..') for part in raw.split('/')[1:]):
        raise ValueError('F05_OPTIONS')
    return ['--enable-f05-before-send', 'true', '--tx-hash', tx_hash,
            '--fault-evidence-root', raw, '--worker-inputs']


@contextmanager
def ready(staged, arguments, audit, *, tx_hash, evidence_root, enable=False,
          stop=lambda: False, timeout=5):
    """Use within before_send_stage.stage; audit reads exact candidate approvals.

    READY only proves child preparation. Exiting always kills/reaps the child;
    this result is neither an execution permit nor F05 verification.
    """
    options = before_send_arguments(tx_hash, evidence_root, enable)
    arguments = list(arguments)
    if any(not isinstance(arg, str) or arg.startswith(('--fault-', '--enable-f14', '--enable-f05', '--tx-hash',
           '--enable-storage')) or arg == '--worker-inputs' for arg in arguments):
        raise ValueError('DUPLICATE_F05_OPTIONS')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='f05-crash-captured'):
        if stop():
            raise ValueError('F05_STOPPED')
        yield {'ready': True, 'fault_started': False, 'service_started': False,
               'F05_verified': False, 'approval_verified': False, 'reusable_permit': False}
