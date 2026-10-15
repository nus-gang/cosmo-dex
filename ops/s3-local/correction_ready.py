"""F14 Prepare READY probe; never sends START or creates evidence."""
from contextlib import contextmanager
import os
from ready_worker import _ready_scope


def correction_arguments(occurrence, evidence_root, enable):
    # Match correction_options.rs before Path normalization can hide bad input.
    try:
        raw = os.fspath(evidence_root)
    except TypeError:
        raise ValueError('F14_OPTIONS') from None
    if enable is not True or type(occurrence) is not int or not 1 <= occurrence <= 1024 or \
       not isinstance(raw, str) or not raw.startswith('/') or '\0' in raw or \
       any(part in ('', '.', '..') for part in raw.split('/')[1:]):
        raise ValueError('F14_OPTIONS')
    return ['--enable-f14-prepare', 'true', '--fault-occurrence', str(occurrence),
            '--fault-evidence-root', raw, '--worker-inputs']


@contextmanager
def ready(staged, arguments, audit, *, occurrence, evidence_root, enable=False,
          stop=lambda: False, timeout=5):
    """Use within correction_stage.stage; audit reads exact candidate approvals.

    READY only proves child preparation. Exiting always kills/reaps the child;
    this result is neither an execution permit nor F14 verification.
    """
    options = correction_arguments(occurrence, evidence_root, enable)
    arguments = list(arguments)
    if any(not isinstance(arg, str) or arg.startswith(('--fault-', '--enable-f14',
           '--enable-storage')) or arg == '--worker-inputs' for arg in arguments):
        raise ValueError('DUPLICATE_F14_OPTIONS')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='f14-apply-captured'):
        if stop():
            raise ValueError('F14_STOPPED')
        yield {'ready': True, 'fault_started': False, 'service_started': False,
               'F14_verified': False, 'approval_verified': False, 'reusable_permit': False}
