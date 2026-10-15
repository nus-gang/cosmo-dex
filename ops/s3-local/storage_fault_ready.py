"""Offline READY supervision for the dedicated storage fault child; never START."""
from contextlib import contextmanager
from pathlib import Path
from ready_worker import _ready_scope

POINTS = frozenset(('candidate_verified evidence_write evidence_complete before_wal '
    'partial_wal wal_sync after_wal_sync marker_sync after_marker_sync '
    'after_marker_rename marker_dir_sync after_marker_dir_sync after_commit '
    'before_publish before_response file_sync publish_dir_sync').split())


def fault_arguments(point, occurrence, purpose, evidence_root, enable, errno=None):
    if errno not in (None, "ENOSPC", "EDQUOT", "EIO"):
        raise ValueError("STORAGE_FAULT_ERRNO")
    path = Path(evidence_root)
    if enable is not True or point not in POINTS or type(occurrence) is not int or \
       not 1 <= occurrence <= 1024 or purpose not in ('NORMAL', 'RESOLVE_FAILURE') or \
       not path.is_absolute() or '..' in path.parts:
        raise ValueError('STORAGE_FAULT_OPTIONS')
    return ['--enable-storage-fault', '--fault-point', point,
            '--fault-occurrence', str(occurrence), '--fault-purpose', purpose,
            '--fault-evidence-root', str(path)] + ([] if errno is None else ['--fault-errno', errno])


@contextmanager
def ready(staged, arguments, audit, *, point, occurrence, purpose, evidence_root,
          enable=False, errno=None, operation="Seal", stop=lambda: False, timeout=5):
    """Call inside storage_fault_stage.stage with the same worker arguments.

    The trusted audit closes over the exact candidate and approval revisions.
    This probe kills/reaps the child without sending START. It neither fetches
    RPC nor reserves fault evidence; actual Rust/C integration is tested apart.
    """
    if operation not in ("Seal", "Apply") or (operation == "Apply" and purpose != "NORMAL"):
        raise ValueError("STORAGE_FAULT_COMMAND")
    options = fault_arguments(point, occurrence, purpose, evidence_root, enable, errno)
    arguments = list(arguments)
    if any(x.startswith('--fault-') or x == '--enable-storage-fault' for x in arguments):
        raise ValueError('DUPLICATE_FAULT_OPTIONS')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='fault-'+operation.lower()+'-captured'):
        if stop():
            raise ValueError('STORAGE_FAULT_STOPPED')
        yield {'ready': True, 'fault_started': False, 'service_started': False,
               'approval_verified': False, 'reusable_permit': False}
