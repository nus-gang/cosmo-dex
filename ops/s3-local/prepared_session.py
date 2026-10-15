"""One-shot mailbox + exact registration preparation. No registration API writes.

Keep the object alive until the authorized L-T session ends. Its packet is a
copy for review/registration, never approval. Evidence is retained on all exits.
"""
from contextlib import contextmanager
import copy
import os
from pathlib import Path

import mailbox_session
from offline_cli import parse
from pid_mailbox import Mailbox, _root, _read, _nonce, _archive_pending
from workspace_registration import prepare

ERROR = 'PREPARED_SESSION_REJECTED'


class PreparedSession:
    def __init__(self, python, candidate, argv, *, fee_bps):
        try:
            self._python, self._candidate = str(python), str(candidate)
            self._argv = tuple(argv)
            self._fee = fee_bps
            # Validate everything before creating a directory. The explicit
            # mailbox path becomes part of the exact registered command.
            self._packet = prepare(self._python, self._candidate, self._argv,
                                   fee_bps=fee_bps)
            parsed, _ = parse(list(self._argv), reviewed=True, managed=True)
            self._mailbox = Mailbox(parsed.pid_mailbox)
            self._used = False
            self._retirable = False
            self._retirement_attempted = False
        except Exception:
            raise ValueError(ERROR) from None

    @property
    def packet(self):
        return copy.deepcopy(self._packet)

    @property
    def evidence_path(self):
        return Path(self._mailbox.root)

    @contextmanager
    def session(self, **kwargs):
        if self._used:
            raise ValueError(ERROR)
        self._used = True  # Failed entry, denial and interrupts consume it too.
        if any(k in kwargs for k in ('mailbox', 'fee_bps')):
            raise ValueError(ERROR)
        with mailbox_session.session(self._python, self._candidate, self._argv,
                mailbox=self._mailbox, fee_bps=self._fee, **kwargs) as evidence:
            yield evidence
        if (evidence.get('control_plane_stop_verified') is True and
                evidence.get('host_release_observations_complete') is True and
                evidence.get('pid_handoff_collected') is True):
            self._pid_raw = _read(self._mailbox.root, 'pids.json')
            self._retirable = True

    def preserve_completed_mailbox(self):
        """Archive after normal observed completion; no deletion or new start.

        Same-uid filesystem owner is trusted. This does not certify unlisted
        descendants. Failed archival is consumed; partial evidence stays.
        """
        if not self._retirable or self._retirement_attempted:
            raise ValueError(ERROR)
        self._retirement_attempted = True
        try:
            box = self._mailbox
            root = _root(box.root)
            parent = _root(root.parent)
            current = root.stat()
            if ((current.st_dev, current.st_ino) !=
                    (box.identity.st_dev, box.identity.st_ino) or
                    _nonce(_read(root, 'challenge')) != box.nonce or
                    set(p.name for p in root.iterdir()) != {'challenge', 'pids.json'}):
                raise ValueError()
            if _read(root, 'pids.json') != self._pid_raw:
                raise ValueError()
            from workspace_reentry import _sync, _rename_exclusive
            archive = parent / ('completed-' + box.nonce)
            if archive.exists(): raise ValueError()
            pending = _archive_pending(root)
            pending.mkdir(mode=0o700)
            _sync(parent)
            destination = archive / 'mailbox'
            _rename_exclusive(root, pending/'mailbox')
            _sync(pending)
            _sync(parent)
            # Same final publication boundary as the receipt-producing path.
            _rename_exclusive(pending, archive)
            return destination
        except Exception:
            raise ValueError(ERROR) from None
