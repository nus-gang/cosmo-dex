"""Offline create READY probe. No START, fetch, home deletion or reusable permit."""
from contextlib import contextmanager
import hashlib
from pathlib import Path
from bootstrap_stage import MAX_RPC
from preflight import bounded, checked_root
from ready_worker import _ready_scope


def _path(value):
    path = Path(value)
    if not path.is_absolute() or '..' in path.parts:
        raise ValueError('INPUT_PATH')
    return str(path)


@contextmanager
def _scope(staged, home, evidence_root, pin, effective_profile, audit,
          stop=lambda: False, timeout=5):
    """Use only inside bootstrap_stage.stage; audit must close over exact inputs.

    RPC origin is the caller's responsibility. Checking bytes does not establish
    chain provenance. The real child preserves evidence before READY; this
    probe deliberately denies creation and leaves that evidence untouched.
    """
    prefix = ['--home', _path(home), '--evidence-root', _path(evidence_root),
              '--rpc-file', _path(staged.rpc_file), '--rpc-sha256', staged.rpc_sha256]
    arguments = ['--runtime-pin', pin, '--local-demo-profile', _path(effective_profile),
                 '--acknowledge-unproven-space']
    if not isinstance(pin, str) or len(pin) != 64 or any(c not in '0123456789abcdef' for c in pin):
        raise ValueError('INPUT_HASH')
    def checked_audit():
        result = audit()
        raw = bounded(checked_root(staged.rpc_file.parent), staged.rpc_file.name, MAX_RPC)
        if not raw or hashlib.sha256(raw).hexdigest() != staged.rpc_sha256:
            raise ValueError('BOOTSTRAP_RPC_CHANGED')
        if stop():
            raise ValueError('BOOTSTRAP_STOPPED')
        return result
    with _ready_scope(staged, arguments, checked_audit, timeout, stop,
                      command='create-captured', prefix=prefix) as state:
        yield state, checked_audit


@contextmanager
def ready(staged, home, evidence_root, pin, effective_profile, audit,
          stop=lambda: False, timeout=5):
    with _scope(staged, home, evidence_root, pin, effective_profile, audit, stop, timeout):
        yield {'ready': True, 'home_created': False, 'service_started': False,
               'approval_verified': False, 'reusable_permit': False}
