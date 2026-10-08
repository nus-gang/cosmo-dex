"""Dedicated crash READY probe. Never sends START or creates crash evidence."""
from contextlib import contextmanager
from ready_worker import _ready_scope
from storage_fault_ready import fault_arguments


def crash_arguments(point, occurrence, purpose, evidence_root, enable, fault_command="Seal"):
    if fault_command not in ("Seal", "Apply") or (fault_command == "Apply" and purpose != "NORMAL"):
        raise ValueError("CRASH_COMMAND")
    options = fault_arguments(point, occurrence, purpose, evidence_root, enable)
    options[0] = '--enable-storage-crash'
    return options + ['--worker-inputs']


@contextmanager
def ready(staged, arguments, audit, *, point, occurrence, purpose, evidence_root,
          enable=False, fault_command="Seal", stop=lambda: False, timeout=5):
    """Use inside storage_crash_stage.stage with its exact worker inputs.

    audit must read authenticated exact-candidate approvals. READY is only a
    rejection/cleanup probe, not permission for later execution.
    """
    options = crash_arguments(point, occurrence, purpose, evidence_root, enable, fault_command)
    arguments = list(arguments)
    if any(not isinstance(x, str) or x.startswith('--fault-') or
           x.startswith('--enable-storage') or x == '--worker-inputs'
           for x in arguments):
        raise ValueError('DUPLICATE_CRASH_OPTIONS')
    with _ready_scope(staged, options + arguments, audit, timeout, stop,
                      command='crash-'+fault_command.lower()+'-captured'):
        if stop():
            raise ValueError('STORAGE_CRASH_STOPPED')
        yield {'ready': True, 'crash_started': False, 'service_started': False,
               'approval_verified': False, 'reusable_permit': False}
