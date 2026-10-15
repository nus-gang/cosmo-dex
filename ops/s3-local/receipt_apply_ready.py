"""F09 Receipt->Apply READY probe; never sends START or creates evidence."""
from contextlib import contextmanager
import os
from ready_worker import _ready_scope

def receipt_apply_arguments(batch_id, evidence_root, enable):
    try: raw = os.fspath(evidence_root)
    except TypeError: raise ValueError('F09_OPTIONS') from None
    if enable is not True or not isinstance(batch_id, str) or len(batch_id) != 64 or \
       any(c not in '0123456789abcdef' for c in batch_id) or not isinstance(raw, str) or \
       not raw.startswith('/') or '\0' in raw or any(part in ('', '.', '..') for part in raw.split('/')[1:]):
        raise ValueError('F09_OPTIONS')
    return ['--enable-f09-receipt-apply', 'true', '--batch-id', batch_id,
            '--fault-evidence-root', raw, '--worker-inputs']

def _arguments(arguments):
    values = list(arguments)
    if any(not isinstance(arg, str) or arg.startswith(('--fault-', '--enable-f05', '--enable-f09',
           '--enable-f14', '--enable-storage', '--batch-id')) or arg == '--worker-inputs' for arg in values):
        raise ValueError('DUPLICATE_F09_OPTIONS')
    return values

@contextmanager
def ready(staged, arguments, audit, *, batch_id, evidence_root, enable=False,
          stop=lambda: False, timeout=5):
    options = receipt_apply_arguments(batch_id, evidence_root, enable)
    arguments = _arguments(arguments)
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='f09-crash-captured'):
        if stop(): raise ValueError('F09_STOPPED')
        yield {'ready': True, 'fault_started': False, 'service_started': False,
               'apply_called_verified': False, 'F09_verified': False,
               'approval_verified': False, 'reusable_permit': False}
